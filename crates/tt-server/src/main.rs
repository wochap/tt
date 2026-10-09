//! `tt-server`: serve, and administer accounts on the server host.

use std::{
    io::IsTerminal,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use tt_server::{ServerOptions, UserRef, admin, admin::AdminError, serve};

#[derive(Parser)]
#[command(name = "tt-server", version, about = "tt sync server")]
struct Cli {
    /// SQLite database holding the root, tokens, and documents
    #[arg(long, global = true, env = "TT_SERVER_DB", default_value = "server.db")]
    db: PathBuf,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Serve the API, the sync websocket, the admin socket, and the web bundle
    Serve(ServeArgs),
    /// Create this server's root (the registry document); once per database
    Init {
        /// Human-friendly server name [default: the host name]
        #[arg(long)]
        name: Option<String>,
    },
    /// Manage accounts
    #[command(subcommand)]
    User(UserCmd),
    /// Manage login tokens
    #[command(subcommand)]
    Token(TokenCmd),
}

#[derive(clap::Args)]
struct ServeArgs {
    /// Address to listen on
    #[arg(long, default_value = "0.0.0.0:8443")]
    listen: SocketAddr,
    /// Built web bundle (directory with index.html), served at /
    #[arg(long)]
    web_dir: Option<PathBuf>,
    /// TLS certificate chain (PEM)
    #[arg(long)]
    tls_cert: Option<PathBuf>,
    /// TLS private key (PEM)
    #[arg(long)]
    tls_key: Option<PathBuf>,
    /// Serve plain HTTP; only behind a TLS-terminating reverse proxy on this host
    #[arg(long)]
    insecure_http: bool,
    /// Trust X-Forwarded-For / X-Real-IP for the client address
    #[arg(long)]
    behind_proxy: bool,
    /// Seconds before an idle document is flushed and dropped from memory
    #[arg(long, default_value_t = 600)]
    idle_evict_secs: u64,
}

#[derive(Subcommand)]
enum UserCmd {
    /// Create an account (prompts for the password; reads one line from a non-terminal stdin)
    Add { name: String },
    /// Change a password
    Passwd { name: String },
    /// Rename an account; use --id for a conflicted account sharing a name
    #[command(
        override_usage = "tt-server user rename <NAME> <NEW_NAME>\n       tt-server user rename --id <ID> <NEW_NAME>"
    )]
    Rename {
        /// Current name (the account owning it) and the new name; only the
        /// new name with --id
        #[arg(required = true, num_args = 1..=2, value_name = "NAME")]
        names: Vec<String>,
        /// Select the account by id (for a conflicted account sharing a name)
        #[arg(long)]
        id: Option<String>,
    },
    /// Delete an account: revokes its tokens and closes its sessions; documents are kept
    Del { name: String },
    /// List accounts, including conflicted and deleted ones
    Ls,
}

#[derive(Subcommand)]
enum TokenCmd {
    /// List tokens
    Ls,
    /// Revoke a token by id (from `token ls`), or every token of --user
    Revoke {
        #[arg(required_unless_present = "user")]
        id: Option<String>,
        #[arg(long, conflicts_with = "id")]
        user: Option<String>,
    },
}

fn time(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "?".into())
}

fn read_password(confirm: bool) -> Result<String> {
    if std::io::stdin().is_terminal() {
        let mut prompt = dialoguer::Password::new().with_prompt("password");
        if confirm {
            prompt = prompt.with_confirmation("repeat password", "passwords do not match");
        }
        Ok(prompt.interact()?)
    } else {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok(line.trim_end_matches(['\n', '\r']).to_owned())
    }
}

/// 4 for a duplicate account or a second `init`, 3 without a root, 2 for
/// unknown names/ids and invalid input, 1 otherwise (including databases of
/// an older format).
fn exit_code(error: &anyhow::Error) -> u8 {
    error
        .downcast_ref::<AdminError>()
        .map_or(1, AdminError::exit_code)
}

async fn user(db: &Path, command: UserCmd) -> Result<()> {
    match command {
        UserCmd::Add { name } => {
            let password = read_password(true)?;
            let user = admin::add_user(db, &name, &password).await?;
            println!(
                "created {} ({})\nindex document: {}",
                user.name, user.id, user.index_doc
            );
        }
        UserCmd::Passwd { name } => {
            let password = read_password(true)?;
            admin::set_password(db, &UserRef::Name(name.clone()), &password).await?;
            println!(
                "password changed for {name}; existing tokens stay valid (revoke with `token revoke --user {name}`)"
            );
        }
        UserCmd::Rename { mut names, id } => {
            let (target, new_name) = match (id, names.len()) {
                (Some(id), 1) => (UserRef::Id(id), names.remove(0)),
                (None, 2) => (UserRef::Name(names.remove(0)), names.remove(0)),
                _ => {
                    return Err(AdminError::Invalid(
                        "give <name> <new-name>, or --id <id> <new-name>".into(),
                    )
                    .into());
                }
            };
            let user = admin::rename_user(db, &target, &new_name).await?;
            println!("renamed {} ({}) to {}", target, user.id, user.name);
        }
        UserCmd::Del { name } => {
            let user = admin::delete_user(db, &UserRef::Name(name)).await?;
            println!(
                "deleted {} ({}); its tokens are revoked and its documents kept",
                user.name, user.id
            );
        }
        UserCmd::Ls => {
            for user in admin::list_users(db).await? {
                let state = match (user.deleted, user.conflicted) {
                    (Some(at), _) => format!("deleted {}", time(at)),
                    (None, true) => "conflicted (rename it)".into(),
                    (None, false) => "active".into(),
                };
                println!(
                    "{:<20} {}  created {}  {:<22} index {}",
                    user.name,
                    user.id,
                    time(user.created),
                    state,
                    user.index_doc
                );
            }
        }
    }
    Ok(())
}

async fn token(db: &Path, command: TokenCmd) -> Result<()> {
    match command {
        TokenCmd::Ls => {
            for token in admin::list_tokens(db).await? {
                let state = match token.revoked {
                    Some(at) => format!("revoked {}", time(at)),
                    None => "live".into(),
                };
                let used = token.last_used.map_or_else(|| "never".into(), time);
                println!(
                    "{}  {:<20} created {}  last used {}  {}",
                    token.id(),
                    token.user_name,
                    time(token.created),
                    used,
                    state
                );
            }
        }
        TokenCmd::Revoke { id: Some(id), .. } => {
            let token = admin::revoke_token(db, &id).await?;
            println!("revoked {} ({})", token.id(), token.user_name);
        }
        TokenCmd::Revoke {
            user: Some(name), ..
        } => {
            let count = admin::revoke_user_tokens(db, &name).await?;
            println!("revoked {count} token(s) of {name}");
        }
        TokenCmd::Revoke { .. } => unreachable!("clap requires an id or --user"),
    }
    Ok(())
}

fn init_logging() {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_env("TT_SERVER_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("tt-server: {error}");
            return ExitCode::from(1);
        }
    };
    let result = runtime.block_on(async {
        match cli.command {
            Cmd::Serve(args) => {
                let transport =
                    match serve::transport(args.tls_cert, args.tls_key, args.insecure_http) {
                        Ok(transport) => transport,
                        Err(message) => {
                            eprintln!("tt-server: {message}");
                            return Ok(Some(1));
                        }
                    };
                init_logging();
                let mut options = ServerOptions::new(&cli.db);
                options.web_dir = args.web_dir;
                options.behind_proxy = args.behind_proxy;
                options.idle_eviction = Duration::from_secs(args.idle_evict_secs.max(1));
                let listener = std::net::TcpListener::bind(args.listen)
                    .map_err(|error| anyhow::anyhow!("binding {}: {error}", args.listen))?;
                serve::run(options, listener, transport)
                    .await
                    .map(|()| None)
            }
            Cmd::Init { name } => {
                let root = admin::init(&cli.db, name.as_deref()).await?;
                println!(
                    "initialized {} (server id {})\nregistry document: {}",
                    root.name, root.server_id, root.registry_doc
                );
                Ok(None)
            }
            Cmd::User(command) => user(&cli.db, command).await.map(|()| None),
            Cmd::Token(command) => token(&cli.db, command).await.map(|()| None),
        }
    });
    match result {
        Ok(None) => ExitCode::SUCCESS,
        Ok(Some(code)) => ExitCode::from(code),
        Err(error) => {
            eprintln!("tt-server: {error:#}");
            ExitCode::from(exit_code(&error))
        }
    }
}
