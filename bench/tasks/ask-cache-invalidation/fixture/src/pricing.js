'use strict';

const { productKey } = require('./keys');

// Merchant-facing price changes.
function updatePrice(catalog, sku, priceCents) {
  if (!Number.isInteger(priceCents) || priceCents <= 0) {
    throw new Error('price must be a positive integer number of cents');
  }
  const updated = catalog.store.update(sku, { priceCents });
  catalog.cache.delete(productKey(sku));
  return updated;
}

module.exports = { updatePrice };
