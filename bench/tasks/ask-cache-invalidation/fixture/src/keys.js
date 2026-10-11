'use strict';

const productKey = (sku) => `product:${sku}`;
const listingKey = (category, page) => `listing:${category}:${page}`;

module.exports = { productKey, listingKey };
