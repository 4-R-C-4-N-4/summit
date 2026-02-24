// preload.js - Exposes safe APIs to renderer
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electron', {
  // Daemon control
  checkDaemon: () => ipcRenderer.invoke('check-daemon'),
  startDaemon: (iface) => ipcRenderer.invoke('start-daemon', iface),
  stopDaemon: () => ipcRenderer.invoke('stop-daemon'),

  // Config
  readConfig: () => ipcRenderer.invoke('read-config'),
  writeConfig: (updates) => ipcRenderer.invoke('write-config', updates),

  // Shell
  openPath: (p) => ipcRenderer.invoke('open-path', p),

  // Platform info
  platform: process.platform,
  versions: {
    node: process.versions.node,
    chrome: process.versions.chrome,
    electron: process.versions.electron,
  },
});
