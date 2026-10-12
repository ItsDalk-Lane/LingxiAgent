import os,sys,json,hashlib,subprocess,pathlib,datetime,re,time,signal
root=pathlib.Path(__file__).resolve().parents[5]; ev=pathlib.Path(__file__).resolve().parent
label=sys.argv[1]; argv=sys.argv[2:]; out=ev/'commands'/label; out.mkdir(parents=True,exist_ok=False)
target=pathlib.Path(os.environ.get('W10_TARGET_DIR',str(root/'rust/target-w10-r01'))); target.mkdir(exist_ok=True); assert not target.is_symlink()
home=out/'LOCAL_ONLY_NEVER_GIT'/'synthetic-home'; home.mkdir(parents=True)
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def inputs():
 names=subprocess.check_output(['git','ls-files','-z'],cwd=root).decode().split('\0'); names += ['rust/crates/lingxi-service/tests/r06_ar_w10_embedding_identity.rs']
 return {n:sha(root/n) for n in sorted(set(names)) if n and (root/n).is_file() and not n.startswith('artifacts/')}
env=os.environ.copy();env.update(LINGXI_HOME=str(home),CARGO_TARGET_DIR=str(target),CARGO_BUILD_JOBS='8',TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',LINGXI_TEST_GUARD='1',TEST_GUARD_VERBOSE='1',W10_EVIDENCE=str(out))
r=dict(cwd=str(root),argv=argv,start=datetime.datetime.now(datetime.timezone.utc).isoformat(),inputs_before=inputs(),environment={k:env[k] for k in ['LINGXI_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','TEST_MEM_LIMIT_MB','TEST_TIME_LIMIT_SEC','LINGXI_TEST_GUARD']},rss_measured_mib=None,target_symlink=False)
(out/'record-start.json').write_text(json.dumps(r,indent=2));start=time.monotonic();timeout=int(os.environ.get('W10_OUTER_TIMEOUT','2700'))
with (out/'output.log').open('w') as f:
 p=subprocess.Popen(argv,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
 try: code=p.wait(timeout=timeout)
 except subprocess.TimeoutExpired:
  os.killpg(p.pid,signal.SIGTERM)
  try:p.wait(timeout=15)
  except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()
  code=p.returncode;r['outer_timeout']=True
log=(out/'output.log').read_text();bins=[]
for match in re.finditer(r'Running .*?\(([^)]+)\)',log):
 b=pathlib.Path(match.group(1));b=b if b.is_absolute() else root/b
 if b.is_file():bins.append({'path':str(b),'sha256':sha(b)})
for match in re.finditer(r'Running `([^`]+)`',log):
 b=pathlib.Path(match.group(1).split()[0]);b=b if b.is_absolute() else root/b
 if b.is_file():bins.append({'path':str(b),'sha256':sha(b)})
r.update(exit_code=code,end=datetime.datetime.now(datetime.timezone.utc).isoformat(),elapsed_seconds=time.monotonic()-start,inputs_after=inputs(),executed_binaries=bins,log_sha256=sha(out/'output.log'),cleanup='Owned command reaped; fixtures retained LOCAL_ONLY_NEVER_GIT; sockets/tasks verified by process end')
r['inputs_stable']=r['inputs_before']==r['inputs_after'];(out/'record.json').write_text(json.dumps(r,indent=2));print(json.dumps({'label':label,'exit':code,'stable':r['inputs_stable'],'bins':bins,'tail':log[-3500:]}));sys.exit(code)
