import os,sys,json,subprocess,hashlib
from pathlib import Path
OUT=Path(__file__).resolve().parent; sys.path.insert(0,str(OUT)); from review_driver import run,COPY
lib=OUT/'discovery-green/fence-functions.sh'
root=OUT/'validator-fault-repo'; root.mkdir(); subprocess.run(['git','init','-q',str(root)],check=True)
unit='artifacts/rust-tauri/R05/declared-run'; path=root/unit; path.mkdir(parents=True); (path/'tracked-source.rs').write_text('source'); subprocess.run(['git','-C',str(root),'add',unit],check=True)
fake=OUT/'validator-fault-bin'; fake.mkdir(); p=fake/'git'; p.write_text('#!/bin/sh\ncase "$*" in *ls-files*) printf "controlled ls-files inspection failure\\n" >&2; exit 2;; esac\nexec /usr/bin/git "$@"\n'); p.chmod(0o755)
results=[]
for name in ['validate_run_output_unit','validate_declared_run_root']:
 argv=['bash','-c','source "$1"; '+name+' "$2" "$3"','bash',str(lib),str(root),unit]
 code=run('validator-normal-reject-'+name,argv,expected=1,cwd=COPY); assert code==1
 code=run('validator-git-error-'+name,argv,{'PATH':str(fake)+os.pathsep+os.environ['PATH']},expected=1,cwd=COPY)
 results.append({'validator':name,'normalExit':1,'controlledGitErrorExit':code,'expected':'非零拒绝；不允许把无法检查当未跟踪','trackedContent':str(path/'tracked-source.rs')})
 code=run('validator-query-restored-'+name,argv,expected=1,cwd=COPY); assert code==1
(OUT/'validator-fault-results.json').write_text(json.dumps(results,indent=2,ensure_ascii=False)+'\n'); print(json.dumps(results,ensure_ascii=False))
