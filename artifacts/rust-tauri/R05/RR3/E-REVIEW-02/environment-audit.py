from pathlib import Path
import json,subprocess,hashlib,datetime
r=Path(__file__).resolve().parent;root=Path.cwd();rows=[]
commands=[['git','rev-parse','HEAD'],['git','rev-parse','refs/remotes/origin/codex/rust-tauri-migration'],['git','branch','--show-current'],['git','status','--short'],['/Users/study_superior/.cargo/bin/rustc','--version'],['/Users/study_superior/.cargo/bin/cargo','--version'],['node','--version'],['npm','--version'],['python3','--version'],['uname','-sm'],['git','diff','--check','--',*json.loads((root/'artifacts/rust-tauri/R05/RR3/E-02/before.json').read_text())['owned']]]
for i,args in enumerate(commands):
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(args,capture_output=True);end=datetime.datetime.now(datetime.timezone.utc).isoformat();logs={}
 for name,data in [('stdout',p.stdout),('stderr',p.stderr)]:
  path=r/f'environment-{i:02}-{name}.log';path.write_bytes(data);logs[name]={'path':path.name,'sha256':hashlib.sha256(data).hexdigest()}
 rows.append({'argv':args,'cwd':str(root),'startUTC':start,'endUTC':end,'exitCode':p.returncode,'logs':logs});assert p.returncode==0,args
(r/'environment.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
request=(r/'dispatch/request.json').read_bytes();first=(r/'dispatch/events.jsonl').open().readline();identity={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'dispatchRequest':json.loads(request),'requestSHA':hashlib.sha256(request).hexdigest(),'eventFirstLine':json.loads(first),'requestOnlyRead':True,'independence':'new review round; no participation in E implementation or earlier review; no spawned agent or message sent'}
(r/'session-identity.json').write_text(json.dumps(identity,ensure_ascii=False,indent=2)+'\n');print('11 read-only environment commands exit0; dispatch first event',identity['eventFirstLine'])
