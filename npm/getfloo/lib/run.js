'use strict';

const { spawn } = require('node:child_process');
const { InstallError } = require('./install.js');

// A supervisor or agent that cancels a command signals the launcher's PID,
// not its process group. Pass those signals on so the CLI can shut down
// cleanly (floo dev stops its services on SIGTERM) and wait for it to exit.
const FORWARDED_SIGNALS = ['SIGINT', 'SIGTERM', 'SIGHUP'];

function run(binary, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { stdio: 'inherit' });
    const forward = (signal) => child.kill(signal);
    for (const signal of FORWARDED_SIGNALS) process.on(signal, forward);
    const stopForwarding = () => {
      for (const signal of FORWARDED_SIGNALS) process.off(signal, forward);
    };
    child.on('error', (error) => {
      stopForwarding();
      reject(new InstallError(`Could not run ${binary}: ${error.message}`));
    });
    child.on('exit', (code, signal) => {
      stopForwarding();
      resolve({ code, signal });
    });
  });
}

module.exports = { run };
