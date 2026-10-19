import os,sys,json,time,threading,tempfile,pathlib,fcntl,signal,shutil,http.server,hashlib
out=pathlib.Path(__file__).resolve().parent
trace=[]; lock=threading.Lock(); replies={}; events=[]; condition=threading.Condition(); seq=0; started=time.monotonic()
def record(kind,**kw):
 with lock: trace.append({'seconds':round(time.monotonic()-started,6),'kind':kind,**kw})
class Handler(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  record('http',path=self.path)
  if self.path=='/form': body=b"<html><body><button id='dialog' onclick=\"alert('local dialog');document.body.dataset.dialog='done'\">Dialog</button></body></html>"; code=200
  else: body=b'blocked'; code=403
  self.send_response(code);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
 def log_message(self,*a): pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
root=pathlib.Path(tempfile.mkdtemp(prefix='lingxi-cdp-readonly-probe-'))
chrome='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
argv=[chrome,'--remote-debugging-pipe','--user-data-dir='+str(root),'--profile-directory=Default','--headless=new','--no-first-run','--no-default-browser-check','--disable-background-networking','--disable-component-update','--disable-sync','--metrics-recording-only','--no-startup-window','--window-size=1280,860','--proxy-server=http://127.0.0.1:'+str(server.server_port),'--proxy-bypass-list=<-loopback>;127.0.0.1']
fds=[]
for _ in range(2):
 a,b=os.pipe(); aa=fcntl.fcntl(a,fcntl.F_DUPFD_CLOEXEC,10);bb=fcntl.fcntl(b,fcntl.F_DUPFD_CLOEXEC,10);os.close(a);os.close(b);fds.extend([aa,bb])
cr,cw,rr,rw=fds
err=os.open(out/'chrome-stderr.log',os.O_WRONLY|os.O_CREAT|os.O_TRUNC,0o600)
actions=[(os.POSIX_SPAWN_DUP2,err,2),(os.POSIX_SPAWN_DUP2,cr,3),(os.POSIX_SPAWN_DUP2,rw,4),(os.POSIX_SPAWN_OPEN,1,'/dev/null',os.O_WRONLY,0),(os.POSIX_SPAWN_OPEN,0,'/dev/null',os.O_RDONLY,0)]
record('launch',argv=argv,profile=str(root))
pid=os.posix_spawn(chrome,argv,os.environ,file_actions=actions,setpgroup=0)
os.close(cr);os.close(rw);os.close(err)
record('owned_process',pid=pid,pgid=os.getpgid(pid))
def read():
 buf=b''
 while True:
  data=os.read(rr,65536)
  if not data: record('pipe_eof');return
  buf+=data
  while b'\0' in buf:
   raw,buf=buf.split(b'\0',1)
   if not raw:continue
   try: msg=json.loads(raw)
   except BaseException as exc:
    record('non_json_frame',raw=raw.decode('utf-8','replace'),error=repr(exc)); continue
   record('receive',message=msg)
   with condition:
    if 'id' in msg:replies[msg['id']]=msg
    else:events.append(msg)
    condition.notify_all()
threading.Thread(target=read,daemon=True).start()
def send(method,params=None,sid=None):
 global seq
 seq+=1;msg={'id':seq,'method':method,'params':params or {}}
 if sid:msg['sessionId']=sid
 record('send',message=msg);os.write(cw,json.dumps(msg).encode()+b'\0');return seq
def wait(i,seconds=8):
 until=time.monotonic()+seconds
 with condition:
  while i not in replies:
   remain=until-time.monotonic()
   if remain<=0:raise TimeoutError('CDP id '+str(i))
   condition.wait(remain)
  msg=replies[i]
 if 'error' in msg:raise RuntimeError(msg)
 return msg['result']
def call(m,p=None,s=None):return wait(send(m,p,s))
summary={'purpose':'真实Chrome中对照Page.getNavigationHistory在alert前/期间响应；无候选源码改动、无Cargo','argv':argv,'pid':pid,'profile':str(root),'checks':[],'status':'RUNNING'}
try:
 summary['version']=call('Browser.getVersion')
 target=call('Target.createTarget',{'url':'about:blank'})['targetId']
 sid=call('Target.attachToTarget',{'targetId':target,'flatten':True})['sessionId']
 call('Page.enable',s=sid);call('Runtime.enable',s=sid)
 call('Page.navigate',{'url':'http://127.0.0.1:'+str(server.server_port)+'/form'},sid)
 until=time.monotonic()+8
 while True:
  r=call('Runtime.evaluate',{'expression':"!!document.querySelector('#dialog')",'returnByValue':True},sid)
  if r['result'].get('value'):break
  if time.monotonic()>until:raise TimeoutError('local page')
  time.sleep(.05)
 for roundn in range(2):
  t=time.monotonic();normal=call('Page.getNavigationHistory',s=sid);normal_sec=time.monotonic()-t
  with condition: cursor=len(events)
  evaluation=send('Runtime.evaluate',{'expression':"document.querySelector('#dialog').click();document.body.dataset.dialog",'returnByValue':True,'awaitPromise':True},sid)
  until=time.monotonic()+8
  with condition:
   while not any(e.get('method')=='Page.javascriptDialogOpening' and e.get('sessionId')==sid for e in events[cursor:]):
    remain=until-time.monotonic()
    if remain<=0:raise TimeoutError('dialog event')
    condition.wait(remain)
  targets=call('Target.getTargets')
  history=send('Page.getNavigationHistory',sid=sid)
  time.sleep(2.2)
  with condition: pre_history=history in replies;pre_eval=evaluation in replies
  record('before_dialog_accept',round=roundn,history_responded=pre_history,evaluate_responded=pre_eval)
  call('Page.handleJavaScriptDialog',{'accept':True},sid)
  hist=wait(history);value=wait(evaluation)
  summary['checks'].append({'round':roundn,'normal_history_seconds':normal_sec,'normal_history_entries':len(normal['entries']),'alert_open_event_observed':True,'history_responded_within_2_2_seconds_before_accept':pre_history,'evaluate_responded_before_accept':pre_eval,'history_responded_after_accept':True,'evaluate_result_after_accept':value,'targets_count_during_alert':len(targets['targetInfos'])})
 summary['status']='OBSERVED'
except BaseException as e:
 summary['status']='FAILED';summary['error']=repr(e)
finally:
 try:call('Browser.close')
 except BaseException as e:record('browser_close_result',error=repr(e))
 until=time.monotonic()+3;reaped=False
 while time.monotonic()<until:
  p,status=os.waitpid(pid,os.WNOHANG)
  if p:summary['wait_status']=status;reaped=True;break
  time.sleep(.05)
 if not reaped:
  record('owned_process_group_sigterm',pid=pid);os.killpg(pid,signal.SIGTERM)
  _,status=os.waitpid(pid,0);summary['wait_status']=status
 os.close(cw);os.close(rr);server.shutdown();server.server_close()
 summary['owned_pid_reaped']=True
 shutil.rmtree(root);summary['only_owned_profile_removed']=not root.exists()
 (out/'trace.json').write_text(json.dumps(trace,ensure_ascii=False,indent=2)+'\n')
 summary['trace_sha256']=hashlib.sha256((out/'trace.json').read_bytes()).hexdigest()
 summary['script_sha256']=hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()
 (out/'result.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
 print(json.dumps(summary,ensure_ascii=False))
