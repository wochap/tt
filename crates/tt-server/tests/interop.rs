//! A stock `@automerge/automerge-repo` node client with the websocket client
//! adapter logs in over HTTP, gets a ticket, and syncs the user's workspace
//! document in both directions through tt-server.

mod common;
#[path = "../../automerge_repo/tests/support/node.rs"]
#[allow(dead_code)]
mod node;

use std::time::Duration;

use automerge::{ROOT, ReadDoc, transaction::Transactable};
use automerge_repo::{DocHandle, DocumentId, Error};
use common::{PASSWORD, TestServer, eventually};
use tokio::time::timeout;

const SCRIPT: &str = r#"
import { Repo } from "@automerge/automerge-repo";
import { WebSocketClientAdapter } from "@automerge/automerge-repo-network-websocket";

const [base, username, password] = process.argv.slice(2);
async function post(path, body, token) {
  const headers = { "content-type": "application/json" };
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(base + path, { method: "POST", headers, body: JSON.stringify(body ?? {}) });
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.json();
}
const login = await post("/api/login", { username, password });
const { ticket } = await post("/api/ws-ticket", {}, login.token);
const url = base.replace(/^http/, "ws") + "/sync?ticket=" + encodeURIComponent(ticket);
const repo = new Repo({ network: [new WebSocketClientAdapter(url)], peerId: "node-client" });
const index = await repo.find(`automerge:${login.index_doc}`);
const workspace = await repo.find(`automerge:${String(index.doc().workspace)}`);
if (String(workspace.doc().kind) !== "workspace") {
  console.error("unexpected workspace", workspace.doc());
  process.exit(2);
}
workspace.change(doc => { doc.interop = "from node"; });
const deadline = Date.now() + 15000;
while (Date.now() < deadline) {
  if (String(workspace.doc().reply ?? "") === "from rust") process.exit(0);
  await new Promise(resolve => setTimeout(resolve, 100));
}
console.error("never saw the server-side reply");
process.exit(3);
"#;

async fn text(handle: &DocHandle, key: &'static str) -> Option<String> {
    handle
        .read(move |doc| match doc.get(ROOT, key).unwrap()? {
            (automerge::Value::Object(automerge::ObjType::Text), id) => doc.text(id).ok(),
            (value, _) => value.into_string().ok(),
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn stock_node_client_syncs_the_workspace_both_ways() {
    let Some(dir) = node::install().await else {
        return;
    };
    let server = TestServer::start(|_| {}).await;
    let alice = server.add_user("alice").await;
    std::fs::write(dir.join("tt-server-client.mjs"), SCRIPT).unwrap();
    let mut child = tokio::process::Command::new("node")
        .arg("tt-server-client.mjs")
        .arg(server.base())
        .arg("alice")
        .arg(PASSWORD)
        .current_dir(&dir)
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    // Node -> server.
    let id = DocumentId::parse_any(&alice.workspace_doc).unwrap();
    let workspace = server.server().repo().open_document(id).await.unwrap();
    eventually("server receives the node client's change", 20, || async {
        text(&workspace, "interop").await.as_deref() == Some("from node")
    })
    .await;

    // Server -> node.
    workspace
        .change(|tx| {
            tx.put(ROOT, "reply", "from rust")
                .map_err(|error| Error::Change(error.to_string()))
        })
        .await
        .unwrap();
    let status = timeout(Duration::from_secs(20), child.wait())
        .await
        .expect("node client finishes within 20 s")
        .unwrap();
    assert!(status.success(), "node client failed: {status}");
}
