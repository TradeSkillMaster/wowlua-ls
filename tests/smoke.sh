#!/bin/bash
# Smoke test: build + quick sanity checks + integration tests
set -e

cd "$(dirname "$0")/.."

echo "=== Building ==="
cargo build 2>&1 | tail -1

echo "=== Running cargo test ==="
cargo test 2>&1 | grep -E "^(test |running |test result)"

echo "=== Smoke: basic hover/definition (no stubs) ==="
# local variable
OUT=$(cargo run -- test-query tests/type-scans2.lua:1:7 2>/dev/null)
echo "$OUT" | grep -q "hover: x: number" || { echo "FAIL: x hover"; exit 1; }
echo "$OUT" | grep -q "definition: local" || { echo "FAIL: x definition"; exit 1; }
echo "  type-scans2.lua:1:7 (x) OK"

# function
OUT=$(cargo run -- test-query tests/type-scans2.lua:4:16 2>/dev/null)
echo "$OUT" | grep -q "hover: AddTwo:" || { echo "FAIL: AddTwo hover"; exit 1; }
echo "  type-scans2.lua:4:16 (AddTwo) OK"

echo "=== Smoke: stubs hover/definition ==="
OUT=$(cargo run -- test-query tests/stubs-test.lua:3:11 --with-stubs 2>/dev/null)
echo "$OUT" | grep -q "hover: setmetatable:" || { echo "FAIL: setmetatable hover"; exit 1; }
echo "$OUT" | grep -q "definition: external" || { echo "FAIL: setmetatable definition"; exit 1; }
echo "  stubs-test.lua:3:11 (setmetatable) OK"

OUT=$(cargo run -- test-query tests/stubs-test.lua:4:11 --with-stubs 2>/dev/null)
echo "$OUT" | grep -q "hover: type:" || { echo "FAIL: type hover"; exit 1; }
echo "  stubs-test.lua:4:11 (type) OK"

echo "=== Smoke: completions ==="
OUT=$(cargo run -- test-query tests/type-scans2.lua:13:10 2>/dev/null)
echo "$OUT" | grep -q "completions:" || { echo "FAIL: completions"; exit 1; }
echo "  completions present OK"

echo ""
echo "All smoke tests passed."
