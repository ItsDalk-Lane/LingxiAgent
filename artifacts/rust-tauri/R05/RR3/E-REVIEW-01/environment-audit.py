import subprocess,json,hashlib,datetime,shutil
from pathlib import Path
P=Path(__file__).resolve().parent;rows=[]
for args in [['git','rev-parse','HEAD'],['git','rev-parse','refs/remotes/origin/codex/rust-tauri-migration'],['git','branch','--show-current'],['git','status','--short'],['/Users/study_superior/.cargo/bin/rustc','--version'],['/Users/study_superior/.cargo/bin/cargo','--version'],['node','--version'],['npm','--version'],['python3','--version'],['uname','-sm']]:
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();r=subprocess.run(args,capture_output=True,text=True);rows.append({'argv':args,'utc_start':start,'utc_end':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exit_code':r.returncode,'stdout':r.stdout,'stderr':r.stderr})
files=list(json.loads(Path('artifacts/rust-tauri/R05/RR3/E-01/before.json').read_text())['owned']);r=subprocess.run(['git','diff','--check','--']+files,capture_output=True,text=True);rows.append({'argv':['git','diff','--check','--']+files,'exit_code':r.returncode,'stdout':r.stdout,'stderr':r.stderr})
(P/'environment.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n');print(json.dumps(rows,ensure_ascii=False,indent=2))
