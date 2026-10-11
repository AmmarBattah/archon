'use strict';

// A transport is `async (request) => response`, where a request is
// `{method, path, query}` and a response is `{status, body}`.

async function request(transport, path, query = {}) {
  const res = await transport({ method: 'GET', path, query });
  if (res.status !== 200) {
    throw new Error(`GET ${path} failed with ${res.status}: ${JSON.stringify(res.body)}`);
  }
  return res.body;
}

async function listItems(transport) {
  const body = await request(transport, '/items');
  return body.items;
}

async function getItem(transport, id) {
  return request(transport, `/items/${id}`);
}

function lowStock(items, threshold) {
  return items.filter((item) => item.qty < threshold).map((item) => item.sku);
}

module.exports = { listItems, getItem, lowStock };
