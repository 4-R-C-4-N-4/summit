// main.js - Electron main process
const { app, BrowserWindow, Menu, Tray, ipcMain, dialog } = require('electron');
const path = require('path');
const { spawn } = require('child_process');

let mainWindow;
let tray;
let summitProcess = null;

const isDev = !app.isPackaged;
const API_PORT = 9001;

// Create the main window
function createWindow() {
  console.log('Creating window...');
  console.log('__dirname:', __dirname);
  console.log('app.isPackaged:', app.isPackaged);

  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 800,
    minHeight: 600,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      preload: path.join(__dirname, 'preload.cjs')
    },
    autoHideMenuBar: true,
    icon: path.join(__dirname, 'assets/tray-icon.png'),
    title: '4str4l',
    show: false
  });


  // Show window when ready
  mainWindow.once('ready-to-show', () => {
    console.log('Window ready to show');
    mainWindow.show();
  });

  // Handle window close - minimize to tray instead
  mainWindow.on('close', (event) => {
    if (!app.isQuitting) {
      event.preventDefault();
      mainWindow.hide();
    }
  });

  mainWindow.webContents.on('did-fail-load', (event, errorCode, errorDescription) => {
    console.error('Failed to load:', errorCode, errorDescription);
  });


  mainWindow.webContents.on('did-finish-load', () => {
    console.log('Finished loading');
  });

  // In packaged app, files are in resources/
  // app.asar is separate from other resources
  let htmlPath;
  if (app.isPackaged) {
    // Go up from app.asar to resources, then into dist
    htmlPath = path.join(process.resourcesPath, 'dist', 'index.html');
  } else {
    htmlPath = path.join(__dirname, 'dist', 'index.html');
  }
  console.log('Loading from:', htmlPath);

  mainWindow.loadFile(htmlPath).catch(err => {
    console.error('Load error:', err);
  });

  /*
  // Load the app
  mainWindow.loadURL('http://localhost:5173');
  if(isDev){
    mainWindow.webContents.openDevTools();
  }*/

  mainWindow.webContents.on('console-message', (event, level, message) => {
    console.log('Console:', message);
  });
  mainWindow.webContents.openDevTools();
}

// Create system tray
function createTray() {
  // Try to create tray, but don't crash if icon missing
  const trayIconPath = path.join(__dirname, 'assets/tray-icon.png');

  // Check if icon exists
  const fs = require('fs');
  if (!fs.existsSync(trayIconPath)) {
    console.warn('Tray icon not found, skipping tray creation');
    return;
  }

  try {
    tray = new Tray(trayIconPath);

    const contextMenu = Menu.buildFromTemplate([
      {
        label: 'Show Summit',
        click: () => mainWindow.show()
      },
      { type: 'separator' },
      {
        label: 'Quit Summit',
        click: () => {
          app.isQuitting = true;
          app.quit();
        }
      }
    ]);

    tray.setToolTip('Summit Protocol');
    tray.setContextMenu(contextMenu);

    tray.on('click', () => {
      mainWindow.show();
    });
  } catch (error) {
    console.warn('Failed to create tray:', error);
  }
}

// Check if summitd is running
async function checkDaemonStatus() {
  try {
    const response = await fetch(`http://127.0.0.1:${API_PORT}/api/status`);
    if (response.ok) {
      const data = await response.json();
      return {
        running: true,
        sessions: data.sessions?.length || 0,
        peers: data.peers_discovered || 0
      };
    }
  } catch (error) {
    // Daemon not running
  }
  return { running: false };
}

// Start summitd daemon
function startDaemon(interface) {
  if (summitProcess) {
    console.log('Daemon already running');
    return;
  }

  console.log(`Starting summitd on ${interface}...`);
  
  summitProcess = spawn('summitd', [interface], {
    stdio: 'inherit'
  });

  summitProcess.on('error', (error) => {
    console.error('Failed to start summitd:', error);
    dialog.showErrorBox('Summit Error', `Failed to start Summit daemon: ${error.message}`);
  });

  summitProcess.on('exit', (code) => {
    console.log(`summitd exited with code ${code}`);
    summitProcess = null;
  });
}

// Stop summitd daemon
function stopDaemon() {
  if (summitProcess) {
    summitProcess.kill();
    summitProcess = null;
  }
}

// ─── Config helpers ──────────────────────────────────────────────────────────

const fs = require('fs');
const os = require('os');

const CONFIG_PATH = path.join(os.homedir(), '.config', 'summit', 'config.toml');

function parseToml(text) {
  const result = {};
  let sectionPath = null;

  for (const raw of text.split('\n')) {
    const line = raw.trim();
    if (!line || line.startsWith('#')) continue;

    const sectionMatch = line.match(/^\[([^\]]+)\]$/);
    if (sectionMatch) {
      sectionPath = sectionMatch[1].split('.');
      let cur = result;
      for (const part of sectionPath) {
        if (!cur[part]) cur[part] = {};
        cur = cur[part];
      }
      continue;
    }

    const kv = line.match(/^(\w+)\s*=\s*(.+)$/);
    if (!kv) continue;
    const key = kv[1];
    const raw_val = kv[2].trim();

    let value;
    if (raw_val === 'true') value = true;
    else if (raw_val === 'false') value = false;
    else if (raw_val.startsWith('"') && raw_val.endsWith('"')) value = raw_val.slice(1, -1);
    else if (raw_val.startsWith('[') && raw_val.endsWith(']')) {
      const inner = raw_val.slice(1, -1).trim();
      value = inner ? inner.split(',').map(s => s.trim().replace(/^"(.*)"$/, '$1')).filter(Boolean) : [];
    }
    else if (!isNaN(raw_val) && raw_val !== '') value = Number(raw_val);
    else value = raw_val;

    let cur = result;
    if (sectionPath) {
      for (const part of sectionPath) {
        if (!cur[part]) cur[part] = {};
        cur = cur[part];
      }
    }
    cur[key] = value;
  }
  return result;
}

// Patch a single key inside a TOML section in-place (string replacement)
function patchToml(text, section, key, newValue) {
  let formatted;
  if (typeof newValue === 'string') formatted = `"${newValue}"`;
  else if (typeof newValue === 'boolean') formatted = String(newValue);
  else if (Array.isArray(newValue)) formatted = `[${newValue.map(v => `"${v}"`).join(', ')}]`;
  else formatted = String(newValue);

  const lines = text.split('\n');
  let inSection = false;

  for (let i = 0; i < lines.length; i++) {
    const t = lines[i].trim();
    if (t === `[${section}]`) { inSection = true; continue; }
    if (t.startsWith('[')) { inSection = false; continue; }
    if (inSection) {
      const m = lines[i].match(/^(\s*)(\w+)(\s*=\s*)/);
      if (m && m[2] === key) {
        lines[i] = `${m[1]}${key}${m[3]}${formatted}`;
        return lines.join('\n');
      }
    }
  }
  return text; // key not found — return unchanged
}

// IPC handlers
ipcMain.handle('check-daemon', async () => {
  return await checkDaemonStatus();
});

ipcMain.handle('start-daemon', async (event, interface) => {
  startDaemon(interface);
  // Wait a bit for daemon to start
  await new Promise(resolve => setTimeout(resolve, 2000));
  return await checkDaemonStatus();
});

ipcMain.handle('stop-daemon', async () => {
  stopDaemon();
  return { running: false };
});

ipcMain.handle('read-config', () => {
  try {
    const text = fs.readFileSync(CONFIG_PATH, 'utf8');
    return { ok: true, config: parseToml(text), path: CONFIG_PATH };
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('write-config', (_, updates) => {
  // updates: [{ section, key, value }, ...]
  try {
    let text = fs.readFileSync(CONFIG_PATH, 'utf8');
    for (const { section, key, value } of updates) {
      text = patchToml(text, section, key, value);
    }
    fs.writeFileSync(CONFIG_PATH, text, 'utf8');
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err.message };
  }
});

ipcMain.handle('open-path', (_, targetPath) => {
  const { shell } = require('electron');
  return shell.openPath(targetPath);
});

// App lifecycle
app.whenReady().then(() => {
  console.log('App ready');
  createWindow();
  createTray();
  
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on('window-all-closed', () => {
    app.quit();
});

app.on('before-quit', () => {
  app.isQuitting = true;
  stopDaemon();
});
