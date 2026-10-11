'use strict';

const { LISTING_TTL_SECONDS, PAGE_SIZE, PRODUCT_TTL_SECONDS } = require('./config');
const { listingKey, productKey } = require('./keys');

class Catalog {
  constructor(store, cache) {
    this.store = store;
    this.cache = cache;
  }

  product(sku) {
    const key = productKey(sku);
    let product = this.cache.get(key);
    if (product === undefined) {
      product = this.store.find(sku);
      if (product) this.cache.set(key, product, PRODUCT_TTL_SECONDS);
    }
    return product;
  }

  listing(category, page = 0) {
    const key = listingKey(category, page);
    let rows = this.cache.get(key);
    if (rows === undefined) {
      const all = this.store.inCategory(category);
      rows = all
        .slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE)
        .map(({ sku, name, priceCents }) => ({ sku, name, priceCents }));
      this.cache.set(key, rows, LISTING_TTL_SECONDS);
    }
    return rows;
  }
}

module.exports = { Catalog };
