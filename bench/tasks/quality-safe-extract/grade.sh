#!/bin/sh
set -eu
cp "$BENCH_TASK_DIR/fixture/test_safe_extract.py" "$BENCH_TASK_DIR/fixture/SPEC.md" .
cp "$BENCH_TASK_DIR"/hidden/*.py .
timeout 20s python3 "$BENCH_GRADERS/expect_tests.py" unittest 13 -- python3 -m unittest -q test_safe_extract test_safe_extract_hidden
