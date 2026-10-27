// 合成宿主连接只指向真实测试服务，业务请求由生产 renderer 发出。
const { contextBridge } = require('electron');
contextBridge.exposeInMainWorld('hana', {
  getServerConnectionInfo: async () => ({ port: Number(new URL(process.env.R06_SLASH_URL).port), token: process.env.R06_SLASH_TOKEN }),
  getPlatform: async () => 'darwin',
});
