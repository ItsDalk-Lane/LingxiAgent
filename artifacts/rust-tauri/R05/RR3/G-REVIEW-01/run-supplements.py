import pathlib,json,subprocess,datetime,hashlib,os,time,re,shutil
R=pathlib.Path.cwd();E=R/'artifacts/rust-tauri/R05/RR3/G-REVIEW-01';C=pathlib.Path('/Users/study_superior/r05t08-work/negcopy.hTXzzN');cargo='/Users/study_superior/.cargo/bin/cargo';env=dict(os.environ,CARGO_NET_OFFLINE='true',CARGO_TARGET_DIR='/Users/study_superior/.cache/lingxi-r05-neg-target',PYTHONDONTWRITEBYTECODE='1')
def utc():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save(p,d):p.write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def snap():
 out={}
 for sub in ['rust','scripts/rust-tauri','docs/rust-tauri','contracts']:
  for d,ds,fs in os.walk(C/sub):
   ds[:]=[x for x in ds if x not in ['target','__pycache__','.git']]
   for f in fs:
    p=pathlib.Path(d)/f
    if p.is_file() and p.suffix!='.pyc':out[str(p.relative_to(C))]=sha(p)
 for p in ['rust-toolchain.toml','.gitignore']:out[p]=sha(C/p)
 return out
commands=[]
def run(name,argv,extra=None):
 d=E/'supplements'/name;d.mkdir(parents=True,exist_ok=False);before=snap();start=utc();ee=env| (extra or {})
 with (d/'stdout.log').open('w') as h:p=subprocess.run(argv,cwd=C,env=ee,stdout=h,stderr=subprocess.STDOUT)
 txt=(d/'stdout.log').read_text(errors='replace');bins=[]
 # cargo依赖清单反推只作候选；真正已执行路径由live观察/生产日志另存。
 for match in re.finditer(r'Running\s+[^\n]*\(([^)]+)\)',txt):
  bp=C/match.group(1)
  if bp.is_file():bins.append({'path':str(bp),'sha256':sha(bp),'bytes':bp.stat().st_size})
 doc={'argv':argv,'cwd':str(C),'startUTC':start,'endUTC':utc(),'exitCode':p.returncode,'inputBefore':before,'inputAfter':snap(),'environmentOverrides':extra or {},'logSHA256':sha(d/'stdout.log'),'counts':re.findall(r'test result: [^\n]+',txt),'cargoRunningBinaries':bins}
 save(d/'command.json',doc);commands.append({'name':name,**{k:v for k,v in doc.items() if k not in ['inputBefore','inputAfter']}});save(E/'supplement-commands.json',commands);print(name,p.returncode,flush=True);return p.returncode
while not (E/'default-command.json').exists():time.sleep(2)
P=E/'default16-01/pristine'; restoration=[]
for p in P.rglob('*'):
 if p.is_file():
  target=C/p.relative_to(P); old=sha(target);shutil.copyfile(p,target);restoration.append({'path':str(p.relative_to(P)),'beforeRestore':old,'pristine':sha(p),'afterRestore':sha(target),'equal':sha(p)==sha(target)})
save(E/'full-restoration.json',{'utc':utc(),'files':restoration,'allEqual':all(x['equal'] for x in restoration),'note':'包括生产reset遗漏的kernel；只恢复审查自有副本，不修主生产脚本'})
original=snap();save(E/'copy-restored-inputs.json',{'utc':utc(),'files':original})
# 主入口的独立stable强制FAIL控制：只隔离R02 map换为明确RX受控负载，不冒充业务gate。
mp=C/'rust/crates/xtask/src/stage_maps/R02.json';origmap=mp.read_bytes();kernel=C/'rust/crates/lingxi-kernel/src/lib.rs';origkernel=kernel.read_bytes()
controlled={'schemaVersion':1,'resultVersion':'lingxi.xtask.verify-stage.v1','stage':'RX','defaultTimeoutSecs':30,'commands':{'binding-control':{'argv':['python3','-c',"import os,pathlib; p=pathlib.Path('rust/crates/lingxi-kernel/src/lib.rs'); p.write_bytes(p.read_bytes()+b'\\n// G controlled mid-command byte change\\n') if os.environ.get('LINGXI_G_MUTATE')=='1' else None; print('controlled command exit zero')"],'timeoutSecs':30,'evidencePaths':['{EVIDENCE}/binding-control/stdout.log']}},'scenarios':[{'id':'RX-BIND','requirement':'REQUIRED','commandRefs':['binding-control']} ]}
mp.write_text(json.dumps(controlled,ensure_ascii=False,indent=2)+'\n');save(E/'controlled-map-mutation.json',{'utc':utc(),'path':str(mp),'beforeSHA256':hashlib.sha256(origmap).hexdigest(),'afterSHA256':sha(mp),'boundary':'只测生产main stable分支；RX无R02业务义务，不能记为R02/R05业务门禁'})
try:
 for name,flag in [('stable-normal','0'),('stable-midbyte-red','1'),('stable-restored','0')]:
  if name=='stable-restored':kernel.write_bytes(origkernel)
  run(name,[cargo,'run','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--','verify-stage','R02','--evidence',str(E/'supplements'/name/'evidence')],{'LINGXI_G_MUTATE':flag})
finally:mp.write_bytes(origmap);kernel.write_bytes(origkernel)
save(E/'controlled-map-restoration.json',{'utc':utc(),'mapEqual':mp.read_bytes()==origmap,'kernelEqual':kernel.read_bytes()==origkernel,'allInputsEqual':snap()==original})
# include map/helper身份恢复后明确重build，不能复用RX旧runner。
run('rebuild-original-runner',[cargo,'build','--manifest-path','rust/Cargo.toml','--locked','-p','xtask'])
run('xtask-restored-all',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--bin','xtask'])
for name,args in [
 ('restore-N08-N10',['-p','lingxi-service','--test','r05_t01_binary_wiring']),
 ('restore-N09',['-p','lingxi-service','--test','r05_t03_protocol_adapters','c05_runtime_nonce']),
 ('restore-N11',['-p','lingxi-adapters','--lib','models::tool_render::tests::running_and_stop_unconfirmed_statuses_are_honest','--','--exact']),
 ('restore-N12',['-p','lingxi-service','--test','r05_t06_worker_model','c06_']),
 ('restore-N04-N15',['-p','lingxi-service','--lib','credentials::tests::handle_refusal_texts_carry_no_material','--','--exact'])]:
 run(name,[cargo,'test','--manifest-path','rust/Cargo.toml','--locked',*args])
run('producer-restored-full',['bash','scripts/rust-tauri/r05_t08_stage_suites.sh',str(E/'supplements/producer-restored-full/suites')])
# 正式R02业务结果只记录；环境FAIL不能充绑定红。正常副本的所有checkpoint需稳定。
run('binding-restored-real-R02',[cargo,'run','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--','verify-stage','R02','--evidence',str(E/'supplements/binding-restored-real-R02/evidence')])
save(E/'supplements-complete.json',{'utc':utc(),'originalInputsEqual':snap()==original,'files':snap(),'commands':commands})
