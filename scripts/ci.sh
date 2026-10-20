#!/usr/bin/env bash
set -euo pipefail

AUTO_FIX=false

for arg in "$@"; do
    case "$arg" in
        --fix) AUTO_FIX=true ;;
        --help|-h)
            echo "Usage: ./scripts/ci.sh [--fix]"
            echo "  --fix   Auto-formats code and applies clippy suggestions where possible"
            exit 0
            ;;
        *)
            echo "Unknown argument: $arg"
            echo "Run './scripts/ci.sh --help' for usage."
            exit 1
            ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if [ "$AUTO_FIX" = true ]; then
    echo "=== Formatting with cargo fmt ==="
    cargo fmt
    echo "=== Applying clippy fixes ==="
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged
fi

echo "=== [1/5] Checking code formatting (cargo fmt) ==="
cargo fmt --check

echo "=== [2/5] Running Clippy lints ==="
cargo clippy --workspace --all-targets --locked -- -D warnings

echo "=== [3/5] Checking benchmarks compilation ==="
cargo check --workspace --benches --locked

echo "=== [4/5] Checking documentation ==="
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --locked

echo "=== [5/5] Running test suite ==="
cargo test --workspace --locked

echo "=== All CI checks passed! ==="
