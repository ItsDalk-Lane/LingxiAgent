// 连接与样本身份全部属于合成服务。
const {contextBridge}=require('electron');contextBridge.exposeInMainWorld('hana',{getServerConnectionInfo:async()=>({port:Number(new URL(process.env.R06_SESSION_URL).port),token:process.env.R06_SESSION_TOKEN}),getPlatform:async()=>'darwin',getSyntheticSessions:async()=>({sessionId:process.env.R06_SESSION_ID,otherSessionId:process.env.R06_SESSION_OTHER_ID})});
