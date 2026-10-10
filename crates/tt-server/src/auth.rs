//! Passwords (argon2id), member-wide bearer tokens (`tt2`, signed by the
//! issuing server's key), tickets (random, stored as SHA-256), and the
//! per-IP login rate limiter.
//!
//! A token is `tt2.<base64url(payload)>.<base64url(signature)>`: the payload
//! is the JSON `{v, iss, tid, uid, iat}` and the signature is the issuer's
//! ed25519 signature over the payload bytes. Any member verifies it against
//! the issuer's key in the registry, so no member can mint tokens for
//! another issuer.

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
use ring::signature::{ED25519, Ed25519KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
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

/// The prefix of the signed token format.
pub const TOKEN_PREFIX: &str = "tt2";
/// The payload version of [`TOKEN_PREFIX`] tokens.
pub const TOKEN_VERSION: u8 = 2;

/// What a token says. Nothing in it is secret: the signature is what makes
/// it a credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenClaims {
    pub v: u8,
    /// The issuing server's id.
    pub iss: String,
    /// The token id: 16 random bytes as lowercase hex.
    pub tid: String,
    /// The user id.
    pub uid: String,
    /// Issued at, UTC seconds.
    pub iat: i64,
}

impl TokenClaims {
    /// Fresh claims with a random token id.
    #[must_use]
    pub fn new(iss: &str, uid: &str, iat: i64) -> Self {
        Self {
            v: TOKEN_VERSION,
            iss: iss.to_owned(),
            tid: random_hex(16),
            uid: uid.to_owned(),
            iat,
        }
    }
}

/// Signs `claims` with the issuer's key.
pub fn encode_token(claims: &TokenClaims, key: &Ed25519KeyPair) -> Result<String> {
    let payload = serde_json::to_vec(claims)?;
    let signature = key.sign(&payload);
    Ok(format!(
        "{TOKEN_PREFIX}.{}.{}",
        URL_SAFE_NO_PAD.encode(&payload),
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}

/// A token split into its parts, before the signature is checked.
#[derive(Clone, Debug)]
pub struct ParsedToken {
    pub claims: TokenClaims,
    payload: Vec<u8>,
    signature: Vec<u8>,
}

impl ParsedToken {
    /// Parses the format; `None` for anything that is not a `tt2` token.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        let mut parts = token.split('.');
        let (prefix, payload, signature) = (parts.next()?, parts.next()?, parts.next()?);
        if prefix != TOKEN_PREFIX || parts.next().is_some() {
            return None;
        }
        let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
        let signature = URL_SAFE_NO_PAD.decode(signature).ok()?;
        let claims: TokenClaims = serde_json::from_slice(&payload).ok()?;
        (claims.v == TOKEN_VERSION).then_some(Self {
            claims,
            payload,
            signature,
        })
    }

    /// Whether the signature verifies with the raw ed25519 `public_key`.
    #[must_use]
    pub fn verify(&self, public_key: &[u8]) -> bool {
        UnparsedPublicKey::new(&ED25519, public_key)
            .verify(&self.payload, &self.signature)
            .is_ok()
    }
}

/// Parses `token` and checks its signature with `public_key`.
#[must_use]
pub fn verify_token(token: &str, public_key: &[u8]) -> Option<TokenClaims> {
    ParsedToken::parse(token)
        .filter(|parsed| parsed.verify(public_key))
        .map(|parsed| parsed.claims)
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

    fn key() -> Ed25519KeyPair {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
    }

    #[test]
    fn tokens_round_trip() {
        use ring::signature::KeyPair;
        let key = key();
        let claims = TokenClaims::new("server-a", "user-1", 1_700_000_000);
        assert_eq!(claims.tid.len(), 32);
        let token = encode_token(&claims, &key).unwrap();
        assert!(token.starts_with("tt2."), "{token}");
        assert_eq!(token.split('.').count(), 3);
        assert_eq!(
            verify_token(&token, key.public_key().as_ref()),
            Some(claims.clone())
        );
        let other = TokenClaims::new("server-a", "user-1", 1_700_000_000);
        assert_ne!(other.tid, claims.tid, "token ids are random");
    }

    #[test]
    fn a_tampered_payload_is_rejected() {
        use ring::signature::KeyPair;
        let key = key();
        let token = encode_token(&TokenClaims::new("server-a", "user-1", 1), &key).unwrap();
        let (_, rest) = token.split_once('.').unwrap();
        let (_, signature) = rest.split_once('.').unwrap();
        let mut forged = TokenClaims::new("server-a", "user-2", 1);
        forged.tid = ParsedToken::parse(&token).unwrap().claims.tid;
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&forged).unwrap());
        let tampered = format!("tt2.{payload}.{signature}");
        assert!(ParsedToken::parse(&tampered).is_some(), "still well formed");
        assert_eq!(verify_token(&tampered, key.public_key().as_ref()), None);
        assert_eq!(verify_token("tt2.e30.AA", key.public_key().as_ref()), None);
        assert!(ParsedToken::parse("not-a-token").is_none());
        assert!(
            ParsedToken::parse(&random_secret()).is_none(),
            "old opaque format"
        );
        assert!(ParsedToken::parse(&format!("{token}.extra")).is_none());
    }

    #[test]
    fn a_token_signed_by_another_key_is_rejected() {
        use ring::signature::KeyPair;
        let (issuer, other) = (key(), key());
        let token = encode_token(&TokenClaims::new("server-a", "user-1", 1), &other).unwrap();
        assert_eq!(verify_token(&token, issuer.public_key().as_ref()), None);
        assert!(verify_token(&token, other.public_key().as_ref()).is_some());
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
