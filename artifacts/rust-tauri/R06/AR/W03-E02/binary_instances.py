import os,pathlib,subprocess,time,signal,json,hashlib
out=pathlib.Path(os.environ['LINGXI_W03_EVIDENCE']);base=pathlib.Path(os.environ['LINGXI_HOME']);binary=pathlib.Path(os.environ['CARGO_TARGET_DIR'])/'debug/lingxi-service';assert binary.is_file()
children=[];records=[]
def start(tag,home):
 home.mkdir(parents=True,exist_ok=True);argv=[str(binary),'--home',str(home),'--bind','127.0.0.1:0'];stdout=(out/(tag+'.stdout.log')).open('w');stderr=(out/(tag+'.stderr.log')).open('w');p=subprocess.Popen(argv,stdout=stdout,stderr=stderr,env=os.environ.copy());children.append(p);record={'tag':tag,'argv':argv,'cwd':str(pathlib.Path.cwd()),'start':time.time(),'pid':p.pid,'binary_before':hashlib.sha256(binary.read_bytes()).hexdigest(),'actual_home':str(home)};records.append(record);return p,record
def ready(p,tag):
 end=time.monotonic()+10
 while time.monotonic()<end:
  if p.poll() is not None:raise AssertionError(f'{tag} exited {p.returncode}')
  text=(out/(tag+'.stdout.log')).read_text()
  if 'LINGXI_SERVICE_READY' in text:return text.strip()
  time.sleep(.05)
 raise AssertionError('no readiness')
def stop(p,record):
 p.send_signal(signal.SIGTERM);record['exit']=p.wait(timeout=15);record['end']=time.time();record['binary_after']=hashlib.sha256(binary.read_bytes()).hexdigest();assert record['exit']==0
try:
 a,ar=start('first',base/'A');ar['readiness']=ready(a,'first')
 b,br=start('same-home-second',base/'A');br['exit']=b.wait(timeout=10);br['end']=time.time();br['binary_after']=hashlib.sha256(binary.read_bytes()).hexdigest();assert br['exit']==3
 assert 'LINGXI_SERVICE_SINGLE_WRITER_BLOCKED' in (out/'same-home-second.stderr.log').read_text()
 c,cr=start('independent-home',base/'B');cr['readiness']=ready(c,'independent-home');assert a.poll() is None
 stop(c,cr);stop(a,ar)
 d,dr=start('reopen-after-exit',base/'A');dr['readiness']=ready(d,'reopen-after-exit');stop(d,dr)
 report={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'records':records,'same_home_second_refused':True,'independent_home_concurrent':True,'after_first_exit_reopen':True,'data_root_effective':all('home='+str(base) in r.get('readiness','home='+str(base)) for r in records),'private_data_excluded':['local-token','device-credentials','private-key','web-session-registry']}
 (out/'binary-instances.json').write_text(json.dumps(report,indent=2));print(json.dumps({k:v for k,v in report.items() if k!='records'}))
finally:
 for p in children:
  if p.poll() is None:
   p.terminate()
   try:p.wait(timeout=5)
   except subprocess.TimeoutExpired:p.kill();p.wait()
