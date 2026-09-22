import { fileURLToPath } from 'node:url';
import fs from 'node:fs';
import process from 'node:process';
const { Buffer, console, fetch, setInterval, clearInterval, setTimeout } = globalThis;
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { spawn, execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
const require = createRequire(process.cwd() + '/package.json');
const repo = process.cwd();
const out = process.env.LINGXI_SOAK_OUTPUT || path.dirname(fileURLToPath(import.meta.url));
const isolated = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-c26-extended-'));
const home = path.join(isolated, path.basename(isolated)+'-data');
fs.mkdirSync(path.join(home, 'agents/lingxi/sessions'), {recursive:true});
fs.mkdirSync(path.join(home, 'user'), {recursive:true});
fs.writeFileSync(path.join(home,'user/preferences.json'),JSON.stringify({setupComplete:true}));
const provider = http.createServer((req,res)=>{
 if(req.method==='GET'){if(req.url==='/soak'){res.writeHead(200,{'content-type':'text/html; charset=utf-8'});res.end('<!doctype html><title>Local browser soak</title><h1>本地浏览器验收页面</h1><p>Only localhost synthetic content</p>');return;}res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({object:'list',data:[{id:'witness-model',object:'model'}]}));return;}
 const chunks=[];req.on('data',chunk=>chunks.push(chunk));req.on('end',()=>{
  fs.appendFileSync(path.join(out,'provider-count.log'),`${new Date().toISOString()} ${req.method} ${req.url}\n`);
  const body=JSON.parse(Buffer.concat(chunks).toString());
  if(!body.stream){res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({id:'local-static',object:'chat.completion',choices:[{index:0,message:{role:'assistant',content:'隔离本地测试助手'},finish_reason:'stop'}]}));return;}
  const messages=body.messages||[];
  const userIndex=messages.findLastIndex(message=>message.role==='user');
  const lastUser=JSON.stringify(messages[userIndex]?.content||'');
  if(lastUser.includes('SOAK_BROWSER')) {
    res.writeHead(200,{'content-type':'text/event-stream'});
    const afterUser=messages.slice(userIndex+1);
    const toolCount=afterUser.filter(message=>message.role==='tool').length;
    const toolDone=toolCount>=2;
    const delta=toolDone?{role:'assistant',content:'本地浏览器循环完成。'}:{role:'assistant',tool_calls:[{index:0,id:'soak-browser-'+Date.now(),type:'function',function:{name:'mcp_call',arguments:JSON.stringify({tool:'browser',arguments:toolCount===0?{action:'start'}:{action:'navigate',url:`http://127.0.0.1:${port}/soak`}})}}]};
    fs.appendFileSync(path.join(out,'tool-fixture.log'),JSON.stringify({at:new Date().toISOString(),toolDone,tools:(body.tools||[]).map(tool=>tool.function?.name)})+'\n');
    res.write(`data: ${JSON.stringify({id:'soak-browser',object:'chat.completion.chunk',model:'witness-model',choices:[{index:0,delta,finish_reason:null}]})}\n\n`);
    res.end(`data: ${JSON.stringify({id:'soak-browser',object:'chat.completion.chunk',choices:[{index:0,delta:{},finish_reason:toolDone?'stop':'tool_calls'}]})}\n\ndata: [DONE]\n\n`);return;
  }
  res.writeHead(200,{'content-type':'text/event-stream'});
  let n=0;
  const timer=setInterval(()=>{
   if(res.destroyed){clearInterval(timer);return;}
   res.write(`data: ${JSON.stringify({id:'local-gui',object:'chat.completion.chunk',created:0,model:'witness-model',choices:[{index:0,delta:{role:'assistant',content:(++n===1?'## 持续输出代码验收\n\n```typescript\n':n===90?'```\n完成。':`const value${n} = ${n}; // 本地合成测试行\n`)},finish_reason:null}]})}\n\n`);
   if(n>=90){clearInterval(timer);res.end(`data: ${JSON.stringify({id:'local-gui',object:'chat.completion.chunk',choices:[{index:0,delta:{},finish_reason:'stop'}]})}\n\ndata: [DONE]\n\n`);}
  },500);
 });
});
await new Promise(resolve=>provider.listen(0,'127.0.0.1',resolve));
const port=provider.address().port;
const template=fs.readFileSync(path.join(repo,'lib/config.example.yaml'),'utf8');
fs.writeFileSync(path.join(home,'agents/lingxi/config.yaml'),template.replace(/^(\s*chat:\s*)".*"/m,'$1{id: witness-model, provider: witness-a}'));
fs.writeFileSync(path.join(home,'provider-catalog.json'),JSON.stringify({catalogVersion:2,providers:{'witness-a':{base_url:`http://127.0.0.1:${port}/v1`,api:'openai-completions',api_key:'synthetic-local-gui-key',models:['witness-model']}}}));
const finder=http.createServer();await new Promise(resolve=>finder.listen(0,'127.0.0.1',resolve));const vitePort=finder.address().port;await new Promise(resolve=>finder.close(resolve));
const env={...process.env,HOME:isolated,LINGXI_HOME:home,LINGXI_DEV_NODE_BIN:process.execPath,LINGXI_CREATE_STARTUP_SESSION:'0',LINGXI_PORT:'0',VITE_DEV_URL:`http://127.0.0.1:${vitePort}`};delete env.ELECTRON_RUN_AS_NODE;
const children=[];
function start(name,bin,args){const log=fs.openSync(path.join(out,`${name}.log`),'w');const child=spawn(bin,args,{cwd:repo,env,stdio:['ignore',log,log]});children.push(child);child.on('exit',(code,signal)=>fs.appendFileSync(path.join(out,'exits.jsonl'),JSON.stringify({name,code,signal,at:new Date().toISOString()})+'\n'));return child;}
start('vite',process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port',String(vitePort),'--strictPort']);
const deadline=Date.now()+30000;while(Date.now()<deadline){try{if((await fetch(env.VITE_DEV_URL)).ok)break;}catch{}await new Promise(resolve=>setTimeout(resolve,200));}
const electron=start('electron',require('electron'),['.','--dev']);
fs.writeFileSync(path.join(out,'launch.json'),JSON.stringify({start:new Date().toISOString(),sha:execFileSync('git',['rev-parse','HEAD']).toString().trim(),node:process.version,isolatedHome:isolated,lingxiHome:home,electronPid:electron.pid,vitePort,providerPort:port,mode:'actual Electron current main.cjs + Vite source + isolated data + local protocol provider'},null,2));
console.log(JSON.stringify({isolated,home,electronPid:electron.pid,vitePort}));
function stop(){for(const child of children)child.kill('SIGTERM');provider.close();setTimeout(()=>process.exit(0),3000);}
process.on('SIGTERM',stop);process.on('SIGINT',stop);
setTimeout(stop,20*60*1000);
