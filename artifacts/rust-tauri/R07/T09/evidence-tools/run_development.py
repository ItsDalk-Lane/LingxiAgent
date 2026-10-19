#!/usr/bin/env python3
"""保存开发期真实命令及原始退出结果；并行编辑期间不冒充冻结候选验收。"""
import datetime,hashlib,json,os,pathlib,subprocess,sys,time
ROOT=pathlib.Path(__file__).resolve().parents[5]
OUT=ROOT/'artifacts/rust-tauri/R07/T09'
label=sys.argv[1]
args=sys.argv[2:]
if args[:1]==['--']: args=args[1:]
if not args or '/' in label: raise SystemExit('label and command required')
log=OUT/(label+'.log');receipt=OUT/(label+'.json')
if log.exists() or receipt.exists(): raise SystemExit('refusing to replace an existing run')
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def capture(cmd):return subprocess.run(cmd,cwd=ROOT,capture_output=True,text=True,check=True).stdout.strip()
def fingerprint():
 paths=capture(['git','ls-files','-m','-o','--exclude-standard']).splitlines()
 files={}
 for name in sorted(set(paths)):
  p=ROOT/name
  if name.startswith(('rust/','desktop/','cli/','lib/','contracts/','shared/','scripts/','tests/','server/','core/','mobile/','config/','plugins/','skills2set/')) and p.is_file() and not p.is_symlink():
   files[name]=hashlib.sha256(p.read_bytes()).hexdigest()
 return hashlib.sha256(json.dumps(files,sort_keys=True).encode()).hexdigest()
env={'PATH':'/Users/study_superior/.cargo/bin:'+os.environ['PATH'],'CARGO_TARGET_DIR':os.environ.get('CARGO_TARGET_DIR','/private/tmp/lingxi-r07-build'),'CARGO_BUILD_JOBS':os.environ.get('CARGO_BUILD_JOBS','4'),'LINGXI_HOME':os.environ.get('LINGXI_HOME','/private/tmp/lingxi-r07-tests/home'),'TMPDIR':os.environ.get('TMPDIR','/private/tmp/lingxi-r07-tests/tmp'),'LINGXI_PRODUCT_DIR':str(ROOT/'lib')}
record={'label':label,'stage':'R07-T09','scope':'DEVELOPMENT_PARALLEL_EDITING_NOT_CANDIDATE_ACCEPTANCE','command':args,'cwd':str(ROOT),'environment':env,'startedAt':now(),'baseCommit':capture(['git','rev-parse','HEAD']),'sourceFingerprintBefore':fingerprint(),'finalAcceptance':False}
print(json.dumps({'started':label,'command':args}),flush=True)
with log.open('xb') as stream:
 try:code=subprocess.run(args,cwd=ROOT,env=os.environ|env,stdout=stream,stderr=subprocess.STDOUT).returncode
 except Exception as error:stream.write(str(error).encode());code=127
record.update(exitCode=code,endedAt=now(),sourceFingerprintAfter=fingerprint(),log=str(log.relative_to(ROOT)),logSha256=hashlib.sha256(log.read_bytes()).hexdigest())
record['sourceUnchanged']=record['sourceFingerprintBefore']==record['sourceFingerprintAfter']
receipt.write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(record,ensure_ascii=False),flush=True)
raise SystemExit(code if code>=0 else 128-code)
