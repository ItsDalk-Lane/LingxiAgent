import process from 'node:process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const { fetch, setTimeout, clearTimeout } = globalThis;
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
import WebSocket from '/Users/study_superior/Desktop/Code/LingxiAgent/node_modules/ws/index.js';
const out=process.env.LINGXI_SOAK_OUTPUT || path.dirname(fileURLToPath(import.meta.url));
const launch=JSON.parse(fs.readFileSync(`${out}/launch.json`));
const info=JSON.parse(fs.readFileSync(`${launch.lingxiHome}/server-info.json`));
const headers={'Content-Type':'application/json',Authorization:`Bearer ${info.token}`};
const log=(file,data)=>fs.appendFileSync(`${out}/${file}.jsonl`,JSON.stringify({at:new Date().toISOString(),...data})+'\n');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function api(route,body){const res=await fetch(`http://127.0.0.1:${info.port}/api/${route}`,{method:body?'POST':'GET',headers,body:body?JSON.stringify(body):undefined});const data=await res.json();log('actions',{route,status:res.status,data});if(!res.ok)throw Error(route);return data;}
async function task(session,text,cancel=false){return new Promise((resolve,reject)=>{const ws=new WebSocket(`ws://127.0.0.1:${info.port}/ws`,{headers});let aborted=false;let streamId;const timer=setTimeout(()=>{ws.close();reject(Error('task timeout '+text));},60000);ws.on('open',()=>ws.send(JSON.stringify({type:'prompt',text,sessionPath:session.path,agentId:session.agentId})));ws.on('message',raw=>{const e=JSON.parse(raw);if(e.sessionPath!==session.path)return;log('events',{text,event:e});const b=e.block; if(b?.type==='session_confirmation'&&b.payload?.toolName==='browser'&&(b.payload.params.action==='start'||(b.payload.params.action==='navigate'&&b.payload.params.url===`http://127.0.0.1:${launch.providerPort}/soak`))){api('confirm/'+b.confirmId,{action:'confirmed'}).catch(reject);}if(e.streamId)streamId=e.streamId;if(cancel&&!aborted&&e.type==='text_delta'){aborted=true;ws.send(JSON.stringify({type:'abort',sessionPath:session.path,streamId}));}if(e.type==='assistant_run_end'){clearTimeout(timer);ws.close();resolve({aborted,end:e});}});ws.on('error',reject);});}
async function sample(window){await sleep(1500);for(let i=0;i<3;i++){const rows=execFileSync('ps',['-axo','pid=,ppid=,rss=,comm=']).toString().trim().split('\n').map(line=>{const m=line.trim().match(/^(\d+)\s+(\d+)\s+(\d+)\s+(.+)$/);return m&&{pid:+m[1],ppid:+m[2],rssKB:+m[3],command:m[4]};}).filter(Boolean);const ids=new Set([launch.electronPid]);for(let pass=0;pass<10;pass++)for(const row of rows)if(ids.has(row.ppid))ids.add(row.pid);const processes=rows.filter(r=>ids.has(r.pid));log('resources',{window,sample:i,processCount:processes.length,rssKB:processes.reduce((s,r)=>s+r.rssKB,0),processes});await sleep(1000);}}
const started=Date.now();
await sample('baseline');
const sessions=[];
for(let batch=1;batch<=5;batch++){
 const batchSessions=[];
 for(let slot=1;slot<=6;slot++){
  const cycle=(batch-1)*6+slot;
  const session=await api('sessions/new-detached',{cwd:launch.isolatedHome,memoryEnabled:false,permissionMode:'ask'});
  sessions.push(session);batchSessions.push(session);
  log('task-results',{cycle,batch,phase:'browser',result:await task(session,'SOAK_BROWSER '+cycle)});
  const states=await api('browser/session-states');
  if(Object.values(states).filter(state=>state.running).length>5)throw Error('browser cap exceeded');
 }
 await sample('batch-'+batch+'-open');
 await sleep(16000);
 await sample('batch-'+batch+'-settled-open');
 for(let slot=1;slot<=6;slot++){
  const cycle=(batch-1)*6+slot;const session=batchSessions[slot-1];
  const result=await task(session,'SOAK_CANCEL '+cycle,true);
  if(result.end.status!=='aborted')throw Error('cancel did not abort');
  log('task-results',{cycle,batch,phase:'cancel',result});
  await api('browser/close-session',{sessionPath:session.path});
 }
 const closed=await api('browser/session-states');if(Object.values(closed).some(state=>state.running))throw Error('browser did not close');
 await sample('batch-'+batch+'-closed');
 await sleep(18000);
 await sample('batch-'+batch+'-settled-closed');
}
fs.writeFileSync(`${out}/soak-result.json`,JSON.stringify({start:new Date(started).toISOString(),end:new Date().toISOString(),elapsedMs:Date.now()-started,cycles:30,status:'completed',sessions:sessions.map(s=>s.path)},null,2));
