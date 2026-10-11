#!/bin/sh
# Each repository's visible tests are restored from the task's fixtures,
# then hidden unit and end-to-end tests run.
set -eu
T="$BENCH_TASK_DIR"
H="$T/hidden"
cd "$BENCH_RESULT_INVENTORY_API"
cp "$T/inventory-api/test_api.py" "$H/test_api_pagination.py" .
python3 "$BENCH_GRADERS/expect_tests.py" unittest 10 -- python3 -m unittest -q
cd "$BENCH_RESULT_INVENTORY_CLIENT"
mkdir -p test
cp "$T/inventory-client/test/client.test.js" "$H/hidden_client.test.js" "$H/e2e.test.js" test/
python3 "$BENCH_GRADERS/expect_tests.py" node 7 -- node --test --test-reporter=tap
