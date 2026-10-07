//! Passwords (argon2id), bearer tokens and tickets (random, stored as
//! SHA-256), and the per-IP login rate limiter.

use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

/// OWASP password storage recommendation for argon2id: 19 MiB memory,
/// 2 iterations, parallelism 1.
pub const ARGON2_MEMORY_KIB: u32 = 19 * 1024;
pub const ARGON2_ITERATIONS: u32 = 2;
pub const ARGON2_PARALLELISM: u32 = 1;

/// Shortest accepted password.
pub const MIN_PASSWORD_LEN: usize = 8;

fn argon2() -> Argon2<'static> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        None,
    )
    .expect("static argon2 parameters are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// PHC string (`$argon2id$v=19$m=19456,t=2,p=1$…`).
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow!("hashing password: {error}"))
}

/// Verifies against a stored hash. With `None` (unknown user) a dummy hash
/// is verified anyway so response time does not reveal which names exist.
#[must_use]
pub fn verify_password(stored: Option<&str>, password: &str) -> bool {
    static DUMMY: OnceLock<String> = OnceLock::new();
    let dummy = DUMMY.get_or_init(|| hash_password("dummy password for timing").unwrap());
    let candidate = stored.unwrap_or(dummy);
    let ok = PasswordHash::new(candidate)
        .is_ok_and(|hash| argon2().verify_password(password.as_bytes(), &hash).is_ok());
    ok && stored.is_some()
}

/// 32 random bytes, base64url without padding (43 characters).
#[must_use]
pub fn random_secret() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Random lowercase hex of `bytes` bytes.
#[must_use]
pub fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What the database stores for a token or ticket.
#[must_use]
pub fn secret_hash(secret: &str) -> String {
    Sha256::digest(secret.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Sliding-window limiter: at most `limit` attempts per `window` per IP.
pub struct RateLimiter {
    limit: usize,
    window: Duration,
    attempts: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl RateLimiter {
    #[must_use]
    pub fn new(limit: usize, window: Duration) -> Self {
        Self {
            limit,
            window,
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// Records an attempt; `false` when it exceeds the limit (the rejected
    /// attempt is not counted, so a client is locked out for at most one
    /// window after it stops).
    pub fn attempt(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut attempts = self.attempts.lock().unwrap();
        attempts.retain(|_, times| {
            while times
                .front()
                .is_some_and(|first| now.duration_since(*first) >= self.window)
            {
                times.pop_front();
            }
            !times.is_empty()
        });
        let times = attempts.entry(ip).or_default();
        if times.len() >= self.limit {
            return false;
        }
        times.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_round_trip_with_owasp_parameters() {
        let hash = hash_password("correct horse").unwrap();
        assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify_password(Some(&hash), "correct horse"));
        assert!(!verify_password(Some(&hash), "wrong horse"));
        assert!(!verify_password(None, "correct horse"));
    }

    #[test]
    fn secrets_are_random_and_hashed() {
        let (a, b) = (random_secret(), random_secret());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert_eq!(secret_hash(&a).len(), 64);
        assert_eq!(secret_hash(&a), secret_hash(&a));
    }

    #[test]
    fn limiter_allows_five_per_window_per_ip() {
        let limiter = RateLimiter::new(5, Duration::from_millis(100));
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let other: IpAddr = "192.0.2.2".parse().unwrap();
        for _ in 0..5 {
            assert!(limiter.attempt(ip));
        }
        assert!(!limiter.attempt(ip));
        assert!(limiter.attempt(other));
        std::thread::sleep(Duration::from_millis(120));
        assert!(limiter.attempt(ip));
    }
}
