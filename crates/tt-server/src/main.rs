//! `tt-server`: serve, and administer accounts and member servers on the
//! server host.

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
use tt_server::{
    ServerOptions, UserRef, admin, admin::AdminError, identity::id_prefix, serve,
    transport::PeerState,
};

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
    /// Manage member servers: pair, list, revoke, rename
    #[command(subcommand)]
    Peer(PeerCmd),
    /// Delete the root and all data (accounts, documents, tokens, peer
    /// state), keeping the server key, so the server can join another root.
    /// The server must be stopped.
    Reset {
        /// Also replace the server key (a new server id)
        #[arg(long)]
        new_identity: bool,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum PeerCmd {
    /// Print a one-time code (valid 10 minutes) another server passes to `peer join`
    Invite {
        /// Peer address put into the code [default: the --peer-listen address]
        #[arg(long)]
        addr: Option<String>,
        /// Without a running server: accept the join on this address, then exit
        #[arg(long)]
        listen: Option<SocketAddr>,
        /// This server's name, if it adopts the joining server's root [default: the host name]
        #[arg(long)]
        name: Option<String>,
    },
    /// Pair with the server that printed CODE (the side without a root adopts the other's)
    Join {
        code: String,
        /// This server's name, if it adopts the inviter's root [default: the host name]
        #[arg(long)]
        name: Option<String>,
    },
    /// List the other member servers with their link state, last seen time and address
    Ls {
        /// List every member instead: this server and revoked ones too, with ids and when they were added
        #[arg(long)]
        all: bool,
    },
    /// Revoke a member server (by name, or id prefix when names repeat)
    Revoke { server: String },
    /// Rename a member server
    Rename { server: String, new_name: String },
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
    /// Accept links from member servers here (mutual TLS, no proxy needed)
    #[arg(long)]
    peer_listen: Option<SocketAddr>,
    /// A member server's peer address (host:port) to dial; repeatable
    #[arg(long = "peer", value_name = "HOST:PORT")]
    peers: Vec<String>,
    /// An address at which members reach --peer-listen (host:port, e.g. a
    /// Tailscale MagicDNS name), advertised before the interface addresses;
    /// repeatable
    #[arg(long = "peer-advertise", value_name = "HOST:PORT")]
    peer_advertise: Vec<String>,
    /// This server's client URL (e.g. https://laptop-b.example.ts.net), told
    /// to members for browsers; never used for peer links
    #[arg(long, value_name = "URL")]
    public_url: Option<String>,
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

/// `now`, `5m ago`, `2h ago`, `3d ago`, or `never`.
fn ago(seconds: Option<i64>, now: i64) -> String {
    let Some(seconds) = seconds else {
        return "never".into();
    };
    match (now - seconds).max(0) {
        age if age < 60 => "now".into(),
        age if age < 3600 => format!("{}m ago", age / 60),
        age if age < 86_400 => format!("{}h ago", age / 3600),
        age => format!("{}d ago", age / 86_400),
    }
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

async fn peer(db: &Path, command: PeerCmd) -> Result<()> {
    match command {
        PeerCmd::Invite { addr, listen, name } => {
            let waits = listen.is_some();
            let paired = admin::invite(db, addr.as_deref(), listen, name.as_deref(), |invite| {
                println!("{}", invite.code);
                eprintln!(
                    "valid until {} for one `tt-server peer join` against {}",
                    time(invite.expires),
                    invite.addr
                );
                if waits {
                    eprintln!("waiting for the other server…");
                }
            })
            .await?;
            if !paired {
                return Err(AdminError::Invalid("the invite code expired unused".into()).into());
            }
            if waits {
                eprintln!("paired");
            }
        }
        PeerCmd::Join { code, name } => {
            let report = admin::join(db, &code, name.as_deref()).await?;
            match report.outcome.as_str() {
                "joined" => println!(
                    "joined {}; this server now holds root {}",
                    report.inviter, report.registry_doc
                ),
                "adopted" => println!("{} adopted this server's root", report.inviter),
                _ => println!("{} and this server now list each other", report.inviter),
            }
        }
        PeerCmd::Ls { all: false } => {
            let peers = admin::peer_status(db).await?;
            if peers.is_empty() {
                println!(
                    "not paired with other servers: run `tt-server peer invite` here and `tt-server peer join <code>` on the other server"
                );
                return Ok(());
            }
            println!(
                "{:<32} {:<20} {:<10} ADDRESS",
                "SERVER", "STATE", "LAST SEEN"
            );
            let now = Utc::now().timestamp();
            for peer in peers {
                let state = match peer.state {
                    PeerState::Online => "online".to_owned(),
                    PeerState::Offline => "offline".to_owned(),
                    PeerState::Syncing => format!("syncing ({} docs)", peer.pending),
                    PeerState::Error => "error".to_owned(),
                };
                let mut line = format!(
                    "{:<32} {:<20} {:<10} {}",
                    format!("{} ({})", peer.name, id_prefix(&peer.id)),
                    state,
                    ago(peer.last_seen, now),
                    peer.address.as_deref().unwrap_or("-")
                );
                if let Some(error) = &peer.error {
                    line.push_str(&format!("  error: {error}"));
                }
                println!("{}", line.trim_end());
            }
        }
        PeerCmd::Ls { all: true } => {
            let own = admin::server_id(db).await?;
            for server in admin::list_servers(db).await? {
                let state = match &server.revoked {
                    Some(revoked) => format!("revoked {}", time(revoked.at)),
                    None if server.id == own => "this server".into(),
                    None => "member".into(),
                };
                println!(
                    "{:<32} {}  added {}  {}",
                    server.display(),
                    server.id,
                    time(server.added_at),
                    state
                );
            }
        }
        PeerCmd::Revoke { server } => {
            let entry = admin::revoke_server(db, &server).await?;
            println!(
                "revoked {}; rotate the passwords it held (`tt-server user passwd`)",
                entry.display()
            );
        }
        PeerCmd::Rename { server, new_name } => {
            let entry = admin::rename_server(db, &server, &new_name).await?;
            println!("renamed {} to {}", id_prefix(&entry.id), entry.name);
        }
    }
    Ok(())
}

fn confirm_reset(new_identity: bool, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err(AdminError::Invalid(
            "reset deletes all data; pass --yes to confirm without a terminal".into(),
        )
        .into());
    }
    let what = if new_identity {
        "Delete the root, every account, document and token, and the server key?"
    } else {
        "Delete the root, every account, document and token (the server key is kept)?"
    };
    Ok(dialoguer::Confirm::new()
        .with_prompt(what)
        .default(false)
        .interact()?)
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
                options.peer_advertise = args.peer_advertise;
                options.public_url = args.public_url;
                let listener = std::net::TcpListener::bind(args.listen)
                    .map_err(|error| anyhow::anyhow!("binding {}: {error}", args.listen))?;
                let peers = serve::Peers {
                    listener: match args.peer_listen {
                        Some(addr) => Some(std::net::TcpListener::bind(addr).map_err(|error| {
                            anyhow::anyhow!("binding --peer-listen {addr}: {error}")
                        })?),
                        None => None,
                    },
                    seeds: args.peers,
                };
                serve::run(options, listener, transport, peers)
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
            Cmd::Peer(command) => peer(&cli.db, command).await.map(|()| None),
            Cmd::Reset { new_identity, yes } => {
                if !confirm_reset(new_identity, yes)? {
                    eprintln!("nothing changed");
                    return Ok(Some(1));
                }
                admin::reset(&cli.db, new_identity).await?;
                let id = admin::server_id(&cli.db).await?;
                println!(
                    "reset; server id {id}. Run `tt-server init` or `tt-server peer join` next"
                );
                Ok(None)
            }
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
