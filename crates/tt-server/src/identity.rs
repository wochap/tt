//! Server identity: an ed25519 key pair in `server.key` (PKCS#8 DER, mode
//! 0600) next to `server.db`, and the `server_id` derived from its public
//! key. The identity exists before and independently of the root, lives
//! outside the database so `tt-server reset` keeps it, and is replaced only
//! by `reset --new-identity`.
//!
//! `ring` provides Ed25519 already (through rustls), so no extra crate is
//! needed. Key files of the first format (the raw 32-byte seed) are
//! rewritten as PKCS#8 on load.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use sha2::{Digest, Sha256};

pub const KEY_FILE: &str = "server.key";
const SEED_LEN: usize = 32;
/// PKCS#8 v1 wrapping of a raw Ed25519 seed (RFC 8410).
const PKCS8_V1_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

pub struct Identity {
    pkcs8: Vec<u8>,
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

/// Refuses a key file that group or others can access.
fn check_mode(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .with_context(|| format!("reading {}", path.display()))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            bail!(
                "{} is accessible by group or others (mode {:o}); run `chmod 600 {}`",
                path.display(),
                mode & 0o777,
                path.display()
            );
        }
    }
    Ok(())
}

impl Identity {
    fn from_pkcs8(pkcs8: Vec<u8>, path: &Path) -> Result<Self> {
        let pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&pkcs8).map_err(|error| {
            anyhow::anyhow!("{} is not an ed25519 key: {error}", path.display())
        })?;
        let public = pair.public_key().as_ref().to_vec();
        let server_id = server_id(&public);
        Ok(Self {
            pkcs8,
            public,
            server_id,
        })
    }

    /// Reads `path`, or creates it with a fresh key when it does not exist.
    /// A key file readable by group or others is refused.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                check_mode(path)?;
                if bytes.len() == SEED_LEN {
                    // First format: the raw seed. Rewrite it as PKCS#8.
                    let mut pkcs8 = PKCS8_V1_PREFIX.to_vec();
                    pkcs8.extend_from_slice(&bytes);
                    let identity = Self::from_pkcs8(pkcs8, path)?;
                    std::fs::remove_file(path)
                        .with_context(|| format!("replacing {}", path.display()))?;
                    write_private(path, &identity.pkcs8)?;
                    return Ok(identity);
                }
                Self::from_pkcs8(bytes, path)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                    .map_err(|_| anyhow::anyhow!("generating an ed25519 key failed"))?;
                let pkcs8 = pkcs8.as_ref().to_vec();
                write_private(path, &pkcs8)?;
                Self::from_pkcs8(pkcs8, path)
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

    /// The private key as PKCS#8 DER (for TLS certificates).
    #[must_use]
    pub fn pkcs8(&self) -> &[u8] {
        &self.pkcs8
    }

    /// The key pair, for signing.
    pub fn key_pair(&self) -> Result<Ed25519KeyPair> {
        Ed25519KeyPair::from_pkcs8_maybe_unchecked(&self.pkcs8)
            .map_err(|error| anyhow::anyhow!("invalid ed25519 key: {error}"))
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

/// Lowercase RFC 4648 base32 without padding.
#[must_use]
pub fn base32(bytes: &[u8]) -> String {
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

/// Decodes [`base32`] (case-insensitive); `None` on invalid input.
#[must_use]
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for c in text.bytes() {
        let value = match c.to_ascii_lowercase() {
            c @ b'a'..=b'z' => c - b'a',
            c @ b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | u32::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    // Leftover bits must be zero padding, at most 4 of them.
    (bits < 5 && buffer == 0).then_some(out)
}

/// The first characters of a server id, shown next to its name.
#[must_use]
pub fn id_prefix(server_id: &str) -> &str {
    &server_id[..server_id.len().min(8)]
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
    fn base32_decodes_what_it_encodes() {
        for bytes in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foobar",
            &[0xff; 16],
            &[0; 37],
        ] {
            assert_eq!(base32_decode(&base32(bytes)).unwrap(), bytes);
        }
        assert_eq!(base32_decode("MZXW6").unwrap(), b"foo");
        assert!(base32_decode("mzxw1").is_none());
        assert!(base32_decode("mzxw7").is_none(), "non-zero padding bits");
    }

    #[test]
    fn key_round_trips_through_the_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        let created = Identity::load_or_create(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), created.pkcs8());
        let loaded = Identity::load_or_create(&path).unwrap();
        assert_eq!(loaded.public_key(), created.public_key());
        let signature = loaded.key_pair().unwrap().sign(b"tt");
        UnparsedPublicKey::new(&ED25519, created.public_key())
            .verify(b"tt", signature.as_ref())
            .expect("a signature from the reloaded key verifies");
    }

    #[test]
    fn a_raw_seed_key_file_is_rewritten_as_pkcs8_with_the_same_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        write_private(&path, &[7_u8; SEED_LEN]).unwrap();
        let pair = Ed25519KeyPair::from_seed_unchecked(&[7_u8; SEED_LEN]).unwrap();
        let identity = Identity::load_or_create(&path).unwrap();
        assert_eq!(identity.public_key(), pair.public_key().as_ref());
        assert_eq!(std::fs::read(&path).unwrap().len(), 48);
        let again = Identity::load_or_create(&path).unwrap();
        assert_eq!(again.server_id(), identity.server_id());
    }

    #[cfg(unix)]
    #[test]
    fn a_key_file_readable_by_others_is_refused_by_name() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        Identity::load_or_create(&path).unwrap();
        for mode in [0o640, 0o604, 0o660] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            let error = Identity::load_or_create(&path).unwrap_err().to_string();
            assert!(error.contains(&path.display().to_string()), "{error}");
            assert!(error.contains("group or others"), "{error}");
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        Identity::load_or_create(&path).unwrap();
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(Identity::load_or_create(&path).is_err());
    }
}
