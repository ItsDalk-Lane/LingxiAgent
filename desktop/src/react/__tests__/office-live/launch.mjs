// 实际 Vite 和 Electron 仅从合成目录启动，不拷贝产物或业务资料。
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const here=path.dirname(fileURLToPath(import.meta.url));const repo=path.resolve(here,'../../../../..');
if(!process.env.R06_OFFICE_HOME||!process.env.R06_OFFICE_BIN)throw Error('synthetic service required');
delete process.env.VITEST;
const vite=await createServer({configFile:path.join(repo,'vite.config.ts'),root:path.join(repo,'desktop/src'),cacheDir:path.join(process.env.R06_OFFICE_HOME,'.office-vite-cache'),plugins:[{name:'actual-office-surface',transformIndexHtml:html=>html.replace('src="./settings-main.tsx"','src="./react/__tests__/office-live/surface.tsx"')}],resolve:{alias:{'@':path.join(repo,'desktop/src/react')}},esbuild:{jsx:'automatic'},server:{host:'127.0.0.1',port:0,hmr:false,fs:{allow:[repo]}},optimizeDeps:{include:['react','react-dom/client','zustand']},logLevel:'error'});
try{await vite.listen();const child=spawn(fs.realpathSync(path.join(repo,'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron')),[path.join(here,'main.cjs')],{env:{...process.env,R06_OFFICE_RENDERER:`http://127.0.0.1:${vite.httpServer.address().port}/settings.html`},stdio:['ignore','pipe','pipe']});child.stdout.pipe(process.stdout);child.stderr.pipe(process.stderr);const timeout=setTimeout(()=>child.kill(),60000);const code=await new Promise((resolve,reject)=>{child.on('exit',resolve);child.on('error',reject);});clearTimeout(timeout);if(code!==0)throw Error(`actual office Electron failed ${code}`);}finally{await vite.close();}
