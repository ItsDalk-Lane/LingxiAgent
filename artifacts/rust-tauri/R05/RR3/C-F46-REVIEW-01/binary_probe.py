from review_capture import *
import selectors,signal
label,binary_arg,mode=sys.argv[1:];ev=EV/label;ev.mkdir();binary=pathlib.Path(binary_arg).resolve();home=ev/'home';home.mkdir();logs=home/'lingxi-service/logs'
if mode in ('three','error'):
 logs.mkdir(parents=True)
 for seq in range(1,4):
  p=logs/f'service-{seq:06}.log'
  if mode=='error' and seq==1:p.mkdir()
  else:p.write_text(f'prior-{seq}\n')
rows=[];count=6 if mode=='six' else 1
for i in range(count):
 cmd=[str(binary),'--home',str(home),'--bind','127.0.0.1:0','--log-max-files','3','--log-max-bytes','65536'];start=utc();output=[];ready=False
 with (ev/f'service-{i}-stderr.log').open('wb') as f:
  p=subprocess.Popen(cmd,cwd=ev,stdout=subprocess.PIPE,stderr=f);sel=selectors.DefaultSelector();sel.register(p.stdout,selectors.EVENT_READ)
  try:
   end=time.monotonic()+30
   while time.monotonic()<end:
    if not sel.select(max(0,end-time.monotonic())):break
    line=p.stdout.readline()
    if not line:break
    output.append(line.decode(errors='replace'))
    if line.startswith(b'LINGXI_SERVICE_READY '):ready=True;break
   inventory=[{'name':q.name,'isDirectory':q.is_dir(),'bytes':q.stat().st_size if q.is_file() else None,'sha256':sha(q) if q.is_file() else None} for q in sorted(logs.glob('service-*.log'))]
   preserved=all((logs/f'service-{seq:06}.log').read_text()==f'prior-{seq}\n' for seq in (2,3)) if mode=='three' else None
  finally:
   sel.close()
   if p.poll() is None:p.send_signal(signal.SIGTERM)
   try:code=p.wait(timeout=15)
   except subprocess.TimeoutExpired:p.kill();code=p.wait();code='BLOCKED forced kill'
 (ev/f'service-{i}-stdout.log').write_text(''.join(output));err=(ev/f'service-{i}-stderr.log').read_text()
 explicit='LINGXI_SERVICE_LOG_WRITE_FAILED' in err and 'continuing stderr-only' in err
 passed=ready and code==0 and (explicit if mode=='error' else len(inventory)<=3) and (preserved is not False)
 rows.append({'command':cmd,'pid':p.pid,'startedAt':start,'endedAt':utc(),'ready':ready,'exitCode':code,'reaped':p.poll() is not None,'filesAtReady':inventory,'count':len(inventory),'preservedPrior2And3':preserved,'explicitError':explicit,'assertionPass':passed})
shutil.rmtree(home)
res={'caseId':'R05-T08-C12','finding':'F46','mode':mode,'binary':str(binary),'binarySha256':sha(binary),'rows':rows,'threshold':3,'cleanup':{'homeRemoved':not home.exists(),'allReaped':all(r['reaped'] for r in rows)},'status':'PASS' if all(r['assertionPass'] for r in rows) else 'FAIL'}
(ev/'result.json').write_text(json.dumps(res,ensure_ascii=False,indent=2));print({'mode':mode,'status':res['status'],'counts':[r['count'] for r in rows]});sys.exit(0 if res['status']=='PASS' else 1)
