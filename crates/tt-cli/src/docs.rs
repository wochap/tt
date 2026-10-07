//! Markdown CLI reference generated from the clap definitions
//! (`tt __docs-cli > docs/cli.md`).

use std::fmt::Write;

fn walk(command: &mut clap::Command, path: &str, out: &mut String) {
    let name = if path.is_empty() {
        command.get_name().to_owned()
    } else {
        format!("{path} {}", command.get_name())
    };
    let help = command.render_long_help().to_string();
    let _ = writeln!(out, "## `{name}`\n\n```text\n{}\n```\n", help.trim_end());
    let children: Vec<String> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        .map(|sub| sub.get_name().to_owned())
        .collect();
    for child in children {
        if let Some(sub) = command.find_subcommand_mut(&child) {
            let mut sub = sub.clone();
            walk(&mut sub, &name, out);
        }
    }
}

#[must_use]
pub fn cli_markdown() -> String {
    let mut out = String::from(
        "# tt command line reference\n\n\
         Generated from the clap definitions with `tt __docs-cli > docs/cli.md`; do not edit by hand.\n\n\
         Exit codes: 0 ok, 1 usage, 2 not found, 3 daemon unreachable, 4 invalid state or conflict.\n\
         Every read command accepts `-j` for JSON (uuids, seqs, UTC timestamps, durations in seconds).\n\n",
    );
    let mut command = crate::command();
    command.build();
    walk(&mut command, "", &mut out);
    out
}
