//! `tt-server completions` needs no database.

use std::process::Command;

#[test]
fn zsh_script_creates_no_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    let output = Command::new(env!("CARGO_BIN_EXE_tt-server"))
        .args(["--db", db.to_str().unwrap(), "completions", "zsh"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let script = String::from_utf8(output.stdout).unwrap();
    assert!(script.contains("#compdef tt-server"), "{script}");
    assert!(script.contains("_tt-server()"), "{script}");
    assert!(!db.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn hidden_from_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_tt-server"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("completions")
    );
}
