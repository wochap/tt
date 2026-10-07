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

e2e: build
    scripts/e2e.sh

ci: fmt-check lint test interop e2e
