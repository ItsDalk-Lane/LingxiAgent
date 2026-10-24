// 仅复用既有纯 PTC 运行时；权限、工具执行、日志事实和任务终态由 Rust 持有。
import readline from 'node:readline';
import { runPtcProgram } from '../../lib/ptc/ptc-runtime.ts';
const input=readline.createInterface({input:process.stdin,terminal:false,crlfDelay:Infinity});
const pending=new Map();let started=false,nextId=0;
const send=message=>process.stdout.write(JSON.stringify(message)+'\n');
const render=value=>{if(typeof value==='string')return value;try{return JSON.stringify(value,null,2)??String(value);}catch{return String(value);}};
input.on('line',line=>{
  let message;try{message=JSON.parse(line);}catch{process.exit(71);return;}
  if(message.type==='result'){
    const resolve=pending.get(message.id);if(!resolve)return;pending.delete(message.id);resolve(message.envelope);return;
  }
  if(message.type!=='run'||started){process.exit(72);return;}started=true;
  void runPtcProgram({code:message.code,call:(name,args)=>new Promise(resolve=>{
    const id=++nextId;pending.set(id,resolve);
    try{send({type:'call',id,name,args});}catch(error){pending.delete(id);resolve({ok:false,error:{toolName:name,message:String(error.message||error)}});}
  })}).then(outcome=>{
    const payload={...outcome};delete payload.value;
    if(outcome.ok){payload.hasReturn=outcome.value!==undefined;if(payload.hasReturn)payload.returnText=render(outcome.value);}
    // 完成后保留可信外围领头；Rust 收齐子调用后整组 kill + wait，再释放会话许可。
    try{process.stdout.write(JSON.stringify({type:'done',result:payload})+'\n');}catch(error){process.stdout.write(JSON.stringify({type:'done',result:{ok:false,kind:'invalid-output',message:String(error.message||error),logs:outcome.logs||[]}})+'\n');}
  },error=>{process.stdout.write(JSON.stringify({type:'done',result:{ok:false,kind:'worker-exit',message:String(error.message||error),logs:[]}})+'\n');});
});
input.on('close',()=>{if(!started)process.exit(74);});
