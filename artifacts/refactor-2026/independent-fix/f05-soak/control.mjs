import process from 'node:process';
const { fetch, console } = globalThis;
import fs from 'node:fs';
import path from 'node:path';
const out='artifacts/refactor-2026/independent-fix/f05-soak';
const launch=JSON.parse(fs.readFileSync(path.join(out,'launch.json')));
const info=JSON.parse(fs.readFileSync(path.join(launch.lingxiHome,'server-info.json')));
const [action,...args]=process.argv.slice(2);
const headers={'Content-Type':'application/json',Authorization:`Bearer ${info.token}`};
const base=`http://127.0.0.1:${info.port}`;
async function req(url,body){const res=await fetch(base+url,{method:body?'POST':'GET',headers,body:body?JSON.stringify(body):undefined});const data=await res.json();fs.appendFileSync(path.join(out,'actions.jsonl'),JSON.stringify({at:new Date().toISOString(),url,status:res.status,data})+'\n');if(!res.ok)throw new Error(`${url} status=${res.status}`);return data;}
if(action==='create'){const result=await req('/api/sessions/new',{cwd:launch.isolatedHome,memoryEnabled:false});fs.writeFileSync(path.join(out,'session.json'),JSON.stringify(result));console.log(JSON.stringify(result));}
if(action==='open'||action==='close'){const session=JSON.parse(fs.readFileSync(path.join(out,'session.json')));const sessionPath=session.path||session.sessionPath||session.session?.path;console.log(await req(`/api/browser/${action}-session`,{sessionPath}));}
if(action==='status')console.log(await req('/api/browser/session-states'));
