'use strict';

// Stand-in for the database.
class ProductStore {
  constructor(products = []) {
    this.products = new Map(products.map((p) => [p.sku, { ...p }]));
  }

  find(sku) {
    const product = this.products.get(sku);
    return product ? { ...product } : undefined;
  }

  inCategory(category) {
    return [...this.products.values()]
      .filter((p) => p.category === category)
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((p) => ({ ...p }));
  }

  update(sku, fields) {
    const product = this.products.get(sku);
    if (!product) throw new Error(`unknown sku ${sku}`);
    Object.assign(product, fields);
    return { ...product };
  }
}

module.exports = { ProductStore };
