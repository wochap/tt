//! Server identity: an ed25519 key pair in `server.key` (the raw 32-byte
//! seed, mode 0600) next to `server.db`, and the `server_id` derived from its
//! public key. The identity exists before and independently of the root.
//!
//! `ring` provides Ed25519 already (through rustls) and builds a key pair
//! from a raw seed, so no extra crate is needed.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use rand_core::{OsRng, RngCore};
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};

pub const KEY_FILE: &str = "server.key";
const SEED_LEN: usize = 32;

pub struct Identity {
    seed: [u8; SEED_LEN],
    public: Vec<u8>,
    server_id: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("server_id", &self.server_id)
            .finish_non_exhaustive()
    }
}

/// Where the key file of the database at `db` lives.
#[must_use]
pub fn key_path(db: &Path) -> PathBuf {
    db.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(KEY_FILE)
}

impl Identity {
    fn from_seed(seed: [u8; SEED_LEN]) -> Result<Self> {
        let pair = Ed25519KeyPair::from_seed_unchecked(&seed)
            .map_err(|error| anyhow::anyhow!("invalid ed25519 seed: {error}"))?;
        let public = pair.public_key().as_ref().to_vec();
        let server_id = server_id(&public);
        Ok(Self {
            seed,
            public,
            server_id,
        })
    }

    /// Reads `path`, or creates it with a fresh seed when it does not exist.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let Ok(seed) = <[u8; SEED_LEN]>::try_from(bytes.as_slice()) else {
                    bail!(
                        "{} is not a tt-server key ({} bytes, expected {SEED_LEN})",
                        path.display(),
                        bytes.len()
                    );
                };
                Self::from_seed(seed)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut seed = [0_u8; SEED_LEN];
                OsRng.fill_bytes(&mut seed);
                write_private(path, &seed)?;
                Self::from_seed(seed)
            }
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// 26 characters of lowercase base32.
    #[must_use]
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    #[must_use]
    pub fn public_key(&self) -> &[u8] {
        &self.public
    }

    /// The key pair, for signing (`server-identity-link`).
    pub fn key_pair(&self) -> Result<Ed25519KeyPair> {
        Ed25519KeyPair::from_seed_unchecked(&self.seed)
            .map_err(|error| anyhow::anyhow!("invalid ed25519 seed: {error}"))
    }
}

/// Writes `bytes` to a new file readable only by its owner, atomically: a
/// crash leaves either no key file or a complete one.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("creating {}", temporary.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temporary, path).with_context(|| format!("installing {}", path.display()))?;
    Ok(())
}

/// Lowercase RFC 4648 base32 without padding of the first 16 bytes of
/// SHA-256(public key).
#[must_use]
pub fn server_id(public_key: &[u8]) -> String {
    base32(&Sha256::digest(public_key)[..16])
}

fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// The host name, the default server name.
#[must_use]
pub fn host_name() -> String {
    let from_file = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| std::fs::read_to_string("/etc/hostname"))
        .ok();
    let from_command = || {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
    };
    from_file
        .or_else(from_command)
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "tt-server".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{ED25519, UnparsedPublicKey};

    #[test]
    fn base32_matches_rfc_4648_vectors() {
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "my");
        assert_eq!(base32(b"fo"), "mzxq");
        assert_eq!(base32(b"foo"), "mzxw6");
        assert_eq!(base32(b"foobar"), "mzxw6ytboi");
    }

    #[test]
    fn key_round_trips_through_the_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        let created = Identity::load_or_create(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap().len(), SEED_LEN);
        let loaded = Identity::load_or_create(&path).unwrap();
        assert_eq!(loaded.public_key(), created.public_key());
        let signature = loaded.key_pair().unwrap().sign(b"tt");
        UnparsedPublicKey::new(&ED25519, created.public_key())
            .verify(b"tt", signature.as_ref())
            .expect("a signature from the reloaded key verifies");
    }

    #[test]
    fn server_id_format_stability_and_file_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        let id = Identity::load_or_create(&path)
            .unwrap()
            .server_id()
            .to_owned();
        assert_eq!(id.len(), 26);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)),
            "{id}"
        );
        assert_eq!(Identity::load_or_create(&path).unwrap().server_id(), id);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::write(&path, b"short").unwrap();
        assert!(Identity::load_or_create(&path).is_err());
    }
}
