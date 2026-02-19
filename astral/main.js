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
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 800,
    minHeight: 600,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
      preload: path.join(__dirname, 'preload.js')
    },
    autoHideMenuBar: true,
    icon: path.join(__dirname, 'assets/tray-icon.png'),
    title: 'Summit Protocol'
  });

  // Load the app
  if (isDev) {
    mainWindow.loadURL('http://localhost:5173');
    mainWindow.webContents.openDevTools();
  } else {
    mainWindow.loadFile(path.join(__dirname, '/index.html'));
  }

  // Handle window close - minimize to tray instead
  mainWindow.on('close', (event) => {
    if (!app.isQuitting) {
      event.preventDefault();
      mainWindow.hide();
    }
  });
}

// Create system tray
function createTray() {
  tray = new Tray(path.join(__dirname, 'assets/tray-icon.png'));
  
  const contextMenu = Menu.buildFromTemplate([
    {
      label: 'Show Summit',
      click: () => {
        mainWindow.show();
      }
    },
    {
      label: 'Summit Status',
      click: async () => {
        const status = await checkDaemonStatus();
        dialog.showMessageBox({
          title: 'Summit Status',
          message: status.running ? 'Summit is running' : 'Summit is not running',
          detail: status.running ? `Sessions: ${status.sessions}\nPeers: ${status.peers}` : 'Start Summit to see details'
        });
      }
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
  createWindow();
  createTray();
  
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on('window-all-closed', () => {
  // On macOS, keep running in background
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('before-quit', () => {
  app.isQuitting = true;
  stopDaemon();
});
