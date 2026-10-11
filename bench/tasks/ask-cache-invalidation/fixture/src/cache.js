'use strict';

// A tiny TTL cache; `now` is injectable for tests.
class TtlCache {
  constructor(now = () => Date.now()) {
    this.now = now;
    this.entries = new Map();
  }

  get(key) {
    const entry = this.entries.get(key);
    if (!entry) return undefined;
    if (entry.expiresAt <= this.now()) {
      this.entries.delete(key);
      return undefined;
    }
    return entry.value;
  }

  set(key, value, ttlSeconds) {
    this.entries.set(key, { value, expiresAt: this.now() + ttlSeconds * 1000 });
  }

  delete(key) {
    this.entries.delete(key);
  }
}

module.exports = { TtlCache };
