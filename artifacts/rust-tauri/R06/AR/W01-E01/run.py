import os,sys,time,json,subprocess,hashlib,pathlib,tempfile,signal,shutil
root=pathlib.Path(__file__).resolve().parents[5]
out=pathlib.Path(__file__).resolve().parent
name=sys.argv[1];cmd=sys.argv[2:];outer_seconds=2430 if name.startswith('workspace-') else 600;home=tempfile.mkdtemp(prefix='w01-'+name+'-')
def binding():
 paths=sorted([p for p in (root/'rust').rglob('*') if p.is_file() and 'target' not in p.parts]+[root/'rust-toolchain.toml'])
 tracked=subprocess.check_output(['git','ls-files','lib','contracts','scripts/rust-tauri','docs/rust-tauri'],cwd=root,text=True).splitlines()
 paths+=sorted(root/p for p in tracked if (root/p).is_file())
 paths+=sorted(p for p in out.glob('*.py') if p.is_file())
 if name.startswith('pure-bc-'):paths+=sorted(p for p in (out/'baseline-source').rglob('*') if p.is_file())
 return {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
def binaries():
 paths=list((root/'rust/target/debug/deps').glob('*'))+[root/'rust/target/debug/lingxi-service']
 if name.startswith('pure-bc-'):paths+=list((root/'rust/target/bc-proof/debug/deps').glob('*'))+[root/'rust/target/bc-proof/debug/lingxi-service']
 found={}
 for p in paths:
  if p.is_file() and os.access(p,os.X_OK):
   with p.open('rb') as f:magic=f.read(4)
   if magic in (b'\xcf\xfa\xed\xfe',b'\xca\xfe\xba\xbe',b'\x7fELF'):
    found[str(p.relative_to(root))]=hashlib.sha256(p.read_bytes()).hexdigest()
 return found
binary_before=binaries();before=binding();start=time.time();env=dict(os.environ,LINGXI_HOME=home,LINGXI_PRODUCT_DIR=str(root/'lib'),CARGO_TARGET_DIR=str(root/'rust/target'),CARGO_INCREMENTAL='0',TEST_GUARD_VERBOSE='1',TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',W01_EVIDENCE=str(out/'probes'))
with open(out/'logs'/f'{name}.log','w') as f:
 p=subprocess.Popen(cmd,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
 try: code=p.wait(timeout=outer_seconds)
 except BaseException as exc:
  f.write('\nSUPERVISOR: '+repr(exc)+'\n');f.flush()
  os.killpg(p.pid,signal.SIGTERM)
  try:p.wait(timeout=20)
  except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()
  code=124
log=(out/'logs'/f'{name}.log').read_text();after=binding();binary_after=binaries();shutil.rmtree(home)
r={'command':cmd,'cwd':str(root),'home':home,'syntheticHomeCleaned':not pathlib.Path(home).exists(),'start':start,'end':time.time(),'exit_code':code,'outer_budget_sec':outer_seconds,'test_binary_guard':{'seconds':600,'memory_mib':8192},'binary_before':binary_before,'binary_after':binary_after,'input_before':before,'input_after':after,'unchanged':before==after,'lock_sha':before.get('rust/Cargo.lock'),'log_sha':hashlib.sha256(log.encode()).hexdigest(),'guard_armed':'LINGXI_TEST_GUARD: armed' in log,'child_reaped':p.poll() is not None}
(out/f'{name}.json').write_text(json.dumps(r,ensure_ascii=False,indent=2));print(json.dumps({k:v for k,v in r.items() if k not in ('input_before','input_after','binary_before','binary_after')}));print(log[-4500:]);sys.exit(code)
