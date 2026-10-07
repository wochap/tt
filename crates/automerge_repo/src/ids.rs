use std::{fmt, str::FromStr, sync::Arc};

use uuid::Uuid;

/// Stable identifier for an Automerge document.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DocumentId(Uuid);

impl DocumentId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 16] {
        *self.0.as_bytes()
    }
    /// The automerge-repo JS rendering: bs58check of the 16 id bytes.
    #[must_use]
    pub fn to_bs58check(self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }
    /// Parses the bs58check rendering used on the wire and in `automerge:` URLs.
    pub fn from_bs58check(value: &str) -> Result<Self, InvalidDocumentId> {
        let bytes = bs58::decode(value)
            .with_check(None)
            .into_vec()
            .map_err(|_| InvalidDocumentId(value.to_owned()))?;
        let bytes: [u8; 16] = bytes
            .try_into()
            .map_err(|_| InvalidDocumentId(value.to_owned()))?;
        Ok(Self::from_bytes(bytes))
    }
    /// Accepts bs58check, `automerge:<bs58check>`, or a hyphenated UUID.
    pub fn parse_any(value: &str) -> Result<Self, InvalidDocumentId> {
        let value = value.trim();
        let value = value.strip_prefix("automerge:").unwrap_or(value);
        Self::from_bs58check(value).or_else(|_| {
            Uuid::parse_str(value)
                .map(Self)
                .map_err(|_| InvalidDocumentId(value.to_owned()))
        })
    }
    /// `automerge:<bs58check>` URL as used by JS clients.
    #[must_use]
    pub fn to_url(self) -> String {
        format!("automerge:{}", self.to_bs58check())
    }
}

/// A document id that is neither valid bs58check of 16 bytes nor a UUID.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid document id {0:?}")]
pub struct InvalidDocumentId(pub String);

impl Default for DocumentId {
    fn default() -> Self {
        Self::new()
    }
}
impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}
impl FromStr for DocumentId {
    type Err = uuid::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

/// Identity asserted by an authenticated transport session.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PeerId(Arc<str>);

impl PeerId {
    #[must_use]
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self(value.into())
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<&str> for PeerId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}
impl From<String> for PeerId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_id_is_canonical_and_exactly_sixteen_bytes() {
        let id: DocumentId = "550E8400-E29B-41D4-A716-446655440000".parse().unwrap();
        assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(DocumentId::from_bytes(id.to_bytes()), id);
        assert_eq!(id.to_bytes().len(), 16);
    }

    #[test]
    fn bs58check_round_trips_and_rejects_bad_checksum() {
        let id: DocumentId = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
        let encoded = id.to_bs58check();
        assert_eq!(DocumentId::from_bs58check(&encoded).unwrap(), id);
        assert_eq!(DocumentId::parse_any(&id.to_url()).unwrap(), id);
        assert_eq!(DocumentId::parse_any(&id.to_string()).unwrap(), id);
        let mut broken = encoded.clone();
        let last = broken.pop().unwrap();
        broken.push(if last == '1' { '2' } else { '1' });
        assert!(DocumentId::from_bs58check(&broken).is_err());
    }

    #[test]
    fn bs58check_matches_automerge_repo_js_fixture() {
        // Fixture produced by automerge-repo JS `stringifyAutomergeUrl` for these bytes.
        let id = DocumentId::from_bytes([
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ]);
        assert_eq!(id.to_bs58check(), JS_FIXTURE);
    }
    const JS_FIXTURE: &str = "2BjHNJQh2prqecCoP3d2NdgawRSP";

    #[test]
    fn peer_id_clones_share_owned_text() {
        let peer = PeerId::from("device-a");
        assert_eq!(peer.clone(), peer);
        assert_eq!(peer.to_string(), "device-a");
    }
}
