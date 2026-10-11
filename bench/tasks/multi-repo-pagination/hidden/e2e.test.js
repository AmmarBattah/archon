'use strict';

// The real client against the real API, one api_cli.py process per request.
const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const test = require('node:test');

const { listItems } = require('../client');

const API = process.env.BENCH_RESULT_INVENTORY_API;

async function cliTransport(request) {
  const out = execFileSync('python3', ['api_cli.py'], {
    cwd: API,
    input: JSON.stringify(request),
    env: { ...process.env, PYTHONDONTWRITEBYTECODE: '1' },
  });
  return JSON.parse(out.toString());
}

test('the client lists all 57 items from the real API', async () => {
  assert.ok(API, 'BENCH_RESULT_INVENTORY_API is set');
  const items = await listItems(cliTransport);
  assert.deepEqual(items.map((i) => i.id), Array.from({ length: 57 }, (_, i) => i + 1));
});
