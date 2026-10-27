// 所有资料来自合成服务，输入框自身仍使用生产持久化。
const {contextBridge}=require('electron');
contextBridge.exposeInMainWorld('hana', {getServerConnectionInfo: async()=>({port:Number(new URL(process.env.R06_SESSION_URL).port),token:process.env.R06_SESSION_TOKEN}),getPlatform:async()=> 'darwin', getSyntheticSessions:async()=>({sessionId:process.env.R06_SESSION_ID,otherSessionId:process.env.R06_SESSION_OTHER_ID})});
