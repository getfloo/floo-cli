'use strict';

// Downloads the signed floo release binary for this machine and verifies it
// the same way install.sh does: SHA-256 checksum, then the RSA signature
// against the release public key pinned below. Nothing runs unverified.

const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const RELEASE_BASE_URL = 'https://github.com/getfloo/floo-cli/releases/latest/download';

// Same key as install.sh and release.yml. A release signed with any other key
// fails verification here.
const RELEASE_PUBLIC_KEY_PEM = `-----BEGIN RSA PUBLIC KEY-----
MIIBigKCAYEAoxTcA648/UUTEcmZbiZbwGsQJIjI/CwEda/3Zwky26hOdu3ccQKD
U3lXj7c/cAvr0Y+ISnf23YvBr68q0kI0IhihYE74MoOKe0QjRv7aK0cYgIWKj5SZ
xcw0CLvMm36rNG7iZBHJb3Jbew5ebMpaRyCZBnruHQocHQammzUkuDjeJ753ZFmu
Y8Fyr/CLO+F2V7Bou/qh4DA0tJ8Ams4HLTUGAfXHgj3Q5L9DIZC6iDzGqg70DblC
wNrr/n+zx6TCjonKraYxDUXruR6Za6XrKSbTrq6Bh1DFYK5DM3m9OIdiMx2EC+yD
3iY/CZRC/auqq4CQeXLQyxTsExxnvG3O4Ci77MTZH4NSnngkkw5KrcvqCC9KVI9J
IViei4zB3GoTGDm9+FC02cCozhKiTvAqzdb+ieszMNsavQNdOy1qO9bQfObWWvay
Z4rrRM3hE+rKyk5WHrPZcR77YiqZ6cwXVl7g8gJ0JIQi2a8oHzmjQc7n+j1Nmglh
Wk6BmNyJThezAgMBAAE=
-----END RSA PUBLIC KEY-----
`;

const MANUAL_INSTALL = 'Install another way: https://getfloo.com/docs/guides/install';

class InstallError extends Error {}

function assetName(platform, arch) {
  const cpu = { x64: 'x86_64', arm64: 'aarch64' }[arch];
  if (!cpu) {
    throw new InstallError(`floo has no build for the ${arch} architecture. Supported: x64, arm64.`);
  }
  if (platform === 'darwin') return `floo-${cpu}-apple-darwin`;
  if (platform === 'linux') return `floo-${cpu}-unknown-linux-musl`;
  if (platform === 'win32' && cpu === 'x86_64') return 'floo-x86_64-pc-windows-msvc.exe';
  throw new InstallError(`floo has no build for ${platform} on ${arch}. Supported: macOS, Linux, Windows x64.`);
}

function binaryPath(homeDir, platform) {
  return path.join(homeDir, '.floo', 'bin', platform === 'win32' ? 'floo.exe' : 'floo');
}

async function download(url) {
  let response;
  try {
    response = await fetch(url, { redirect: 'follow' });
  } catch (error) {
    throw new InstallError(`Could not reach ${url}: ${error.cause?.message ?? error.message}. ${MANUAL_INSTALL}`);
  }
  if (!response.ok) {
    throw new InstallError(`Download failed for ${url}: HTTP ${response.status}. ${MANUAL_INSTALL}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

function verify(binary, checksumFile, signature, publicKeyPem, asset) {
  const expected = checksumFile.toString('utf8').trim().split(/\s+/)[0]?.toLowerCase();
  const actual = crypto.createHash('sha256').update(binary).digest('hex');
  if (!expected || expected !== actual) {
    throw new InstallError(`Checksum mismatch for ${asset}. Expected ${expected || 'a checksum'}, got ${actual}. Nothing was installed.`);
  }
  if (!crypto.verify('sha256', binary, publicKeyPem, signature)) {
    throw new InstallError(`Signature verification failed for ${asset}. Nothing was installed.`);
  }
}

// Fetches, verifies, and writes the binary. The write goes to a temp file in
// the destination directory and is renamed into place, so an interrupted run
// never leaves a half-written executable behind.
async function install({ baseUrl, publicKeyPem, asset, destination }) {
  const url = `${baseUrl}/${asset}`;
  const [binary, checksumFile, signature] = await Promise.all([
    download(url),
    download(`${url}.sha256`),
    download(`${url}.sig`),
  ]);
  verify(binary, checksumFile, signature, publicKeyPem, asset);

  fs.mkdirSync(path.dirname(destination), { recursive: true });
  const staging = `${destination}.${process.pid}.tmp`;
  fs.writeFileSync(staging, binary, { mode: 0o755 });
  fs.renameSync(staging, destination);
}

async function ensureBinary() {
  const destination = binaryPath(os.homedir(), process.platform);
  if (!fs.existsSync(destination)) {
    const asset = assetName(process.platform, process.arch);
    process.stderr.write(`Downloading the floo CLI (${asset})...\n`);
    await install({
      baseUrl: RELEASE_BASE_URL,
      publicKeyPem: RELEASE_PUBLIC_KEY_PEM,
      asset,
      destination,
    });
  }
  return destination;
}

module.exports = { InstallError, assetName, binaryPath, ensureBinary, install };
