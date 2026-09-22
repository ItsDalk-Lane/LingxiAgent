const { fetch, setTimeout, clearTimeout } = globalThis;
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
import WebSocket from 'ws';
const out='artifacts/refactor-2026/independent-fix/f05-soak';
const launch=JSON.parse(fs.readFileSync(`${out}/launch.json`));
const info=JSON.parse(fs.readFileSync(`${launch.lingxiHome}/server-info.json`));
const headers={'Content-Type':'application/json',Authorization:`Bearer ${info.token}`};
const log=(file,data)=>fs.appendFileSync(`${out}/${file}.jsonl`,JSON.stringify({at:new Date().toISOString(),...data})+'\n');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
async function api(route,body){const res=await fetch(`http://127.0.0.1:${info.port}/api/${route}`,{method:body?'POST':'GET',headers,body:body?JSON.stringify(body):undefined});const data=await res.json();log('actions',{route,status:res.status,data});if(!res.ok)throw Error(route);return data;}
async function task(session,text,cancel=false){return new Promise((resolve,reject)=>{const ws=new WebSocket(`ws://127.0.0.1:${info.port}/ws`,{headers});let aborted=false;let streamId;const timer=setTimeout(()=>{ws.close();reject(Error('task timeout '+text));},60000);ws.on('open',()=>ws.send(JSON.stringify({type:'prompt',text,sessionPath:session.path,agentId:session.agentId})));ws.on('message',raw=>{const e=JSON.parse(raw);if(e.sessionPath!==session.path)return;log('events',{text,event:e});const b=e.block; if(b?.type==='session_confirmation'&&b.payload?.toolName==='browser'&&(b.payload.params.action==='start'||(b.payload.params.action==='navigate'&&b.payload.params.url===`http://127.0.0.1:${launch.providerPort}/soak`))){api('confirm/'+b.confirmId,{action:'confirmed'}).catch(reject);}if(e.streamId)streamId=e.streamId;if(cancel&&!aborted&&e.type==='text_delta'){aborted=true;ws.send(JSON.stringify({type:'abort',sessionPath:session.path,streamId}));}if(e.type==='assistant_run_end'){clearTimeout(timer);ws.close();resolve({aborted,end:e});}});ws.on('error',reject);});}
async function sample(window){await sleep(1500);for(let i=0;i<3;i++){const rows=execFileSync('ps',['-axo','pid=,ppid=,rss=,comm=']).toString().trim().split('\n').map(line=>{const m=line.trim().match(/^(\d+)\s+(\d+)\s+(\d+)\s+(.+)$/);return m&&{pid:+m[1],ppid:+m[2],rssKB:+m[3],command:m[4]};}).filter(Boolean);const ids=new Set([launch.electronPid]);for(let pass=0;pass<10;pass++)for(const row of rows)if(ids.has(row.ppid))ids.add(row.pid);const processes=rows.filter(r=>ids.has(r.pid));log('resources',{window,sample:i,processCount:processes.length,rssKB:processes.reduce((s,r)=>s+r.rssKB,0),processes});await sleep(1000);}}
await sample('baseline');
const sessions=[];
for(let cycle=1;cycle<=6;cycle++){const session=await api('sessions/new-detached',{cwd:launch.isolatedHome,memoryEnabled:false,permissionMode:'ask'});sessions.push(session);log('task-results',{cycle,phase:'browser',result:await task(session,'SOAK_BROWSER '+cycle)});await api('browser/session-states');if(cycle===1||cycle===3||cycle===6)await sample('open-'+cycle);}
for(let cycle=1;cycle<=6;cycle++){const session=sessions[cycle-1];log('task-results',{cycle,phase:'cancel',result:await task(session,'SOAK_CANCEL '+cycle,true)});await api('browser/close-session',{sessionPath:session.path});if(cycle===1||cycle===3||cycle===6)await sample('closed-'+cycle);}
await api('browser/session-states');fs.writeFileSync(`${out}/soak-result.json`,JSON.stringify({end:new Date().toISOString(),cycles:6,status:'completed',sessions:sessions.map(s=>s.path)},null,2));
