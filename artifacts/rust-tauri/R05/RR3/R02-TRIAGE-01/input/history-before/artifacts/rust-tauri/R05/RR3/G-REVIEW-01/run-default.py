import pathlib,hashlib,json,subprocess,datetime,time,re,os
R=pathlib.Path.cwd(); E=R/'artifacts/rust-tauri/R05/RR3/G-REVIEW-01'
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def snap(root):
 out={}
 for sub in ['rust','scripts/rust-tauri','contracts','docs/rust-tauri']:
  for d,ds,fs in os.walk(root/sub):
   ds[:]=[x for x in ds if x not in ['target','__pycache__','.git']]
   for f in fs:
    p=pathlib.Path(d)/f
    if p.is_file() and p.suffix!='.pyc': out[str(p.relative_to(root))]=sha(p)
 for f in ['rust-toolchain.toml','.gitignore']: out[f]=sha(root/f)
 return out
def save(name,obj): (E/name).write_text(json.dumps(obj,ensure_ascii=False,indent=2)+'\n')
save('source-before.json',{'utc':utc(),'files':snap(R),'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'status':subprocess.check_output(['git','status','--porcelain'],text=True)})
argv=['bash','scripts/rust-tauri/r05_t08_negative_gate.sh','artifacts/rust-tauri/R05/RR3/G-REVIEW-01/default16-01']
start=utc(); log=E/'default-console.log'
with log.open('w') as h:
 p=subprocess.Popen(argv,stdout=h,stderr=subprocess.STDOUT,cwd=R)
 copy=None; prev=None; n=0; events=[]
 while p.poll() is None:
  time.sleep(.5)
  txt=log.read_text(errors='replace')
  if copy is None:
   m=re.search(r'isolated copy at (\S+) ',txt)
   if m: copy=pathlib.Path(m[1]); save('copy-path.json',{'path':str(copy),'utc':utc()})
  pristine=E/'default16-01/pristine'
  if copy and (pristine/'scripts/rust-tauri/r05_t08_stage_suites.sh').exists():
   if not (E/'copy-initial.json').exists(): save('copy-initial.json',{'utc':utc(),'files':snap(copy)})
   cur={str(f.relative_to(pristine)):sha(copy/f.relative_to(pristine)) for f in pristine.rglob('*') if f.is_file() and (copy/f.relative_to(pristine)).is_file()}
   if cur!=prev:
    n+=1; state={'utc':utc(),'consoleLastLines':txt.splitlines()[-3:],'hashes':cur}; save(f'mutation-state-{n:03d}.json',state); events.append(state); prev=cur
  if copy and n and n%10==0: pass
 exitcode=p.wait()
save('default-command.json',{'argv':argv,'cwd':str(R),'startUTC':start,'endUTC':utc(),'exitCode':exitcode,'log':str(log.relative_to(E)),'logSHA256':sha(log)})
save('mutation-observations.json',events)
save('source-after-default.json',{'utc':utc(),'files':snap(R)})
if copy: save('copy-after-default.json',{'utc':utc(),'path':str(copy),'files':snap(copy)})
print(json.dumps({'exitCode':exitcode,'copy':str(copy),'states':n}))
