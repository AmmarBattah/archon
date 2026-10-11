#!/bin/sh
# Visible tests are restored from the task's own fixture (never from the
# agent's git history), then the hidden edge cases join them.
set -eu
cp "$BENCH_TASK_DIR/fixture/test_window.py" .
cp "$BENCH_TASK_DIR"/hidden/*.py .
python3 "$BENCH_GRADERS/expect_tests.py" unittest 8 -- python3 -m unittest -q
