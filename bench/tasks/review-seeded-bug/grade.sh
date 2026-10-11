#!/bin/sh
# Review task: the operator-visible report is scored against a keyword rubric.
set -eu
python3 "$BENCH_GRADERS/rubric.py" "$BENCH_TASK_DIR/rubric.json"
