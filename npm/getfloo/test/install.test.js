'use strict';

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { after, before, test } = require('node:test');

const { InstallError, assetName, binaryPath, install } = require('../lib/install.js');

const ASSET = 'floo-x86_64-unknown-linux-musl';
const BINARY = Buffer.from('#!/bin/sh\necho floo\n');

const { publicKey, privateKey } = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
const PUBLIC_KEY_PEM = publicKey.export({ type: 'pkcs1', format: 'pem' });

function sha256File(bytes) {
  return `${crypto.createHash('sha256').update(bytes).digest('hex')}  ${ASSET}\n`;
}

// A release, as GitHub serves it: the binary, its checksum, its signature.
function release(overrides = {}) {
  return {
    [`/${ASSET}`]: BINARY,
    [`/${ASSET}.sha256`]: Buffer.from(sha256File(BINARY)),
    [`/${ASSET}.sig`]: crypto.sign('sha256', BINARY, privateKey),
    ...overrides,
  };
}

let server;
let files = {};
let workDir;

before(async () => {
  server = http.createServer((request, response) => {
    const body = files[request.url];
    response.statusCode = body ? 200 : 404;
    response.end(body);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  workDir = fs.mkdtempSync(path.join(os.tmpdir(), 'getfloo-test-'));
});

after(() => {
  server.close();
  fs.rmSync(workDir, { recursive: true, force: true });
});

function installInto(name) {
  const destination = path.join(workDir, name, 'bin', 'floo');
  const run = install({
    baseUrl: `http://127.0.0.1:${server.address().port}`,
    publicKeyPem: PUBLIC_KEY_PEM,
    asset: ASSET,
    destination,
  });
  return { destination, run };
}

test('installs a release whose checksum and signature verify', async () => {
  files = release();
  const { destination, run } = installInto('ok');
  await run;
  assert.deepEqual(fs.readFileSync(destination), BINARY);
  assert.ok(fs.statSync(destination).mode & 0o100, 'binary is executable');
  assert.deepEqual(fs.readdirSync(path.dirname(destination)), ['floo']);
});

test('refuses a binary that does not match its checksum', async () => {
  files = release({ [`/${ASSET}.sha256`]: Buffer.from(sha256File(Buffer.from('other'))) });
  const { destination, run } = installInto('bad-checksum');
  await assert.rejects(run, (error) => error instanceof InstallError && /Checksum mismatch/.test(error.message));
  assert.equal(fs.existsSync(destination), false);
});

test('refuses a binary signed with a different key', async () => {
  const other = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
  files = release({ [`/${ASSET}.sig`]: crypto.sign('sha256', BINARY, other.privateKey) });
  const { destination, run } = installInto('bad-signature');
  await assert.rejects(run, (error) => error instanceof InstallError && /Signature verification failed/.test(error.message));
  assert.equal(fs.existsSync(destination), false);
});

test('reports a missing release asset with the manual install link', async () => {
  files = {};
  const { run } = installInto('missing');
  await assert.rejects(run, (error) => error instanceof InstallError && /HTTP 404/.test(error.message) && /getfloo\.com\/docs\/guides\/install/.test(error.message));
});

test('maps each supported machine to its release asset', () => {
  assert.equal(assetName('darwin', 'arm64'), 'floo-aarch64-apple-darwin');
  assert.equal(assetName('darwin', 'x64'), 'floo-x86_64-apple-darwin');
  assert.equal(assetName('linux', 'x64'), 'floo-x86_64-unknown-linux-musl');
  assert.equal(assetName('linux', 'arm64'), 'floo-aarch64-unknown-linux-musl');
  assert.equal(assetName('win32', 'x64'), 'floo-x86_64-pc-windows-msvc.exe');
});

test('names the machine when floo has no build for it', () => {
  assert.throws(() => assetName('win32', 'arm64'), InstallError);
  assert.throws(() => assetName('linux', 'ia32'), InstallError);
  assert.throws(() => assetName('freebsd', 'x64'), InstallError);
});

test('keeps the binary under the floo config directory', () => {
  assert.equal(binaryPath('/home/u', 'linux'), path.join('/home/u', '.floo', 'bin', 'floo'));
  assert.equal(binaryPath('/home/u', 'win32'), path.join('/home/u', '.floo', 'bin', 'floo.exe'));
});
