import datetime,hashlib,json,os,pathlib,shutil,subprocess
ROOT=pathlib.Path.cwd().resolve();EV=ROOT/'artifacts/rust-tauri/R05/RR3/J-02';actual=EV/'actual-final-02'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert json.loads((actual/'full-default-command.json').read_text())['exitCode']==0
copy=pathlib.Path((actual/'full-copy-path.txt').read_text().strip()); inputs=json.loads((actual/'actual-inputs-before.json').read_text())
assert all(sha(ROOT/p)==v and sha(copy/p)==v for p,v in inputs.items())
rows=[]
def run(name,argv,cwd,env=None):
 rec=dict(argv=[str(v) for v in argv],cwd=str(cwd),startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),freeBefore=shutil.disk_usage(ROOT).free)
 with (EV/(name+'.stdout')).open('wb') as out,(EV/(name+'.stderr')).open('wb') as err:
  p=subprocess.run(argv,cwd=cwd,stdout=out,stderr=err,env=env)
 rec.update(name=name,exitCode=p.returncode,finishedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),freeAfter=shutil.disk_usage(ROOT).free,stdoutSha256=sha(EV/(name+'.stdout')),stderrSha256=sha(EV/(name+'.stderr')))
 rows.append(rec);(EV/'legacy-commands-final.json').write_text(json.dumps(rows,indent=2)+'\n');print(name,p.returncode,flush=True);return p.returncode
rc=run('legacy-directed-final',['bash',str(copy/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh'),str(EV/'legacy-directed-02')],copy,dict(os.environ,R02_LEGACY_REGRESSION_MODE='directed-no-seal-family'))
verify=run('node-final-verify-02',['python3','-B',str(ROOT/'scripts/rust-tauri/r05_t08_prepare_node.py'),'--verify',str(actual/'full-default-01/node-preparation/result.json'),'--evidence',str(EV/'node-final-verify-02')],ROOT)
(EV/'final-inputs-preserved-02.json').write_text(json.dumps({p:dict(expected=v,main=sha(ROOT/p),copy=sha(copy/p),equal=sha(ROOT/p)==v==sha(copy/p)) for p,v in inputs.items()},indent=2)+'\n')
raise SystemExit(rc or verify)
