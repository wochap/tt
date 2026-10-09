# tt-server

One binary, one SQLite file, one port. It holds every user's Automerge
documents, authenticates users with passwords and bearer tokens, relays
sync between a user's devices over the automerge-repo websocket protocol,
makes sure one user never sees another's documents, and serves the web app.

Every server has one **root**: a private Automerge *registry* document that
holds all accounts and all member servers. A fresh database has no root
(state `NeedsDecision`) until `tt-server init` creates one (state `Ready`)
or `tt-server peer join` fetches another server's (state `Joining`, then
`Ready`). Servers sharing a root replicate everything between each other
over their own peer port (see [Peering](#peering)).

## Quick start

```sh
cargo build --release -p tt-server
tt-server --db /var/lib/tt-server/server.db init --name laptop-a   # once
# accounts are created on the server host; there is no public sign-up
tt-server --db /var/lib/tt-server/server.db user add alice     # prompts for a password
tt-server --db /var/lib/tt-server/server.db serve \
    --tls-cert /etc/tt-server/fullchain.pem --tls-key /etc/tt-server/privkey.pem

# on each device
tt login https://tt.example.com --username alice
tt status          # sync: connected
```

`contrib/tt-server.service` is a hardened systemd unit for the same setup;
it runs `init` before the first start (remove that line on a server that
will `peer join` another).

### Web app

The browser client lives in `web/` (see `docs/web.md`). Build it once and
point `serve` at the bundle:

```sh
pnpm install --frozen-lockfile
pnpm build                       # writes web/dist
tt-server --db /var/lib/tt-server/server.db serve \
    --tls-cert … --tls-key … --web-dir web/dist
```

Users sign in at `https://tt.example.com/` with the same accounts as
`tt login`. After the first sign-in the app works offline (service worker +
IndexedDB) and syncs over `/sync` with the ticket flow below.

## Commands

All commands take `--db <path>` (or `TT_SERVER_DB`; default `./server.db`).
Next to the database live `server.key` (the server's ed25519 key, PKCS#8,
mode 0600; every command refuses to start, naming the file, when group or
others can access it), `server.db.lock`, and, while `serve` runs,
`admin.sock`.

| Command | Effect |
| --- | --- |
| `serve` | Serve the API, `/sync`, the web bundle, and the admin socket (flags below). Without a root it runs in limited mode (below). |
| `init [--name <name>]` | Create the root: the registry document and the `server` row. The name defaults to the host name and is shown by the API and the web app. Exit 4 if a root exists (nothing changes). |
| `user add <name>` | Prompt for a password (twice; from a non-terminal stdin: one line), store its argon2id hash in a new registry account, and create the user's index and workspace documents. Exit 4 if a non-deleted account has the name (nothing changes). Names: 1-64 of `a-z A-Z 0-9 - _ .`; passwords: at least 8 characters. |
| `user passwd <name>` | Change a password. Existing tokens stay valid; revoke them with `token revoke --user`. |
| `user rename <name> <new-name>` | Rename an account. Exit 4 if another account owns `<new-name>`. |
| `user rename --id <id> <new-name>` | Rename by account id: the way to rename a *conflicted* account that shares its name with an older one. |
| `user del <name>` | Delete an account: it becomes a tombstone (`deleted` time) in the registry, its tokens are revoked and its websockets closed; its documents are kept. The name is free again afterwards. |
| `user ls` | List accounts with id, creation time, state (`active`, `conflicted (rename it)`, `deleted <time>`) and index document. |
| `token ls` | List tokens: id (first 12 hex characters of the stored hash), user, created, last used, revoked. |
| `token revoke <id>` | Revoke one token (give at least 6 characters of its id). |
| `token revoke --user <name>` | Revoke every live token of a user. |
| `peer invite [--addr <host:port>] [--name <name>]` | Print a one-time pairing code (valid 10 minutes, single use) for another server's `peer join`. The code holds the peer address (default: the `--peer-listen` address, the host name when it binds every interface), this server's id, and a secret; only the secret's SHA-256 is stored. `--name` is the name this server takes if it adopts the joiner's root. Needs a running `serve --peer-listen`, or `--listen <addr:port>`: then the command serves the peer port itself, waits until the code is used and the pairing completes (or the code expires, exit 2), and exits. |
| `peer join <code> [--name <name>]` | Pair with the server that printed the code. Checks that the server at the code's address proves the key of the code's server id before sending anything; the side with a root writes the other's membership entry; the side without a root fetches the other's whole root and returns once it is stored (see [Root direction](#root-direction)). `--name` is this server's name if it adopts the inviter's root (default: the host name). |
| `peer ls` | List member servers as `name (id prefix)`, with the full id, when it was added, and `this server`, `member` or `revoked <time>`. |
| `peer revoke <name\|id-prefix>` | Revoke a member for good (the registry's `revoked` map is append-only); its links close at once here and on every member the change reaches. A name shared by several members is refused: give an id prefix. Revoking this server itself is refused (use `reset`). Rotate the passwords afterwards: the revoked server holds every hash. |
| `peer rename <name\|id-prefix> <new-name>` | Change a member's display name. |
| `reset [--new-identity] [--yes]` | With the server stopped: delete the root, every document and account, tokens, tickets, invite secrets and peer state, and return to `NeedsDecision`. The key (and so the server id) is kept unless `--new-identity`. Asks for confirmation on a terminal; elsewhere it needs `--yes`. A reset is recorded before anything is deleted and completes on the next start of any command if it was interrupted. |

Exit codes: 0 success, 1 failure (including refusing to serve without
TLS, a database in use by another `serve`, and a database of an older
format), 2 unknown user/token/member, invalid input, or a refused pairing,
3 no root yet (run `init`) or a join still in progress, 4 duplicate account
or a second `init`.

### Admin socket

`serve` takes an exclusive `flock` on `server.db.lock` and listens on
`admin.sock` (mode 0600, newline-delimited JSON-RPC 2.0; methods `init`,
`user.add`, `user.passwd`, `user.rename`, `user.del`, `user.ls`, `token.ls`,
`token.revoke`, `server.id`, `peer.ls`, `peer.revoke`, `peer.rename`,
`peer.invite`, `peer.join`). Every admin command first tries to take the lock: if it
gets it, no server is running and it writes the database directly; if the
lock is held, it sends the change to the running server over the socket.
So admin commands work while the server is up, take effect at once (a new
user can log in immediately, a revoked token's websockets close
immediately), and never lose a concurrent registry write. Passwords are
hashed by the command before they are sent; the socket never carries a
plaintext password. Two direct commands wait for each other on the lock.
A second `serve` on the same database exits 1 (`… is in use by another
tt-server process`).

### Limited mode (no root)

`serve` on a database without a root keeps running and logs that
`tt-server init` (create a new root) or `tt-server peer join` (join an
existing server) is needed. Its peer listener accepts pairings, so an
empty server can also `peer invite` a rooted one. It serves `GET /api/health`
(`state: "NeedsDecision"`), a "not set up" page for every non-API path, and
the admin socket; every other `/api/*` route and `/sync` answer 503 with
`{"error": "… tt-server init … tt-server peer join …", "state":
"NeedsDecision"}`. Running `tt-server init` while it serves (through the
admin socket) switches it to `Ready` without a restart.

While a join fetches a root the state is `Joining`: the same routes answer
503 with `"state": "Joining"`, until every document is stored.

### Upgrading from a registry-less version

Databases written before the registry (accounts in SQLite tables) are
refused by every command with exit code 1 and left unchanged; there is no
migration. Stop the server, move the old `server.db` aside, then:

```sh
tt-server --db server.db init --name <name>
tt-server --db server.db user add <name>      # for each user
tt-server --db server.db serve …
```

and run `tt login` again on each daemon and sign in again in the web app.

### `serve` flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--listen <addr:port>` | `0.0.0.0:8443` | Listening socket. |
| `--tls-cert <pem>` / `--tls-key <pem>` | | Terminate TLS here (rustls, TLS 1.2+1.3, ring). Both or neither. |
| `--insecure-http` | off | Serve plain HTTP. Only for a TLS-terminating reverse proxy on the same host; bind `--listen 127.0.0.1:…`. |
| `--behind-proxy` | off | Take the client address for rate limiting and the audit log from `X-Forwarded-For` (first entry) or `X-Real-IP`. Only with a proxy you control. |
| `--web-dir <dir>` | | Built web bundle. Files are served at `/`; unknown non-API paths get `index.html` (status 200) for client-side routing. A directory without `index.html` is ignored with a warning. |
| `--idle-evict-secs <n>` | `600` | A document nobody is syncing and nothing has touched for this long is flushed and dropped from memory; it reloads on the next request. |
| `--peer-listen <addr:port>` | | Accept links and pairings from other servers here. Mutual TLS 1.3 of its own: works the same next to `--tls-cert` or `--insecure-http`, and never goes through the reverse proxy. Without it the server still dials `--peer` seeds and remembered addresses but accepts no inbound links. |
| `--peer <host:port>` | | A member's peer address to keep a link to (repeatable). Addresses learned from invite codes are remembered and dialed too. |

Without TLS flags and without `--insecure-http`, `serve` exits 1 and says
which of the two to add. Logs go to stderr (`TT_SERVER_LOG`, e.g. `debug`).

### Reverse proxy

```nginx
location / {
    proxy_pass http://127.0.0.1:8080;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header X-Forwarded-For $remote_addr;
    proxy_read_timeout 1h;           # sync websockets are long-lived
    access_log off;                  # or make sure query strings are not logged
}
```

with `tt-server serve --insecure-http --behind-proxy --listen 127.0.0.1:8080`.
Websocket tickets are single-use and expire after 60 s, but there is no
reason to keep them in proxy logs.

## Peering

Servers that share a root are *members*: each lists the others in the
registry's `servers` map, and each replicates every document reachable
from the registry with every member it can reach. Typical uses: two
laptops that sync whenever they see each other on the LAN or over
Tailscale, and later a cloud server that joins them.

```sh
# laptop-a (has the root) and laptop-b (fresh), both serving
#   tt-server serve … --peer-listen 0.0.0.0:8772
laptop-a$ tt-server peer invite
tt-pair:aeaq…
laptop-b$ tt-server peer join tt-pair:aeaq…
joined laptop-a (or4xxbd5); this server now holds root 4Jd…
laptop-b$ tt-server peer ls
laptop-a (or4xxbd5)   or4xxbd5…  added 2026-10-09 10:00  member
laptop-b (pagx3bv4)   pagx3bv4…  added 2026-10-09 10:02  this server
```

Users log in to any member with the same accounts; a client keeps using
the one server it logged in to.

### Root direction

Pairing compares the two servers' roots, whichever side ran `invite`:

| Inviter | Joiner | Result |
| --- | --- | --- |
| root X | none | The inviter lists the joiner; the joiner fetches X. |
| none | root X | The joiner lists the inviter; the inviter fetches X (`peer join` returns once it has). |
| root X | root X | Each lists the other (or refreshes its name); no documents change. Use it to introduce two members directly. |
| root X | root Y | Refused, nothing changes and the code stays usable: run `tt-server reset` on the server that should join, then pair again. |
| none | none | Refused: run `tt-server init` on one of them first. |

A server fetching a root records a `Joining` intent (root, source server,
address) before fetching anything, refuses clients with 503 meanwhile,
resumes the same fetch after a restart, and becomes `Ready` only once the
registry, every account's index and every document an index lists are
stored. A revoked server cannot pair again with its old key: reset it with
`--new-identity`.

### Links

- **Identity.** Each server has an ed25519 key pair in `server.key`; its
  `server_id` is derived from the public key. The key survives `reset`
  unless `--new-identity`.
- **Trust.** A server accepts a link only from a key that its own registry
  copy lists under `servers` and that is not in `revoked`. There is no other
  way in: certificate authorities and host names play no part, so a member
  keeps linking when its address changes.
- **TLS.** Both sides present a self-signed certificate made at start from
  the server key and check the other side's key against membership; the
  TLS 1.3 handshake itself proves the key is held. TLS 1.2, other key types
  and session resumption are refused. A connection to the client port
  never becomes a peer link, and a client can never pose as a server: peer
  ids are `srv:<server_id>` from the proven key, client peer ids always
  carry the user's session identity.
- **What syncs.** A member may sync the registry, every account's index and
  every document an index lists (deleted accounts included). A member
  pushing any other document is held like a client's new document and the
  link closes with a protocol error after 10 s. On every new link and on
  every change of the registry or an index, the server loads every such
  document (also ones evicted from memory) and offers it to its members.
- **Mesh.** Each server dials every address it knows (`--peer`, codes it
  joined with) and redials after a few seconds when a link drops. Changes
  relay through members: if B and C both reach A, they sync through A.
  When both sides dial each other the newer link replaces the older one.
- **Revocation** closes a link at once on the revoking server and on every
  member within moments of the registry change reaching it.

### Peering security model

- **Encrypted in transit:** peer traffic is mutual TLS 1.3 pinned to member
  keys; clients use the client listener's TLS as before.
- **Not encrypted at rest:** every member stores all documents and every
  account's password hash in plain form in its `server.db`.
- **Every member is root-equivalent:** any member can read and change all
  data, every account (including passwords) and the membership itself.
  Only pair servers you would trust with the whole registry.
- **Removing a member** means revoking it (`tt-server peer revoke`) on a
  member it cannot outrun and rotating every password (`tt-server user
  passwd`), because the removed server keeps the hashes and data it had.
  Members apply the revocation when the registry change reaches them.
- **No end-to-end encryption:** servers see all data in clear.
- **Reachability:** members link directly. Members behind NAT on different
  networks need Tailscale (or another VPN) or a member with a publicly
  reachable peer port; tt does no NAT traversal of its own.
- **Exposure:** the peer port can listen on every interface: non-members
  fail the TLS handshake before any application data, and pairing needs a
  valid secret, with 5 pairing attempts per minute per IP.

### Operations

The systemd unit in `contrib/tt-server.service` passes
`--peer-listen 0.0.0.0:8772`; add `--peer <host:port>` lines for members
this server should dial, and open the peer port in the firewall (not in
the reverse proxy). Its `ExecStartPre` runs `init` on the first start:
remove it on a server that will `peer join` another (or `tt-server reset
--yes` before joining).

On NixOS (the flake ships the package, not a module), the same as a
service in your configuration:

```nix
{ pkgs, tt, ... }:
let tt-server = "${tt.packages.${pkgs.system}.tt}/bin/tt-server";
in {
  users.users.tt-server = { isSystemUser = true; group = "tt-server"; };
  users.groups.tt-server = { };
  systemd.services.tt-server = {
    wantedBy = [ "multi-user.target" ];
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    serviceConfig = {
      User = "tt-server";
      StateDirectory = "tt-server";
      StateDirectoryMode = "0700";
      UMask = "0077";
      ExecStart = ''
        ${tt-server} --db /var/lib/tt-server/server.db serve \
          --insecure-http --behind-proxy --listen 127.0.0.1:8080 \
          --peer-listen 0.0.0.0:8772 \
          --peer laptop-a.tail1234.ts.net:8772
      '';
    };
  };
  # The peer port: on every interface, or only on the tailnet.
  networking.firewall.allowedTCPPorts = [ 8772 ];
  # networking.firewall.interfaces.tailscale0.allowedTCPPorts = [ 8772 ];
}
```

Options worth setting: the peer port (`--peer-listen`), the seeds
(`--peer`, one per member this host should dial), and the firewall opening
for the peer port. Run `tt-server peer invite|join|ls` as the `tt-server`
user with `--db /var/lib/tt-server/server.db`.

## HTTP API

All bodies are JSON. Errors are `{"error": "…"}`. Authenticated endpoints
take `Authorization: Bearer <token>` and answer 401 (with
`WWW-Authenticate: Bearer`) for a missing, unknown, or revoked token.

| Endpoint | Auth | Response |
| --- | --- | --- |
| `GET /api/health` | – | `{ok:true, version, sessions, state, server:{id,name}}`; `state` is `Ready`, `NeedsDecision` or `Joining` (then `name` is null). |
| `POST /api/login` `{username, password}` | – | `{token, index_doc, user:{id,name}, server:{id,name}}`; 401 `invalid username or password` (same for unknown and deleted users); 409 `{"error":"account_conflict"}` when the password matches a conflicted account (below); 429 after 5 attempts per minute from one IP. |
| `GET /api/me` | bearer | `{user:{id,name}, index_doc, server:{id,name}}` |
| `POST /api/logout` | bearer | `{ok:true}`; revokes the token and closes its websockets. |
| `POST /api/ws-ticket` | bearer | `{ticket, expires_in:60}`: single-use, 60 s. |
| `GET /api/export` | bearer | The core JSON export (`docs/export.md`) of the caller's workspace and entries, read only from documents the caller owns (below). |
| `GET /sync?ticket=<ticket>` | ticket or bearer header | Websocket upgrade (below). |
| other `/api/*` | – | 404 |

`index_doc` is the bs58check id of the user's index document. `tt login`
stores it with the token and user id, and a fresh device bootstraps
everything from it. `server.id` is the server's id: lowercase base32 (26
characters) of the first 16 bytes of SHA-256 of its ed25519 public key;
displays use the first 8 characters.

## Registry

The registry document is the server's root. It is private to the members:
it is never announced or sent to a client, and a client that asks for it
gets `doc-unavailable`.

```
{ kind: "tt-registry", version: 1,
  users: { <uuid>: {
      index_doc, workspace_doc, created,      // written once
      name:     { value, at },                // versioned field
      password: { hash, at },                 // argon2id PHC string
      deleted:  <ms> | absent } },
  servers: { <server_id>: {
      pubkey, added_by, added_at,             // written once; pubkey: hex
      name: { value, at } } },                // versioned field
  revoked: { <server_id>: { by, at } } }      // append-only
```

`init` lists the initializing server as the first member; `serve` adds
itself when an older registry lacks it. An entry whose public key does not
hash to its id is ignored.

Tokens, websocket tickets, invite secrets, rate limits and the login audit
stay in the local SQLite tables and are never written to the registry.
Concurrent edits from servers sharing a root resolve the same way on every
server, whatever order they merge in:

- **Password, name:** the value with the latest change time wins; equal
  times: the greater hash/name. Clock skew decides between servers.
- **Deletion** is final: a set `deleted` time is never cleared and wins
  against any concurrent edit. Keys are never removed.
- **Duplicate names:** among non-deleted accounts with one name, the one
  created first (equal times: lower id) owns it. The others are
  *conflicted*: `user ls` flags them, login with their password answers 409
  `account_conflict` (only after the correct password, so it reveals
  nothing), and `user rename --id <id> <new-name>` unblocks them.
- **Revocation** of a member is final: `revoked` lives outside `servers`,
  so a concurrent rename or re-add of the member cannot undo it.

When an account becomes deleted, by `user del` or a merged change, the
server revokes its tokens and closes its websockets.

## Sync

`/sync` speaks the automerge-repo JS protocol (version 1). Stock
`@automerge/automerge-repo` clients connect with the websocket client
adapter:

```js
const { token, index_doc } = await (await fetch("/api/login", …)).json();
const { ticket } = await (await fetch("/api/ws-ticket", { method: "POST", headers: { authorization: `Bearer ${token}` } })).json();
const repo = new Repo({ network: [new WebSocketClientAdapter(`wss://tt.example.com/sync?ticket=${ticket}`)] });
const index = await repo.find(`automerge:${index_doc}`);
```

A ticket works once, so a client that reconnects needs a fresh ticket per
connection (the daemon does this). The long-lived token is never accepted
in a URL (`?token=` is refused with 401); clients that can set headers may
send `Authorization: Bearer` on the upgrade instead.

**Identity.** Each connection is the peer `<user id>.<nonce>/<senderId>`:
the user comes from the ticket or token, never from the client, and two
devices of one user never collide. The server announces itself as
`srv:<server_id>`.

**Visibility.** Access is derived from the registry and the indexes: a
user owns its index document and every document that index lists, and a
connection sees exactly what its user owns. Anything else is answered
`doc-unavailable` without sync data, and the attempt is logged. When two
users' indexes list the same document, only the account created first has
access and the conflict is logged; listing another user's document gains
nothing. Deleted accounts own nothing.

**New documents.** A client may create documents (for example the
`entries-2027` document on the first entry of a new year). The server
accepts a document it has never seen only when the user's index document
lists it. Because the index edit often arrives a moment after the new
document, the document's frames are held for up to 10 s while the index
syncs. A document still unlisted after that is discarded and the connection
closed with a protocol `error`.

**Storage.** Documents live in the `documents` table of the same SQLite file
(WAL, `synchronous=FULL`), are loaded on first use, are written after every
change (50 ms debounce), and are evicted when idle (see `--idle-evict-secs`).
A restart loses nothing that was acknowledged.

## Daemon behaviour

`tt login <url>` posts the credentials, writes `server.url`, `server.token`,
`user.index_doc`, `user.id`, `user.name` (and `server.ca_cert` with
`--ca-cert`) to `config.toml` (mode 0600),
and tells the daemon to connect. Before every connection attempt the daemon
asks `/api/ws-ticket` for a ticket. When that request answers 401 the token
was revoked: the daemon stops reconnecting and `tt status` shows
`sync: login required`. Everything keeps working offline. `tt logout` revokes
the token on the server, removes it from the config, stops sync, and keeps
local data. A server without `/api/ws-ticket` (404, such as the plain
automerge-repo sync server used in tests) gets the token in an
`Authorization` header instead.

The daemon only syncs documents reachable from its index (index, workspace,
listed entries), so documents left over from offline use before the first
login are never pushed.

## Security checklist

- **TLS.** `serve` refuses to start without `--tls-cert/--tls-key` unless
  `--insecure-http` is given. With `--insecure-http`, bind to `127.0.0.1` and
  terminate TLS in a proxy on the same host. Passwords and tokens must only
  ever travel over TLS. Clients (`tt login`, the daemon's `wss://` sync)
  verify the certificate against the bundled Mozilla (webpki) roots, plus
  an optional extra CA. For a private or local CA, pass the CA certificate
  (not the server's own certificate) at login:

  ```sh
  tt login https://tt.example.local --username alice --ca-cert ./ca.pem
  ```

  The absolute path is stored as `server.ca_cert` (also settable with
  `tt config set server.ca_cert <pem>`); login, logout, the ticket request
  and `wss://` sync all trust it on top of the webpki roots, and hostname
  checks stay on. The file is read on every daemon start and `sync.reload`,
  so rotating it needs no config change. If it becomes unreadable or holds
  no certificate, the daemon keeps serving local commands but opens no
  connection, and `tt status` reports the sync state `ca_cert_invalid`
  naming the file. Without `--ca-cert`, a private-CA certificate fails with
  `UnknownIssuer` (exit code 4) and a hint to use `--ca-cert`.
- **Passwords.** argon2id with the OWASP parameters: 19 MiB memory
  (`m=19456`), 2 iterations (`t=2`), parallelism 1, random 16-byte salt,
  stored as a PHC string. Unknown user names are verified against a dummy
  hash so timing and response do not reveal which names exist. Minimum
  length 8.
- **Tokens.** 32 random bytes from the OS RNG, base64url, returned once at
  login; only the SHA-256 is stored. Long-lived until revoked
  (`tt logout`, `tt-server token revoke`, `tt-server user del`). Revocation
  takes effect on the next request; open websockets for the token are
  closed immediately (logout, or the CLI through the admin socket), or
  within 5 s when the CLI wrote the database directly while no server ran.
- **Login rate limit.** 5 attempts per minute per client IP (sliding
  window); the sixth gets 429. Use `--behind-proxy` behind a reverse proxy,
  otherwise every client shares the proxy's address.
- **Audit.** Every login attempt (time, IP, user name, outcome, reason:
  `ok`, `invalid credentials`, `account conflict`, `rate limited`) goes to the `login_audit`
  table. Refused cross-user document requests and discarded pushes are
  logged as warnings.
- **Ticket flow.** Browsers cannot set headers on websockets, so `/sync`
  takes `?ticket=`: 32 random bytes, stored as SHA-256, bound to the user
  and the token that requested it, valid 60 s, consumed atomically on first
  use (a second upgrade with it gets 401), and refused if the token was
  revoked in between. The long-lived token is never accepted in a URL.
- **Isolation.** Peer identity comes from the server, not the client.
  Every inbound sync/request is checked against the derived access before the
  repository sees it, and every outbound message is checked again; new
  documents are accepted only through the index rule above.
- **Peers.** See [Peering security model](#peering-security-model).
- **Files.** `server.db`, `server.key` and `admin.sock` are mode 0600 (the
  socket's permissions are its only credential; a `server.key` readable by
  group or others stops every command); run as a dedicated user
  (`contrib/tt-server.service` uses `StateDirectoryMode=0700`, `UMask=0077`).
  Back it up with `sqlite3 server.db ".backup …"` or per user with
  `GET /api/export`.
- **No sign-up, no reset by email.** Accounts and password resets happen on
  the server host only.
