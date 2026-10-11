#!/bin/sh
# Hidden tests use the new API; the visible tests are restored unchanged.
set -eu
cp "$BENCH_TASK_DIR/fixture/tests/orders.rs" "$BENCH_TASK_DIR/hidden/hidden_status.rs" tests/
leaks=$(grep -rnE '"(pending|paid|shipped|cancelled)"' src --include='*.rs' | grep -v '^src/status.rs:' || true)
if [ -n "$leaks" ]; then
    echo "status literals outside src/status.rs:" >&2
    echo "$leaks" >&2
    exit 1
fi
python3 "$BENCH_GRADERS/expect_tests.py" cargo 8 -- cargo test -q --offline
