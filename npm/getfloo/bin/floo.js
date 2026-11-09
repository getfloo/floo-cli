#!/usr/bin/env node
'use strict';

const { InstallError, ensureBinary } = require('../lib/install.js');
const { run } = require('../lib/run.js');

async function main() {
  const binary = await ensureBinary();
  const { code, signal } = await run(binary, process.argv.slice(2));
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exitCode = code;
}

main().catch((error) => {
  if (!(error instanceof InstallError)) throw error;
  process.stderr.write(`floo: ${error.message}\n`);
  process.exitCode = 1;
});
