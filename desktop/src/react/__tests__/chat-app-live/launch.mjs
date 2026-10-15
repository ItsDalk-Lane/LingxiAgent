// 真实 HTTP/WS 透明转发；只控制正文何时到达，不生成或改写业务响应。
import {createServer as createVite} from 'vite';
import http from 'node:http';import net from 'node:net';import fs from 'node:fs';import path from 'node:path';import {spawn} from 'node:child_process';import {fileURLToPath} from 'node:url';
const here=path.dirname(fileURLToPath(import.meta.url)),repo=path.resolve(here,'../../../../..'),home=process.env.R06_CHAT_HOME;
const upstream=new URL(process.env.R06_CHAT_URL);delete process.env.VITEST;
let index=0;const delayed=new Set();const dropped=[];
const proxy=http.createServer((req,res)=>{
 if(req.method==='POST'&&req.url==='/lingxi/v1/session-thinking-level'&&fs.existsSync(path.join(home,'app-drop-thinking'))){dropped.push(req.url);req.destroy();return;}
 const target=http.request({host:upstream.hostname,port:upstream.port,method:req.method,path:req.url,headers:req.headers},reply=>{
  const delay=req.method==='POST'&&req.url==='/lingxi/v1/models/switch'&&fs.existsSync(path.join(home,'app-delay-model'));
  res.writeHead(reply.statusCode,reply.headers);
  if(!delay){reply.pipe(res);return;}
  const id=++index;const chunks=[];reply.on('data',chunk=>chunks.push(chunk));reply.on('end',()=>{
   res.flushHeaders();fs.writeFileSync(path.join(home,`app-delayed-${id}.json`),JSON.stringify({status:reply.statusCode,body:Buffer.concat(chunks).toString()}));
   const timer=setInterval(()=>{if(fs.existsSync(path.join(home,`app-release-${id}`))){clearInterval(timer);delayed.delete(timer);res.end(Buffer.concat(chunks));}},10);delayed.add(timer);
  });
 });target.on('error',err=>res.destroy(err));req.pipe(target);
});
proxy.on('upgrade',(req,socket,head)=>{const target=net.connect(Number(upstream.port),upstream.hostname,()=>{target.write(`${req.method} ${req.url} HTTP/${req.httpVersion}\r\n${Object.entries(req.headers).map(([k,v])=>`${k}: ${v}`).join('\r\n')}\r\n\r\n`);if(head.length)target.write(head);socket.pipe(target);target.pipe(socket);});target.on('error',()=>socket.destroy());socket.on('error',()=>target.destroy());socket.on('close',()=>target.destroy());});
await new Promise(resolve=>proxy.listen(0,'127.0.0.1',resolve));
const portFinder=net.createServer();await new Promise(resolve=>portFinder.listen(0,'127.0.0.1',resolve));const rendererPort=portFinder.address().port;await new Promise(resolve=>portFinder.close(resolve));
const vite=await createVite({configFile:path.join(repo,'vite.config.ts'),root:path.join(repo,'desktop/src'),cacheDir:path.join(home,'.chat-app-vite-cache'),plugins:[{name:'actual-app-surface',transformIndexHtml:html=>html.replace('src="./settings-main.tsx"','src="./react/__tests__/chat-app-live/surface.tsx"')}],resolve:{alias:{'@':path.join(repo,'desktop/src/react')}},esbuild:{jsx:'automatic'},server:{host:'127.0.0.1',port:rendererPort,strictPort:true,hmr:false,fs:{allow:[repo]}},optimizeDeps:{include:['react','react-dom/client','zustand']},logLevel:'error'});
try{await vite.listen();const child=spawn(fs.realpathSync(path.join(repo,'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron')),[path.join(here,'main.cjs')],{env:{...process.env,R06_CHAT_URL:`http://127.0.0.1:${proxy.address().port}`,R06_CHAT_RENDERER:`http://127.0.0.1:${vite.httpServer.address().port}/settings.html`},stdio:['ignore','pipe','pipe']});child.stdout.pipe(process.stdout);child.stderr.pipe(process.stderr);const timeout=setTimeout(()=>child.kill(),90000);const code=await new Promise((resolve,reject)=>{child.on('exit',resolve);child.on('error',reject);});clearTimeout(timeout);if(code!==0)throw Error(`actual AppPages failed ${code}`);}finally{for(const timer of delayed)clearInterval(timer);proxy.closeAllConnections();proxy.close();await vite.close();fs.writeFileSync(path.join(home,'app-proxy-receipt.json'),JSON.stringify({delayed:index,dropped}));}
