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
