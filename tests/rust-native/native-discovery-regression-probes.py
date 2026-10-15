#!/usr/bin/env python3
"""独立复核原六条真实Run反例，保留全部原断言，仅把二进制和证据目录参数化。"""
import importlib.util,pathlib,json,hashlib,os,argparse
ap=argparse.ArgumentParser();ap.add_argument("--binary",type=pathlib.Path,required=True);ap.add_argument("--output",type=pathlib.Path,required=True);ap.add_argument("--product-dir",type=pathlib.Path,required=True);cli_args=ap.parse_args()
spec=importlib.util.spec_from_file_location('author_native_discovery',str(pathlib.Path(__file__).with_name('native-discovery-cases.py')));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
root=cli_args.output.resolve();root.mkdir(exist_ok=False)
provider=m.Provider();service=m.Service(cli_args.binary.resolve(),cli_args.product_dir.resolve(),root,provider)
ws=service.ws;odd=r'odd\name.txt';(ws/odd).write_text('NEEDLE\n');raw=b'\xef\xbb\xbf'+ '🙂 café\r\nßβ NEEdLE\r\n'.encode();(ws/'unicode.txt').write_bytes(raw)
(ws/'oversized.txt').write_bytes(b'x'*(16*1024*1024+1));(ws/'too-many-lines.txt').write_bytes(b'x\n'*200001)
cases=[];seq=0;sid='independent_discovery';before={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in ws.iterdir()}
def invoke(tool,args):
 global seq
 seq+=1;ident=f'independent-{seq}';start=len(provider.requests)
 provider.steps.put(('mcp_describe_tool',{'name':tool},ident+'-definition'));provider.steps.put(('mcp_call',{'tool':tool,'arguments':args},ident));provider.steps.put(None)
 status,response=service.http('POST',f'/lingxi/v1/sessions/{sid}/execute',{'input':f'独立真实读取检查{seq}'})
 assert status==200,(status,response)
 reqs=provider.requests[start:];assert len(reqs)==3,len(reqs);m.dump(root/f'provider-{seq}.json',reqs)
 msg=[x for x in reqs[2]['messages'] if x.get('tool_call_id')==ident];assert len(msg)==1,msg
 content=msg[0]['content'];m.dump(root/f'outcome-{seq}.json',{'tool':tool,'arguments':args,'actual':content})
 try:return json.JSONDecoder().raw_decode(content)[0]
 except ValueError:return content
try:
 service.start();agent=service.sql("SELECT agent_id FROM sessions WHERE session_id='sess_local_alpha'")[0][0]
 assert service.http('POST','/lingxi/v1/sessions',{'sessionId':sid,'agentId':agent,'title':'独立发现工具边界','permissionMode':'read_only','memoryEnabled':False})[0] in (200,201)
 checks=[('find preserves actual Unix backslash filename','find',{'pattern':'*.txt'},lambda r:isinstance(r,dict) and odd in r['paths']),('grep source path preserves actual Unix backslash filename','grep',{'path':odd,'pattern':'NEEDLE'},lambda r:isinstance(r,dict) and r['matches'][0]['path']==odd),('ls actual Unix filename positive control','ls',{},lambda r:isinstance(r,dict) and odd in [e['name'] for e in r['entries']]),('grep astral Unicode BOM CRLF positions','grep',{'path':'unicode.txt','pattern':'needle','ignoreCase':True},lambda r:isinstance(r,dict) and r['matches'][0]['positions'][0]['byteStart']==20 and r['matches'][0]['positions'][0]['column']==4 and r['matches'][0]['sourceSha256']==hashlib.sha256(raw).hexdigest()),('oversized file fails explicitly','grep',{'path':'oversized.txt','pattern':'x'},lambda r:isinstance(r,str) and 'NATIVE_GREP_FILE_BUDGET_EXCEEDED' in r),('too many lines fails explicitly','grep',{'path':'too-many-lines.txt','pattern':'x'},lambda r:isinstance(r,str) and 'NATIVE_GREP_LINE_BUDGET_EXCEEDED' in r)]
 for name,tool,args,check in checks:
  actual=invoke(tool,args);passed=bool(check(actual));case={'name':name,'result':'PASS' if passed else 'FAIL','actual':actual};cases.append(case);print(json.dumps(case,ensure_ascii=False),flush=True)
 assert {p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in ws.iterdir()}==before
 assert service.sql('SELECT count(*) FROM session_files')[0][0]==0;assert service.sql('SELECT count(*) FROM file_history_snapshots')[0][0]==0
 m.dump(root/'cases.json',cases);m.dump(root/'summary.json',{'pass':sum(c['result']=='PASS' for c in cases),'fail':sum(c['result']=='FAIL' for c in cases),'zeroFileMutation':True,'zeroFileRegistration':True,'binarySha256':hashlib.sha256(cli_args.binary.read_bytes()).hexdigest()})
 assert all(case['result']=='PASS' for case in cases),cases
finally:
 if getattr(service,'p',None) and service.p.poll() is None:service.stop()
 provider.close()
