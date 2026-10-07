#!/usr/bin/env bash
# End-to-end: spawns real daemons in temp XDG dirs and runs the brief's flows
# (crates/tt-cli/tests/e2e.rs). The sync part needs node + npm; set
# TT_REQUIRE_NODE=1 to fail instead of skipping when they are missing.
set -euo pipefail
cd "$(dirname "$0")/.."
exec rtk cargo test -p tt-cli --test e2e -- --nocapture --test-threads=1 "$@"
