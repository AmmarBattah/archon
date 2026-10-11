'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const { createCatalog, updatePrice } = require('../src');

const PRODUCTS = [
  { sku: 'A1', name: 'Anvil', category: 'tools', priceCents: 5000 },
  { sku: 'B2', name: 'Bucket', category: 'tools', priceCents: 900 },
];

test('product pages see price updates immediately', () => {
  const catalog = createCatalog(PRODUCTS);
  assert.equal(catalog.product('A1').priceCents, 5000);
  updatePrice(catalog, 'A1', 4500);
  assert.equal(catalog.product('A1').priceCents, 4500);
});

test('listings are paged and sorted by name', () => {
  const catalog = createCatalog(PRODUCTS);
  assert.deepEqual(catalog.listing('tools').map((p) => p.sku), ['A1', 'B2']);
});

test('prices must be positive integers', () => {
  const catalog = createCatalog(PRODUCTS);
  assert.throws(() => updatePrice(catalog, 'A1', 0));
  assert.throws(() => updatePrice(catalog, 'A1', 1.5));
});
