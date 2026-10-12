import sys,subprocess,os,json,hashlib,pathlib,datetime,re,shlex
root=pathlib.Path(__file__).resolve().parents[5]
ev=pathlib.Path(__file__).resolve().parent
label=sys.argv[1]; argv=sys.argv[2:]; out=ev/'commands'/label
out.mkdir(exist_ok=False)
def hashes():
 return {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((root/'rust').rglob('*')) if p.is_file() and 'target' not in p.parts}
home=out/'synthetic-home'
if 'verify-stage' in argv and '--evidence' in argv:
 stage_evidence=pathlib.Path(argv[argv.index('--evidence')+1]);stage_evidence=stage_evidence if stage_evidence.is_absolute() else root/stage_evidence
 home=stage_evidence/'synthetic-home'
 # 正式门禁自行建立全新输出目录；本轮 home 归该门禁输出子树，不先制造非空证据。
else:
 home.mkdir()

env=os.environ.copy();env.update(LINGXI_HOME=str(home),CARGO_TARGET_DIR=str(root/'rust/target'),CARGO_BUILD_JOBS='8',TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',LINGXI_TEST_GUARD='1',TEST_GUARD_VERBOSE='1',W10_EVIDENCE=str(out))
record=dict(cwd=str(root),argv=argv,start=datetime.datetime.now(datetime.timezone.utc).isoformat(),source_before=hashes(),environment={k:env[k] for k in ['LINGXI_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','TEST_MEM_LIMIT_MB','TEST_TIME_LIMIT_SEC','LINGXI_TEST_GUARD']},memory_measured_mb=None)
(out/'record-start.json').write_text(json.dumps(record,indent=2))
with (out/'output.log').open('w') as f:
 p=subprocess.run(argv,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT)
record.update(exit=p.returncode,end=datetime.datetime.now(datetime.timezone.utc).isoformat(),source_after=hashes())
log=(out/'output.log').read_text(); binaries=[]
for match in re.finditer(r'Running .*?\(([^)]+)\)',log):
 b=pathlib.Path(match.group(1)); b=b if b.is_absolute() else root/b
 if b.is_file(): binaries.append({'path':str(b),'sha256':hashlib.sha256(b.read_bytes()).hexdigest()})

for match in re.finditer(r'Running `([^`]+)`',log):
 b=pathlib.Path(shlex.split(match.group(1))[0]);b=b if b.is_absolute() else root/b
 if b.is_file(): binaries.append({'path':str(b),'sha256':hashlib.sha256(b.read_bytes()).hexdigest()})
record['executed_binaries']=binaries;record['log_sha256']=hashlib.sha256(log.encode()).hexdigest();record['fixture_cleanup']='LOCAL_ONLY_SENSITIVE synthetic homes retained locally, excluded from proposed Git evidence; no real home used'
(out/'record.json').write_text(json.dumps(record,indent=2));print(json.dumps({'label':label,'exit':p.returncode,'binaries':binaries,'tail':log[-3500:]}))
sys.exit(p.returncode)
