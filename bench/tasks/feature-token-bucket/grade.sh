#!/bin/sh
set -eu
cp "$BENCH_TASK_DIR/fixture/test_ratelimit.py" "$BENCH_TASK_DIR/fixture/SPEC.md" .
cp "$BENCH_TASK_DIR"/hidden/*.py .
python3 "$BENCH_GRADERS/expect_tests.py" unittest 14 -- python3 -m unittest -q
