import pathlib, os, sys, json, subprocess, time, datetime, hashlib, signal, ctypes, shlex
ROOT=pathlib.Path('/private/tmp/lingxi-r06-w15-reviewer-01')
BASE=pathlib.Path('/private/tmp/lingxi-w15-r01-runtime')
OUTROOT=ROOT/'artifacts/rust-tauri/R06/AR/W15-REVIEWER-01'
tag=sys.argv[1];argv=sys.argv[2:];out=OUTROOT/'windows'/tag;out.mkdir(parents=True,exist_ok=False)
env=dict(os.environ);env.update(LINGXI_HOME=str(BASE/'home'),TMPDIR=str(BASE/'tmp'),LINGXI_PRODUCT_DIR=str(ROOT/'lib'),CARGO_TARGET_DIR=str(BASE/'target'),CARGO_INCREMENTAL='0',LINGXI_TEST_GUARD='1',TEST_TIME_LIMIT_SEC='600',TEST_MEM_LIMIT_MB='8192',TEST_GUARD_VERBOSE='1',R06_T08_EVAL_OUT=str(out/'evaluation.jsonl'),R06_T08_RAW_OUT=str(out/'raw'))
def sha(p):
 try:return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
 except (OSError,ValueError):return None
def snapshot(name):
 original=json.loads((ROOT/'artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01/frozen-source-manifest.json').read_text())['files']
 data=[dict(path=x['path'],sha256=sha(ROOT/x['path'])) for x in original]
 (out/('source-'+name+'.json')).write_text(json.dumps(data,indent=2))
 return hashlib.sha256(json.dumps(data,sort_keys=True).encode()).hexdigest()
lib=ctypes.CDLL('/usr/lib/libproc.dylib');lib.proc_pidpath.argtypes=[ctypes.c_int,ctypes.c_void_p,ctypes.c_uint32]
def executable(pid):
 b=ctypes.create_string_buffer(4096)
 return b.value.decode() if lib.proc_pidpath(pid,b,len(b))>0 else None
def table():
 rows={}
 for line in subprocess.check_output(['ps','-axo','pid=,ppid=,rss=,stat=,args='],text=True).splitlines():
  b=line.strip().split(None,4)
  if len(b)==5:
   try:rows[int(b[0])]=(int(b[1]),int(b[2])*1024,b[3],b[4])
   except ValueError:pass
 return rows
start=time.monotonic();record=dict(argv=argv,cwd=str(ROOT),started=datetime.datetime.now(datetime.timezone.utc).isoformat(),head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),effective_env={k:env[k] for k in ('HOME','LINGXI_HOME','TMPDIR','LINGXI_PRODUCT_DIR','CARGO_TARGET_DIR','CARGO_INCREMENTAL','LINGXI_TEST_GUARD','TEST_TIME_LIMIT_SEC','TEST_MEM_LIMIT_MB')},guard={'business_seconds':600,'rss_mib':8192,'controller_seconds':2700,'sampling_interval_seconds':0.05},lock_sha256=sha(ROOT/'rust/Cargo.lock'),source_before=snapshot('before'),processes=[],launch_binary_before={str(p):sha(p) for p in (pathlib.Path(argv[0]),BASE/'target/debug/lingxi-service')})
seen={};breaches=[];table()
with (out/'stdout.log').open('w') as stdout,(out/'stderr.log').open('w') as stderr:
 p=subprocess.Popen(argv,cwd=ROOT,env=env,stdout=stdout,stderr=stderr);record['controller_pid']=p.pid
 (out/'start.json').write_text(json.dumps(record,indent=2))
 while p.poll() is None:
  rows=table();owned={p.pid};change=True
  while change:
   n=len(owned);owned.update(pid for pid,row in rows.items() if row[0] in owned);change=len(owned)!=n
  now=time.monotonic()
  for pid in owned:
   if pid not in rows:continue
   pp,rss,state,args=rows[pid];path=executable(pid);name=pathlib.Path(path or shlex.split(args)[0]).name
   control=name in ('cargo','rustc','rustdoc','cc','clang','ld','ar','ranlib','xtask') or 'build-script' in name
   x=seen.setdefault(pid,dict(pid=pid,ppid=pp,argv=args,actual_executable=path,executable_sha256_first_observed=sha(path) if path else None,first_seen_elapsed=now-start,last_seen_elapsed=None,peak_sampled_rss_bytes=None,controller=control,limit_seconds=2700 if control else 600,exit_observation=None,wait_reap='direct parent only; descendants not independently waitable'))
   x['last_seen_elapsed']=now-start;x['peak_sampled_rss_bytes']=max(x['peak_sampled_rss_bytes'] or 0,rss)
   if rss>8192*1024*1024 or now-start-x['first_seen_elapsed']>x['limit_seconds']:breaches.append(dict(pid=pid,rss=rss,elapsed=now-start-x['first_seen_elapsed']))
  for pid,x in seen.items():
   if pid not in rows and x['exit_observation'] is None:x['exit_observation']={'kind':'absent_from_process_table','elapsed':now-start}
  if now-start>2700:breaches.append({'controller_budget_exceeded':True})
  if breaches:
   for pid in reversed(sorted(owned)):
    try:os.kill(pid,signal.SIGTERM)
    except ProcessLookupError:pass
   time.sleep(.2)
   for pid in reversed(sorted(owned)):
    try:os.kill(pid,signal.SIGKILL)
    except ProcessLookupError:pass
   break
  time.sleep(.05)
 code=p.wait()
rows=table()
for pid,x in seen.items():
 x['executable_sha256_after']=sha(x['actual_executable']) if x['actual_executable'] else None
 x['present_after']=pid in rows
 if pid==p.pid:x['wait_reap']='Popen.wait executed';x['exit_code']=code
 if pid not in rows and x['exit_observation'] is None:x['exit_observation']={'kind':'absent_at_final_table','elapsed':time.monotonic()-start}
record.update(ended=datetime.datetime.now(datetime.timezone.utc).isoformat(),elapsed_seconds=time.monotonic()-start,exit_code=code,guard_breaches=breaches,source_after=snapshot('after'),processes=list(seen.values()),unsampled_process_metrics='null/unknown; inventory cannot replace execution measurement',owned_survivors=[pid for pid,x in seen.items() if x['present_after']],supervisor_sha256=sha(__file__),launch_binary_after={p:sha(p) for p in record['launch_binary_before']})
(out/'run.json').write_text(json.dumps(record,ensure_ascii=False,indent=2));print(json.dumps({'window':str(out),'exit_code':code,'elapsed_seconds':record['elapsed_seconds'],'breaches':breaches,'survivors':record['owned_survivors']}));sys.exit(code if code>=0 else 128-code)
