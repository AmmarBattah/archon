#!/bin/sh
set -eu
cp "$BENCH_TASK_DIR/fixture/test_ledger.py" "$BENCH_TASK_DIR/fixture/SPEC.md" .
cp "$BENCH_TASK_DIR"/hidden/*.py .
timeout 20s python3 "$BENCH_GRADERS/expect_tests.py" unittest 10 -- python3 -m unittest -q test_ledger test_ledger_hidden
