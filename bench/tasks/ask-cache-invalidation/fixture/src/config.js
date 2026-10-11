'use strict';

module.exports = {
  PRODUCT_TTL_SECONDS: 3600,
  // Category listings are expensive to build (joins + sorting), so they are
  // cached separately.
  LISTING_TTL_SECONDS: 600,
  PAGE_SIZE: 24,
};
