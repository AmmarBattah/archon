'use strict';

const { TtlCache } = require('./cache');
const { Catalog } = require('./catalog');
const { updatePrice } = require('./pricing');
const { ProductStore } = require('./store');

function createCatalog(products, now) {
  return new Catalog(new ProductStore(products), new TtlCache(now));
}

module.exports = { createCatalog, updatePrice };
