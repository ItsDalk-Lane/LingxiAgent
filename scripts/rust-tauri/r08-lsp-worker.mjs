// 单次外围协议转换；授权、版本检查、实际写回和任务状态全部由 Rust 决定。
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, writeFile, rm, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { JsonRpcClient } from '../../lib/lsp/jsonrpc-client.ts';
const chunks=[]; for await (const chunk of process.stdin) chunks.push(chunk);
const input=JSON.parse(Buffer.concat(chunks).toString('utf8'));
let root, client;
const servers={typescript:['typescript-language-server',['--stdio']],python:['pyright-langserver',['--stdio']],go:['gopls',[]],rust:['rust-analyzer',[]],cpp:['clangd',['--background-index=false','--clang-tidy=false']]};
const versionCommands={typescript:['typescript-language-server','--version'],python:['pyright','--version'],go:['gopls','version'],rust:['rust-analyzer','--version'],cpp:['clangd','--version']};
try {
  if(input.op==='status') {
    const availability=await Promise.all(Object.entries(servers).map(async([language,[server]])=>({language,server,available:await new Promise(resolve=>{let done=false;const [versionCmd,...versionArgs]=versionCommands[language];const child=spawn(versionCmd,versionArgs,{stdio:'ignore'});const finish=value=>{if(done)return;done=true;clearTimeout(timer);resolve(value);};const timer=setTimeout(()=>{child.kill();finish(false);},3000);child.once('error',()=>finish(false));child.once('exit',code=>finish(code===0));})})));
    process.stdout.write(JSON.stringify({availability}));
  } else if(input.op==='ast') {
    root=await realpath(input._scratch || await mkdtemp(path.join(tmpdir(),'lingxi-r08-ast-')));
    const config=path.join(root,'sgconfig.yml');await writeFile(config,'ruleDirs: []\n',{flag:'wx'});
    const output=await new Promise((resolve,reject)=>{
      const child=spawn(input.binary,['run','--pattern',input.pattern,'--lang',input.language,'--stdin','--json=compact','--config',config],{cwd:root,stdio:['pipe','pipe','pipe']});
      const out=[],err=[];let size=0;
      child.stdout.on('data',b=>{size+=b.length;if(size>4*1024*1024){child.kill();reject(new Error('AST_GREP_OUTPUT_LIMIT'));}else out.push(b);});
      child.stderr.on('data',b=>{if(err.reduce((n,b)=>n+b.length,0)<16000)err.push(b);});
      child.once('error',reject);child.once('exit',code=>{
        if(code!==0&&code!==1)return reject(new Error(`AST_GREP_FAILED (${code}): ${Buffer.concat(err)}`));
        try {resolve(JSON.parse(Buffer.concat(out).toString('utf8')||'[]'));}catch(e){reject(new Error(`AST_GREP_PARSE_FAILED: ${e.message}`));}
      });child.stdin.end(input.text);
    });
    process.stdout.write(JSON.stringify({matches:output}));
  } else {
    root=await realpath(input._scratch || await mkdtemp(path.join(tmpdir(),'lingxi-r08-lsp-')));
    for(const document of input.documents){const target=path.join(root,document.relative);await mkdir(path.dirname(target),{recursive:true});await writeFile(target,document.text,{flag:'wx'});}
    const server=servers[input.language];if(!server)throw new Error('LSP_LANGUAGE_INVALID');
    let diagnostics,diagnosticReady;
    const ready=new Promise(resolve=>{diagnosticReady=resolve;});
    const file=path.join(root,input.file),uri=pathToFileURL(file).href;
    client=new JsonRpcClient({command:server[0],args:server[1],cwd:root,env:{HOME:root},requestTimeoutMs:20000,onNotification(method,params){if(method==='textDocument/publishDiagnostics'&&params.uri===uri){diagnostics=params.diagnostics;diagnosticReady();}}});
    await client.start();
    await client.initialize(pathToFileURL(root).href,{textDocument:{synchronization:{openClose:true,change:1},rename:{prepareSupport:true}},workspace:{workspaceFolders:false}});
    for(const document of input.documents)client.notify('textDocument/didOpen',{textDocument:{uri:pathToFileURL(path.join(root,document.relative)).href,languageId:input.language==='cpp'?'cpp':input.language,version:1,text:document.text}});
    const position={line:input.line-1,character:input.column-1},base={textDocument:{uri},position};
    let result;
    if(input.action==='diagnostics'){
      // 未收到发布不能伪造“无诊断”；超时作为错误交回 Rust。
      await Promise.race([ready,new Promise((_,reject)=>setTimeout(()=>reject(new Error('LSP_DIAGNOSTICS_TIMEOUT')),20000).unref())]);result=diagnostics;
    }else if(input.action==='rename'){
      try{const p=await client.request('textDocument/prepareRename',base);if(!p)throw new Error('LSP_RENAME_NOT_PREPARABLE');}catch(e){if(!/-32601|MethodNotFound|method not found/i.test(e.message))throw e;}
      result=await client.request('textDocument/rename',{...base,newName:input.new_name});
    }else result=await client.request({symbols:'textDocument/documentSymbol',definition:'textDocument/definition',references:'textDocument/references',hover:'textDocument/hover'}[input.action],{...base,...(input.action==='references'?{context:{includeDeclaration:true}}:{})});
    // 服务器只能返回快照根内文件。原路径由 Rust 精确映射，绝不接受 worker 自报授权。
    const map=(value)=>{if(Array.isArray(value))return value.map(map);if(value&&typeof value==='object'){const out={};for(const[k,v]of Object.entries(value)){if((k==='uri'||k==='targetUri')&&typeof v==='string'){const p=fileURLToPath(v),relative=path.relative(root,p);if(relative.startsWith('..')||path.isAbsolute(relative))throw new Error('LSP_LOCATION_OUTSIDE_SNAPSHOT');out[k]=relative;}else if(k==='changes'){out.changes={};for(const[u,edits]of Object.entries(v)){const relative=path.relative(root,fileURLToPath(u));if(relative.startsWith('..')||path.isAbsolute(relative))throw new Error('LSP_EDIT_OUTSIDE_SNAPSHOT');out.changes[relative]=map(edits);}}else out[k]=map(v);}return out;}return value;};
    process.stdout.write(JSON.stringify({result:map(result)}));
  }
}catch(error){process.stdout.write(JSON.stringify({error:String(error.message||error)}));process.exitCode=1;}
finally{if(client)await client.shutdown();if(root)await rm(root,{recursive:true,force:true});}
