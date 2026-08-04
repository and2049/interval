'use strict';

// Supervises the Rust `interval-backend` child process in the packaged app.
//
// The backend resolves every path it touches (.env, interval.db, backend/assets/tracks,
// scripts/, cache/) relative to its working directory, so the single most important
// thing here is spawning it with a writable per-user data directory as cwd.

const { app } = require('electron');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');

const BASE_PORT = 45900;
const PORT_ATTEMPTS = 5;
const HEALTH_TIMEOUT_MS = 30_000;
const HEALTH_INTERVAL_MS = 250;
const STOP_GRACE_MS = 3_000;
const LOG_TAIL_LIMIT = 8_192;

let child = null;
let logStream = null;
let logTail = '';
let logPath = '';
let onUnexpectedExit = null;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// A subdirectory, not userData itself: Electron keeps its own Cache/, Local Storage/,
// Network/ and friends at the root of userData, and on case-insensitive filesystems our
// `cache/` lands directly inside Chromium's `Cache/`. Keeping the backend's working
// directory separate stops the FastF1 virtualenv from living in the browser cache.
function dataDir() {
  return path.join(app.getPath('userData'), 'data');
}

function logFilePath() {
  return logPath;
}

/**
 * Mirror the read-only payload out of the app bundle into the writable data dir.
 *
 * The `backend/assets/tracks` nesting is not a style choice: curated_tracks.rs hardcodes
 * ASSETS_DIR = "backend/assets/tracks" relative to cwd. Get this wrong and the backend
 * silently falls back to a stub track outline instead of erroring.
 *
 * The payload is ~120 KB and is always exactly what shipped, so it is copied
 * unconditionally rather than version-stamped.
 */
function prepareDataDir(dir, resources) {
  for (const sub of ['logs', 'scripts', path.join('backend', 'assets', 'tracks'), 'cache']) {
    fs.mkdirSync(path.join(dir, sub), { recursive: true });
  }

  const copy = (from, to) => {
    if (fs.existsSync(from)) fs.cpSync(from, to, { recursive: true, force: true });
  };

  copy(path.join(resources, 'assets', 'tracks'), path.join(dir, 'backend', 'assets', 'tracks'));

  // If the FastF1 requirements changed since last launch, drop only the readiness
  // sentinel. ensure_managed_python then re-runs `pip install -r` against the existing
  // venv instead of rebuilding it -- without this, an app update never upgrades FastF1.
  const reqSrc = path.join(resources, 'scripts', 'fastf1-requirements.txt');
  const reqDst = path.join(dir, 'scripts', 'fastf1-requirements.txt');
  const reqChanged =
    fs.existsSync(reqSrc) &&
    (!fs.existsSync(reqDst) ||
      fs.readFileSync(reqSrc, 'utf8') !== fs.readFileSync(reqDst, 'utf8'));

  copy(path.join(resources, 'scripts'), path.join(dir, 'scripts'));

  if (reqChanged) {
    fs.rmSync(path.join(dir, 'cache', 'fastf1-venv', '.interval-fastf1-ready'), { force: true });
  }

  // Seed .env once so the remaining INTERVAL_* switches have an obvious home. The OpenF1
  // token belongs in the settings panel now. Never overwrite: it may hold credentials.
  const envDst = path.join(dir, '.env');
  const envSrc = path.join(resources, '.env.example');
  if (!fs.existsSync(envDst) && fs.existsSync(envSrc)) fs.copyFileSync(envSrc, envDst);
}

/**
 * The backend reads .env only for keys absent from its process environment, so anything
 * inherited here silently overrides the user's own <userData>/.env. Worse, a leaked
 * INTERVAL_REBUILD_SESSION_ON_START aborts startup before the port is bound and no
 * window ever appears. So: strip everything the app owns, then set only what we control.
 */
function childEnv(port, resources) {
  const env = { ...process.env };
  for (const key of Object.keys(env)) {
    if (key.startsWith('INTERVAL_') || key === 'DATABASE_URL' || key === 'RUST_LOG') {
      delete env[key];
    }
  }
  env.INTERVAL_BIND = `127.0.0.1:${port}`;
  env.INTERVAL_STATIC_DIR = path.join(resources, 'frontend');
  env.INTERVAL_SHUTDOWN_ON_STDIN_EOF = '1';
  // The settings routes read and write an API credential and the backend has no
  // authentication, so they are desktop-only: they are exempt from the permissive CORS
  // layer and must never be enabled on anything reachable beyond 127.0.0.1. The strip
  // loop above means this cannot be forced on from the parent environment.
  env.INTERVAL_ENABLE_SETTINGS_API = '1';
  return env;
}

function backendExecutable(resources) {
  const name = process.platform === 'win32' ? 'interval-backend.exe' : 'interval-backend';
  return path.join(resources, 'backend', name);
}

function pingHealth(port) {
  return new Promise((resolve) => {
    const req = http.get(
      { host: '127.0.0.1', port, path: '/healthz', timeout: 1_000 },
      (res) => {
        res.resume();
        resolve(res.statusCode === 200);
      }
    );
    req.on('error', () => resolve(false));
    req.on('timeout', () => {
      req.destroy();
      resolve(false);
    });
  });
}

function looksLikePortConflict(text) {
  return /10048|EADDRINUSE|address (already )?in use|AddrInUse/i.test(text);
}

function attachLogging(proc) {
  const record = (buf) => {
    const text = buf.toString();
    if (logStream) logStream.write(text);
    logTail = (logTail + text).slice(-LOG_TAIL_LIMIT);
    process.stdout.write(text);
  };
  proc.stdout.on('data', record);
  proc.stderr.on('data', record);
}

/**
 * Starts the backend, retrying on the next port if it fails to bind. The bind attempt
 * itself is the port check -- probing for a free port first would leave a race window.
 * A stable port also matters because the renderer's localStorage is keyed by origin,
 * so a moving port would reset the remembered session on every launch.
 */
async function start() {
  const dir = dataDir();
  const resources = process.resourcesPath;
  const exe = backendExecutable(resources);

  if (!fs.existsSync(exe)) {
    throw new Error(`Backend executable not found at:\n${exe}`);
  }

  prepareDataDir(dir, resources);

  logPath = path.join(dir, 'logs', 'backend.log');
  logStream = fs.createWriteStream(logPath, { flags: 'w' });

  const basePort = Number(process.env.INTERVAL_DESKTOP_PORT) || BASE_PORT;
  let lastError = null;

  for (let attempt = 0; attempt < PORT_ATTEMPTS; attempt += 1) {
    const port = basePort + attempt;
    logTail = '';

    const proc = spawn(exe, [], {
      cwd: dir,
      env: childEnv(port, resources),
      // stdin must be a pipe: closing it is how the backend learns we are gone.
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });

    const exited = { done: false, code: null };
    proc.on('exit', (code) => {
      exited.done = true;
      exited.code = code;
    });
    proc.on('error', (err) => {
      exited.done = true;
      lastError = err;
    });
    attachLogging(proc);

    const deadline = Date.now() + HEALTH_TIMEOUT_MS;
    let healthy = false;
    while (Date.now() < deadline && !exited.done) {
      if (await pingHealth(port)) {
        healthy = true;
        break;
      }
      await sleep(HEALTH_INTERVAL_MS);
    }

    if (healthy) {
      child = proc;
      proc.on('exit', (code) => {
        if (child === proc && onUnexpectedExit) onUnexpectedExit(code, logTail);
      });
      return { url: `http://127.0.0.1:${port}`, port };
    }

    if (exited.done && looksLikePortConflict(logTail)) {
      continue; // port taken -- try the next one
    }

    try {
      proc.kill();
    } catch {
      /* already gone */
    }

    const detail = logTail.trim() || (lastError && lastError.message) || '(no output)';
    throw new Error(
      exited.done
        ? `The backend exited during startup (code ${exited.code}).\n\n${detail}\n\nLog: ${logPath}`
        : `The backend did not respond within ${HEALTH_TIMEOUT_MS / 1000}s.\n\n${detail}\n\nLog: ${logPath}`
    );
  }

  throw new Error(
    `Could not find a free port in ${basePort}-${basePort + PORT_ATTEMPTS - 1}.\n\nLog: ${logPath}`
  );
}

/** Closing stdin triggers the backend's graceful shutdown; kill() is only a backstop. */
async function stop() {
  const proc = child;
  child = null;
  if (!proc) return;

  try {
    proc.stdin.end();
  } catch {
    /* pipe already closed */
  }

  const exited = new Promise((resolve) => proc.once('exit', resolve));
  await Promise.race([exited, sleep(STOP_GRACE_MS)]);

  if (proc.exitCode === null && proc.signalCode === null) {
    try {
      proc.kill();
    } catch {
      /* already gone */
    }
  }

  if (logStream) {
    logStream.end();
    logStream = null;
  }
}

function onExit(handler) {
  onUnexpectedExit = handler;
}

module.exports = { start, stop, onExit, dataDir, logFilePath };
