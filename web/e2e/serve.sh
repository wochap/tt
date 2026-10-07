#!/usr/bin/env bash
# Builds the web bundle and tt-server, creates throwaway users in a temp
# database, and serves the bundle the way production does
# (`tt-server serve --web-dir web/dist`). Used by playwright.config.ts.
# `--behind-proxy` lets each test log in from its own X-Forwarded-For
# address, so the 5-per-minute login limit applies per test.
set -euo pipefail
cd "$(dirname "$0")/.."
WEB=$(pwd)
ROOT=$(cd .. && pwd)
PORT=${TT_E2E_PORT:-8123}

if [ -z "${TT_E2E_SKIP_BUILD:-}" ]; then
  pnpm exec vite build >&2
  (cd "$ROOT" && cargo build -q -p tt-server -p tt-cli >&2)
fi
BIN=${TT_SERVER_BIN:-$ROOT/target/debug/tt-server}

DIR=$(mktemp -d /tmp/tt-web-e2e.XXXXXX)
trap 'rm -rf "$DIR"' EXIT
for user in $(seq -f "e2e%g" 1 16); do
  printf 'password123\n' | "$BIN" --db "$DIR/server.db" user add "$user" >/dev/null
done
"$BIN" --db "$DIR/server.db" serve --insecure-http --behind-proxy --listen "127.0.0.1:$PORT" --web-dir "$WEB/dist" &
SERVER=$!
trap 'kill $SERVER 2>/dev/null; rm -rf "$DIR"' EXIT
wait $SERVER
