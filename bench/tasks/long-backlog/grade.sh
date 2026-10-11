#!/bin/sh
# One point per backlog ticket; the task passes only with all four.
set -u
F="$BENCH_TASK_DIR/fixture"
mkdir -p tests
cp "$F/tests/__init__.py" "$F/tests/test_basic.py" tests/
cp "$F/BACKLOG.md" .
cp "$BENCH_TASK_DIR"/hidden/test_hidden_*.py tests/
check() {
    python3 "$BENCH_GRADERS/expect_tests.py" unittest "$1" -- python3 -m unittest -q "$2"
}
check 4 tests.test_basic || { echo '{"score":0}'; exit 1; }
passed=0
for ticket in dates:5 export:4 search:5 undo:5; do
    name=${ticket%%:*}
    if check "${ticket#*:}" "tests.test_hidden_$name"; then
        passed=$((passed + 1))
    else
        echo "ticket $name: FAIL" >&2
    fi
done
echo "{\"score\":$(python3 -c "print($passed / 4)")}"
[ "$passed" -eq 4 ]
