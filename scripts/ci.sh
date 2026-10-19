#!/usr/bin/env bash
set -euo pipefail

echo "=== Running cargo fmt --check ==="
cargo fmt --check

echo "=== Running cargo clippy --all-targets -- -D warnings ==="
cargo clippy --all-targets -- -D warnings

echo "=== Running cargo test --all ==="
cargo test --all

echo "=== All CI checks passed! ==="
