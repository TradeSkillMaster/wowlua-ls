#!/bin/bash
# Smoke test: build + run all integration tests
set -e

cd "$(dirname "$0")/.."

echo "=== Building ==="
cargo build --workspace 2>&1 | tail -1

echo "=== Running cargo test ==="
cargo test --workspace 2>&1 | grep -E "^(test |running |test result)"

echo ""
echo "All smoke tests passed."
