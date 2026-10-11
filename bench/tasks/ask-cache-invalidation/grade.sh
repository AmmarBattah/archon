#!/bin/sh
# Answer task: the operator-visible answer is scored against a keyword rubric.
set -eu
python3 "$BENCH_GRADERS/rubric.py" "$BENCH_TASK_DIR/rubric.json"
