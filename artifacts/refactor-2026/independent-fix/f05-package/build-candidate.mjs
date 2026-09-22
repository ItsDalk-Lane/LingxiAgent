import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import process from 'node:process';
import { spawnSync, execFileSync } from 'node:child_process';
import { generateKeyPairSync, createHash } from 'node:crypto';
const { console } = globalThis;
const repo=process.cwd();
const out=path.join(repo,'artifacts/refactor-2026/independent-fix/f05-package');
const resume=process.env.LINGXI_PACKAGE_RESUME;
const temp=resume||fs.mkdtempSync(path.join(os.tmpdir(),'lingxi-f05-package-'));
const suffix=resume?'-r2':'';
const build=path.join(temp,'build');fs.mkdirSync(build,{recursive:true});
const env={...process.env,SKIP_NOTARIZE:'true',CSC_IDENTITY_AUTO_DISCOVERY:'false'};
for(const key of Object.keys(env)){if(/^(CSC_|APPLE_|LINGXI_SIGN_|ELECTRON_RUN_AS_NODE)/.test(key))delete env[key];}
env.CSC_IDENTITY_AUTO_DISCOVERY='false';
function run(id,bin,args,cwd=build){const start=new Date().toISOString();const fd=fs.openSync(path.join(out,`${id}${suffix}.log`),'w');const result=spawnSync(bin,args,{cwd,env,stdio:['ignore',fd,fd]});fs.closeSync(fd);fs.appendFileSync(path.join(out,'commands.jsonl'),JSON.stringify({id:id+suffix,command:[bin,...args],cwd,start,end:new Date().toISOString(),exit:result.status,signal:result.signal,node:process.version})+'\n');console.log(id,result.status);if(result.status!==0)throw new Error(`${id} failed exit=${result.status}`);}
env.LINGXI_SOURCE_COMMIT=execFileSync('git',['rev-parse','HEAD'],{cwd:repo}).toString().trim();
try{
 if(!resume){
 run('copy-source','rsync',['-a','--exclude=/.git','--exclude=/node_modules','--exclude=/.cache','--exclude=/artifacts','--exclude=/dist*','--exclude=/.claude','--exclude=/desktop/dist-*','--exclude=/desktop/*.bundle.cjs','--exclude=*.app','--exclude=.build',`${repo}/`,`${build}/`],repo);
 fs.symlinkSync(path.join(repo,'node_modules'),path.join(build,'node_modules'),'dir');
 for(const relative of ['.cache/node-runtime/node-v24.15.0-darwin-arm64','dist-computer-use/mac-arm64','dist-speech/mac-arm64']){fs.mkdirSync(path.dirname(path.join(build,relative)),{recursive:true});fs.cpSync(path.join(repo,relative),path.join(build,relative),{recursive:true});}
 const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
 const snapshots=[];for(const dir of ['core','lib','shared','server','desktop/src','hub','scripts']){for(const name of fs.readdirSync(path.join(build,dir),{recursive:true})){const file=path.join(build,dir,name);if(fs.statSync(file).isFile())snapshots.push({path:path.join(dir,name),sha256:hash(file)});}}
 fs.writeFileSync(path.join(out,'source-snapshot.json'),JSON.stringify(snapshots,null,2));
 fs.writeFileSync(path.join(out,'build-context.json'),JSON.stringify({at:new Date().toISOString(),sha:execFileSync('git',['rev-parse','HEAD'],{cwd:repo}).toString().trim(),dirty:execFileSync('git',['status','--short'],{cwd:repo}).toString(),node:process.version,platform:process.platform,arch:process.arch,temp,build,lockSha256:hash(path.join(build,'package-lock.json')),localOnly:true,reusedHelpers:['dist-computer-use/mac-arm64','dist-speech/mac-arm64'],sourceSnapshot:'source-snapshot.json'},null,2));
 }
 const keys=generateKeyPairSync('ed25519');
 const keyFile=path.join(temp,'ephemeral-private.pem');const keyset=path.join(temp,'ephemeral-public.json');
 fs.writeFileSync(keyFile,keys.privateKey.export({type:'pkcs8',format:'pem'}),{mode:0o600});
 fs.writeFileSync(keyset,JSON.stringify([{keyId:'independent-fix-local-only',publicKey:keys.publicKey.export({type:'spki',format:'pem'})}]));
 env.LINGXI_SIGN_KEY=keyFile;env.LINGXI_SIGN_KEYSET=keyset;
 run(process.env.LINGXI_PACKAGE_MINIMAL?'build-main':'build-client','npm',['run',process.env.LINGXI_PACKAGE_MINIMAL?'build:main':'build:client']);
 run('build-server','npm',['run','build:server']);
 run('verify-seed','npm',['run','verify:seed-kit']);
 run('electron-builder','node',['node_modules/electron-builder/cli.js','--dir','--mac','--arm64','--config.directories.output='+path.join(temp,'package')]);
 fs.writeFileSync(path.join(out,'package-result.json'),JSON.stringify({status:'BUILT',app:path.join(temp,'package/mac-arm64/Lingxi.app'),sourceSnapshot:'source-snapshot.json',temporarySigningKeyDeleted:true},null,2));
}finally{
 for(const name of ['ephemeral-private.pem','ephemeral-public.json']){fs.rmSync(path.join(temp,name),{force:true});}
}
