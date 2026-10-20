#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
command -v cargo >/dev/null 2>&1 || { echo 'Cargo is missing. Install Rust >=1.75 using https://rustup.rs/' >&2; exit 1; }
"${LISAN_CLANG:-clang}" --version
cargo test --locked --all-targets
cargo build --locked --release
sample_dir=$(mktemp -d)
trap 'rm -rf "$sample_dir"' EXIT HUP INT TERM
./target/release/lisan --native examples/native.lsn --output "$sample_dir/native-sample"
"$sample_dir/native-sample" > "$sample_dir/actual.txt"
printf '0\n1\n9\n16\nانتهى\n' > "$sample_dir/expected.txt"
diff -u "$sample_dir/expected.txt" "$sample_dir/actual.txt"
