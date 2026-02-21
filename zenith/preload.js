// preload.js - Exposes safe APIs to renderer
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electron', {
  // Check if summitd is running
  checkDaemon: () => ipcRenderer.invoke('check-daemon'),
  
  // Start summitd
  startDaemon: (interface) => ipcRenderer.invoke('start-daemon', interface),
  
  // Stop summitd
  stopDaemon: () => ipcRenderer.invoke('stop-daemon'),
  
  // Platform info
  platform: process.platform,
  
  // Version info
  versions: {
    node: process.versions.node,
    chrome: process.versions.chrome,
    electron: process.versions.electron
  }
});
