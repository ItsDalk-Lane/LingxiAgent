import os, sys, json, hashlib, shutil, subprocess, datetime, time
from pathlib import Path
MAIN=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
OUT=MAIN/'artifacts/rust-tauri/R05/RR3/A-REVIEW-01'
COPY=OUT/'snapshot'
CARGO='/Users/study_superior/.cargo/bin/cargo'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def run(name,argv,env=None,expected=0,cwd=COPY):
    overrides=env or {}; begin=datetime.datetime.now(datetime.timezone.utc).isoformat(); stamp=time.monotonic()
    with (OUT/(name+'.log')).open('wb') as log:
        p=subprocess.run(argv,cwd=cwd,env=dict(os.environ,**overrides),stdout=log,stderr=subprocess.STDOUT)
    item={'caseId':name,'F-ID':'F42','argv':argv,'cwd':str(cwd),'envOverrides':overrides,'startedAt':begin,'finishedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'durationSeconds':time.monotonic()-stamp,'exitCode':p.returncode,'expectedExitCode':expected,'logSha256':sha(OUT/(name+'.log'))}
    records=json.loads((OUT/'commands.json').read_text()) if (OUT/'commands.json').exists() else []
    records.append(item); (OUT/'commands.json').write_text(json.dumps(records,indent=2)+'\n'); print(name,p.returncode,flush=True); return p.returncode
if len(sys.argv)>1 and sys.argv[1]=='snapshot':
    COPY.mkdir()
    for rel in ['rust','scripts','docs']:
        shutil.copytree(MAIN/rel,COPY/rel,ignore=shutil.ignore_patterns('target','node_modules','.cache','__pycache__'))
    for rel in ['.gitignore','rust-toolchain.toml']:
        shutil.copyfile(MAIN/rel,COPY/rel)
    files=[]
    for p in COPY.rglob('*'):
        if p.is_file(): files.append({'path':str(p.relative_to(COPY)),'sha256':sha(p),'bytes':p.stat().st_size})
    (OUT/'input-manifest.json').write_text(json.dumps({'mainHead':subprocess.check_output(['git','rev-parse','HEAD'],cwd=MAIN,text=True).strip(),'branch':subprocess.check_output(['git','branch','--show-current'],cwd=MAIN,text=True).strip(),'snapshotAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'files':files},indent=2)+'\n')
    old=subprocess.check_output(['git','show','HEAD:scripts/rust-tauri/r02_t08_legacy_entry_regression.sh'],cwd=MAIN,text=True)
    code=old.split("  python3 - \"$MAIN_REPO\" \"$EVIDENCE_DIR\" <<'PYDISC'\n",1)[1].split('\nPYDISC',1)[0]
    (OUT/'discover-old.py').write_text(code+'\n')
    print('snapshot',len(files),flush=True)
if len(sys.argv)>1 and sys.argv[1]=='baseline':
    run('toolchain',[CARGO,'--version']); run('rustc',['/Users/study_superior/.cargo/bin/rustc','--version'])
    run('discovery-green',['python3','scripts/rust-tauri/r02_run_output_regression.py','--evidence',str(OUT/'discovery-green')])
    run('discovery-old-red',['python3','scripts/rust-tauri/r02_run_output_regression.py','--evidence',str(OUT/'discovery-old-red'),'--discovery',str(OUT/'discover-old.py')],expected=1)
    run('discovery-restored-green',['python3','scripts/rust-tauri/r02_run_output_regression.py','--evidence',str(OUT/'discovery-restored-green')])
    target='/tmp/lingxi-rr3-a-review-01-target'
    common={'CARGO_TARGET_DIR':target}
    run('xtask-all',[CARGO,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask'],dict(common,LINGXI_NESTED_ARCHIVE=str(OUT/'nested-internal')))
    run('nested-external',[CARGO,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','rr3_real_fd_standard_nested_layout','--','--nocapture'],dict(common,LINGXI_NESTED_EXTERNAL='1',LINGXI_NESTED_ARCHIVE=str(OUT/'nested-external')))
    run('build-xtask',[CARGO,'build','--manifest-path','rust/Cargo.toml','--locked','-p','xtask'],common)
