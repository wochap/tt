# peer-addresses Specification

## Purpose
Keeps paired tt servers reachable as their network addresses change, by exchanging and remembering addresses outside the replicated registry, and by reconnecting to every member it knows.

## Requirements

### Requirement: Addresses are exchanged in the peer link hello
When a peer link is established, each side SHALL send its current advertised addresses (every `host:port` at which its peer listener can be reached, including configured names such as a Tailscale MagicDNS name), its client URL (`--public-url`, if configured; used by browsers, never for peer links), and its last-seen addresses of other members as hints. A server SHALL accept addresses only from an authenticated member, and SHALL accept addresses for a member only for a `server_id` its own registry lists and does not revoke.

#### Scenario: Laptop changes network
- **WHEN** laptop B moves from home to work, its LAN address changes, and B then links with A over Tailscale
- **THEN** A records B's new addresses from the hello and uses them for later dials

#### Scenario: Hint for an unknown member
- **WHEN** a member sends last-seen addresses for a `server_id` that is not in the receiver's registry, or is revoked
- **THEN** the receiver discards those addresses

#### Scenario: Advertised address configuration
- **WHEN** `serve` runs with `--peer-advertise laptop-b.example.ts.net:8772` and `--peer-listen 0.0.0.0:8772`
- **THEN** the hello advertises `laptop-b.example.ts.net:8772` and the non-loopback interface addresses with port 8772

### Requirement: Addresses never enter the registry
Peer addresses SHALL be stored only in the server's local database with the time each was last seen and its source (self-advertised, hint, or static seed). The registry document SHALL contain no address data.

#### Scenario: Registry unchanged by roaming
- **WHEN** a member changes addresses ten times in a day
- **THEN** the registry document gains no changes from it, and the local address store holds the new addresses with timestamps

#### Scenario: Stale addresses expire
- **WHEN** an address learned as a hint has not been confirmed by a successful link for 30 days
- **THEN** it is removed from the local address store, while static seeds are kept

### Requirement: Reconnect to every known member
A server SHALL keep trying to link with every non-revoked member it does not currently link with, dialing each known address of that member in order of most recently successful, with exponential backoff per member (starting at 1 second, capped at 5 minutes, with jitter). A successful link SHALL reset that member's backoff. No member SHALL be dialed by a busy loop.

#### Scenario: Member comes back online
- **WHEN** member B has been unreachable for an hour and becomes reachable at a known address
- **THEN** A links with B within the current backoff cap (at most 5 minutes) without manual action

#### Scenario: Revoked member
- **WHEN** a member is revoked in the registry
- **THEN** the server stops dialing it and closes any open link with it

### Requirement: One link per member pair
When two members link with each other in both directions at once, both sides SHALL keep exactly one link: the link dialed by the member with the lower `server_id` survives and the other is closed. Closing the duplicate SHALL NOT drop pending sync data.

#### Scenario: Simultaneous dial
- **WHEN** A and B dial each other at the same moment and both links authenticate
- **THEN** within a few seconds exactly one link remains, the same one on both sides, and documents keep converging

### Requirement: Peer status listing
`tt-server peer ls` SHALL list every non-revoked member except the local server with name and id prefix, state (`online`, `offline`, `syncing` with the count of documents not yet in sync, or `error` with a message), last seen time, and the address of the current or last successful link.

#### Scenario: Two members, one offline
- **WHEN** A links with B and has not seen C for two hours
- **THEN** `peer ls` shows B as `online` with its link address and C as `offline` with last seen two hours ago and C's last link address
