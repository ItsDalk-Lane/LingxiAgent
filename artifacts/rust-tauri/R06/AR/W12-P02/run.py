import os,sys,subprocess,json,time,pathlib,tempfile,hashlib,platform,datetime,shutil
out=pathlib.Path(__file__).parent.resolve(); name=sys.argv[1]; cmd=sys.argv[2:]; start=time.time(); env=os.environ.copy()
env.update(LINGXI_HOME=tempfile.mkdtemp(prefix='w12-p02-'+name+'-'),TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',TEST_GUARD_VERBOSE='1');env.pop('LINGXI_TEST_GUARD',None)
cwd=env.get('W12_CWD','/Users/study_superior/Desktop/Code/LingxiAgent')
target=env.get('W12_TARGET_DIR')
if target: env['CARGO_TARGET_DIR']=target
assert not (out/(name+'.log')).exists()
owned=json.loads((out/'initial-binding.json').read_text())['owned']
def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def binding():
 r=[dict(path=x['path'],sha256=sha(pathlib.Path(cwd)/x['path'])) for x in owned]
 return dict(owned=r,aggregate=hashlib.sha256(json.dumps(r,sort_keys=True).encode()).hexdigest(),lock_sha256=sha(pathlib.Path(cwd)/'rust/Cargo.lock'))
before=binding(); executable=shutil.which(cmd[0]) or cmd[0]
header=dict(argv=cmd,cwd=cwd,home=env['LINGXI_HOME'],start_utc=datetime.datetime.fromtimestamp(start,datetime.timezone.utc).isoformat(),candidate=before,platform=platform.platform(),target=target,guard='8192MiB/600s+parent600s',executable=executable,executable_sha256=sha(executable),mock_boundary='Only loopback embedding provider responses; actual SQLite/storage/worker/gateway/messages')
with (out/(name+'.log')).open('w') as f:
 f.write(json.dumps(header)+'\n');f.flush()
 child = subprocess.Popen(cmd,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT)
 peak_rss_kib=None; limit_reason=None; owned_pids=[]
 while True:
  try: code=child.wait(timeout=1); break
  except subprocess.TimeoutExpired: pass
  rows=subprocess.check_output(['ps','-axo','pid=,ppid=,rss='],text=True).splitlines()
  processes=[tuple(map(int,row.split())) for row in rows if len(row.split())==3]
  descendants={child.pid}
  while True:
   discovered={pid for pid,ppid,rss in processes if ppid in descendants};new=discovered-descendants
   if not new: break
   descendants.update(new)
  rss=sum(rss for pid,ppid,rss in processes if pid in descendants)
  peak_rss_kib=rss if peak_rss_kib is None else max(peak_rss_kib,rss)
  if rss>8192*1024: limit_reason='PARENT_8GIB_GUARD'
  elif time.time()-start>600: limit_reason='PARENT_600S_GUARD'
  if limit_reason:
   owned_pids=list(descendants);f.write(json.dumps(dict(guard=limit_reason,owned_pids=owned_pids,rss_kib=rss))+'\n');f.flush()
   for pid in sorted(descendants,reverse=True):
    try: os.kill(pid,15)
    except ProcessLookupError: pass
   try: child.wait(timeout=2)
   except subprocess.TimeoutExpired:
    for pid in sorted(descendants,reverse=True):
     try: os.kill(pid,9)
     except ProcessLookupError: pass
    child.wait()
   code=124 if limit_reason=='PARENT_600S_GUARD' else 137
   break

end=time.time();res=dict(header,end_utc=datetime.datetime.fromtimestamp(end,datetime.timezone.utc).isoformat(),exit_code=code,peak_parent_tree_rss_kib=peak_rss_kib,guard_failure=limit_reason,supervised_pid=child.pid,log_sha256=sha(out/(name+'.log')),candidate_after=binding(),home_files=[str(p.relative_to(env['LINGXI_HOME'])) for p in pathlib.Path(env['LINGXI_HOME']).rglob('*') if p.is_file()]);(out/(name+'.json')).write_text(json.dumps(res,indent=2));print(json.dumps(dict(name=name,exit_code=code,home=env['LINGXI_HOME'],candidate=before['aggregate'],log_sha256=res['log_sha256'])));sys.exit(code)
