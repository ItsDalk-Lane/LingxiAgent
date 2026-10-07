import sys,os,json,subprocess,hashlib
from pathlib import Path
from datetime import datetime,timezone
ROOT=Path(__file__).resolve().parents[5]
OUT=Path(__file__).resolve().parent
COMMANDS=OUT/'commands.json'
def run(name,argv,expected=0,env=None,cwd=ROOT):
 start=datetime.now(timezone.utc).isoformat()
 log=OUT/(name+'.log')
 with log.open('wb') as f:
  proc=subprocess.run(argv,cwd=cwd,env=dict(os.environ,**(env or {})),stdout=f,stderr=subprocess.STDOUT)
 record={'name':name,'argv':list(map(str,argv)),'cwd':str(cwd),'environmentOverrides':env or {},'startedAt':start,'endedAt':datetime.now(timezone.utc).isoformat(),'exitCode':proc.returncode,'expectedExitCode':expected,'log':str(log),'logSha256':hashlib.sha256(log.read_bytes()).hexdigest()}
 rows=json.loads(COMMANDS.read_text()) if COMMANDS.exists() else []; rows.append(record);COMMANDS.write_text(json.dumps(rows,indent=2,ensure_ascii=False)+'\n')
 print(name,proc.returncode,'expected',expected,flush=True)
 if proc.returncode!=expected: raise RuntimeError(record)
 return proc.returncode
if __name__=='__main__':
 mode=sys.argv[1]
 if mode=='old-red': run('validator-old-red',[sys.executable,str(OUT/'old-validator-snapshot/scripts/rust-tauri/r02_run_output_regression.py'),'--evidence',str(OUT/'validator-old-red'),'--validators-only'],1)
 elif mode=='new-green': run('validator-new-green',[sys.executable,str(ROOT/'scripts/rust-tauri/r02_run_output_regression.py'),'--evidence',str(OUT/'validator-new-green'),'--validators-only'])
 elif mode=='discovery': run('discovery-new-green',[sys.executable,str(ROOT/'scripts/rust-tauri/r02_run_output_regression.py'),'--evidence',str(OUT/'discovery-new-green')])
