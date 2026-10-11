import os,sys,json,hashlib,subprocess,tempfile,time,re
from pathlib import Path
OUT=Path(__file__).resolve().parent
ROOT=Path(os.environ.get('W08_REVIEW_ROOT','/Users/study_superior/.codex/worktrees/r06-w08-review/LingxiAgent'))
def hashfile(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
 paths=[p for p in (ROOT/'rust').rglob('*') if p.is_file() and not any(x in p.relative_to(ROOT/'rust').parts for x in ['target','.git'])]
 return {str(p.relative_to(ROOT)):hashfile(p) for p in sorted(paths)}
tag=sys.argv[1]; cmd=sys.argv[2:]; assert not (OUT/(tag+'.json')).exists()
home=Path(tempfile.mkdtemp(prefix='w08-r01-'+tag+'-'));env=os.environ.copy();env.update(LINGXI_HOME=str(home),TEST_GUARD_VERBOSE='1',TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',LINGXI_TEST_GUARD='1',LINGXI_F08_EVIDENCE=str(OUT/'observations'/tag),CARGO_TARGET_DIR=str(ROOT/'rust/target'))
before=snapshot(); binaries_before={str(p.relative_to(ROOT)):hashfile(p) for p in (ROOT/'rust/target/debug').rglob('*') if p.is_file() and os.access(p,os.X_OK) and p.suffix not in ['.o','.so','.dylib'] and '.fingerprint' not in str(p)}; command_binary_before=hashfile(Path(cmd[0])) if Path(cmd[0]).is_file() else None; start=time.time();log=OUT/(tag+'.log')
with log.open('w') as f:
 f.write(json.dumps({'cmd':cmd,'cwd':str(ROOT),'home':str(home),'guard':'8GiB/600s','supervision':'subprocess child of wrapper; wrapper child of exec session'})+'\n');f.flush();r=subprocess.run(cmd,cwd=ROOT,env=env,stdout=f,stderr=subprocess.STDOUT)
after=snapshot(); text=log.read_text(); binaries={}
for p in (ROOT/'rust/target/debug').rglob('*'):
 if p.is_file() and os.access(p,os.X_OK) and p.suffix not in ['.o','.so','.dylib'] and '.fingerprint' not in str(p):binaries[str(p.relative_to(ROOT))]=hashfile(p)
record={'tag':tag,'command':cmd,'cwd':str(ROOT),'home':str(home),'home_entries':list(str(p.relative_to(home)) for p in home.iterdir()),'start':start,'end':time.time(),'exit_code':r.returncode,'tests':sum(map(int,re.findall(r'running (\d+) tests?',text))),'before':before,'after':after,'source_unchanged':before==after,'lock':hashfile(ROOT/'rust/Cargo.lock'),'binaries_before':binaries_before,'command_binary_before':command_binary_before,'command_binary_after':hashfile(Path(cmd[0])) if Path(cmd[0]).is_file() else None,'binaries_after':binaries,'log_sha256':hashfile(log),'guard_armed':'armed (pid' in text}
(OUT/(tag+'.json')).write_text(json.dumps(record,ensure_ascii=False,indent=2)); print(json.dumps({k:record[k] for k in ['tag','exit_code','tests','source_unchanged','guard_armed']}));sys.exit(r.returncode)
