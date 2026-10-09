//! End-to-end: real `tt` binary, real daemon, temp XDG dirs.
//!
//! Covers the brief's flows: quick create, fuzzy start, concurrent start/stop,
//! rename propagation, time edits (flags, table, editor), task editor
//! round-trip, reports, rounded export, hooks, the watch stream, exit codes,
//! import round trip, and (with node available) two daemons syncing through
//! the automerge-repo sync server: fresh-device join, remote stop events, and
//! seq collision repair.

#[path = "../../automerge_repo/tests/support/node.rs"]
#[allow(dead_code)]
mod node;

mod support;

use std::{
    path::Path,
    time::{Duration, Instant},
};

use serde_json::Value;
use support::*;

#[test]
fn local_flows_from_the_brief() {
    let env = Env::new();

    // Auto spawn on first command; daemon stays alive.
    assert_eq!(env.json(&["task", "ls"]), Value::Array(vec![]));
    assert!(env.socket().exists());

    // Hooks: one per event, a directory hook, and a failing all.d hook.
    let log = env.path("hook.log");
    env.script(
        "config/tt/hooks/entry.started",
        &format!(
            "printf '%s %s %s ' \"$TT_EVENT\" \"$TT_ORIGIN\" \"$TT_SEQ\" >> {log}; cat >> {log}",
            log = log.display()
        ),
    );
    env.script("config/tt/hooks/all.d/fail", "exit 3");
    env.script("config/tt/hooks/all.d/slow", "sleep 30");

    // Quick create.
    let task = env.json(&[
        "task",
        "add",
        "Fix login PROJ-123",
        "+backend",
        "+urgent",
        "@web",
        "ticket:PROJ-123",
    ]);
    assert_eq!(task["seq"], 1);
    assert_eq!(task["title"], "Fix login PROJ-123");
    assert_eq!(task["project"]["name"], "web");
    assert_eq!(task["metadata"]["ticket"], "PROJ-123");
    let tags: Vec<&str> = task["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(tags, vec!["backend", "urgent"]);
    env.ok(&["task", "add", "Write docs", "+work"]);
    env.ok(&["task", "add", "Review", "+work"]);

    // Not found: empty stdout, one stderr line, exit 2.
    let (code, stdout, stderr) = env.code(&["task", "show", "9999", "-j"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert_eq!(stderr.lines().count(), 1);

    // Fuzzy find and fuzzy start by ticket id.
    let found = env.json(&["task", "find", "proj123"]);
    assert_eq!(found[0]["seq"], 1);
    let watch = env.watch(&["--tag", "work"]);
    let snapshot = watch.next(Duration::from_secs(10)).expect("snapshot line");
    assert_eq!(snapshot["type"], "snapshot");
    let started = env.json(&["start", "proj123", "--at", "-25m"]);
    assert_eq!(started["matched_by"], "search");
    assert_eq!(started["entry"]["task"]["seq"], 1);

    // Hook fired with env and payload; failing/slow hooks do not block writes.
    wait_for("entry.started hook", Duration::from_secs(10), || {
        std::fs::read_to_string(&log).is_ok_and(|text| text.contains("entry.started local"))
    });
    let hook_text = std::fs::read_to_string(&log).unwrap();
    let payload = &hook_text[hook_text.find('{').unwrap()..];
    let payload: Value = serde_json::from_str(payload.trim()).unwrap();
    assert_eq!(payload["entry"]["task"]["metadata"]["ticket"], "PROJ-123");
    assert_eq!(payload["running"].as_array().unwrap().len(), 1);

    // Tag filter: the task without `work` printed nothing; #2 with `work` does.
    let quick = Instant::now();
    env.ok(&["start", "#2", "--at", "-5m"]);
    assert!(
        quick.elapsed() < Duration::from_secs(5),
        "hooks must not block"
    );
    let event = watch.expect("entry.started");
    assert_eq!(event["task"]["seq"], 2);
    assert_eq!(event["origin"], "local");
    assert_eq!(event["running"].as_array().unwrap().len(), 2);

    // Ambiguous stop lists both and changes nothing.
    let (code, _, stderr) = env.code(&["stop"]);
    assert_eq!(code, 4);
    assert!(stderr.contains("Fix login") && stderr.contains("Write docs"));
    assert_eq!(
        env.json(&["status"])["workspace"]["running"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // Stop all: same stop time, 25 and 5 minutes.
    let stopped = env.json(&["stop", "--all"]);
    let mut durations: Vec<i64> = stopped
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["duration"].as_i64().unwrap() + 30) / 60)
        .collect();
    durations.sort_unstable();
    assert_eq!(durations, vec![5, 25]);
    assert_eq!(stopped[0]["end"], stopped[1]["end"]);
    watch.expect("entry.stopped");
    drop(watch);

    // Rename propagates to entries.
    env.ok(&["task", "mod", "1", "--title", "Fix login flow"]);
    let entries = env.json(&["time", "ls", "today"]);
    assert!(titles_of(&entries).contains(&"Fix login flow".to_owned()));

    // Edit time with flags: invalid range rejected (exit 4) and unchanged.
    let entry_id = entries
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["task"]["seq"] == 1)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let short = &entry_id[..8];
    let (code, _, _) = env.code(&["time", "mod", short, "--end", "2000-01-01T00:00:00Z"]);
    assert_eq!(code, 4);
    let entry = env.json(&["time", "mod", short, "--note", "pairing"]);
    assert_eq!(entry["note"], "pairing");

    // Split a running entry; move; remove.
    let running = env.json(&["start", "3", "--at", "-30m"]);
    let running_id = running["entry"]["id"].as_str().unwrap()[..8].to_owned();
    let parts = env.json(&["time", "split", &running_id, "--at", "-10m"]);
    assert!(parts[0]["end"].is_string());
    assert!(parts[1]["end"].is_null());
    let second = parts[1]["id"].as_str().unwrap()[..8].to_owned();
    let moved = env.json(&["time", "move", &second, "--task", "2"]);
    assert_eq!(moved["task"]["seq"], 2);
    env.ok(&["stop", &second]);

    // Bulk table edit: deleting a line deletes the entry.
    env.json(&["time", "rm", &running_id]);
    let editor = env.script("edit-table.sh", "sed -i '/pairing/d' \"$1\"");
    let mut env = env;
    env.editor = Some(editor);
    let result = env.json(&["time", "edit", "today"]);
    assert_eq!(result["deleted"], 1);
    let entries = env.json(&["time", "ls", "today"]);
    assert!(
        entries
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["note"] != "pairing")
    );

    // Task editor round trip, and strict parse keeping the temp file.
    env.editor = Some(env.script(
        "edit-task.sh",
        "sed -i 's/^title: .*/title: Edited title/' \"$1\"; printf 'More details\\n' >> \"$1\"",
    ));
    let edited = env.json(&["task", "edit", "2"]);
    assert_eq!(edited["title"], "Edited title");
    assert_eq!(edited["description"], "More details");
    env.editor = Some(env.script(
        "break-task.sh",
        "printf -- '---\\ntitle: [x\\n---\\n' > \"$1\"",
    ));
    let (code, _, stderr) = env.code(&["task", "edit", "2"]);
    assert_eq!(code, 4);
    let kept = stderr.split("kept at ").nth(1).unwrap().trim();
    assert!(Path::new(kept).exists());
    std::fs::remove_file(kept).unwrap();
    env.editor = None;

    // Report: summed exceeds wall by the overlap.
    let report = env.json(&["report", "today"]);
    assert!(report["summed"].as_i64().unwrap() >= report["wall"].as_i64().unwrap());

    // Rounded export, the brief's example on a fresh task.
    env.ok(&["task", "add", "Billing"]);
    let day = "2026-01-05";
    let billing = env.json(&["task", "find", "Billing"])[0]["seq"].to_string();
    env.json(&["start", &billing, "--at", &format!("{day}T13:00")]);
    env.json(&["stop", &billing, "--at", &format!("{day}T13:05")]);
    env.json(&["start", &billing, "--at", &format!("{day}T13:10")]);
    env.json(&["stop", &billing, "--at", &format!("{day}T13:45")]);
    let rounded: Value = serde_json::from_str(&env.ok(&[
        "export", day, "--round", "15m", "--mode", "up", "--group", "entry",
    ]))
    .unwrap();
    let spans: Vec<(String, String)> = rounded["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["start"].as_str().unwrap()[11..16].to_owned(),
                i["end"].as_str().unwrap()[11..16].to_owned(),
            )
        })
        .collect();
    assert_eq!(
        spans,
        vec![
            ("13:00".into(), "13:15".into()),
            ("13:15".into(), "14:00".into())
        ]
    );
    let per_day: Value =
        serde_json::from_str(&env.ok(&["export", day, "--round", "15m", "--group", "task-day"]))
            .unwrap();
    assert_eq!(per_day["items"].as_array().unwrap().len(), 1);
    assert_eq!(per_day["items"][0]["duration"], 45 * 60);
    let csv = env.ok(&["export", day, "--format", "csv"]);
    assert!(csv.starts_with("id,task_seq,task_title"));
    assert_eq!(csv.lines().count(), 3);

    // Done task keeps its number; the next task gets the next one.
    env.ok(&["task", "done", "1"]);
    let next = env.json(&["task", "add", "Next"]);
    assert_eq!(next["seq"], 5);
    assert_eq!(env.json(&["task", "show", "#1"])["state"], "done");

    // Second daemon refuses with exit 4.
    let (code, _, stderr) = env.code(&["daemon"]);
    assert_eq!(code, 4);
    assert!(stderr.contains("pid"));

    // Watch replay with --since.
    let watch = env.watch(&["--since", "1"]);
    let first = watch.next(Duration::from_secs(10)).unwrap();
    assert_eq!(first["type"], "snapshot");
    let second = watch.next(Duration::from_secs(10)).unwrap();
    assert_eq!(second["seq"], 2);
    drop(watch);

    // JSON export → import into an empty daemon → identical export.
    let exported = env.ok(&["export"]);
    let file = env.path("export.json");
    std::fs::write(&file, &exported).unwrap();
    let other = Env::new();
    let result = other.json(&["import", file.to_str().unwrap()]);
    assert!(result["entries"].as_u64().unwrap() > 0);
    let reexported = other.ok(&["export"]);
    let normalize = |text: &str| {
        let mut value: Value = serde_json::from_str(text).unwrap();
        value["exported_at"] = Value::Null;
        for key in ["projects", "tags", "tasks", "entries"] {
            value[key]
                .as_array_mut()
                .unwrap()
                .sort_by_key(|item| item["id"].as_str().unwrap().to_owned());
        }
        value
    };
    assert_eq!(normalize(&reexported), normalize(&exported));
}

#[test]
fn two_daemons_sync_through_node_server() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let Some(server) = runtime.block_on(node::SyncServer::start()) else {
        return;
    };
    let url = format!("http://127.0.0.1:{}", server.port);

    let a = Env::new();
    a.set("server.url", &url);
    a.set("server.token", "test-token");
    a.ok(&["task", "add", "Shared task", "+work"]);
    a.json(&["start", "1", "--at", "-10m"]);
    wait_for("A to sync", Duration::from_secs(15), || {
        sync_progress_done(&a)
    });
    let config = std::fs::read_to_string(a.path("config/tt/config.toml")).unwrap();
    let index = config
        .lines()
        .find_map(|line| line.strip_prefix("index_doc = "))
        .unwrap()
        .trim_matches('"')
        .to_owned();

    // Fresh device: index id in config, empty store.
    let b = Env::new();
    b.set("server.url", &url);
    b.set("server.token", "test-token");
    b.set("user.index_doc", &index);
    let tasks = b.json(&["task", "ls"]);
    assert_eq!(tasks[0]["title"], "Shared task");
    assert_eq!(
        b.json(&["status"])["workspace"]["running"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Remote stop: B stops, A emits entry.stopped with origin remote.
    let watch = a.watch(&["--event", "entry.*"]);
    assert_eq!(
        watch.next(Duration::from_secs(10)).unwrap()["type"],
        "snapshot"
    );
    b.json(&["stop"]);
    let event = watch.expect("entry.stopped");
    assert_eq!(event["origin"], "remote");
    assert_eq!(event["task"]["title"], "Shared task");
    assert!(event["running"].as_array().unwrap().is_empty());
    drop(watch);

    // Offline seq collision: B goes offline, both create #2, B reconnects.
    b.set("server.url", "http://127.0.0.1:9");
    let offline = b.json(&["task", "add", "Offline on B"]);
    assert_eq!(offline["seq"], 2);
    std::thread::sleep(Duration::from_millis(50));
    let online = a.json(&["task", "add", "Online on A"]);
    assert_eq!(online["seq"], 2);
    wait_for("A to sync #2", Duration::from_secs(15), || {
        sync_progress_done(&a)
    });
    let log = a.path("renumbered.log");
    a.script(
        "config/tt/hooks/task.renumbered",
        &format!(
            "printf '%s ' \"$TT_EVENT\" >> {log}; cat >> {log}",
            log = log.display()
        ),
    );
    let watch = a.watch(&["--event", "task.*"]);
    assert_eq!(
        watch.next(Duration::from_secs(10)).unwrap()["type"],
        "snapshot"
    );
    b.set("server.url", &url);
    let unique = |env: &Env| {
        let tasks = env.json(&["task", "ls", "--state", "all"]);
        let mut seqs: Vec<u64> = tasks
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["seq"].as_u64().unwrap())
            .collect();
        let count = seqs.len();
        seqs.sort_unstable();
        seqs.dedup();
        count == 3 && seqs == vec![1, 2, 3]
    };
    wait_for(
        "seq repair on both devices",
        Duration::from_secs(20),
        || unique(&a) && unique(&b),
    );
    let tasks = a.json(&["task", "ls"]);
    let by_title = |title: &str| {
        tasks
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["title"] == title)
            .unwrap()["seq"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(by_title("Offline on B"), 2, "earlier-created task keeps #2");
    assert_eq!(by_title("Online on A"), 3);

    // A emits task.renumbered (not task.updated) and runs its hook.
    let event = watch.expect("task.renumbered");
    assert_eq!(
        (event["from"].clone(), event["to"].clone()),
        (2.into(), 3.into())
    );
    assert_eq!(event["task"]["title"], "Online on A");
    assert_eq!(event["task"]["previous_seqs"], serde_json::json!([2]));
    drop(watch);
    wait_for("task.renumbered hook", Duration::from_secs(10), || {
        std::fs::read_to_string(&log).is_ok_and(|text| text.starts_with("task.renumbered {"))
    });

    // The old number shows its current holder, with a hint to the moved task.
    let shown = a.json(&["task", "show", "2"]);
    assert_eq!(shown["title"], "Offline on B");
    assert_eq!(shown["renumbered_from"][0]["title"], "Online on A");
    assert_eq!(shown["renumbered_from"][0]["to"], 3);
    let text = a.ok(&["task", "show", "2"]);
    assert!(
        text.contains("note: \"Online on A\" was renumbered from #2 to #3"),
        "{text}"
    );
    assert!(a.ok(&["task", "show", "3"]).contains("previously: #2"));
    drop(server);
}
