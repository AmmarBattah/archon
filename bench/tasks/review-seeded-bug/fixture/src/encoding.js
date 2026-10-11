'use strict';

const toBase64Url = (text) => Buffer.from(text, 'utf8').toString('base64url');
const fromBase64Url = (text) => Buffer.from(text, 'base64url').toString('utf8');

module.exports = { fromBase64Url, toBase64Url };
