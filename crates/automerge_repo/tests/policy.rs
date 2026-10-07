//! `AccessPolicy` enforcement, observed from a raw peer that speaks the
//! automerge-repo JS protocol directly over a manual `MemoryTransport`.

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use automerge::{Automerge, ROOT, sync::SyncDoc, transaction::Transactable};
use automerge_repo::{
    DocumentId, Error, FnPolicy, PeerId, Repo, RepoConfig,
    network::{NetworkEvent, NetworkTransport},
    protocol::WireMessage,
    testing::{MemoryStore, MemoryTransport},
};
use bytes::Bytes;
use tokio::sync::mpsc;

fn put(
    tx: &mut automerge::transaction::Transaction<'_>,
    key: &str,
    value: i64,
) -> automerge_repo::Result<()> {
    tx.put(ROOT, key, value)
        .map_err(|error| Error::Change(error.to_string()))
}

struct Harness {
    repo: Repo,
    net_repo: Arc<MemoryTransport>,
    raw: Arc<MemoryTransport>,
    raw_events: mpsc::Receiver<NetworkEvent>,
    denied: Arc<Mutex<HashSet<DocumentId>>>,
    deny_all: Arc<AtomicBool>,
}

impl Harness {
    async fn new() -> Self {
        let (net_repo, raw) = MemoryTransport::pair("repo", "raw", 256);
        let raw_events = raw.take_events().unwrap();
        let denied = Arc::new(Mutex::new(HashSet::<DocumentId>::new()));
        let policy_denied = denied.clone();
        let deny_all = Arc::new(AtomicBool::new(false));
        let policy_deny_all = deny_all.clone();
        let repo = Repo::open_with_policy(
            Arc::new(MemoryStore::default()),
            Arc::new(MemoryStore::default()),
            net_repo.clone(),
            RepoConfig::default(),
            Arc::new(FnPolicy(move |peer: &PeerId, document: DocumentId| {
                peer.as_str() != "raw"
                    || !(policy_deny_all.load(Ordering::SeqCst)
                        || policy_denied.lock().unwrap().contains(&document))
            })),
        )
        .await
        .unwrap();
        Self {
            repo,
            net_repo,
            raw,
            raw_events,
            denied,
            deny_all,
        }
    }

    fn deny(&self, document: DocumentId) {
        self.denied.lock().unwrap().insert(document);
    }

    /// Delivers everything both ways and returns the decoded frames the
    /// repository sent to the raw peer, once the link is quiescent.
    async fn exchange(&mut self) -> Vec<WireMessage> {
        let mut frames = Vec::new();
        let mut idle = 0;
        for _ in 0..1_000 {
            tokio::task::yield_now().await;
            let delivered = self.net_repo.deliver_all().await + self.raw.deliver_all().await;
            let mut received = 0;
            while let Ok(event) = self.raw_events.try_recv() {
                if let NetworkEvent::Message { bytes, .. } = event {
                    frames.push(WireMessage::decode(&bytes).unwrap());
                }
                received += 1;
            }
            if delivered + received == 0 {
                idle += 1;
                if idle >= 12 {
                    return frames;
                }
            } else {
                idle = 0;
            }
        }
        panic!("link did not become quiescent");
    }

    async fn send(&self, message: WireMessage) {
        self.raw
            .send(
                &PeerId::from("repo"),
                Bytes::from(message.encode().unwrap()),
            )
            .await
            .unwrap();
    }
}

fn empty_request_payload() -> Vec<u8> {
    let mut state = automerge::sync::State::new();
    Automerge::new()
        .generate_sync_message(&mut state)
        .expect("a first sync message always exists")
        .encode()
}

fn sync_payload_with_content() -> Vec<u8> {
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        tx.put(ROOT, "smuggled", 1)?;
        Ok(())
    })
    .unwrap();
    let mut state = automerge::sync::State::new();
    let first = doc.generate_sync_message(&mut state).unwrap();
    // The first message only advertises heads; pretend the repository
    // answered "I have nothing" so the next message carries the change.
    let mut peer = Automerge::new();
    let mut peer_state = automerge::sync::State::new();
    peer.receive_sync_message(&mut peer_state, first).unwrap();
    let reply = peer.generate_sync_message(&mut peer_state).unwrap();
    doc.receive_sync_message(&mut state, reply).unwrap();
    doc.generate_sync_message(&mut state).unwrap().encode()
}

fn mentions(frames: &[WireMessage], document: DocumentId) -> Vec<&WireMessage> {
    frames
        .iter()
        .filter(|frame| match frame {
            WireMessage::Sync { document_id, .. }
            | WireMessage::Request { document_id, .. }
            | WireMessage::DocUnavailable { document_id, .. }
            | WireMessage::Ephemeral { document_id, .. } => *document_id == document,
            _ => false,
        })
        .collect()
}

fn is_unavailable_for(frame: &WireMessage, document: DocumentId) -> bool {
    matches!(
        frame,
        WireMessage::DocUnavailable { document_id, sender_id, target_id }
            if *document_id == document && sender_id == "repo" && target_id == "raw"
    )
}

#[tokio::test]
async fn denied_documents_are_not_announced_but_allowed_ones_are() {
    let mut harness = Harness::new().await;
    let allowed = harness
        .repo
        .create_with(|tx| put(tx, "public", 1))
        .await
        .unwrap();
    let denied = harness
        .repo
        .create_with(|tx| put(tx, "secret", 2))
        .await
        .unwrap();
    harness.deny(denied.id());
    harness.net_repo.connect().await;
    let frames = harness.exchange().await;
    assert!(
        mentions(&frames, allowed.id()).iter().any(
            |frame| matches!(frame, WireMessage::Sync { target_id, .. } if target_id == "raw")
        ),
        "allowed document is announced with a sync frame: {frames:?}"
    );
    assert!(
        mentions(&frames, denied.id()).is_empty(),
        "denied document must never be mentioned: {frames:?}"
    );

    // A document created while connected is subject to the same check
    // before it is pushed live.
    harness.deny_all.store(true, Ordering::SeqCst);
    let late = harness.repo.create().await.unwrap();
    harness.deny(late.id());
    harness.deny_all.store(false, Ordering::SeqCst);
    let late_allowed = harness.repo.create().await.unwrap();
    let frames = harness.exchange().await;
    assert!(mentions(&frames, late.id()).is_empty(), "{frames:?}");
    assert!(
        !mentions(&frames, late_allowed.id()).is_empty(),
        "{frames:?}"
    );
}

#[tokio::test]
async fn request_for_a_denied_document_is_answered_with_doc_unavailable() {
    let mut harness = Harness::new().await;
    let denied = harness
        .repo
        .create_with(|tx| put(tx, "secret", 2))
        .await
        .unwrap();
    harness.deny(denied.id());
    harness.net_repo.connect().await;
    harness.exchange().await;

    harness
        .send(WireMessage::Request {
            sender_id: "raw".into(),
            target_id: "repo".into(),
            document_id: denied.id(),
            data: empty_request_payload(),
        })
        .await;
    let frames = harness.exchange().await;
    let about = mentions(&frames, denied.id());
    assert_eq!(about.len(), 1, "{frames:?}");
    assert!(is_unavailable_for(about[0], denied.id()), "{frames:?}");
}

#[tokio::test]
async fn sync_for_a_denied_document_is_answered_with_doc_unavailable_and_not_applied() {
    let mut harness = Harness::new().await;
    let denied = harness
        .repo
        .create_with(|tx| put(tx, "secret", 2))
        .await
        .unwrap();
    harness.deny(denied.id());
    harness.net_repo.connect().await;
    harness.exchange().await;
    let heads_before = denied.read(|doc| doc.get_heads()).await.unwrap();

    for data in [empty_request_payload(), sync_payload_with_content()] {
        harness
            .send(WireMessage::Sync {
                sender_id: "raw".into(),
                target_id: "repo".into(),
                document_id: denied.id(),
                data,
            })
            .await;
        let frames = harness.exchange().await;
        let about = mentions(&frames, denied.id());
        assert_eq!(about.len(), 1, "{frames:?}");
        assert!(is_unavailable_for(about[0], denied.id()), "{frames:?}");
    }
    assert_eq!(
        denied.read(|doc| doc.get_heads()).await.unwrap(),
        heads_before
    );

    // A denied document the repository does not hold is not created either.
    let unknown = DocumentId::new();
    harness.deny(unknown);
    harness
        .send(WireMessage::Sync {
            sender_id: "raw".into(),
            target_id: "repo".into(),
            document_id: unknown,
            data: sync_payload_with_content(),
        })
        .await;
    let frames = harness.exchange().await;
    assert!(is_unavailable_for(mentions(&frames, unknown)[0], unknown));
    assert!(harness.repo.get(unknown).await.unwrap().is_none());
}

#[tokio::test]
async fn request_for_an_allowed_document_is_served() {
    let mut harness = Harness::new().await;
    let allowed = harness
        .repo
        .create_with(|tx| put(tx, "public", 1))
        .await
        .unwrap();
    harness.net_repo.connect().await;
    harness.exchange().await;
    harness
        .send(WireMessage::Request {
            sender_id: "raw".into(),
            target_id: "repo".into(),
            document_id: allowed.id(),
            data: empty_request_payload(),
        })
        .await;
    let frames = harness.exchange().await;
    assert!(
        mentions(&frames, allowed.id())
            .iter()
            .all(|frame| matches!(frame, WireMessage::Sync { .. })),
        "{frames:?}"
    );
    assert!(!mentions(&frames, allowed.id()).is_empty(), "{frames:?}");
}

#[tokio::test]
async fn find_never_requests_a_denied_document_from_a_peer() {
    let mut harness = Harness::new().await;
    harness.net_repo.connect().await;
    harness.exchange().await;
    let denied = DocumentId::new();
    harness.deny(denied);
    let handle = harness.repo.find(denied).await.unwrap();
    let frames = harness.exchange().await;
    assert!(mentions(&frames, denied).is_empty(), "{frames:?}");
    assert_eq!(handle.status(), automerge_repo::DocumentStatus::Unavailable);

    let allowed = DocumentId::new();
    let handle = harness.repo.find(allowed).await.unwrap();
    let frames = harness.exchange().await;
    assert!(
        mentions(&frames, allowed)
            .iter()
            .any(|frame| matches!(frame, WireMessage::Request { .. })),
        "{frames:?}"
    );
    harness
        .send(WireMessage::DocUnavailable {
            sender_id: "raw".into(),
            target_id: "repo".into(),
            document_id: allowed,
        })
        .await;
    harness.exchange().await;
    assert_eq!(handle.status(), automerge_repo::DocumentStatus::Unavailable);
}
