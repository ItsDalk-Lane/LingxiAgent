import {spawn} from 'node:child_process';import assert from 'node:assert/strict';
async function execute(code){return await new Promise((resolve,reject)=>{
 const child=spawn(process.execPath,['scripts/rust-tauri/r08-ptc-worker.mjs'],{env:{PATH:process.env.PATH},stdio:['pipe','pipe','pipe'],detached:true});let stdout='',stderr='',result,stopRequested=false;const timeout=setTimeout(()=>reject(new Error('adapter timeout')),5000);
 child.stdout.on('data',chunk=>{stdout+=chunk;const end=stdout.indexOf('\n');if(end>=0&&!result){try{result=JSON.parse(stdout.slice(0,end));assert.equal(child.exitCode,null,'领头必须活到显式停止');stopRequested=true;process.kill(-child.pid,'SIGKILL');}catch(e){reject(e)}}});child.stderr.on('data',chunk=>stderr+=chunk);child.on('error',reject);child.on('close',(exit,signal)=>{clearTimeout(timeout);try{assert.equal(signal,'SIGKILL',stderr);assert.equal(stopRequested,true);resolve(result);}catch(e){reject(e)}});child.stdin.write(JSON.stringify({type:'run',code})+'\n');
});}
const ok=await execute("const n:number=19;console.log('REAL_NODE_PTC');return n+23;");assert.equal(ok.result.ok,true);assert.deepEqual(ok.result.logs,['REAL_NODE_PTC']);assert.equal(ok.result.returnText,'42');
const missing=await execute('return process.env');assert.equal(missing.result.ok,false);assert.equal(missing.result.kind,'exception');
const huge=await execute("console.log('汉'.repeat(50000));return 1;");assert.equal(huge.result.ok,false);assert.equal(huge.result.kind,'output-limit');
const invalid=await execute('enum Invalid { A };return Invalid.A');assert.equal(invalid.result.ok,false);assert.equal(invalid.result.kind,'exception');
console.log(JSON.stringify({status:'PASS_ADAPTER_ONLY',cases:['erasable_ts_log_return','node_globals_absent','real_output_limit','non_erasable_ts_refused','leader_alive_until_explicit_group_kill_and_reap'],boundary:'No child-tool results were substituted; this checks the real pure runtime adapter, not Rust authorization or storage.'}));
