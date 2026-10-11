'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const { listItems } = require('../client');

const ITEMS = Array.from({ length: 23 }, (_, i) => ({ id: i + 1, sku: `S${i + 1}`, qty: i }));

function pagedTransport(pageSize) {
  const seen = [];
  const transport = async (request) => {
    seen.push(request);
    assert.equal(request.path, '/items');
    const query = request.query || {};
    let start = 0;
    if (query.cursor !== undefined) {
      const decoded = /^opaque:(\d+):end$/.exec(query.cursor);
      if (!decoded) return { status: 400, body: { error: 'invalid cursor' } };
      start = Number(decoded[1]);
    }
    const items = ITEMS.slice(start, start + pageSize);
    const next = start + pageSize < ITEMS.length ? `opaque:${start + pageSize}:end` : null;
    return { status: 200, body: { items, next_cursor: next } };
  };
  return { transport, seen };
}

test('follows next_cursor across every page', async () => {
  const { transport, seen } = pagedTransport(5);
  assert.deepEqual(await listItems(transport), ITEMS);
  assert.equal(seen.length, 5);
  assert.equal((seen[0].query || {}).cursor, undefined, 'first request sends no cursor');
  assert.equal(seen[1].query.cursor, 'opaque:5:end');
});

test('one page when the server has one page', async () => {
  const { transport, seen } = pagedTransport(100);
  assert.deepEqual(await listItems(transport), ITEMS);
  assert.equal(seen.length, 1);
});

test('a failing page rejects', async () => {
  let calls = 0;
  const transport = async () => {
    calls += 1;
    if (calls === 1) return { status: 200, body: { items: [ITEMS[0]], next_cursor: 'c1' } };
    return { status: 500, body: { error: 'boom' } };
  };
  await assert.rejects(listItems(transport), /500/);
});
