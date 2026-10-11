'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const { getItem, listItems, lowStock } = require('../client');

const ITEMS = [
  { id: 1, sku: 'SKU-0001', qty: 3 },
  { id: 2, sku: 'SKU-0002', qty: 40 },
];

async function singlePage(request) {
  if (request.path === '/items') return { status: 200, body: { items: ITEMS } };
  if (request.path === '/items/2') return { status: 200, body: ITEMS[1] };
  return { status: 404, body: { error: 'not found' } };
}

test('lists items', async () => {
  assert.deepEqual(await listItems(singlePage), ITEMS);
});

test('gets one item and reports failures', async () => {
  assert.equal((await getItem(singlePage, 2)).sku, 'SKU-0002');
  await assert.rejects(getItem(singlePage, 9), /404/);
});

test('finds low stock', () => {
  assert.deepEqual(lowStock(ITEMS, 10), ['SKU-0001']);
});
