// 使用生产 platform.js 的 Electron 分支，连接资料只指向本轮合成服务。
const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('hana', {
  getServerConnectionInfo: async () => ipcRenderer.invoke('fixture:connection'),
  getPlatform: async () => 'darwin',
  getServerPort: async () => Number(new URL(process.env.R06_SETTINGS_URL).port),
  getServerToken: async () => process.env.R06_SETTINGS_TOKEN,
  showInFinder: async filePath => ipcRenderer.invoke('fixture:show-in-finder', filePath),
  getFilePath: file => file.path || null,
  onSettingsChanged: () => () => {}, onServerRestarted: () => () => {}, onSwitchTab: () => () => {},
  appReady: async () => {}, syncWindowTheme: () => {}, runEditCommand: async () => false,
});
