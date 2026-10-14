#!/usr/bin/env sh
set -eu
if ! command -v cargo >/dev/null 2>&1; then
  echo 'Cargo is missing. Install Rust using https://rustup.rs/' >&2
  exit 1
fi
cd "$(dirname "$0")/.."
cargo test --all-targets
cargo build --release
./target/release/lisan examples/lists.lsn
