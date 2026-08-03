'use strict';

// Dev launcher: starts Vite and Electron together and tears both down when either dies.
//
// The Rust backend is deliberately NOT started here -- keep `cargo run -p interval-backend`
// in its own terminal so rebuilds and backend logs stay out of the way. Electron loads
// Vite's dev server, whose proxy forwards /api and /healthz to 127.0.0.1:4000, so the
// renderer stays same-origin and HMR is unaffected.

const { spawn } = require('node:child_process');
const path = require('node:path');
const electron = require('electron');

const desktopDir = path.join(__dirname, '..');
const frontendDir = path.join(desktopDir, '..', 'frontend');

const children = [];
let shuttingDown = false;

function killTree(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === 'win32') {
    // Vite runs as a child of bun; only a tree kill gets both.
    spawn('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
  } else {
    child.kill('SIGTERM');
  }
}

function shutdown(code) {
  if (shuttingDown) return;
  shuttingDown = true;
  for (const child of children) killTree(child);
  setTimeout(() => process.exit(code), 200);
}

function run(label, command, args, options) {
  const child = spawn(command, args, { stdio: 'inherit', ...options });
  children.push(child);
  child.on('error', (error) => {
    console.error(`[${label}] failed to start: ${error.message}`);
    shutdown(1);
  });
  child.on('exit', (code) => {
    if (!shuttingDown) {
      console.log(`[${label}] exited (${code}); stopping.`);
      shutdown(code ?? 0);
    }
  });
  return child;
}

console.log('[dev] starting vite + electron (run `cargo run -p interval-backend` separately)');

run('vite', 'bun', ['run', 'dev'], { cwd: frontendDir });
run('electron', electron, ['.'], { cwd: desktopDir });

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => shutdown(0));
}
