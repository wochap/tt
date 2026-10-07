# Commands go through rtk (see RTK.md).

default: build

build:
    rtk cargo build --workspace

test:
    rtk cargo test --workspace

lint:
    rtk cargo clippy --workspace --all-targets --all-features -- -D warnings

fmt:
    rtk cargo fmt --all

fmt-check:
    rtk cargo fmt --all -- --check

# Spike: Rust clients through the real @automerge/automerge-repo-sync-server (needs node + npm).
interop:
    rtk cargo test -p automerge-repo --test js_interop -- --nocapture
    rtk cargo test -p tt-server --test interop -- --nocapture

# Run a local server for development (plain HTTP on localhost).
server db="target/dev-server.db":
    rtk cargo run -p tt-server -- --db {{db}} serve --insecure-http --listen 127.0.0.1:8080

e2e: build
    scripts/e2e.sh

# Web app (node >= 20 + pnpm): lint, typecheck, vitest (domain + web), build to web/dist.
web:
    rtk pnpm install --frozen-lockfile
    rtk pnpm run ci

# Playwright against `tt-server serve --web-dir web/dist` (web/e2e/serve.sh builds both).
web-e2e:
    rtk pnpm --filter web e2e

# Serve the built web app from a local dev server.
web-serve db="target/dev-server.db":
    rtk pnpm build
    rtk cargo run -p tt-server -- --db {{db}} serve --insecure-http --listen 127.0.0.1:8080 --web-dir web/dist

ci: fmt-check lint test interop e2e web web-e2e
