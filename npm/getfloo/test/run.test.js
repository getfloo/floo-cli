'use strict';

const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, before, test } = require('node:test');

const { InstallError } = require('../lib/install.js');
const { run } = require('../lib/run.js');

const RUN_MODULE = require.resolve('../lib/run.js');

let workDir;

before(() => {
  workDir = fs.mkdtempSync(path.join(os.tmpdir(), 'getfloo-run-'));
});

after(() => {
  fs.rmSync(workDir, { recursive: true, force: true });
});

function waitForFile(file) {
  return new Promise((resolve) => {
    const timer = setInterval(() => {
      if (fs.existsSync(file)) {
        clearInterval(timer);
        resolve();
      }
    }, 20);
  });
}

test('passes the exit code through', async () => {
  const result = await run(process.execPath, ['-e', 'process.exit(7)']);
  assert.deepEqual(result, { code: 7, signal: null });
});

test('reports a binary that cannot start', async () => {
  await assert.rejects(run(path.join(workDir, 'missing'), []), InstallError);
});

test('a SIGTERM sent to the launcher reaches the CLI, which shuts down cleanly', { skip: process.platform === 'win32' }, async () => {
  const ready = path.join(workDir, 'ready');
  const cleanedUp = path.join(workDir, 'cleaned-up');
  // Stands in for the CLI: it stays up until SIGTERM, then records its cleanup.
  const cli = `
    process.on('SIGTERM', () => { require('node:fs').writeFileSync(${JSON.stringify(cleanedUp)}, ''); process.exit(0); });
    require('node:fs').writeFileSync(${JSON.stringify(ready)}, '');
    setInterval(() => {}, 1000);
  `;
  const launcher = spawn(process.execPath, [
    '-e',
    `require(${JSON.stringify(RUN_MODULE)}).run(process.execPath, ['-e', ${JSON.stringify(cli)}]).then(({ code }) => { process.exitCode = code; });`,
  ]);
  const exited = new Promise((resolve) => launcher.on('exit', (code, signal) => resolve({ code, signal })));

  await waitForFile(ready);
  launcher.kill('SIGTERM');

  assert.deepEqual(await exited, { code: 0, signal: null });
  assert.ok(fs.existsSync(cleanedUp), 'the CLI ran its shutdown');
});
