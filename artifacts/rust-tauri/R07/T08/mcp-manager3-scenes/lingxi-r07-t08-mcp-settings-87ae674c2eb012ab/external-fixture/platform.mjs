
import fs from 'node:fs';import readline from 'node:readline';import {spawn} from 'node:child_process';
const root=process.argv[2],mode=process.argv[3];
const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});
fs.writeFileSync(root+'/pids.json',JSON.stringify({parent:process.pid,child:child.pid}));
setInterval(()=>{},1000);
const send=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
readline.createInterface({input:process.stdin}).on('line',line=>{
 const m=JSON.parse(line);if(!m.method)return;fs.appendFileSync(root+'/arrivals.ndjson',JSON.stringify({method:m.method})+'\n');
 if(m.method==='initialize'){
  fs.writeFileSync(root+'/initialize.json','{}');
  const answer=()=>send(m.id,{protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'owned-platform',version:'1'}});
  if(mode==='hold-init'){const timer=setInterval(()=>{if(fs.existsSync(root+'/release')){clearInterval(timer);answer();}},10);}else answer();
 }
 if(m.method==='tools/list')send(m.id,{tools:[{name:'env_report',description:'owned MCP tool',inputSchema:{type:'object',properties:{}}}]});
 if(m.method==='tools/call'){fs.writeFileSync(root+'/call.json','{}');if(mode!=='hold-call')send(m.id,{content:[{type:'text',text:'owned-mcp-completed'}]});}
});
