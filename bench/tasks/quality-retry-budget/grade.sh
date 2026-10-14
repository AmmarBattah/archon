#!/bin/sh
set -eu
cp "$BENCH_TASK_DIR/fixture/test_retry_budget.py" "$BENCH_TASK_DIR/fixture/SPEC.md" .
cp "$BENCH_TASK_DIR"/hidden/*.py .
timeout 20s python3 "$BENCH_GRADERS/expect_tests.py" unittest 14 -- python3 -m unittest -q test_retry_budget test_retry_budget_hidden
