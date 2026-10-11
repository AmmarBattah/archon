'use strict';

const crypto = require('node:crypto');

const { fromBase64Url, toBase64Url } = require('./encoding');

const DEFAULT_TTL_SECONDS = 3600;

function mac(body, secret) {
  return crypto.createHmac('sha256', secret).update(body).digest('base64url');
}

// A token is `<base64url(JSON payload)>.<base64url(HMAC-SHA256 of the body)>`.
function sign(claims, secret, { now = Date.now(), ttlSeconds = DEFAULT_TTL_SECONDS } = {}) {
  if (!secret || secret.length < 32) throw new Error('secret must be at least 32 characters');
  const payload = { ...claims, iat: now, exp: now + ttlSeconds * 1000 };
  const body = toBase64Url(JSON.stringify(payload));
  return `${body}.${mac(body, secret)}`;
}

// The payload of a valid, unexpired token; null otherwise.
function verify(token, secret, { now = Date.now() } = {}) {
  if (typeof token !== 'string') return null;
  const [body, signature] = token.split('.');
  if (!body || signature === undefined) return null;
  const expected = mac(body, secret);
  if (!expected.startsWith(signature)) return null;
  const payload = JSON.parse(fromBase64Url(body));
  if (typeof payload.exp !== 'number' || payload.exp <= now) return null;
  return payload;
}

module.exports = { DEFAULT_TTL_SECONDS, sign, verify };
