import fs from 'node:fs';
import process from 'node:process';
const { Buffer, console, setInterval, clearInterval, setTimeout } = globalThis;
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { spawn } from 'node:child_process';
const repo = process.cwd();
const out = path.join(repo, 'artifacts/refactor-2026/independent-fix/f05-package');
const isolated = fs.mkdtempSync(path.join(os.tmpdir(), 'lingxi-f05-packaged-run-'));
const home = path.join(isolated, path.basename(isolated));
fs.mkdirSync(path.join(home, 'agents/lingxi/sessions'), {recursive:true});
fs.mkdirSync(path.join(home, 'user'), {recursive:true});
fs.writeFileSync(path.join(home,'user/preferences.json'),JSON.stringify({setupComplete:true}));
const workspace=path.join(isolated,'Desktop/OH-WorkSpace');fs.mkdirSync(workspace,{recursive:true});fs.copyFileSync(path.join(repo,'tests/fixtures/document-extract/sample-text.pdf'),path.join(workspace,'package-test.pdf'));
const provider = http.createServer((req,res)=>{
 if(req.url==='/page'){res.writeHead(200,{'content-type':'text/html; charset=utf-8'});res.end('<!doctype html><title>Lingxi packaged browser witness</title><h1>本地新包浏览器验证</h1><p>PACKAGED_BROWSER_OK</p>');return;}
 if(req.method==='GET'){res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({object:'list',data:[{id:'witness-model',object:'model'}]}));return;}
 const chunks=[];req.on('data',chunk=>chunks.push(chunk));req.on('end',()=>{
  fs.appendFileSync(path.join(out,'provider-count.log'),`${new Date().toISOString()} ${req.method} ${req.url}\n`);
  const body=JSON.parse(Buffer.concat(chunks).toString());
  if(!body.stream){res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({id:'local-static',object:'chat.completion',choices:[{index:0,message:{role:'assistant',content:'隔离本地测试助手'},finish_reason:'stop'}]}));return;}
  if(process.env.LINGXI_PACKAGE_TERMINAL_SMOKE==='1'&&!body.messages?.some(message=>message.role==='tool')){
   const chunk={id:'local-terminal',object:'chat.completion.chunk',created:0,model:'witness-model',choices:[{index:0,delta:{role:'assistant',tool_calls:[{index:0,id:'terminal-witness',type:'function',function:{name:'exec_command',arguments:JSON.stringify({cmd:'printf PACKAGED_TERMINAL_GUI_START; sleep 10; printf PACKAGED_TERMINAL_GUI_DONE',tty:true,yield_time_ms:1000})}}]},finish_reason:null}]};
   const done={id:'local-terminal',object:'chat.completion.chunk',choices:[{index:0,delta:{},finish_reason:'tool_calls'}]};
   res.writeHead(200,{'content-type':'text/event-stream'});res.end(`data: ${JSON.stringify(chunk)}\n\ndata: ${JSON.stringify(done)}\n\ndata: [DONE]\n\n`);return;
  }
  res.writeHead(200,{'content-type':'text/event-stream'});
  let n=0;
  const timer=setInterval(()=>{
   if(res.destroyed){clearInterval(timer);return;}
   res.write(`data: ${JSON.stringify({id:'local-gui',object:'chat.completion.chunk',created:0,model:'witness-model',choices:[{index:0,delta:{role:'assistant',content:`隔离验收输出第${++n}行：中文输入、选中、复制和滚动验证。\n${n===8?`[打开本地验收页面](http://127.0.0.1:${port}/page)\n`:""}`},finish_reason:null}]})}\n\n`);
   if(n>=8){clearInterval(timer);res.end(`data: ${JSON.stringify({id:'local-gui',object:'chat.completion.chunk',choices:[{index:0,delta:{},finish_reason:'stop'}]})}\n\ndata: [DONE]\n\n`);}
  },500);
 });
});
await new Promise(resolve=>provider.listen(0,'127.0.0.1',resolve));
const port=provider.address().port;
const template=fs.readFileSync(path.join(repo,'lib/config.example.yaml'),'utf8');
fs.writeFileSync(path.join(home,'agents/lingxi/config.yaml'),template.replace(/^(\s*chat:\s*)".*"/m,'$1{id: witness-model, provider: witness-a}').replace(/^desk:\s*$/m, 'desk:\n  home_folder: '+JSON.stringify(workspace)));
fs.writeFileSync(path.join(home,'provider-catalog.json'),JSON.stringify({catalogVersion:2,providers:{'witness-a':{base_url:`http://127.0.0.1:${port}/v1`,api:'openai-completions',api_key:'synthetic-local-gui-key',models:['witness-model']}}}));
const env={...process.env,LINGXI_HOME:home,LINGXI_CREATE_STARTUP_SESSION:'0',LINGXI_PORT:'0'};
for(const key of Object.keys(env)){if(/^(ELECTRON_RUN_AS_NODE|VITE_DEV_URL|LINGXI_ROOT|LINGXI_DEV_|LINGXI_SERVER_ENTRY|LINGXI_SIGN_|CSC_|APPLE_)/.test(key))delete env[key];}
const context=JSON.parse(fs.readFileSync(path.join(out,'build-context.json'),'utf8'));
const app=path.join(context.temp,'package/mac-arm64/Lingxi.app');
const children=[];
function start(name,bin,args){const log=fs.openSync(path.join(out,`${name}.log`),'w');const child=spawn(bin,args,{cwd:isolated,env,stdio:['ignore',log,log]});children.push(child);child.on('exit',(code,signal)=>fs.appendFileSync(path.join(out,'exits.jsonl'),JSON.stringify({name,code,signal,at:new Date().toISOString()})+'\n'));return child;}
const electron=start('packaged-electron',path.join(app,'Contents/MacOS/Lingxi'),[]);
fs.writeFileSync(path.join(out,'packaged-launch.json'),JSON.stringify({start:new Date().toISOString(),sourceSnapshot:'source-snapshot.json',isolatedHome:isolated,lingxiHome:home,electronPid:electron.pid,app,providerPort:port,mode:'packaged Electron + bundled seed server + bundled renderer + isolated data + local protocol provider'},null,2));
console.log(JSON.stringify({isolated,home,electronPid:electron.pid,app}));
function stop(){for(const child of children)child.kill('SIGTERM');provider.close();setTimeout(()=>process.exit(0),3000);}
process.on('SIGTERM',stop);process.on('SIGINT',stop);
setTimeout(stop,20*60*1000);
