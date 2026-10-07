//! automerge-repo JS wire protocol, version "1".
//!
//! Every frame is one CBOR map with text keys, as produced by `cbor-x` with
//! `tagUint8Array: false`: binary payloads are CBOR byte strings and document
//! ids are bs58check strings of the 16 id bytes. Decoding is tolerant of the
//! encodings `cbor-x` emits (16-bit map headers, `undefined`, tag 64 typed
//! arrays) and of message types this crate does not act on.

use ciborium::value::Value;

use crate::{DocumentId, error::ProtocolError};

/// The only protocol version this crate speaks.
pub const PROTOCOL_VERSION: &str = "1";
/// Largest accepted frame.
pub const MAX_FRAME_LEN: usize = 64 * 1024 * 1024;

/// Metadata exchanged in `join` and `peer`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PeerMetadata {
    pub storage_id: Option<String>,
    pub is_ephemeral: Option<bool>,
}

/// One automerge-repo wire message.
#[derive(Clone, Debug, PartialEq)]
pub enum WireMessage {
    Join {
        sender_id: String,
        peer_metadata: PeerMetadata,
        supported_protocol_versions: Vec<String>,
    },
    Peer {
        sender_id: String,
        target_id: String,
        selected_protocol_version: String,
        peer_metadata: PeerMetadata,
    },
    Error {
        sender_id: String,
        target_id: Option<String>,
        message: String,
    },
    /// The sender has the document (or is replying to a request).
    Sync {
        sender_id: String,
        target_id: String,
        document_id: DocumentId,
        data: Vec<u8>,
    },
    /// The sender does not have the document and asks for it.
    Request {
        sender_id: String,
        target_id: String,
        document_id: DocumentId,
        data: Vec<u8>,
    },
    /// The sender cannot or will not supply the document. Decodes the legacy
    /// `unavailable` type as well; always encodes as `doc-unavailable`.
    DocUnavailable {
        sender_id: String,
        target_id: String,
        document_id: DocumentId,
    },
    Ephemeral {
        sender_id: String,
        target_id: String,
        document_id: DocumentId,
        count: u64,
        session_id: String,
        data: Vec<u8>,
    },
    /// A well-formed message of a type this crate does not act on, such as
    /// `remote-heads-changed` or `remote-subscription-change`.
    Other {
        message_type: String,
        sender_id: Option<String>,
    },
}

impl WireMessage {
    #[must_use]
    pub fn type_name(&self) -> &str {
        match self {
            Self::Join { .. } => "join",
            Self::Peer { .. } => "peer",
            Self::Error { .. } => "error",
            Self::Sync { .. } => "sync",
            Self::Request { .. } => "request",
            Self::DocUnavailable { .. } => "doc-unavailable",
            Self::Ephemeral { .. } => "ephemeral",
            Self::Other { message_type, .. } => message_type,
        }
    }

    #[must_use]
    pub fn sender_id(&self) -> Option<&str> {
        match self {
            Self::Join { sender_id, .. }
            | Self::Peer { sender_id, .. }
            | Self::Error { sender_id, .. }
            | Self::Sync { sender_id, .. }
            | Self::Request { sender_id, .. }
            | Self::DocUnavailable { sender_id, .. }
            | Self::Ephemeral { sender_id, .. } => Some(sender_id),
            Self::Other { sender_id, .. } => sender_id.as_deref(),
        }
    }

    /// Encodes to one CBOR frame.
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut map: Vec<(Value, Value)> = Vec::new();
        let mut put = |key: &str, value: Value| map.push((Value::Text(key.into()), value));
        put("type", Value::Text(self.type_name().into()));
        match self {
            Self::Join {
                sender_id,
                peer_metadata,
                supported_protocol_versions,
            } => {
                put("senderId", text(sender_id));
                put("peerMetadata", metadata_value(peer_metadata));
                put(
                    "supportedProtocolVersions",
                    Value::Array(supported_protocol_versions.iter().map(text).collect()),
                );
            }
            Self::Peer {
                sender_id,
                target_id,
                selected_protocol_version,
                peer_metadata,
            } => {
                put("senderId", text(sender_id));
                put("targetId", text(target_id));
                put("peerMetadata", metadata_value(peer_metadata));
                put("selectedProtocolVersion", text(selected_protocol_version));
            }
            Self::Error {
                sender_id,
                target_id,
                message,
            } => {
                put("senderId", text(sender_id));
                if let Some(target_id) = target_id {
                    put("targetId", text(target_id));
                }
                put("message", text(message));
            }
            Self::Sync {
                sender_id,
                target_id,
                document_id,
                data,
            }
            | Self::Request {
                sender_id,
                target_id,
                document_id,
                data,
            } => {
                put("senderId", text(sender_id));
                put("targetId", text(target_id));
                put("documentId", Value::Text(document_id.to_bs58check()));
                put("data", Value::Bytes(data.clone()));
            }
            Self::DocUnavailable {
                sender_id,
                target_id,
                document_id,
            } => {
                put("senderId", text(sender_id));
                put("targetId", text(target_id));
                put("documentId", Value::Text(document_id.to_bs58check()));
            }
            Self::Ephemeral {
                sender_id,
                target_id,
                document_id,
                count,
                session_id,
                data,
            } => {
                put("senderId", text(sender_id));
                put("targetId", text(target_id));
                put("documentId", Value::Text(document_id.to_bs58check()));
                put("count", Value::Integer((*count).into()));
                put("sessionId", text(session_id));
                put("data", Value::Bytes(data.clone()));
            }
            Self::Other { .. } => return Err(ProtocolError::Unencodable(self.type_name().into())),
        }
        let mut out = Vec::new();
        ciborium::into_writer(&Value::Map(map), &mut out)
            .map_err(|error| ProtocolError::Cbor(error.to_string()))?;
        Ok(out)
    }

    /// Decodes one CBOR frame.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_FRAME_LEN {
            return Err(ProtocolError::FrameTooLarge {
                actual: bytes.len(),
                maximum: MAX_FRAME_LEN,
            });
        }
        let value: Value =
            ciborium::from_reader(bytes).map_err(|error| ProtocolError::Cbor(error.to_string()))?;
        let Value::Map(entries) = value else {
            return Err(ProtocolError::NotAMap);
        };
        let fields = Fields(entries);
        let message_type = fields.text("type")?;
        Ok(match message_type.as_str() {
            "join" => Self::Join {
                sender_id: fields.text("senderId")?,
                peer_metadata: fields.metadata("peerMetadata"),
                supported_protocol_versions: fields.text_list("supportedProtocolVersions"),
            },
            "peer" => Self::Peer {
                sender_id: fields.text("senderId")?,
                target_id: fields.text("targetId")?,
                selected_protocol_version: fields
                    .optional_text("selectedProtocolVersion")
                    .unwrap_or_else(|| PROTOCOL_VERSION.into()),
                peer_metadata: fields.metadata("peerMetadata"),
            },
            "error" => Self::Error {
                sender_id: fields.optional_text("senderId").unwrap_or_default(),
                target_id: fields.optional_text("targetId"),
                message: fields.optional_text("message").unwrap_or_default(),
            },
            "sync" | "request" => {
                let sender_id = fields.text("senderId")?;
                let target_id = fields.text("targetId")?;
                let document_id = fields.document_id()?;
                let data = fields.bytes("data")?;
                if message_type == "sync" {
                    Self::Sync {
                        sender_id,
                        target_id,
                        document_id,
                        data,
                    }
                } else {
                    Self::Request {
                        sender_id,
                        target_id,
                        document_id,
                        data,
                    }
                }
            }
            "doc-unavailable" | "unavailable" => Self::DocUnavailable {
                sender_id: fields.text("senderId")?,
                target_id: fields.text("targetId")?,
                document_id: fields.document_id()?,
            },
            "ephemeral" => Self::Ephemeral {
                sender_id: fields.text("senderId")?,
                target_id: fields.text("targetId")?,
                document_id: fields.document_id()?,
                count: fields.unsigned("count").unwrap_or_default(),
                session_id: fields.optional_text("sessionId").unwrap_or_default(),
                data: fields.bytes("data").unwrap_or_default(),
            },
            _ => Self::Other {
                sender_id: fields.optional_text("senderId"),
                message_type,
            },
        })
    }
}

/// Replaces `targetId` in an encoded frame; used by transports whose
/// repository-facing peer id differs from the id the remote uses on the wire.
pub fn retarget(frame: &[u8], target: &str) -> Result<Vec<u8>, ProtocolError> {
    let value: Value =
        ciborium::from_reader(frame).map_err(|error| ProtocolError::Cbor(error.to_string()))?;
    let Value::Map(mut entries) = value else {
        return Err(ProtocolError::NotAMap);
    };
    for (key, value) in &mut entries {
        if key.as_text() == Some("targetId") {
            *value = Value::Text(target.into());
        }
    }
    let mut out = Vec::new();
    ciborium::into_writer(&Value::Map(entries), &mut out)
        .map_err(|error| ProtocolError::Cbor(error.to_string()))?;
    Ok(out)
}

fn text(value: impl AsRef<str>) -> Value {
    Value::Text(value.as_ref().to_owned())
}

fn metadata_value(metadata: &PeerMetadata) -> Value {
    let mut map = Vec::new();
    if let Some(storage_id) = &metadata.storage_id {
        map.push((text("storageId"), text(storage_id)));
    }
    if let Some(ephemeral) = metadata.is_ephemeral {
        map.push((text("isEphemeral"), Value::Bool(ephemeral)));
    }
    Value::Map(map)
}

struct Fields(Vec<(Value, Value)>);

impl Fields {
    fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(candidate, _)| candidate.as_text() == Some(key))
            .map(|(_, value)| value)
            .filter(|value| !value.is_null())
    }
    fn text(&self, key: &'static str) -> Result<String, ProtocolError> {
        self.optional_text(key)
            .ok_or(ProtocolError::MissingField(key))
    }
    fn optional_text(&self, key: &str) -> Option<String> {
        self.get(key).and_then(Value::as_text).map(str::to_owned)
    }
    fn text_list(&self, key: &str) -> Vec<String> {
        self.get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_text)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }
    fn unsigned(&self, key: &str) -> Option<u64> {
        self.get(key)
            .and_then(Value::as_integer)
            .and_then(|value| u64::try_from(value).ok())
    }
    fn bytes(&self, key: &'static str) -> Result<Vec<u8>, ProtocolError> {
        match self.get(key) {
            Some(Value::Bytes(bytes)) => Ok(bytes.clone()),
            // cbor-x tags typed arrays when `tagUint8Array` is left on.
            Some(Value::Tag(64, inner)) => match inner.as_ref() {
                Value::Bytes(bytes) => Ok(bytes.clone()),
                _ => Err(ProtocolError::InvalidField(key)),
            },
            Some(_) => Err(ProtocolError::InvalidField(key)),
            None => Err(ProtocolError::MissingField(key)),
        }
    }
    fn document_id(&self) -> Result<DocumentId, ProtocolError> {
        let value = self.text("documentId")?;
        DocumentId::from_bs58check(&value).map_err(|_| ProtocolError::InvalidField("documentId"))
    }
    fn metadata(&self, key: &str) -> PeerMetadata {
        let Some(Value::Map(entries)) = self.get(key) else {
            return PeerMetadata::default();
        };
        let fields = Fields(entries.clone());
        PeerMetadata {
            storage_id: fields.optional_text("storageId"),
            is_ephemeral: fields.get("isEphemeral").and_then(Value::as_bool),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&value[index..index + 2], 16).unwrap())
            .collect()
    }

    fn doc() -> DocumentId {
        DocumentId::from_bs58check("2BjHNJQh2prqecCoP3d2NdgawRSP").unwrap()
    }

    fn all_messages() -> Vec<WireMessage> {
        vec![
            WireMessage::Join {
                sender_id: "a".into(),
                peer_metadata: PeerMetadata {
                    storage_id: Some("s".into()),
                    is_ephemeral: Some(false),
                },
                supported_protocol_versions: vec!["1".into()],
            },
            WireMessage::Peer {
                sender_id: "b".into(),
                target_id: "a".into(),
                selected_protocol_version: "1".into(),
                peer_metadata: PeerMetadata::default(),
            },
            WireMessage::Error {
                sender_id: "b".into(),
                target_id: Some("a".into()),
                message: "unsupported protocol version".into(),
            },
            WireMessage::Sync {
                sender_id: "a".into(),
                target_id: "b".into(),
                document_id: doc(),
                data: vec![1, 2, 3],
            },
            WireMessage::Request {
                sender_id: "a".into(),
                target_id: "b".into(),
                document_id: doc(),
                data: vec![4],
            },
            WireMessage::DocUnavailable {
                sender_id: "b".into(),
                target_id: "a".into(),
                document_id: doc(),
            },
            WireMessage::Ephemeral {
                sender_id: "a".into(),
                target_id: "b".into(),
                document_id: doc(),
                count: 7,
                session_id: "xyz".into(),
                data: vec![9, 9],
            },
        ]
    }

    #[test]
    fn every_message_type_round_trips() {
        for message in all_messages() {
            let encoded = message.encode().unwrap();
            assert_eq!(WireMessage::decode(&encoded).unwrap(), message);
        }
    }

    #[test]
    fn decodes_cbor_x_fixture_with_wide_map_header_and_undefined() {
        // `cbor.encode` from @automerge/automerge-repo 2.5.6.
        let sync = hex(
            "b9000564747970656473796e636873656e6465724964616168746172676574496461626a646f63756d656e744964781c32426a484e4a5168327072716563436f503364324e64676177525350646461746143010203",
        );
        assert_eq!(
            WireMessage::decode(&sync).unwrap(),
            WireMessage::Sync {
                sender_id: "a".into(),
                target_id: "b".into(),
                document_id: doc(),
                data: vec![1, 2, 3],
            }
        );
        let join = hex(
            "b900046474797065646a6f696e6873656e6465724964626a736c706565724d65746164617461b900026973746f726167654964f76b6973457068656d6572616cf47819737570706f7274656450726f746f636f6c56657273696f6e73816131",
        );
        assert_eq!(
            WireMessage::decode(&join).unwrap(),
            WireMessage::Join {
                sender_id: "js".into(),
                peer_metadata: PeerMetadata {
                    storage_id: None,
                    is_ephemeral: Some(false),
                },
                supported_protocol_versions: vec!["1".into()],
            }
        );
    }

    #[test]
    fn decodes_join_from_js_with_undefined_storage_id() {
        let value = Value::Map(vec![
            (text("type"), text("join")),
            (text("senderId"), text("js-peer")),
            (
                text("peerMetadata"),
                Value::Map(vec![(text("isEphemeral"), Value::Bool(true))]),
            ),
            (
                text("supportedProtocolVersions"),
                Value::Array(vec![text("1")]),
            ),
        ]);
        let mut frame = Vec::new();
        ciborium::into_writer(&value, &mut frame).unwrap();
        let WireMessage::Join {
            sender_id,
            peer_metadata,
            supported_protocol_versions,
        } = WireMessage::decode(&frame).unwrap()
        else {
            panic!("expected join");
        };
        assert_eq!(sender_id, "js-peer");
        assert_eq!(peer_metadata.is_ephemeral, Some(true));
        assert_eq!(supported_protocol_versions, vec!["1".to_string()]);
    }

    #[test]
    fn tagged_byte_arrays_and_legacy_unavailable_are_accepted() {
        let value = Value::Map(vec![
            (text("type"), text("sync")),
            (text("senderId"), text("a")),
            (text("targetId"), text("b")),
            (text("documentId"), text(doc().to_bs58check())),
            (
                text("data"),
                Value::Tag(64, Box::new(Value::Bytes(vec![5, 6]))),
            ),
        ]);
        let mut frame = Vec::new();
        ciborium::into_writer(&value, &mut frame).unwrap();
        assert!(matches!(
            WireMessage::decode(&frame).unwrap(),
            WireMessage::Sync { data, .. } if data == vec![5, 6]
        ));
        let legacy = Value::Map(vec![
            (text("type"), text("unavailable")),
            (text("senderId"), text("a")),
            (text("targetId"), text("b")),
            (text("documentId"), text(doc().to_bs58check())),
        ]);
        let mut frame = Vec::new();
        ciborium::into_writer(&legacy, &mut frame).unwrap();
        assert!(matches!(
            WireMessage::decode(&frame).unwrap(),
            WireMessage::DocUnavailable { .. }
        ));
    }

    #[test]
    fn unknown_types_are_tolerated_and_garbage_is_rejected() {
        let value = Value::Map(vec![
            (text("type"), text("remote-heads-changed")),
            (text("senderId"), text("a")),
        ]);
        let mut frame = Vec::new();
        ciborium::into_writer(&value, &mut frame).unwrap();
        assert_eq!(
            WireMessage::decode(&frame).unwrap(),
            WireMessage::Other {
                message_type: "remote-heads-changed".into(),
                sender_id: Some("a".into())
            }
        );
        assert!(WireMessage::decode(&[0xff, 0x00]).is_err());
        let mut frame = Vec::new();
        ciborium::into_writer(&Value::Integer(3.into()), &mut frame).unwrap();
        assert_eq!(
            WireMessage::decode(&frame).unwrap_err(),
            ProtocolError::NotAMap
        );
    }

    #[test]
    fn retarget_rewrites_only_target() {
        let message = all_messages().remove(3);
        let frame = retarget(&message.encode().unwrap(), "wire-id").unwrap();
        let WireMessage::Sync {
            target_id,
            sender_id,
            ..
        } = WireMessage::decode(&frame).unwrap()
        else {
            panic!("expected sync")
        };
        assert_eq!(target_id, "wire-id");
        assert_eq!(sender_id, "a");
    }
}
