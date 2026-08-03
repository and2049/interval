'use strict';

// Copies the release backend binary to a fixed path so electron-builder's extraResources
// entry is identical on every platform. Run after `cargo build --release`.

const fs = require('node:fs');
const path = require('node:path');

const desktopDir = path.join(__dirname, '..');
const exeName = process.platform === 'win32' ? 'interval-backend.exe' : 'interval-backend';
const source = path.join(desktopDir, '..', 'target', 'release', exeName);
const targetDir = path.join(desktopDir, 'resources', 'backend');
const target = path.join(targetDir, exeName);

if (!fs.existsSync(source)) {
  console.error(
    `Backend binary not found at ${source}\n` +
      'Build it first: cargo build --release -p interval-backend'
  );
  process.exit(1);
}

fs.rmSync(targetDir, { recursive: true, force: true });
fs.mkdirSync(targetDir, { recursive: true });
fs.copyFileSync(source, target);
if (process.platform !== 'win32') fs.chmodSync(target, 0o755);

console.log(`staged ${exeName} -> ${path.relative(desktopDir, target)}`);
