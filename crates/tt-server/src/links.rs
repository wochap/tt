//! Peer addresses: the hello both sides of a peer link exchange before the
//! automerge-repo protocol starts, the addresses a server advertises in it,
//! and the per-member dial loop with its backoff.
//!
//! Addresses never enter the registry: what a hello carries is stored in
//! the local `peer_addresses` table ([`crate::db`]) only.

use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, watch};

/// Addresses accepted per member in one hello (advertised or hinted).
pub const MAX_ADDRESSES: usize = crate::db::MAX_HINTS;
/// First delay between dial attempts.
pub const BACKOFF_MIN: Duration = Duration::from_secs(1);
/// Longest delay between dial attempts.
pub const BACKOFF_MAX: Duration = Duration::from_secs(300);
const HELLO_KIND: &str = "tt-hello";

/// The first frame each side sends on a peer link, once mutual TLS proved
/// both keys. Encoded as CBOR.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    #[serde(rename = "type")]
    pub kind: String,
    pub server_id: String,
    pub name: String,
    pub version: String,
    /// `host:port` endpoints of the sender's peer listener.
    pub advertise: Vec<String>,
    /// The sender's client URL (`--public-url`), for browsers only.
    #[serde(default)]
    pub public_url: Option<String>,
    /// Last-seen addresses of other members.
    #[serde(default)]
    pub hints: Vec<Hint>,
}

/// Addresses of one member, as the sender last saw them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hint {
    pub server_id: String,
    pub addrs: Vec<HintAddr>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HintAddr {
    pub addr: String,
    /// UTC seconds, by the sender's clock (informational).
    pub seen_at: i64,
}

impl Hello {
    #[must_use]
    pub fn new(server_id: &str, name: &str) -> Self {
        Self {
            kind: HELLO_KIND.into(),
            server_id: server_id.to_owned(),
            name: name.to_owned(),
            version: env!("CARGO_PKG_VERSION").into(),
            advertise: Vec::new(),
            public_url: None,
            hints: Vec::new(),
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes).context("encoding the hello")?;
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let hello: Self = ciborium::from_reader(bytes).context("decoding the hello")?;
        if hello.kind != HELLO_KIND {
            bail!("expected a hello, got {:?}", hello.kind);
        }
        Ok(hello)
    }
}

/// Whether an interface address can be reached from another host.
fn reachable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !(v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()),
        IpAddr::V6(v6) => {
            !(v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

/// The addresses advertised for a peer listener bound to `listen`: every
/// configured `--peer-advertise` value, then the listen address itself, or
/// for a wildcard listen address every reachable interface address of the
/// same family (both families for `[::]`) with its port. Loopback
/// addresses are never advertised.
#[must_use]
pub fn advertised_addresses(
    listen: SocketAddr,
    configured: &[String],
    interfaces: &[IpAddr],
) -> Vec<String> {
    let mut addresses: Vec<String> = Vec::new();
    let mut push = |address: String| {
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    };
    for address in configured {
        push(address.clone());
    }
    let ip = listen.ip();
    if ip.is_unspecified() {
        let mut ips: Vec<IpAddr> = interfaces
            .iter()
            .copied()
            .filter(|candidate| reachable(*candidate))
            .filter(|candidate| ip.is_ipv6() || candidate.is_ipv4())
            .collect();
        ips.sort();
        for candidate in ips {
            push(SocketAddr::new(candidate, listen.port()).to_string());
        }
    } else if reachable(ip) {
        push(listen.to_string());
    }
    addresses
}

/// The host's interface addresses, read anew on each call.
#[must_use]
pub fn interface_addresses() -> Vec<IpAddr> {
    if_addrs::get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .filter(|interface| interface.is_oper_up() || interface.index.is_none())
                .map(|interface| interface.ip())
                .collect()
        })
        .unwrap_or_default()
}

/// Exponential backoff with ±20 % jitter.
#[derive(Clone, Debug)]
pub struct Backoff {
    min: Duration,
    max: Duration,
    current: Duration,
}

impl Backoff {
    #[must_use]
    pub fn new(min: Duration, max: Duration) -> Self {
        Self {
            min,
            max: max.max(min),
            current: min,
        }
    }

    /// The next delay; the one after it doubles, up to the maximum.
    pub fn next_delay(&mut self) -> Duration {
        let base = self.current;
        self.current = (self.current * 2).min(self.max);
        let jitter = f64::from(OsRng.next_u32()) / f64::from(u32::MAX);
        base.mul_f64(0.8 + 0.4 * jitter)
    }

    pub fn reset(&mut self) {
        self.current = self.min;
    }
}

/// How one dial attempt went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attempt {
    /// A link came up (and has ended by now).
    Linked,
    /// Every known address failed.
    Failed,
    /// Nothing to dial yet.
    NoAddress,
}

/// Keeps a member linked: idles while a link with it exists, otherwise
/// calls `attempt` (which dials its known addresses in order) and waits
/// `backoff` between failures. A link resets the backoff. `wake` (new
/// addresses) cuts a wait short; `keep` is checked before every attempt
/// and ends the loop when false (the member was revoked).
pub async fn dial_loop<F, Fut>(
    member: String,
    mut links: watch::Receiver<Vec<String>>,
    wake: Arc<Notify>,
    mut backoff: Backoff,
    keep: impl Fn() -> bool,
    mut attempt: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = Attempt>,
{
    loop {
        if links
            .wait_for(|linked| !linked.contains(&member))
            .await
            .is_err()
        {
            return;
        }
        if !keep() {
            return;
        }
        let delay = match attempt().await {
            Attempt::Linked => {
                backoff.reset();
                backoff.next_delay()
            }
            Attempt::Failed => backoff.next_delay(),
            Attempt::NoAddress => {
                tokio::select! {
                    () = wake.notified() => {}
                    changed = links.changed() => if changed.is_err() { return },
                }
                continue;
            }
        };
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = wake.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tokio::time::Instant;

    use super::*;

    #[test]
    fn hello_round_trips_as_cbor() {
        let mut hello = Hello::new("abc", "laptop-b");
        hello.advertise = vec!["laptop-b.example.ts.net:8772".into()];
        hello.public_url = Some("https://laptop-b.example.ts.net".into());
        hello.hints = vec![Hint {
            server_id: "def".into(),
            addrs: vec![HintAddr {
                addr: "10.0.0.3:8772".into(),
                seen_at: 1_700_000_000,
            }],
        }];
        let bytes = hello.encode().unwrap();
        assert_eq!(Hello::decode(&bytes).unwrap(), hello);
        assert!(Hello::decode(b"\xa0").is_err());
        let mut other = hello.clone();
        other.kind = "join".into();
        assert!(Hello::decode(&other.encode().unwrap()).is_err());
    }

    #[test]
    fn wildcard_listen_advertises_names_and_interfaces() {
        let interfaces: Vec<IpAddr> = [
            "127.0.0.1",
            "192.168.1.20",
            "100.101.102.103",
            "169.254.3.4",
            "::1",
            "fe80::1",
            "fd7a:115c:a1e0::1",
        ]
        .iter()
        .map(|ip| ip.parse().unwrap())
        .collect();
        let configured = vec!["laptop-b.example.ts.net:8772".to_owned()];
        assert_eq!(
            advertised_addresses("0.0.0.0:8772".parse().unwrap(), &configured, &interfaces),
            [
                "laptop-b.example.ts.net:8772",
                "100.101.102.103:8772",
                "192.168.1.20:8772"
            ]
        );
        assert_eq!(
            advertised_addresses("[::]:8772".parse().unwrap(), &configured, &interfaces),
            [
                "laptop-b.example.ts.net:8772",
                "100.101.102.103:8772",
                "192.168.1.20:8772",
                "[fd7a:115c:a1e0::1]:8772"
            ]
        );
        // A specific address is advertised as is; loopback never.
        assert_eq!(
            advertised_addresses("192.168.1.20:9000".parse().unwrap(), &[], &interfaces),
            ["192.168.1.20:9000"]
        );
        assert!(
            advertised_addresses("127.0.0.1:9000".parse().unwrap(), &[], &interfaces).is_empty()
        );
        assert_eq!(
            advertised_addresses("127.0.0.1:9000".parse().unwrap(), &configured, &interfaces),
            ["laptop-b.example.ts.net:8772"]
        );
    }

    #[test]
    fn backoff_doubles_with_jitter_up_to_the_cap() {
        let mut backoff = Backoff::new(BACKOFF_MIN, BACKOFF_MAX);
        let mut expected = 1.0_f64;
        for _ in 0..20 {
            let delay = backoff.next_delay().as_secs_f64();
            assert!(
                delay >= expected * 0.8 - 1e-9 && delay <= expected * 1.2 + 1e-9,
                "{delay} vs {expected}"
            );
            expected = (expected * 2.0).min(300.0);
        }
        backoff.reset();
        assert!(backoff.next_delay() <= BACKOFF_MIN.mul_f64(1.2));
    }

    /// With a paused clock: an unreachable member is retried ever less often
    /// but at least every 5 minutes (plus jitter), is linked as soon as it
    /// is reachable, and is not dialed while linked.
    #[tokio::test(start_paused = true)]
    async fn dialing_backs_off_to_five_minutes_and_links_when_reachable() {
        let (links, receiver) = watch::channel(Vec::<String>::new());
        let links = Arc::new(links);
        let wake = Arc::new(Notify::new());
        let attempts: Arc<Mutex<Vec<Instant>>> = Arc::default();
        let reachable_after = 15;
        let task = {
            let (attempts, links) = (attempts.clone(), links.clone());
            tokio::spawn(dial_loop(
                "b".into(),
                receiver,
                wake.clone(),
                Backoff::new(BACKOFF_MIN, BACKOFF_MAX),
                || true,
                move || {
                    let (attempts, links) = (attempts.clone(), links.clone());
                    async move {
                        let count = {
                            let mut attempts = attempts.lock().unwrap();
                            attempts.push(Instant::now());
                            attempts.len()
                        };
                        if count < reachable_after {
                            return Attempt::Failed;
                        }
                        // Reachable: the link stays up for an hour.
                        links.send_replace(vec!["b".into()]);
                        tokio::time::sleep(Duration::from_secs(3600)).await;
                        links.send_replace(Vec::new());
                        Attempt::Linked
                    }
                },
            ))
        };
        let start = Instant::now();
        tokio::time::sleep(Duration::from_secs(3 * 3600)).await;
        task.abort();
        let attempts = attempts.lock().unwrap().clone();
        assert!(attempts.len() > reachable_after, "{}", attempts.len());
        assert!(
            attempts[0] - start < Duration::from_millis(10),
            "dials at once"
        );
        let gaps: Vec<Duration> = attempts.windows(2).map(|w| w[1] - w[0]).collect();
        let cap = BACKOFF_MAX.mul_f64(1.2);
        for (i, gap) in gaps.iter().take(reachable_after - 1).enumerate() {
            assert!(*gap <= cap, "gap {i} is {gap:?}");
            assert!(*gap >= BACKOFF_MIN.mul_f64(0.8), "no busy loop: {gap:?}");
        }
        // The cap is reached.
        assert!(gaps[reachable_after - 2] >= BACKOFF_MAX.mul_f64(0.8));
        // Linked for an hour: no dial meanwhile; then redialed after about
        // a second, the backoff being reset.
        let after_link = gaps[reachable_after - 1];
        assert!(after_link >= Duration::from_secs(3600), "{after_link:?}");
        assert!(
            after_link <= Duration::from_secs(3600) + BACKOFF_MIN.mul_f64(1.2),
            "{after_link:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn without_addresses_the_loop_waits_for_a_wake_and_stops_when_dropped() {
        let (_links, receiver) = watch::channel(Vec::<String>::new());
        let wake = Arc::new(Notify::new());
        let calls = Arc::new(Mutex::new(0_usize));
        let keep = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let task = {
            let (calls, keep) = (calls.clone(), keep.clone());
            tokio::spawn(dial_loop(
                "b".into(),
                receiver,
                wake.clone(),
                Backoff::new(BACKOFF_MIN, BACKOFF_MAX),
                move || keep.load(std::sync::atomic::Ordering::SeqCst),
                move || {
                    let calls = calls.clone();
                    async move {
                        *calls.lock().unwrap() += 1;
                        Attempt::NoAddress
                    }
                },
            ))
        };
        tokio::time::sleep(Duration::from_secs(3600)).await;
        assert_eq!(*calls.lock().unwrap(), 1, "no polling without addresses");
        wake.notify_one();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(*calls.lock().unwrap(), 2);
        keep.store(false, std::sync::atomic::Ordering::SeqCst);
        wake.notify_one();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("the loop ends once the member is dropped")
            .unwrap();
    }
}
