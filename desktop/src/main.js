'use strict';

const { app, BrowserWindow, dialog, screen, shell } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const backend = require('./backend');

// Not packaged means we were launched as `electron .` from the repo, where cargo and
// Vite are run by hand in their own terminals. The shell starts no backend there.
const isDev = !app.isPackaged;
const DEV_URL = 'http://127.0.0.1:5173';
const LOAD_TIMEOUT_MS = 30_000;
const LOAD_RETRY_MS = 300;

const DEFAULT_BOUNDS = { width: 1440, height: 900 };

let win = null;
let quitting = false;
let cleanedUp = false;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// --- window state -----------------------------------------------------------------

function stateFile() {
  return path.join(app.getPath('userData'), 'window-state.json');
}

function loadWindowState() {
  try {
    const saved = JSON.parse(fs.readFileSync(stateFile(), 'utf8'));
    if (!Number.isFinite(saved.width) || !Number.isFinite(saved.height)) return DEFAULT_BOUNDS;
    // A remembered position on a monitor that is no longer attached would open the
    // window off-screen, so only restore x/y if they still land on some display.
    const placed =
      Number.isFinite(saved.x) && Number.isFinite(saved.y) && intersectsADisplay(saved);
    return {
      width: saved.width,
      height: saved.height,
      ...(placed ? { x: saved.x, y: saved.y } : {}),
      maximized: Boolean(saved.maximized),
    };
  } catch {
    return DEFAULT_BOUNDS;
  }
}

function intersectsADisplay(bounds) {
  return screen.getAllDisplays().some(({ workArea: a }) => {
    return (
      bounds.x < a.x + a.width &&
      bounds.x + bounds.width > a.x &&
      bounds.y < a.y + a.height &&
      bounds.y + bounds.height > a.y
    );
  });
}

function saveWindowState() {
  if (!win || win.isDestroyed()) return;
  try {
    const bounds = win.isMaximized() ? win.getNormalBounds() : win.getBounds();
    fs.writeFileSync(stateFile(), JSON.stringify({ ...bounds, maximized: win.isMaximized() }));
  } catch {
    /* window position is not worth failing a quit over */
  }
}

// --- window -----------------------------------------------------------------------

function createWindow() {
  const state = loadWindowState();

  win = new BrowserWindow({
    width: state.width,
    height: state.height,
    ...(Number.isFinite(state.x) ? { x: state.x, y: state.y } : {}),
    minWidth: 900,
    minHeight: 600,
    backgroundColor: '#0a0a0a',
    title: 'Interval',
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
      spellcheck: false,
    },
  });

  if (state.maximized) win.maximize();

  // The renderer is an ordinary same-origin page served over HTTP, so there is no
  // preload and nothing to expose. These two guards just keep it that way.
  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:$/.test(safeProtocol(url))) shell.openExternal(url);
    return { action: 'deny' };
  });

  win.webContents.on('will-navigate', (event, url) => {
    const current = win.webContents.getURL();
    if (current && safeOrigin(url) !== safeOrigin(current)) event.preventDefault();
  });

  win.on('close', saveWindowState);
  win.on('closed', () => {
    win = null;
  });

  win.loadFile(path.join(__dirname, 'loading.html'));
}

function safeProtocol(url) {
  try {
    return new URL(url).protocol;
  } catch {
    return '';
  }
}

function safeOrigin(url) {
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
}

/**
 * Retry until the server answers, so launch order does not matter: in dev the Vite
 * server may still be starting, and in the packaged app the backend runs migrations
 * and seeds before it binds.
 */
async function loadWithRetry(url) {
  const deadline = Date.now() + LOAD_TIMEOUT_MS;
  for (;;) {
    if (!win || win.isDestroyed()) return;
    try {
      await win.loadURL(url);
      return;
    } catch (error) {
      if (Date.now() > deadline) {
        dialog.showErrorBox(
          'Interval could not connect',
          `Nothing is serving ${url}.\n\n` +
            (isDev
              ? 'Is the dev server running? Start the backend with:\n' +
                '  cargo run -p interval-backend\n'
              : `${error.message}\n\nLog: ${backend.logFilePath()}`)
        );
        return;
      }
      await sleep(LOAD_RETRY_MS);
    }
  }
}

// --- lifecycle --------------------------------------------------------------------

// Two instances would mean two writers on one SQLite file and two FastF1 venv
// bootstraps racing in the same directory. Claim the lock before anything else.
if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (!win) return;
    if (win.isMinimized()) win.restore();
    win.focus();
  });

  app.on('window-all-closed', () => app.quit());

  app.on('before-quit', (event) => {
    quitting = true;
    if (isDev || cleanedUp) return;
    event.preventDefault();
    backend.stop().finally(() => {
      cleanedUp = true;
      app.quit();
    });
  });

  app.whenReady().then(main);
}

async function main() {
  createWindow();

  let url = DEV_URL;

  if (!isDev) {
    backend.onExit((code, tail) => {
      if (quitting) return;
      dialog.showErrorBox(
        'Interval backend stopped',
        `The backend exited unexpectedly (code ${code}).\n\n` +
          `${tail.trim().slice(-1500)}\n\nLog: ${backend.logFilePath()}`
      );
      app.quit();
    });

    try {
      ({ url } = await backend.start());
    } catch (error) {
      dialog.showErrorBox('Interval failed to start', error.message);
      app.quit();
      return;
    }
  }

  await loadWithRetry(url);
}
