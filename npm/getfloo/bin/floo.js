#!/usr/bin/env node
'use strict';

const { spawnSync } = require('node:child_process');
const { InstallError, ensureBinary } = require('../lib/install.js');

async function main() {
  const binary = await ensureBinary();
  const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });
  if (result.error) {
    throw new InstallError(`Could not run ${binary}: ${result.error.message}`);
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
    return;
  }
  process.exitCode = result.status;
}

main().catch((error) => {
  if (!(error instanceof InstallError)) throw error;
  process.stderr.write(`floo: ${error.message}\n`);
  process.exitCode = 1;
});
