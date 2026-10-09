//! `tt` — task and time tracker.

mod client;
mod docs;
mod output;

use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use chrono::Utc;
use chrono_tz::Tz;
use clap::{Args, CommandFactory, Parser, Subcommand};
use serde_json::{Value, json};
use tt_core::{ops::TaskPatch, text, time};
use tt_daemon::{
    config::Config,
    paths::Paths,
    rpc::{Client, RpcError},
};

/// An error with a CLI exit code.
#[derive(Debug)]
pub struct Failure {
    pub code: i32,
    pub message: String,
    pub detail: Option<String>,
}

impl Failure {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }
    fn usage(message: impl Into<String>) -> Self {
        Self::new(1, message)
    }
    fn conflict(message: impl Into<String>) -> Self {
        Self::new(4, message)
    }
    pub fn rpc(error: RpcError) -> Self {
        Self {
            code: error.exit_code(),
            message: error.message.clone(),
            detail: error.data.map(|data| data.to_string()),
        }
    }
}

impl From<tt_core::CoreError> for Failure {
    fn from(error: tt_core::CoreError) -> Self {
        // Client-side parse errors are usage errors.
        let code = match error {
            tt_core::CoreError::Invalid(_) => 1,
            ref other => other.exit_code(),
        };
        Self::new(code, error.to_string())
    }
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        Self::new(1, format!("{error:#}"))
    }
}

type CliResult<T = ()> = Result<T, Failure>;

#[derive(Parser)]
#[command(
    name = "tt",
    version,
    about = "Task and time tracker: offline-first, concurrent timers, hooks and an event stream",
    propagate_version = true
)]
pub struct Cli {
    /// JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)
    #[arg(short = 'j', long = "json", global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage tasks
    #[command(subcommand)]
    Task(TaskCmd),
    /// Manage projects
    #[command(subcommand)]
    Project(NamedCmd),
    /// Manage tags
    #[command(subcommand)]
    Tag(NamedCmd),
    /// Start a timer: by #seq/uuid, fuzzy text, or (on a TTY, no argument) a picker
    Start {
        /// Task id (#12, 12, uuid) or text to fuzzy-match against open tasks
        task: Vec<String>,
        /// Start time (13:00, -15m, yesterday 9:00, ISO); default now
        #[arg(long, allow_hyphen_values = true)]
        at: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    /// Stop the running entry; with several running pass an entry or task id, or --all
    Stop {
        /// Entry id (prefix) or task id
        target: Option<String>,
        #[arg(long)]
        all: bool,
        /// Stop time; default now
        #[arg(long, allow_hyphen_values = true)]
        at: Option<String>,
    },
    /// Time entries
    #[command(subcommand)]
    Time(TimeCmd),
    /// Summaries with summed and wall-clock totals
    Report {
        /// today|yesterday|week|lastweek|month|lastmonth|year|all|YYYY-MM-DD|<from>..<to>
        #[arg(default_value = "week")]
        range: String,
        /// task|tag|project|day
        #[arg(long, default_value = "task")]
        by: String,
    },
    /// Export data as JSON or CSV, optionally rounded and flattened
    Export {
        /// Range (default: everything)
        range: Option<String>,
        /// json|csv
        #[arg(long, default_value = "json")]
        format: String,
        /// Rounding grid, e.g. 15m (default: config `snap` when --mode/--group given)
        #[arg(long)]
        round: Option<String>,
        /// up|nearest
        #[arg(long)]
        mode: Option<String>,
        /// entry|task-day
        #[arg(long)]
        group: Option<String>,
        /// Write to a file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Import a JSON export, matching records by uuid
    Import { file: PathBuf },
    /// Stream events as NDJSON (first line: snapshot of running entries)
    Watch(WatchArgs),
    /// Daemon, sync and running entries
    ///
    /// Sync states: offline, connecting, connected, disconnected, failed,
    /// login_required, closed, and ca_cert_invalid (server.ca_cert cannot be
    /// read or holds no certificate; nothing syncs until it is fixed).
    Status,
    /// Run the daemon in the foreground
    Daemon {
        #[arg(long, hide = true)]
        spawned: bool,
    },
    /// Log in to a tt server and enable sync
    Login {
        /// Server base URL, e.g. https://tt.example.com
        url: String,
        #[arg(long)]
        username: Option<String>,
        /// PEM file with an extra CA to trust (private or local CA); its
        /// absolute path is stored as server.ca_cert
        #[arg(long, value_name = "PEM")]
        ca_cert: Option<std::path::PathBuf>,
    },
    /// Revoke the server token and stop syncing; local data stays
    Logout,
    /// Read or change config.toml
    #[command(subcommand)]
    Config(ConfigCmd),
    #[command(name = "__docs-cli", hide = true)]
    DocsCli,
}

#[derive(Subcommand)]
enum TaskCmd {
    /// Create: tt task add "Title" +tag @project key:value
    Add {
        #[arg(required = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// List tasks (default: open)
    Ls {
        /// open|done|archived|all
        #[arg(long)]
        state: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    /// Show one task with its entries
    ///
    /// A short id shows the task holding it now; tasks renumbered away from it
    /// after a sync collision are noted (JSON: `renumbered_from`).
    Show { task: String },
    /// Edit in $EDITOR as markdown with YAML frontmatter
    Edit { task: String },
    /// Modify fields
    Mod {
        task: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long = "add-tag")]
        add_tag: Vec<String>,
        #[arg(long = "rm-tag")]
        rm_tag: Vec<String>,
        #[arg(long, conflicts_with = "no_project")]
        project: Option<String>,
        #[arg(long = "no-project")]
        no_project: bool,
        /// key=value (repeatable)
        #[arg(long = "meta")]
        meta: Vec<String>,
        #[arg(long = "rm-meta")]
        rm_meta: Vec<String>,
        /// open|done|archived
        #[arg(long)]
        state: Option<String>,
    },
    /// Mark done
    Done { task: String },
    /// Delete (refuses when entries exist unless --force)
    Rm {
        task: String,
        #[arg(long)]
        force: bool,
    },
    /// Fuzzy search title, tags and metadata values
    Find {
        #[arg(required = true)]
        query: Vec<String>,
        #[arg(long)]
        state: Option<String>,
    },
}

#[derive(Subcommand)]
enum NamedCmd {
    Add {
        name: String,
        #[arg(long)]
        color: Option<String>,
    },
    Ls,
    /// Rename, recolor, (un)archive
    Mod {
        name: String,
        #[arg(long = "name")]
        new_name: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long)]
        archive: bool,
        #[arg(long, conflicts_with = "archive")]
        unarchive: bool,
    },
    Rm {
        name: String,
    },
}

#[derive(Subcommand)]
enum TimeCmd {
    /// List entries (default range: today)
    Ls {
        #[arg(default_value = "today")]
        range: String,
    },
    /// Change start, end, task or note
    Mod {
        entry: String,
        #[arg(long, allow_hyphen_values = true)]
        start: Option<String>,
        /// End time, or "running"
        #[arg(long, allow_hyphen_values = true)]
        end: Option<String>,
        #[arg(long)]
        task: Option<String>,
        /// Note (empty string clears)
        #[arg(long)]
        note: Option<String>,
    },
    /// Move to another task
    Move {
        entry: String,
        #[arg(long)]
        task: String,
    },
    /// Split into two adjacent entries
    Split {
        entry: String,
        #[arg(long, allow_hyphen_values = true)]
        at: String,
    },
    /// Delete
    Rm { entry: String },
    /// Edit entries as a table in $EDITOR (default range: today)
    Edit {
        #[arg(default_value = "today")]
        range: String,
    },
}

#[derive(Args)]
struct WatchArgs {
    /// Event type, e.g. entry.started or entry.* (repeatable)
    #[arg(long = "event")]
    events: Vec<String>,
    #[arg(long = "task")]
    tasks: Vec<String>,
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long = "project")]
    projects: Vec<String>,
    /// Replay events after this seq
    #[arg(long)]
    since: Option<u64>,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Print one key, or the whole file
    Get { key: Option<String> },
    /// Set a key; omit the value to unset
    ///
    /// Keys: server.url, server.token, server.ca_cert, user.index_doc, user.id,
    /// user.name, week_start, snap, tz, editor. server.ca_cert takes a PEM file
    /// with an extra CA to trust; it is stored as an absolute path and must
    /// hold at least one certificate.
    Set { key: String, value: Option<String> },
    /// Print the config file path
    Path,
}

struct Ctx {
    json: bool,
    paths: Paths,
    config: Config,
    tz: Tz,
    client: Option<Client>,
}

impl Ctx {
    fn call(&mut self, method: &str, params: Value) -> CliResult<Value> {
        if self.client.is_none() {
            self.client = Some(client::connect(&self.paths)?);
        }
        let client = self.client.as_mut().expect("connected");
        client::call(client, &self.paths, method, params)
    }

    fn now(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }

    fn time(&self, input: &str) -> CliResult<String> {
        Ok(time::parse_time(input, self.now(), self.tz)?.to_rfc3339())
    }

    fn range(&self, input: &str) -> CliResult<time::Range> {
        Ok(time::parse_range(
            input,
            self.now(),
            self.tz,
            self.config.week_start(),
        )?)
    }

    fn print_json(&self, value: &Value) {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    }

    fn emit(&self, value: &Value, human: impl FnOnce(&Value) -> String) {
        if self.json {
            self.print_json(value);
        } else {
            let text = human(value);
            if !text.is_empty() {
                print!("{text}");
                if !text.ends_with('\n') {
                    println!();
                }
            }
        }
    }

    fn editor(&self) -> String {
        self.config
            .editor
            .clone()
            .or_else(|| std::env::var("EDITOR").ok())
            .or_else(|| std::env::var("VISUAL").ok())
            .filter(|editor| !editor.trim().is_empty())
            .unwrap_or_else(|| "vi".into())
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(if error.use_stderr() { 1 } else { 0 });
        }
    };
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(failure) => {
            eprintln!("tt: {}", failure.message);
            if let Some(detail) = failure.detail {
                eprintln!("{detail}");
            }
            ExitCode::from(u8::try_from(failure.code).unwrap_or(1))
        }
    }
}

fn run(cli: Cli) -> CliResult<u8> {
    let paths = Paths::from_env();
    if let Cmd::Daemon { spawned } = cli.command {
        let runtime = tokio::runtime::Runtime::new().map_err(|e| Failure::new(1, e.to_string()))?;
        let code = runtime.block_on(tt_daemon::run(spawned))?;
        return Ok(u8::try_from(code).unwrap_or(1));
    }
    if let Cmd::DocsCli = cli.command {
        print!("{}", docs::cli_markdown());
        return Ok(0);
    }
    let config = Config::load(&paths.config_file())?;
    let tz = config.time_zone();
    let mut ctx = Ctx {
        json: cli.json,
        paths,
        config,
        tz,
        client: None,
    };
    match cli.command {
        Cmd::Task(command) => task(&mut ctx, command)?,
        Cmd::Project(command) => named(&mut ctx, "project", command)?,
        Cmd::Tag(command) => named(&mut ctx, "tag", command)?,
        Cmd::Start { task, at, note } => start(&mut ctx, &task, at, note)?,
        Cmd::Stop { target, all, at } => stop(&mut ctx, target, all, at)?,
        Cmd::Time(command) => time_cmd(&mut ctx, command)?,
        Cmd::Report { range, by } => {
            let range = ctx.range(&range)?;
            let tz = ctx.tz;
            let report = ctx.call(
                "report",
                json!({"from": range.from, "to": range.to, "by": by, "tz": tz.name()}),
            )?;
            ctx.emit(&report, |r| output::report(r, tz));
        }
        Cmd::Export {
            range,
            format,
            round,
            mode,
            group,
            output,
        } => export(&mut ctx, range, format, round, mode, group, output)?,
        Cmd::Import { file } => {
            let text = std::fs::read_to_string(&file)
                .map_err(|e| Failure::usage(format!("{}: {e}", file.display())))?;
            let data: Value = serde_json::from_str(&text)
                .map_err(|e| Failure::usage(format!("{}: {e}", file.display())))?;
            let result = ctx.call("import", json!({"data": data}))?;
            ctx.emit(&result, |r| {
                format!(
                    "imported {} tasks, {} entries, {} projects, {} tags ({} unchanged)\n",
                    r["tasks"], r["entries"], r["projects"], r["tags"], r["unchanged"]
                )
            });
        }
        Cmd::Watch(args) => return watch(&mut ctx, args),
        Cmd::Status => {
            let status = ctx.call("status", json!({}))?;
            let tz = ctx.tz;
            ctx.emit(&status, |s| output::status(s, tz));
        }
        Cmd::Login {
            url,
            username,
            ca_cert,
        } => login(&mut ctx, &url, username, ca_cert.as_deref())?,
        Cmd::Logout => logout(&mut ctx)?,
        Cmd::Config(command) => config_cmd(&mut ctx, command)?,
        Cmd::Daemon { .. } | Cmd::DocsCli => unreachable!("handled above"),
    }
    Ok(0)
}

fn task(ctx: &mut Ctx, command: TaskCmd) -> CliResult {
    let tz = ctx.tz;
    match command {
        TaskCmd::Add { args } => {
            let input = text::parse_quick(&args);
            if input.title.trim().is_empty() {
                return Err(Failure::usage("task title must not be empty"));
            }
            let task = ctx.call("task.add", json!(input))?;
            ctx.emit(&task, |t| format!("created {}\n", output::task_label(t)));
        }
        TaskCmd::Ls {
            state,
            tag,
            project,
        } => {
            let tasks = ctx.call(
                "task.list",
                json!({"state": state, "tag": tag, "project": project}),
            )?;
            ctx.emit(&tasks, |tasks| {
                tasks
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|t| output::task_line(t) + "\n")
                    .collect()
            });
        }
        TaskCmd::Show { task } => {
            let task = ctx.call("task.show", json!({"task": task}))?;
            ctx.emit(&task, |t| output::task_detail(t, tz));
        }
        TaskCmd::Edit { task } => edit_task(ctx, &task)?,
        TaskCmd::Mod {
            task,
            title,
            description,
            add_tag,
            rm_tag,
            project,
            no_project,
            meta,
            rm_meta,
            state,
        } => {
            let mut set_metadata = std::collections::BTreeMap::new();
            for pair in &meta {
                let (key, value) = text::metadata_pair(pair).ok_or_else(|| {
                    Failure::usage(format!("--meta expects key=value, got {pair:?}"))
                })?;
                set_metadata.insert(key, value);
            }
            let patch = TaskPatch {
                title,
                description,
                add_tags: add_tag,
                remove_tags: rm_tag,
                project: if no_project {
                    Some(None)
                } else {
                    project.map(Some)
                },
                set_metadata,
                remove_metadata: rm_meta,
                state: state.as_deref().map(str::parse).transpose()?,
                ..TaskPatch::default()
            };
            let task = ctx.call("task.modify", json!({"task": task, "patch": patch}))?;
            ctx.emit(&task, |t| format!("updated {}\n", output::task_line(t)));
        }
        TaskCmd::Done { task } => {
            let task = ctx.call("task.done", json!({"task": task}))?;
            ctx.emit(&task, |t| format!("done {}\n", output::task_label(t)));
        }
        TaskCmd::Rm { task, force } => {
            let task = ctx.call("task.remove", json!({"task": task, "force": force}))?;
            ctx.emit(&task, |t| format!("removed {}\n", output::task_label(t)));
        }
        TaskCmd::Find { query, state } => {
            let tasks = ctx.call(
                "task.find",
                json!({"query": query.join(" "), "state": state}),
            )?;
            ctx.emit(&tasks, |tasks| {
                tasks
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|t| output::task_line(t) + "\n")
                    .collect()
            });
        }
    }
    Ok(())
}

fn named(ctx: &mut Ctx, kind: &str, command: NamedCmd) -> CliResult {
    match command {
        NamedCmd::Add { name, color } => {
            let value = ctx.call(
                &format!("{kind}.add"),
                json!({"name": name, "color": color}),
            )?;
            ctx.emit(&value, |v| {
                format!("created {kind} {}\n", output::named_line(v))
            });
        }
        NamedCmd::Ls => {
            let value = ctx.call(&format!("{kind}.list"), json!({}))?;
            ctx.emit(&value, |items| {
                items
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| output::named_line(v) + "\n")
                    .collect()
            });
        }
        NamedCmd::Mod {
            name,
            new_name,
            color,
            archive,
            unarchive,
        } => {
            if kind == "tag" && (archive || unarchive) {
                return Err(Failure::usage("tags cannot be archived"));
            }
            let archived = if archive {
                Some(true)
            } else if unarchive {
                Some(false)
            } else {
                None
            };
            let color = color.map(|c| Some(c).filter(|c| !c.is_empty()));
            let value = ctx.call(
                &format!("{kind}.modify"),
                json!({"key": name, "patch": {"name": new_name, "color": color, "archived": archived}}),
            )?;
            ctx.emit(&value, |v| {
                format!("updated {kind} {}\n", output::named_line(v))
            });
        }
        NamedCmd::Rm { name } => {
            let value = ctx.call(&format!("{kind}.remove"), json!({"key": name}))?;
            ctx.emit(&value, |v| {
                format!("removed {kind} {}\n", output::named_line(v))
            });
        }
    }
    Ok(())
}

fn pick_task(ctx: &mut Ctx) -> CliResult<String> {
    let tasks = ctx.call("task.list", json!({"state": "open"}))?;
    let tasks = tasks.as_array().cloned().unwrap_or_default();
    if tasks.is_empty() {
        return Err(Failure::conflict(
            "no open tasks; create one with tt task add",
        ));
    }
    let labels: Vec<String> = tasks
        .iter()
        .map(|task| {
            let mut label = output::task_label(task);
            if let Some(metadata) = task["metadata"].as_object() {
                for value in metadata.values() {
                    label.push_str(&format!("  {}", value.as_str().unwrap_or_default()));
                }
            }
            label
        })
        .collect();
    let choice = dialoguer::FuzzySelect::new()
        .with_prompt("start task")
        .items(&labels)
        .default(0)
        .interact_opt()
        .map_err(|e| Failure::usage(e.to_string()))?;
    let Some(index) = choice else {
        return Err(Failure::usage("cancelled"));
    };
    Ok(tasks[index]["id"].as_str().unwrap_or_default().to_owned())
}

fn start(ctx: &mut Ctx, task: &[String], at: Option<String>, note: Option<String>) -> CliResult {
    let key = if task.is_empty() {
        if ctx.json || !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return Err(Failure::usage(
                "no task given; pass a task id or text (the picker needs a terminal)",
            ));
        }
        pick_task(ctx)?
    } else {
        task.join(" ")
    };
    let at = at.map(|at| ctx.time(&at)).transpose()?;
    let result = ctx.call("entry.start", json!({"task": key, "at": at, "note": note}))?;
    let tz = ctx.tz;
    ctx.emit(&result, |r| {
        let entry = &r["entry"];
        format!(
            "started {} {} at {}\n",
            entry["id"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(8)
                .collect::<String>(),
            output::task_label(&entry["task"]),
            output::time(tz, &entry["start"])
        )
    });
    Ok(())
}

fn stop(ctx: &mut Ctx, target: Option<String>, all: bool, at: Option<String>) -> CliResult {
    let at = at.map(|at| ctx.time(&at)).transpose()?;
    let tz = ctx.tz;
    let result = match ctx.call(
        "entry.stop",
        json!({"target": target, "all": all, "at": at}),
    ) {
        Ok(result) => result,
        Err(mut failure) => {
            // Ambiguous stop: list what is running instead of raw JSON.
            if let Some(detail) = &failure.detail
                && let Ok(data) = serde_json::from_str::<Value>(detail)
                && let Some(running) = data["running"].as_array()
            {
                failure.detail = Some(
                    running
                        .iter()
                        .map(|e| format!("  {}", output::entry_line(e, tz)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            return Err(failure);
        }
    };
    ctx.emit(&result, |entries| {
        entries
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| format!("stopped {}\n", output::entry_line(e, tz)))
            .collect()
    });
    Ok(())
}

fn time_cmd(ctx: &mut Ctx, command: TimeCmd) -> CliResult {
    let tz = ctx.tz;
    let print_entry = |e: &Value| output::entry_line(e, tz) + "\n";
    match command {
        TimeCmd::Ls { range } => {
            let range = ctx.range(&range)?;
            let entries = ctx.call("entry.list", json!(range))?;
            ctx.emit(&entries, |entries| {
                entries
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(print_entry)
                    .collect()
            });
        }
        TimeCmd::Mod {
            entry,
            start,
            end,
            task,
            note,
        } => {
            let start = start.map(|s| ctx.time(&s)).transpose()?;
            let end = match end.as_deref() {
                Some("running" | "-") => Some("running".to_owned()),
                Some(end) => Some(ctx.time(end)?),
                None => None,
            };
            let value = ctx.call(
                "entry.modify",
                json!({"entry": entry, "start": start, "end": end, "task": task, "note": note}),
            )?;
            ctx.emit(&value, print_entry);
        }
        TimeCmd::Move { entry, task } => {
            let value = ctx.call("entry.move", json!({"entry": entry, "task": task}))?;
            ctx.emit(&value, print_entry);
        }
        TimeCmd::Split { entry, at } => {
            let at = ctx.time(&at)?;
            let value = ctx.call("entry.split", json!({"entry": entry, "at": at}))?;
            ctx.emit(&value, |parts| {
                parts
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(print_entry)
                    .collect()
            });
        }
        TimeCmd::Rm { entry } => {
            let value = ctx.call("entry.remove", json!({"entry": entry}))?;
            ctx.emit(&value, |e| format!("removed {}", print_entry(e)));
        }
        TimeCmd::Edit { range } => edit_table(ctx, &range)?,
    }
    Ok(())
}

/// Runs the editor on `path`; `$EDITOR` may contain arguments.
fn run_editor(ctx: &Ctx, path: &Path) -> CliResult {
    let editor = ctx.editor();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\""))
        .arg("tt-editor")
        .arg(path)
        .status()
        .map_err(|e| Failure::usage(format!("cannot run editor {editor:?}: {e}")))?;
    if !status.success() {
        return Err(Failure::conflict(format!(
            "editor exited with {status}; edited file kept at {}",
            path.display()
        )));
    }
    Ok(())
}

fn temp_file(name: &str, content: &str) -> CliResult<PathBuf> {
    let path = std::env::temp_dir().join(format!("tt-{}-{name}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|e| Failure::new(1, format!("{}: {e}", path.display())))?;
    file.write_all(content.as_bytes())
        .map_err(|e| Failure::new(1, e.to_string()))?;
    Ok(path)
}

fn edit_task(ctx: &mut Ctx, key: &str) -> CliResult {
    let task = ctx.call("task.show", json!({"task": key}))?;
    let view: tt_core::TaskView = serde_json::from_value(task.clone())
        .map_err(|e| Failure::new(4, format!("unexpected task shape: {e}")))?;
    let original = text::render_task_file(&view);
    let path = temp_file(&format!("task-{}.md", view.seq), &original)?;
    run_editor(ctx, &path)?;
    let edited = std::fs::read_to_string(&path).map_err(|e| Failure::new(4, e.to_string()))?;
    if edited == original {
        let _ = std::fs::remove_file(&path);
        if !ctx.json {
            println!("no changes");
        } else {
            ctx.print_json(&task);
        }
        return Ok(());
    }
    let patch = match text::parse_task_file(&edited) {
        Ok(patch) => patch,
        Err(error) => {
            return Err(Failure::conflict(format!(
                "{error}; nothing was changed, edited file kept at {}",
                path.display()
            )));
        }
    };
    let id = view.id.to_string();
    match ctx.call("task.modify", json!({"task": id, "patch": patch})) {
        Ok(updated) => {
            let _ = std::fs::remove_file(&path);
            ctx.emit(&updated, |t| format!("updated {}\n", output::task_line(t)));
            Ok(())
        }
        Err(mut failure) => {
            failure.message = format!(
                "{}; edited file kept at {}",
                failure.message,
                path.display()
            );
            Err(failure)
        }
    }
}

fn edit_table(ctx: &mut Ctx, range: &str) -> CliResult {
    let range = ctx.range(range)?;
    let tz = ctx.tz;
    let table = ctx.call(
        "entry.table",
        json!({"from": range.from, "to": range.to, "tz": tz.name()}),
    )?;
    let original = table["text"].as_str().unwrap_or_default().to_owned();
    let path = temp_file("time.txt", &original)?;
    run_editor(ctx, &path)?;
    let edited = std::fs::read_to_string(&path).map_err(|e| Failure::new(4, e.to_string()))?;
    if edited == original {
        let _ = std::fs::remove_file(&path);
        ctx.emit(&json!({"created": 0, "updated": 0, "deleted": 0}), |_| {
            "no changes\n".into()
        });
        return Ok(());
    }
    match ctx.call(
        "entry.apply_table",
        json!({"text": edited, "ids": table["ids"], "tz": tz.name()}),
    ) {
        Ok(result) => {
            let _ = std::fs::remove_file(&path);
            ctx.emit(&result, |r| {
                format!(
                    "{} created, {} updated, {} deleted\n",
                    r["created"], r["updated"], r["deleted"]
                )
            });
            Ok(())
        }
        Err(mut failure) => {
            failure.message = format!(
                "{}; nothing was changed, edited file kept at {}",
                failure.message,
                path.display()
            );
            Err(failure)
        }
    }
}

fn export(
    ctx: &mut Ctx,
    range: Option<String>,
    format: String,
    round: Option<String>,
    mode: Option<String>,
    group: Option<String>,
    output_path: Option<PathBuf>,
) -> CliResult {
    let range = range.map(|r| ctx.range(&r)).transpose()?;
    let grid = round
        .or_else(|| {
            (mode.is_some() || group.is_some())
                .then(|| ctx.config.snap.clone())
                .flatten()
        })
        .map(|g| time::parse_duration(&g))
        .transpose()?;
    if grid.is_none() && (mode.is_some() || group.is_some()) {
        return Err(Failure::usage(
            "--mode/--group need --round (or config snap)",
        ));
    }
    let round =
        grid.map(|grid| json!({"grid_seconds": grid.num_seconds(), "mode": mode, "group": group}));
    let result = ctx.call(
        "export",
        json!({"range": range, "format": format, "round": round, "tz": ctx.tz.name()}),
    )?;
    let content = match result["format"].as_str() {
        Some("csv") => result["text"].as_str().unwrap_or_default().to_owned(),
        _ => serde_json::to_string_pretty(&result["data"]).unwrap_or_default() + "\n",
    };
    match output_path {
        Some(path) => std::fs::write(&path, content)
            .map_err(|e| Failure::new(1, format!("{}: {e}", path.display())))?,
        None => print!("{content}"),
    }
    Ok(())
}

fn watch(ctx: &mut Ctx, args: WatchArgs) -> CliResult<u8> {
    let mut client = client::connect(&ctx.paths)?;
    let params = json!({
        "events": args.events,
        "tasks": args.tasks,
        "tags": args.tags,
        "projects": args.projects,
        "since": args.since,
    });
    client::call(&mut client, &ctx.paths, "subscribe", params)?;
    client
        .stream_mode()
        .map_err(|e| Failure::new(3, e.to_string()))?;
    let stdout = std::io::stdout();
    loop {
        let message = match client.read_message() {
            Ok(message) => message,
            Err(error) => {
                return Err(Failure::new(3, format!("event stream ended: {error}")));
            }
        };
        if message.get("method").and_then(Value::as_str) != Some("event") {
            continue;
        }
        let mut lock = stdout.lock();
        if writeln!(lock, "{}", message["params"]).is_err() || lock.flush().is_err() {
            return Ok(0);
        }
    }
}

/// A login request error; certificate failures get a `--ca-cert` hint.
fn login_failure(error: &ureq::Error) -> Failure {
    let text = error.to_string();
    if text.contains("UnknownIssuer") {
        Failure::conflict(format!(
            "login failed: {text} (use --ca-cert for a private CA; pass the CA certificate, not the server's)"
        ))
    } else {
        Failure::conflict(format!("login failed: {text}"))
    }
}

fn login(
    ctx: &mut Ctx,
    url: &str,
    username: Option<String>,
    ca_cert: Option<&std::path::Path>,
) -> CliResult {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(Failure::usage(
            "server URL must start with http:// or https://",
        ));
    }
    let base = url.trim_end_matches('/').to_owned();
    // Validate the CA before prompting, so a bad path never sends a password.
    let path = ctx.paths.config_file();
    let (ca_cert, extra_roots) = match ca_cert {
        Some(ca_cert) => {
            let absolute = tt_daemon::config::validate_ca_cert(ca_cert)?;
            let roots = tt_daemon::tls::load_extra_roots(&absolute)?;
            (Some(absolute.display().to_string()), roots)
        }
        None => {
            let config = Config::load(&path)?;
            (config.server.ca_cert.clone(), config.extra_roots()?)
        }
    };
    let username = match username {
        Some(username) => username,
        None => {
            eprint!("username: ");
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| Failure::usage(e.to_string()))?;
            line.trim().to_owned()
        }
    };
    let password = if std::io::stdin().is_terminal() {
        dialoguer::Password::new()
            .with_prompt("password")
            .interact()
            .map_err(|e| Failure::usage(e.to_string()))?
    } else {
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| Failure::usage(e.to_string()))?;
        line.trim_end_matches(['\n', '\r']).to_owned()
    };
    let endpoint = format!("{base}/api/login");
    let agent = tt_daemon::tls::ureq_agent(&extra_roots, std::time::Duration::from_secs(30));
    let mut response = agent
        .post(&endpoint)
        .send_json(json!({"username": username, "password": password}))
        .map_err(|e| login_failure(&e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(Failure::conflict(format!(
            "login failed: http status: {}",
            status.as_u16()
        )));
    }
    let response: Value = response
        .body_mut()
        .read_json()
        .map_err(|e| Failure::conflict(format!("login response: {e}")))?;
    let token = response["token"]
        .as_str()
        .ok_or_else(|| Failure::conflict("login response has no token"))?;
    let mut config = Config::load(&path)?;
    config.server.url = Some(base);
    config.server.token = Some(token.to_owned());
    config.server.ca_cert = ca_cert;
    if let Some(index) = response["index_doc"].as_str() {
        config.user.index_doc = Some(index.to_owned());
    }
    if let Some(id) = response["user"]["id"].as_str() {
        config.user.id = Some(id.to_owned());
    }
    if let Some(name) = response["user"]["name"].as_str() {
        config.user.name = Some(name.to_owned());
    }
    config.save(&path)?;
    let result = ctx.call("sync.reload", json!({}))?;
    ctx.emit(&result, |_| "logged in; sync enabled\n".into());
    Ok(())
}

fn logout(ctx: &mut Ctx) -> CliResult {
    let path = ctx.paths.config_file();
    let mut config = Config::load(&path)?;
    let Some(token) = config.server.token.take() else {
        ctx.emit(&json!({"ok": true, "revoked": false}), |_| {
            "not logged in\n".into()
        });
        return Ok(());
    };
    let mut revoked = false;
    if let Some(base) = config.server.url.as_deref() {
        // An unreadable CA still lets the token be dropped locally.
        let extra_roots = config.extra_roots().unwrap_or_else(|error| {
            eprintln!("tt: warning: {error:#}");
            Vec::new()
        });
        let agent = tt_daemon::tls::ureq_agent(&extra_roots, std::time::Duration::from_secs(10));
        let endpoint = format!("{}/api/logout", base.trim_end_matches('/'));
        match agent
            .post(&endpoint)
            .header("Authorization", &format!("Bearer {token}"))
            .send_json(json!({}))
        {
            // 401: already revoked or unknown; either way it is dead.
            Ok(response) if matches!(response.status().as_u16(), 200 | 401) => revoked = true,
            Ok(response) => eprintln!(
                "tt: warning: server answered {} to logout; token removed locally only",
                response.status()
            ),
            Err(error) => {
                eprintln!("tt: warning: server not reached ({error}); token removed locally only");
            }
        }
    }
    config.save(&path)?;
    // Only a running daemon needs to hear about it; never spawn for this.
    if let Ok(mut client) = Client::connect(&ctx.paths.socket) {
        let _ = client.call("sync.reload", json!({}));
    }
    ctx.emit(&json!({"ok": true, "revoked": revoked}), |_| {
        "logged out; sync stopped, local data kept\n".into()
    });
    Ok(())
}

fn config_cmd(ctx: &mut Ctx, command: ConfigCmd) -> CliResult {
    let path = ctx.paths.config_file();
    match command {
        ConfigCmd::Path => println!("{}", path.display()),
        ConfigCmd::Get { key: None } => {
            if ctx.json {
                ctx.print_json(&json!(ctx.config));
            } else {
                print!(
                    "{}",
                    toml::to_string_pretty(&ctx.config)
                        .map_err(|e| Failure::new(1, e.to_string()))?
                );
            }
        }
        ConfigCmd::Get { key: Some(key) } => match ctx.config.get(&key)? {
            Some(value) if ctx.json => ctx.print_json(&json!(value)),
            Some(value) => println!("{value}"),
            None => return Err(Failure::new(2, format!("{key} is not set"))),
        },
        ConfigCmd::Set { key, value } => {
            let mut config = Config::load(&path)?;
            config.set(&key, value)?;
            config.save(&path)?;
            if key.starts_with("server.") || key == "user.index_doc" {
                // Only reachable daemons need to hear about it; never spawn for this.
                if let Ok(mut client) = Client::connect(&ctx.paths.socket) {
                    let _ = client.call("sync.reload", json!({}));
                }
            }
        }
    }
    Ok(())
}

#[must_use]
pub fn command() -> clap::Command {
    Cli::command()
}
