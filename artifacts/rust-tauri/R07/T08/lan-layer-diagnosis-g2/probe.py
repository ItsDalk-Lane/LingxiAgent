"""同一固定g副本的局域网健康/拒绝登录区别诊断，不改产品或系统设置。"""
import datetime,hashlib,json,os,queue,re,shutil,signal,socket,subprocess,threading,time
from pathlib import Path
HERE=Path(__file__).resolve().parent
OUT=HERE.parent
REPO=HERE.parents[4]
sha=lambda value:hashlib.sha256(value).hexdigest()
now=lambda:datetime.datetime.now(datetime.timezone.utc).isoformat()
expected='0aa124e8db5e2c791d2c568145e3b75a897df9ac988dbccb9ae01c7af0cdfb2b'
previous=OUT/'lan-layer-diagnosis-g1/receipt.json'
past=json.loads(previous.read_text())
assert sha(previous.read_bytes())=='828fb70cdfee2926a33827c751ba8f7dd86e4b3bd4a7c798988a283a310799f2'
build_path=OUT/'t08g-build.json'
build=json.loads(build_path.read_text())
assert build['exitCode']==0 and build['sourceUnchanged']
assert build['binary']['sha256']==past['binarySha256Before']==past['binarySha256After']==expected
original=Path(past['binary'])
assert sha(original.read_bytes())==expected
assert not (HERE/'receipt.json').exists()
root=HERE/'isolated';root.mkdir()
binary=root/'owned-service';shutil.copyfile(original,binary);binary.chmod(0o500)
home=root/'home';home.mkdir()
assert sha(binary.read_bytes())==expected
lan_ip=subprocess.check_output(['/usr/sbin/ipconfig','getifaddr','en0'],text=True).strip()
assert not lan_ip.startswith('127.')
record=dict(scope='OWNED_FIXED_G_BUILD_LAN_HEALTH_THEN_FOREIGN_ORIGIN_DIFFERENCE_NOT_ACCEPTANCE',startedAt=now(),
 binary=str(binary),isolatedHome=str(home),originalStableCopy=str(original),
 previousReceiptSha256=sha(previous.read_bytes()),buildReceiptSha256=sha(build_path.read_bytes()),
 sourceFingerprint=build['sourceFingerprint'],binarySha256Before=sha(binary.read_bytes()),
 interface='en0',lanIp=lan_ip,networkSettingsChanged=False,credentialsUsed=False,manualClockInjected=False,
 requests=[],samples=[],budgetSeconds=20)
env=dict(os.environ);env.pop('LINGXI_HOME',None);env['LINGXI_PRODUCT_DIR']=str(REPO/'lib')
proc=subprocess.Popen([str(binary),'--home',str(home),'--bind','0.0.0.0:0','--network-mode','lan','--shutdown-timeout-ms','2000'],
 env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
record['servicePid']=proc.pid;record['clientPid']=os.getpid()
ready=queue.Queue();stdout=[];stderr=[]
def collect(pipe,lines,find=False):
 for line in pipe:
  lines.append(line)
  if find and (match:=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)):ready.put(match[1])
threads=[threading.Thread(target=collect,args=(proc.stdout,stdout,True),daemon=True),threading.Thread(target=collect,args=(proc.stderr,stderr),daemon=True)]
for thread in threads:thread.start()
stop_samples=threading.Event();condition=threading.Condition();stage='startup';began=time.monotonic()
def command(args,port=None):
 start=time.monotonic();result=subprocess.run(args,capture_output=True,text=True,timeout=3)
 lines=result.stdout.splitlines()
 if port is not None:
  pattern=re.compile(r'[.:]'+str(port)+r'\b')
  lines=[line for line in lines if pattern.search(line) or line.strip().startswith(('Active Internet','Proto ','Current listen','Listen '))]
 return dict(argv=args,startedElapsed=round(start-began,6),endedElapsed=round(time.monotonic()-began,6),exitCode=result.returncode,stdout=lines,stderr=result.stderr.splitlines())
def sample(port):
 while not stop_samples.is_set():
  with condition:label=stage
  start=time.monotonic();item=dict(stage=label,startedElapsed=round(start-began,6),startedAt=now())
  try:
   item['ownedTcpFds']=command(['/usr/sbin/lsof','-nP','-a','-p',f'{proc.pid},{os.getpid()}','-iTCP'])
   item['tcpStates']=command(['/usr/sbin/netstat','-anv','-p','tcp'],port)
   item['listenQueues']=command(['/usr/sbin/netstat','-Lan','-p','tcp'],port)
  except Exception as error:item['observationErrorType']=type(error).__name__
  item['endedElapsed']=round(time.monotonic()-began,6)
  with condition:
   record['samples'].append(item);condition.notify_all()
  stop_samples.wait(.2)
def request(label,address,port,method,path,body=None):
 global stage
 with condition:stage=label
 item=dict(label=label,method=method,path=path,destination=[address,port],budgetSeconds=20,
           connected=False,requestWritten=False,responseBytes=0,startedAt=now(),startedElapsed=round(time.monotonic()-began,6))
 raw=b'';start=time.monotonic();sock=None
 try:
  sock=socket.create_connection((address,port),timeout=10)
  item.update(connected=True,localSocket=list(sock.getsockname()),peerSocket=list(sock.getpeername()),connectedElapsedSeconds=round(time.monotonic()-start,6))
  deadline=time.monotonic()+20;sock.settimeout(20)
  data=b'' if body is None else body.encode()
  header=f'{method} {path} HTTP/1.1\r\nHost: {address}:{port}\r\nConnection: close\r\n'
  if method=='POST':header+='Origin: http://evil.example\r\nContent-Type: application/json\r\n'+f'Content-Length: {len(data)}\r\n'
  wire=header.encode()+b'\r\n'+data
  sock.sendall(wire);item.update(requestWritten=True,requestBytes=len(wire),requestWrittenElapsedSeconds=round(time.monotonic()-start,6))
  while True:
   sock.settimeout(max(.001,deadline-time.monotonic()));chunk=sock.recv(4096)
   if not chunk:item['responseEof']=True;break
   if not raw:item['firstResponseByteElapsedSeconds']=round(time.monotonic()-start,6)
   raw+=chunk
  if b'\r\n\r\n' in raw:item['status']=int(raw.split(b'\r\n',1)[0].split()[1])
 except (OSError,TimeoutError) as error:item.update(errorType=type(error).__name__,timedOut=isinstance(error,TimeoutError))
 finally:
  if sock:sock.close()
  end=time.monotonic();item.update(responseBytes=len(raw),responseSha256=sha(raw),elapsedSeconds=round(end-start,6),endedElapsed=round(end-began,6),endedAt=now())
  (HERE/f'{label}.response.http').write_bytes(raw)
  with condition:
   record['requests'].append(item)
   # 每一阶段响应或超时后继续观察两次；不延长请求本身20秒预算。
   condition.wait_for(lambda:len([row for row in record['samples'] if row['stage']==label and row['startedElapsed']>=item['endedElapsed']])>=2,timeout=8)
   item['postResponseSamples']=len([row for row in record['samples'] if row['stage']==label and row['startedElapsed']>=item['endedElapsed']])
  print(json.dumps(dict(stage=label,status=item.get('status'),responseBytes=len(raw),elapsedSeconds=item['elapsedSeconds'],postResponseSamples=item['postResponseSamples']),ensure_ascii=False),flush=True)
try:
 port=int(ready.get(timeout=20).rsplit(':',1)[1]);record['port']=port
 record['route']=command(['/sbin/route','-n','get',lan_ip]);record['soqlencomp']=command(['/usr/sbin/sysctl','-n','kern.ipc.soqlencomp'])
 sampler=threading.Thread(target=sample,args=(port,),daemon=True);sampler.start()
 request('lan-health',lan_ip,port,'GET','/lingxi/v1/health')
 invalid='{"credential":"synthetic-invalid-local-diagnosis","clientKind":"mobile"}'
 request('lan-foreign-origin-login',lan_ip,port,'POST','/lingxi/v1/web-auth/login',invalid)
 request('same-process-loopback-foreign-origin-login','127.0.0.1',port,'POST','/lingxi/v1/web-auth/login',invalid)
except Exception as error:record['probeErrorType']=type(error).__name__;print(json.dumps({'probeErrorType':type(error).__name__}),flush=True)
finally:
 stop_samples.set()
 if 'sampler' in globals():sampler.join(timeout=10);record['samplerStopped']=not sampler.is_alive()
 if proc.poll() is None:proc.send_signal(signal.SIGTERM)
 try:record['serviceExitCode']=proc.wait(timeout=7)
 except subprocess.TimeoutExpired:proc.kill();record.update(serviceExitCode=proc.wait(timeout=5),forcedKill=True)
 for thread in threads:thread.join(timeout=2)
 for name,lines in [('stdout',stdout),('stderr',stderr)]:
  content=''.join(lines);(HERE/(name+'.log')).write_text(content);record[name+'Sha256']=sha(content.encode())
 record.update(binarySha256After=sha(binary.read_bytes()),originalStableCopySha256After=sha(original.read_bytes()),probeSha256=sha(Path(__file__).read_bytes()),endedAt=now(),serviceGone=proc.poll() is not None)
 if 'port' in record:record['listenerAfterCleanup']=command(['/usr/sbin/lsof','-nP',f'-iTCP:{record["port"]}','-sTCP:LISTEN'])
 record['expectedResults']=[200,403,403];record['observedResults']=[item.get('status') for item in record['requests']]
 record['diagnosticSucceeded']=record['observedResults']==record['expectedResults'] and all(item['postResponseSamples']>=2 for item in record['requests']) and record['serviceExitCode']==0 and record.get('samplerStopped') and record['binarySha256After']==record['originalStableCopySha256After']==expected
 record['conclusion']='仅记录同固定g程序的一次health与foreign-Origin拒绝区别，不替代原真实账号登录/LAN门禁，不自动归因任何过滤器。'
 (HERE/'receipt.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n')
 print(json.dumps(dict(diagnosticSucceeded=record['diagnosticSucceeded'],observedResults=record['observedResults'],samples=len(record['samples']),serviceExitCode=record['serviceExitCode'],serviceGone=record['serviceGone'],receiptSha256=sha((HERE/'receipt.json').read_bytes())),ensure_ascii=False),flush=True)
raise SystemExit(0 if record['diagnosticSucceeded'] else 1)
