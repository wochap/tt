# server-peering Specification

## Purpose
Lets tt servers that share one registry root authenticate each other by key, pair through a one-time invite, and replicate every document in a full mesh, so a user's laptops (and later a cloud server) stay in sync without a central server.

## Requirements

### Requirement: Server keypair is installation identity
Each server SHALL hold the ed25519 keypair created by `server-registry` on first open, stored in its state directory with owner-only permissions. The `server_id` SHALL be derived from the public key. The keypair SHALL survive `tt-server reset` and SHALL be replaced only by `tt-server reset --new-identity`.

#### Scenario: Identity is stable across restarts
- **WHEN** a server is restarted
- **THEN** it reports the same `server_id` as before

#### Scenario: Reset keeps identity
- **WHEN** `tt-server reset` completes
- **THEN** the server reports the same `server_id`, and after `reset --new-identity` it reports a different one

#### Scenario: Key file permissions
- **WHEN** the key file is readable by group or others at start
- **THEN** the server refuses to start and names the file

### Requirement: Membership lives in the registry
The registry SHALL contain `servers`, mapping `server_id` to `{name, pubkey, added_by, added_at}`, and an append-only `revoked` map from `server_id` to `{by, at}`. An entry in `revoked` SHALL never be removed, and a revoked `server_id` SHALL stay revoked even if a concurrent edit rewrites its `servers` entry. `tt-server init` SHALL list the initializing server as the first member.

#### Scenario: Init records the first member
- **WHEN** `tt-server init --name laptop-a` runs on an empty database
- **THEN** the registry lists exactly one member with this server's id, public key and name `laptop-a`

#### Scenario: Revocation wins over a concurrent rename
- **WHEN** member C is revoked on server A while server B concurrently renames C, and A and B then sync
- **THEN** both servers treat C as revoked

### Requirement: Trust is registry membership
A server SHALL accept a peer link only when the peer proves possession of a private key whose public key is listed in its own registry copy under `servers` and is not in `revoked`. There SHALL be no other trust path. Every member SHALL be treated as root-equivalent: it may read and edit every account and every membership entry.

#### Scenario: Unknown key is refused
- **WHEN** a server whose key is not in B's registry connects to B's peer port
- **THEN** B refuses the link during the handshake and syncs nothing with it

#### Scenario: Member learned through another member
- **WHEN** C joined through A, B synced with A afterwards, and A is offline
- **THEN** C and B can link directly and sync

#### Scenario: Revoked member is refused
- **WHEN** a revoked member connects
- **THEN** the link is refused and any open link with it is closed within 5 seconds of the revocation reaching the server

### Requirement: Peer links use mutual key-pinned TLS
Peer links SHALL use a dedicated listener set by `--peer-listen <addr:port>`, separate from the client listener. Both sides SHALL present self-signed TLS 1.3 certificates bound to their server key and SHALL verify the other side's public key against registry membership. Certificate authorities and host names SHALL NOT be used to verify peers. Successful completion of the handshake SHALL be the proof of key possession.

#### Scenario: Peer reached by changing address
- **WHEN** B dials A at a new IP address that is not in any certificate
- **THEN** the link succeeds because A's key is a member

#### Scenario: Impersonation
- **WHEN** a host presents A's public key in its certificate without holding A's private key
- **THEN** the handshake fails

#### Scenario: Client port does not accept peers
- **WHEN** a server dials another server's client listener as a peer
- **THEN** no peer link is established

### Requirement: Invite and join
`tt-server peer invite` SHALL print a one-time code containing the inviter's reachable peer address, its `server_id`, and a secret. The secret SHALL be stored only as a hash, SHALL be valid for 10 minutes, and SHALL be consumed by its first successful use. `tt-server peer join <code>` SHALL verify that the inviter's key matches the `server_id` in the code before sending the secret. The side that holds the root SHALL write the newcomer's membership entry, whichever side issued the invite.

#### Scenario: Fresh server joins
- **WHEN** an empty server B runs `peer join` with a valid code from A
- **THEN** A writes B's entry into the registry, B adopts A's registry as its root, and both list each other in `peer ls`

#### Scenario: Rooted server joins an empty inviter
- **WHEN** an empty public server C runs `peer invite` and rooted laptop A runs `peer join` with that code
- **THEN** C adopts A's root and A writes C's entry

#### Scenario: Code reuse
- **WHEN** a code that has already been used is presented again
- **THEN** the join fails and nothing changes

#### Scenario: Expired code
- **WHEN** a code is presented more than 10 minutes after it was created
- **THEN** the join fails with an expiry message

#### Scenario: Wrong inviter key
- **WHEN** the server reached at the code's address presents a key that does not match the code's `server_id`
- **THEN** `peer join` aborts before sending the secret

### Requirement: Root direction is decided by state
Pairing SHALL compare the two servers' roots: when exactly one side has a root, the empty side SHALL adopt it; when both have the same root, the pairing SHALL add or refresh membership entries only; when the roots differ, pairing SHALL be refused with a message to run `tt-server reset` on one side; when neither has a root, pairing SHALL be refused with a message to run `tt-server init` first.

#### Scenario: Different roots
- **WHEN** two servers with different registry roots try to pair
- **THEN** both stay unchanged and the joiner exits non-zero naming `tt-server reset`

#### Scenario: Same root reintroduction
- **WHEN** B and C share a root but neither lists the other, and they pair
- **THEN** each lists the other and no documents are replaced

#### Scenario: Both empty
- **WHEN** two servers without a root try to pair
- **THEN** pairing is refused naming `tt-server init`

### Requirement: Join is crash-safe and reports ready after the first pull
Before fetching, a joining server SHALL durably record a `Joining` intent with the root id and the inviter. On restart with that intent it SHALL resume fetching the same root. The join SHALL report success only after the registry and every document reachable from it are stored locally.

#### Scenario: Crash during join
- **WHEN** the joining server is killed after recording the intent and before all documents arrive
- **THEN** on restart it resumes the join for the same root and does not accept clients until the join completes

#### Scenario: Join completes
- **WHEN** `peer join` returns success
- **THEN** a client logging into the joined server sees the same tasks and entries as on the inviter

### Requirement: Reset returns a server to NeedsDecision
`tt-server reset` SHALL ask for confirmation, record a reset intent, delete the root, all documents, accounts, local tokens, peer state and pairing secrets, keep the keypair, and leave the server in `NeedsDecision`. An interrupted reset SHALL complete on the next start before anything else runs.

#### Scenario: Reset then join another root
- **WHEN** a server with root X runs `reset` and then `peer join` with a code from a server with root Y
- **THEN** it ends with root Y and no documents from X

#### Scenario: Crash during reset
- **WHEN** the process is killed midway through a reset
- **THEN** the next start completes the reset and reaches `NeedsDecision`

### Requirement: Members replicate everything in a full mesh
Every server SHALL dial every non-revoked member it has an address for, keep the link open, and reconnect after it drops. On every link established and on every change to the registry or to any index document, a server SHALL load every document reachable from the registry (each user's index and the documents it lists) and synchronize it with the linked member, including documents that were evicted from memory or never loaded. Changes received from one member SHALL reach other linked members.

#### Scenario: Evicted document replicates
- **WHEN** a document on A was evicted from memory and B links to A
- **THEN** B receives that document

#### Scenario: Relay through a member
- **WHEN** B and C cannot reach each other but both link to A, and a task is created through B
- **THEN** the task reaches C

#### Scenario: Offline edits converge
- **WHEN** A and B both change data while unlinked and later link
- **THEN** both converge to the same documents

### Requirement: Member management commands
`tt-server peer ls` SHALL list members as `name (id prefix)` with revoked state. `tt-server peer revoke <id-prefix|name>` SHALL add the member to `revoked`. `tt-server peer rename <id-prefix|name> <new-name>` SHALL change its display name. Ambiguous names SHALL require an id prefix.

#### Scenario: Duplicate host names
- **WHEN** two members are both named `nixos`
- **THEN** `peer ls` shows both with distinct id prefixes and `peer revoke nixos` fails asking for an id prefix

#### Scenario: Revoke self
- **WHEN** a server revokes its own id
- **THEN** the command is refused with a message to use `tt-server reset`

### Requirement: Security model is documented
The server documentation SHALL state that peer traffic is encrypted in transit, data and password hashes are stored unencrypted on every member, every member can read and change all data and accounts, removing a member requires revoking it and rotating passwords, end-to-end encryption is not provided, and members behind NAT on different networks need Tailscale or a publicly reachable member to link.

#### Scenario: Docs state the model
- **WHEN** a reader opens `docs/server.md`
- **THEN** a "Peering security model" section lists each of these points
