'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

const { sign, verify } = require('../src');

const SECRET = 'k'.repeat(40);

test('a signed token verifies', () => {
  const token = sign({ user: 'ada' }, SECRET, { now: 1000 });
  assert.equal(verify(token, SECRET, { now: 2000 }).user, 'ada');
});

test('tampered payloads are rejected', () => {
  const token = sign({ user: 'ada' }, SECRET, { now: 1000 });
  const [, signature] = token.split('.');
  const forged = `${Buffer.from(JSON.stringify({ user: 'root', exp: 9e15 })).toString('base64url')}.${signature}`;
  assert.equal(verify(forged, SECRET, { now: 2000 }), null);
});

test('expired tokens are rejected', () => {
  const token = sign({ user: 'ada' }, SECRET, { now: 1000, ttlSeconds: 1 });
  assert.equal(verify(token, SECRET, { now: 5000 }), null);
});

test('short secrets are refused', () => {
  assert.throws(() => sign({}, 'short'));
});
