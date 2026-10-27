#!/usr/bin/env python3
"""保留独立400层实际目录反例，并验证真实服务存活、下轮及物理扫描期间取消。"""
import argparse,importlib.util,pathlib,json,time,resource,socket,subprocess
resource.setrlimit(resource.RLIMIT_CORE,(0,0))
ap=argparse.ArgumentParser();ap.add_argument('--binary',type=pathlib.Path,required=True);ap.add_argument('--output',type=pathlib.Path,required=True);ap.add_argument('--product-dir',type=pathlib.Path,required=True);args=ap.parse_args()
spec=importlib.util.spec_from_file_location('author_native_discovery',str(pathlib.Path(__file__).with_name('native-discovery-cases.py')));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
root=args.output.resolve();root.mkdir(exist_ok=False)
provider=m.Provider();service=m.Service(args.binary.resolve(),args.product_dir.resolve(),root,provider)
p=service.ws/'deep';p.mkdir()
for _ in range(400):p=p/'d';p.mkdir()
(p/'target.txt').write_text('deep actual text')
expected=str((p/'target.txt').relative_to(service.ws/'deep'));cases=[]
def record(name,detail):cases.append({'case':name,'status':'PASS','detail':detail});m.dump(root/'cases.json',cases);print('PASS',name,flush=True)
def schedule(ident):
 provider.steps.put(('mcp_describe_tool',{'name':'find'},ident+'-definition'));provider.steps.put(('mcp_call',{'tool':'find','arguments':{'path':'deep','pattern':'target.txt'}},ident))
try:
 service.start();agent=service.sql("SELECT agent_id FROM sessions WHERE session_id='sess_local_alpha'")[0][0]
 assert service.http('POST','/lingxi/v1/sessions',{'sessionId':'depth-probe','agentId':agent,'title':'深目录实际资源上限','permissionMode':'read_only','memoryEnabled':False})[0] in (200,201)
 schedule('depth-find');provider.steps.put(None);started=time.monotonic()
 status,result=service.http('POST','/lingxi/v1/sessions/depth-probe/execute',{'input':'实际深目录查找'})
 assert status==200 and service.p.poll() is None,(status,result,service.p.poll())
 requests=list(provider.requests);m.dump(root/'provider-requests.json',requests)
 assert len(requests)==3,requests
 text=[x['content'] for x in requests[2]['messages'] if x.get('tool_call_id')=='depth-find'];assert len(text)==1,text
 actual=json.JSONDecoder().raw_decode(text[0])[0];assert actual['paths']==[expected] and actual['total']==1 and not actual['truncated'],actual
 assert ((service.ws/'deep')/actual['paths'][0]).read_text()=='deep actual text'
 record('original legal 400-depth 401-entry actual find correct and service alive',{'elapsed':time.monotonic()-started,'actual':actual})
 provider.steps.put(None);status,result=service.http('POST','/lingxi/v1/sessions/depth-probe/execute',{'input':'深目录后下一真实运行'});assert status==200 and service.p.poll() is None,(status,result)
 record('same service accepts next genuine Run after deep scan',result)
 schedule('depth-cancel');host,port=service.addr.rsplit(':',1);transport=socket.create_connection((host,int(port)),timeout=10);payload=json.dumps({'input':'深目录实际扫描中取消'}).encode();transport.sendall((f'POST /lingxi/v1/sessions/depth-probe/execute HTTP/1.1\r\nHost: {service.addr}\r\nAuthorization: Bearer {service.token}\r\nContent-Type: application/json\r\nContent-Length: {len(payload)}\r\n\r\n').encode()+payload)
 deadline=time.monotonic()+10;observed=None
 while time.monotonic()<deadline:
  rows=service.sql("SELECT run_id,phase,target FROM invocation_journal ORDER BY rowid DESC LIMIT 1")
  if rows and rows[0][1:] == ('started','tool:first-party:find'):
   fds=subprocess.run(['/usr/sbin/lsof','-nP','-a','-p',str(service.p.pid),'-F','n'],capture_output=True,text=True,timeout=5).stdout
   if any(line.startswith('n'+str((service.ws/'deep').resolve())+'/d') for line in fds.splitlines()):observed=rows[0];(root/'cancel-physical-fds.txt').write_text(fds);break
  time.sleep(.002)
 assert observed,'真实深入目录句柄必须先于取消'
 status,busy=service.http('POST','/lingxi/v1/sessions/depth-probe/execute',{'input':'深扫描并发忙'});assert status==409,(status,busy)
 transport.shutdown(socket.SHUT_RDWR);transport.close();deadline=time.monotonic()+20;final=None
 while time.monotonic()<deadline:
  final=service.sql('SELECT status FROM runs WHERE run_id=?',(observed[0],))[0][0]
  if final in ('cancelled','failed','completed'):break
  time.sleep(.01)
 assert final=='cancelled',(observed,final)
 receipts=service.sql("SELECT phase,receipt_outcome FROM invocation_journal WHERE run_id=? AND target='tool:first-party:find'",(observed[0],));assert receipts and all(r[1]!='succeeded' for r in receipts),receipts
 record('deep physical scan cancellation terminates honestly and preserves concurrent busy',{'observed':observed,'run':final,'receipts':receipts})
 provider.steps.put(None);deadline=time.monotonic()+5
 while True:
  status,result=service.http('POST','/lingxi/v1/sessions/depth-probe/execute',{'input':'深目录取消后下一轮'})
  if status==200:break
  assert status==409 and time.monotonic()<deadline,(status,result)
  time.sleep(.01)
 assert service.p.poll() is None
 record('same service accepts next genuine Run after physical deep cancellation',result)
 assert (p/'target.txt').read_text()=='deep actual text' and service.sql('SELECT count(*) FROM session_files')[0][0]==0 and service.sql('SELECT count(*) FROM file_history_snapshots')[0][0]==0
 m.dump(root/'summary.json',{'status':'PASS','cases':len(cases),'binarySha256':m.digest(args.binary.read_bytes()),'zeroFileMutation':True,'zeroFileRegistration':True})
finally:
 if getattr(service,'p',None) and service.p.poll() is None:service.stop()
 provider.close()
