// 合成连接仅供当前真实渲染器验收。
const {contextBridge}=require('electron');contextBridge.exposeInMainWorld('hana',{getServerConnectionInfo:async()=>({port:Number(new URL(process.env.R06_SESSION_URL).port),token:process.env.R06_SESSION_TOKEN}),getPlatform:async()=>'darwin'});
