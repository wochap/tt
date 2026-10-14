//! `tt completions` and the enumerated-argument validation behind it.

mod support;

use support::*;

#[test]
fn zsh_script_without_a_daemon() {
    let env = Env::new();
    let script = env.ok(&["completions", "zsh"]);
    assert!(script.contains("#compdef tt"), "{script}");
    assert!(script.contains("_tt()"), "{script}");
    for value in [
        "task",
        "tag",
        "project",
        "day",
        "--start",
        "--start-mode",
        "snap_start",
    ] {
        assert!(script.contains(value), "{value}");
    }
    assert!(
        !env.socket().exists(),
        "completions must not spawn the daemon"
    );
}

#[test]
fn unknown_shell_is_a_usage_error() {
    let env = Env::new();
    let (code, stdout, _) = env.code(&["completions", "tcsh"]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(!env.socket().exists());
}

#[test]
fn completions_are_hidden_from_help() {
    let env = Env::new();
    assert!(!env.ok(&["--help"]).contains("completions"));
}

#[test]
fn invalid_enumerated_values_are_usage_errors() {
    let env = Env::new();
    for args in [
        &["report", "--by", "bogus"][..],
        &["export", "--format", "xml"],
        &["export", "--mode", "down"],
        &["export", "--start-mode", "down"],
        &["export", "--group", "week"],
        &["task", "ls", "--state", "bogus"],
        &["config", "get", "nope"],
    ] {
        let (code, _, stderr) = env.code(args);
        assert_eq!(code, 1, "{args:?}");
        assert!(stderr.contains("possible values"), "{args:?}: {stderr}");
    }
    assert!(!env.socket().exists());
}

#[test]
fn old_spellings_still_parse() {
    let env = Env::new();
    for group in ["taskday", "task_day", "task-day"] {
        env.ok(&["export", "--round", "15m", "--mode", "up", "--group", group]);
    }
}
