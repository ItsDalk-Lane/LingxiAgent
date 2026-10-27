"""F01永久业务探针：独立Rust进程；仅外部模型为合成loopback服务。"""
import shutil,base64,hashlib,http.server,json,os,re,signal,sqlite3,subprocess,sys,tempfile,threading,time,urllib.request,urllib.error
from pathlib import Path
BIN=Path(sys.argv[1]);CASE=sys.argv[2];nonce='F01-'+os.urandom(16).hex()
class Fixture:
 def __init__(self,tag,protocol='openai-completions',window=1000000):
  self.base=Path(tempfile.mkdtemp(prefix='lingxi-f01-'+tag+'-'));self.home=self.base/'home';self.ws=self.base/'workspace';self.home.mkdir();self.ws.mkdir();self.protocol=protocol;self.window=window;self.wire=[];self.logs=[];self.p=None;self.reply=self.normal;self.answer='ACK '+nonce;self.provider='fixture';self.starts=[];self.closed=[];self.script_errors=[];self.http=[]
  # 程序可能放在仓库外；合成材料必须由夹具提供，不能依赖程序祖先目录或宿主环境。
  self.product=self.base/'product';(self.product/'yuan').mkdir(parents=True)
  (self.product/'yuan/lingxi.md').write_text('F01 合成通用人格材料。',encoding='utf-8')
  (self.product/'identity.example.md').write_text('F01 合成助手身份。',encoding='utf-8')
  (self.product/'agents.example.md').write_text('F01 合成助手协作说明。',encoding='utf-8')
  f=self
  class Handler(http.server.BaseHTTPRequestHandler):
   def do_POST(self):
    body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));f.wire.append(body)
    try:
     parts,status=f.reply(body);data=''.join('data: '+json.dumps(x,ensure_ascii=False)+'\n\n' for x in parts).encode()
     if f.protocol=='openai-completions':data+=b'data: [DONE]\n\n'
     self.send_response(status);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
    except (BrokenPipeError,ConnectionResetError):pass
    except Exception as e:f.script_errors.append(repr(e));raise
   def log_message(self,*args):pass
  self.server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);self.server.daemon_threads=True;self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start();self.config=self.base/'config.json';self.configure()
 def configure(self):
  self.config.write_text(json.dumps({'home':str(self.home),'workspace':str(self.ws),'providers':{self.provider:{'endpoint':f'http://127.0.0.1:{self.server.server_port}/v1','protocol':self.protocol,'auth':{'kind':'none'}}},'models':{'chat':{'provider':self.provider,'model':'fixture-model','capabilities':{'tools':True},'compat':{'contextWindow':self.window}},'summarize':{'provider':self.provider,'model':'summarize-model','capabilities':{'tools':True},'compat':{'contextWindow':200000}}}}))
  if self.protocol=='openai-codex-responses':
   cfg=json.loads(self.config.read_text());cfg['providers'][self.provider]['auth']={'kind':'oauth','flow':'deviceCode','clientId':'synthetic-client','tokenEndpoint':f'http://127.0.0.1:{self.server.server_port}/token','deviceAuthorizationEndpoint':f'http://127.0.0.1:{self.server.server_port}/device'};self.config.write_text(json.dumps(cfg));runtime=self.home/'lingxi-service';runtime.mkdir(exist_ok=True);payload=base64.urlsafe_b64encode(json.dumps({'https://api.openai.com/auth':{'chatgpt_account_id':'synthetic-account'}}).encode()).decode().rstrip('=');(runtime/'credentials.json').write_text(json.dumps({'version':1,'providers':{self.provider:{'tokens':{'accessToken':'synthetic.'+payload+'.signature','refreshToken':'synthetic-unused','expiresAtUnixMs':4102444800000}}}}))
 def normal(self,b):return self.text(self.answer),200
 def text(self,text):
  if self.protocol=='google-generative-ai':return [{'candidates':[{'content':{'role':'model','parts':[{'text':text}]},'finishReason':'STOP'}],'usageMetadata':{'promptTokenCount':10,'candidatesTokenCount':3,'totalTokenCount':13}}]
  if self.protocol in ('openai-responses','openai-codex-responses'):return [{'type':'response.completed','response':{'id':'resp_'+str(len(self.wire)),'status':'completed','output':[{'type':'message','id':'msg_'+str(len(self.wire)),'role':'assistant','status':'completed','content':[{'type':'output_text','text':text}]}],'usage':{'input_tokens':10,'output_tokens':3}}}]
  if self.protocol=='anthropic-messages':return [{'type':'message_start','message':{'id':'msg_'+str(len(self.wire)),'type':'message','role':'assistant','content':[],'model':'fixture-model','usage':{'input_tokens':10,'output_tokens':0}}},{'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}},{'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':text}},{'type':'content_block_stop','index':0},{'type':'message_delta','delta':{'stop_reason':'end_turn','stop_sequence':None},'usage':{'output_tokens':3}},{'type':'message_stop'}]
  return [{'choices':[{'index':0,'delta':{'role':'assistant','content':text},'finish_reason':None}]},{'choices':[{'index':0,'delta':{},'finish_reason':'stop'}]},{'choices':[],'usage':{'prompt_tokens':10,'completion_tokens':3}}]
 def tool(self,name,args,identity='provider_tool_1'):
  return [{'choices':[{'index':0,'delta':{'role':'assistant','tool_calls':[{'index':0,'id':identity,'type':'function','function':{'name':name,'arguments':json.dumps(args)}}]},'finish_reason':None}]},{'choices':[{'index':0,'delta':{},'finish_reason':'tool_calls'}]},{'choices':[],'usage':{'prompt_tokens':10,'completion_tokens':3}}]
 def start(self):
  env=os.environ.copy();env['LINGXI_HOME']=str(self.home);env['LINGXI_PRODUCT_DIR']=str(self.product);self.p=subprocess.Popen([str(BIN),'--home',str(self.home),'--config',str(self.config),'--bind','127.0.0.1:0','--shutdown-timeout-ms',str(getattr(self,'shutdown_ms',10000))],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env);self.err=threading.Thread(target=lambda:self.logs.extend(self.p.stderr.readlines()),daemon=True);self.err.start();line=self.p.stdout.readline().strip();assert line.startswith('LINGXI_SERVICE_READY '),(line,self.logs);self.addr=re.search(r'addr=([^ ]+)',line).group(1);self.token=json.loads((self.home/'lingxi-service/local-token.json').read_text())['token'];self.starts.append({'ready':line,'binarySHA':hashlib.sha256(BIN.read_bytes()).hexdigest(),'argv':[str(BIN),'--home',str(self.home),'--config',str(self.config),'--bind','127.0.0.1:0','--shutdown-timeout-ms',str(getattr(self,'shutdown_ms',10000))],'effectiveLINGXI_HOME':env['LINGXI_HOME'],'effectiveLINGXI_PRODUCT_DIR':env['LINGXI_PRODUCT_DIR']})
 def stop(self):
  if self.p is not None:
   self.p.send_signal(signal.SIGTERM)
   try:code=self.p.wait(timeout=20)
   except subprocess.TimeoutExpired:self.p.kill();code=self.p.wait()
   self.err.join(2);self.closed.append({'exit':code,'reaped':self.p.poll() is not None});self.p=None
 def request(self,path,body=None,method=None,expected=200):
  req=urllib.request.Request('http://'+self.addr+'/lingxi/v1/'+path,data=None if body is None else json.dumps(body).encode(),headers={'Authorization':'Bearer '+self.token,'Content-Type':'application/json'},method=method or ('GET' if body is None else 'POST'))
  try:
   with urllib.request.urlopen(req,timeout=50) as res:status=res.status;raw=res.read()
  except urllib.error.HTTPError as e:status=e.code;raw=e.read()
  value=json.loads(raw);self.http.append({'path':path,'method':method or ('GET' if body is None else 'POST'),'input':body,'status':status,'response':value,'authorization':'Bearer <redacted synthetic>'});assert status==expected,(path,status,value);return value
 def run(self,text,sid='sess_local_alpha',expected=200):return self.request(f'sessions/{sid}/execute',{'input':text},expected=expected)
 def sql(self,query,args=()):
  with sqlite3.connect(self.home/'lingxi-service/data/runs.db') as db:return db.execute(query,args).fetchall()
 def snapshot(self):return {'base':str(self.base),'wire':self.wire,'httpRequests':self.http,'starts':self.starts,'closed':self.closed,'logs':self.logs,'runs':self.sql('SELECT run_id,session_id,status,terminal_reason FROM runs ORDER BY run_id'),'usage':self.sql('SELECT * FROM model_call_usage ORDER BY rowid'),'messages':self.sql('SELECT message_id,session_id,run_id,role,entry_type,content_json,parent_message_id FROM messages ORDER BY session_id,seq'),'heads':self.sql('SELECT * FROM session_branch_heads'),'events':self.sql('SELECT event_id,event_type,payload_json FROM key_events ORDER BY stream_id,seq'),'script_errors':self.script_errors,'databaseSHA':hashlib.sha256((self.home/'lingxi-service/data/runs.db').read_bytes()).hexdigest(),'workspaceSHA':{str(p.relative_to(self.ws)):hashlib.sha256(p.read_bytes()).hexdigest() for p in self.ws.rglob('*') if p.is_file()}}
 def close(self):self.stop();self.server.shutdown();self.server.server_close();self.thread.join()
def msgs(w):return w['messages']
def count(w,text,role):return sum(x.get('content')==text and x.get('role')==role for x in msgs(w))
def has(w,text):return text in json.dumps(w,ensure_ascii=False)
def tool_count(f):return sum('tool_calls' in json.dumps(w) for w in f.wire)
fixtures=[];checks=[]
def make(*args,**kw):f=Fixture(*args,**kw);fixtures.append(f);f.start();return f
def c01():
 f=make('c01');first='记住 '+nonce;second='上一轮的标记是什么';f.run(first);assert count(f.wire[-1],first,'user')==1;f.answer='other';f.run(second);w=f.wire[-1];assert count(w,first,'user')==count(w,'ACK '+nonce,'assistant')==count(w,second,'user')==1;assert [x['role'] for x in msgs(w) if x.get('role')!='system']==['user','assistant','user'];f.run('独立会话','sess_local_beta');assert not has(f.wire[-1],nonce);export=f.request('sessions/sess_local_alpha/export');assert 'model_history' not in json.dumps(export) and 'lingxi-model-exchange-v1' not in json.dumps(export);assert sum(i['kind']=='final_message' for i in export['items'])==2;assert sum(i['kind']=='user_message' for i in export['items'])==2;assert not any(i['kind']=='tool_call' for i in export['items']);assert f.request('sessions/sess_local_alpha/history?all=1')['items']==export['items'];
 # 真实观测整文及逐段摘要必须与第二请求实际发送的system字节完全一致。
 view=f.request('sessions/sess_local_alpha/context-observation');system=next(m['content'] for m in msgs(w) if m['role']=='system').encode();assert view['renderSha256']==hashlib.sha256(system).hexdigest()
 for segment in view['segments']:
  part=system[segment['byteStart']:segment['byteEnd']];assert segment['sha256']==hashlib.sha256(part).hexdigest()
  if segment['disclosure']=='full':assert segment['text'].encode()==part
  else:assert 'text' not in segment
 checks.append('C01 actual wire roles/order/current once/session isolation/UI no archive duplicates')
 # 旧普通会话保留兼容，移除专用档案并修复其原树链，绝不造工具历史。
 f.stop();rows=f.sql('SELECT message_id,parent_message_id,entry_type FROM messages WHERE session_id=? ORDER BY seq',('sess_local_alpha',));parents={r[0]:r[1] for r in rows};archives={r[0] for r in rows if r[2]=='lingxi-model-exchange-v1'}
 with sqlite3.connect(f.home/'lingxi-service/data/runs.db') as db:
  for mid,parent,kind in rows:
   while parent in archives:parent=parents[parent]
   if mid not in archives:db.execute('UPDATE messages SET parent_message_id=? WHERE message_id=?',(parent,mid))
  db.execute("DELETE FROM messages WHERE entry_type='lingxi-model-exchange-v1' AND session_id='sess_local_alpha'")
 f.start();f.run('旧文本兼容');assert has(f.wire[-1],nonce);checks.append('legacy plain user/assistant history restored without guessed run')
def c02():
 f=make('c02');(f.ws/'ancestor.txt').write_text('ancestor tool '+nonce);issued=[False]
 def ancestor(body):
  if not issued[0]:issued[0]=True;return f.tool('read',{'path':'ancestor.txt'}),200
  return f.normal(body)
 f.reply=ancestor
 a='A-'+nonce;b='B-'+nonce;root='共同祖先-'+nonce;r=f.run(root);rid=r['runId'];f.request('sessions/sess_local_alpha/fork',{'newSessionId':'fork_B','boundaryMessageId':rid+'-final'});f.answer='A-answer';f.run(a);f.answer='B-answer';br=f.run(b,'fork_B');assert has(f.wire[-1],root) and not has(f.wire[-1],a);f.request('sessions/fork_B/checkpoints',{'name':'beforeB','targetMessageId':'user:'+br['runId'],'filePaths':[]});f.stop();f.start();f.run('A恢复');assert has(f.wire[-1],a) and not has(f.wire[-1],b);f.run('B恢复','fork_B');assert has(f.wire[-1],b) and not has(f.wire[-1],a);retry=f.request('sessions/fork_B/turns/retry',{'fileRollback':'none'});f.run('B有效重试','fork_B');assert has(f.wire[-1],b) and not has(f.wire[-1],'B恢复');f.request('sessions/fork_B/rewind',{'checkpoint':'beforeB','restoreFiles':False});f.run('B回退后','fork_B');assert has(f.wire[-1],root) and not has(f.wire[-1],b) and not has(f.wire[-1],'B恢复');f.request('sessions/fork_B/turns/retry',{'targetMessageId':'user:'+br['runId'],'fileRollback':'none'},expected=400);assert len(f.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_started'"))==1
 checks.append('C02 real ancestor tool executed once; fork mutually exclusive nonce/restart/rewind old suffix absent')
 f.request('sessions/archive',{'sessionId':'sess_local_alpha','childMode':'detach_children'});f.request('sessions/archived/delete',{'sessionId':'sess_local_alpha'});f.stop();f.start();f.run('源删除后','fork_B');assert has(f.wire[-1],root) and not has(f.wire[-1],a);assert all(r[0] is None for r in f.sql("SELECT run_id FROM messages WHERE session_id='fork_B' AND content_json LIKE ?",('%'+rid+'%',)));checks.append('fork archive survives real source delete without source Run FK')
def c03():
 f=make('c03');middle='RAW-MIDDLE-'+nonce;raw='HEAD-'+('x'*17000)+middle+('y'*17000)+'-TAIL';(f.ws/'fixture.txt').write_text(raw);tool_issued=[False]
 def reply(body):
  if not tool_issued[0]:tool_issued[0]=True;return f.tool('read',{'path':'fixture.txt'}),200
  return f.text('read complete'),200
 f.reply=reply;f.run('读取合成文件');assert len(f.wire)==2;first_result=[m for m in msgs(f.wire[-1]) if m['role']=='tool'][0];assert len(first_result['content'].encode())<=32768 and middle not in first_result['content'] and first_result['tool_call_id']=='provider_tool_1';f.stop();f.start();f.run('使用刚才工具的结果');w=f.wire[-1];results=[m for m in msgs(w) if m['role']=='tool'];assert len(results)==1 and results[0]==first_result;assert not has(w,middle);events=f.sql("SELECT event_type,payload_json FROM key_events WHERE event_type='tool_call_completed'");assert any(middle in p for _,p in events),events;assert len(f.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_started'"))==1;ui=f.request('sessions/sess_local_alpha/export');assert sum(i['kind']=='tool_call' for i in ui['items'])==1;assert sum(i['kind']=='final_message' for i in ui['items'])==2;assert 'lingxi-model-exchange-v1' not in json.dumps(ui);assert f.request('sessions/sess_local_alpha/history?all=1')['items']==ui['items'];checks.append('C03 real read raw complete/F08 projection only/restart IDs/no re-execution')
 f.stop();before=len(f.wire)
 with sqlite3.connect(f.home/'lingxi-service/data/runs.db') as db:
  mid,value=db.execute("SELECT message_id,content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1' ORDER BY seq LIMIT 1").fetchone();v=json.loads(value);v['sourceRun']='forged-source';db.execute('UPDATE messages SET content_json=? WHERE message_id=?',(json.dumps(v),mid))
 f.start();f.request('sessions/sess_local_alpha/fork',{'newSessionId':'corrupted_fork','boundaryMessageId':f.sql("SELECT head_message_id FROM session_branch_heads WHERE session_id='sess_local_alpha'")[0][0]},expected=500);assert not f.sql("SELECT session_id FROM sessions WHERE session_id='corrupted_fork'");f.run('拒绝损坏关联',expected=500);assert len(f.wire)==before;checks.append('scope/source binding corruption rejected before provider request')
 g=make('opaque',protocol='anthropic-messages');opaque='signed-'+nonce
 (g.ws/'signed.txt').write_text('SIGNED-TOOL-'+nonce)
 def thinking(body):
  g.reply=g.normal
  return [{'type':'message_start','message':{'id':'msg_signed','type':'message','role':'assistant','content':[],'model':'fixture-model','usage':{'input_tokens':10,'output_tokens':0}}},{'type':'content_block_start','index':0,'content_block':{'type':'thinking','thinking':'reason-'+nonce,'signature':opaque}},{'type':'content_block_stop','index':0},{'type':'content_block_start','index':1,'content_block':{'type':'tool_use','id':'toolu_signed','name':'read','input':{}}},{'type':'content_block_delta','index':1,'delta':{'type':'input_json_delta','partial_json':json.dumps({'path':'signed.txt'})}},{'type':'content_block_stop','index':1},{'type':'message_delta','delta':{'stop_reason':'tool_use','stop_sequence':None},'usage':{'output_tokens':3}},{'type':'message_stop'}],200
 g.reply=thinking;g.run('签名首轮');g.stop();g.start();g.run('签名恢复');assert has(g.wire[-1],opaque) and has(g.wire[-1],'toolu_signed') and has(g.wire[-1],'SIGNED-TOOL-'+nonce);assert len(g.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_started'"))==1;g.stop();g.provider='different-provider';g.configure();before=len(g.wire);g.start();g.run('换供应商');assert len(g.wire)==before;checks.append('opaque exact restored; changed serving provider explicitly failed before wire');legacy_boundaries();cross_session_archive_control();process_projection_windows();forged_argument_refusal_controls();deep_fork_controls()

def deep_fork_controls():
 outcomes=[]
 for mode in ['tool_pair','opaque_origin_missing','opaque_origin_forged','actor']:
  f=make('deep-fork-'+mode,protocol='anthropic-messages' if mode.startswith('opaque') else 'openai-completions');(f.ws/'deep.txt').write_text('TOOL-'+nonce);issued=[False]
  def reply(body):
   if not issued[0]:
    issued[0]=True
    if mode.startswith('opaque'):
     return [{'type':'message_start','message':{'id':'msg_signed','type':'message','role':'assistant','content':[],'model':'fixture-model','usage':{'input_tokens':10,'output_tokens':0}}},{'type':'content_block_start','index':0,'content_block':{'type':'thinking','thinking':'reason-'+nonce,'signature':'signed-'+nonce}},{'type':'content_block_stop','index':0},{'type':'content_block_start','index':1,'content_block':{'type':'text','text':'SIGNED-FINAL-'+nonce}},{'type':'content_block_stop','index':1},{'type':'message_delta','delta':{'stop_reason':'end_turn','stop_sequence':None},'usage':{'output_tokens':3}},{'type':'message_stop'}],200
    return f.tool('read',{'path':'deep.txt'}),200
   return f.normal(body)
  f.reply=reply;r=f.run('实际首轮-'+nonce);f.stop()
  with sqlite3.connect(f.home/'lingxi-service/data/runs.db') as db:
   rows=db.execute("SELECT message_id,content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1' ORDER BY seq").fetchall()
   for mid,raw in rows:
    v=json.loads(raw);item=v['payload']['items'][0]
    change=False
    if mode=='tool_pair' and item['kind']=='tool':item['providerId']='forged-provider';v['payload']['projection']['providerCallId']='forged-provider';change=True
    if mode.startswith('opaque') and item['kind']=='assistant' and any(c['type']=='opaque' for c in item['content']):item['origin']=None if mode.endswith('missing') else {'provider':'forged-provider','model':'forged-model'};change=True
    if mode=='actor':v['actorKind']='web_session';v['actorSubject']='user_local';change=True
    if change:
     v.pop('digest');v['digest']=hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest();db.execute('UPDATE messages SET content_json=? WHERE message_id=?',(json.dumps(v,ensure_ascii=False),mid))
  f.start();before=len(f.wire);source_messages=f.sql("SELECT * FROM messages WHERE session_id='sess_local_alpha' ORDER BY seq");source_head=f.sql("SELECT * FROM session_branch_heads WHERE session_id='sess_local_alpha'");source_runs=f.sql("SELECT * FROM runs ORDER BY run_id")
  try:f.request('sessions/sess_local_alpha/fork',{'newSessionId':'bad_'+mode,'boundaryMessageId':r['runId']+'-final'},expected=500)
  except AssertionError:pass
  status=f.http[-1]['status'];created=bool(f.sql('SELECT session_id FROM sessions WHERE session_id=?',('bad_'+mode,)));assert f.sql("SELECT * FROM messages WHERE session_id='sess_local_alpha' ORDER BY seq")==source_messages;assert f.sql("SELECT * FROM session_branch_heads WHERE session_id='sess_local_alpha'")==source_head;assert f.sql("SELECT * FROM runs ORDER BY run_id")==source_runs;outcomes.append({'mode':mode,'status':status,'targetCreated':created,'wireBefore':before,'wireAfter':len(f.wire)});checks.append(outcomes[-1])
 g=make('deep-fork-suffix');first=g.run('PREFIX-'+nonce);second=g.run('BAD-SUFFIX-'+nonce);g.stop()
 with sqlite3.connect(g.home/'lingxi-service/data/runs.db') as db:
  mid,raw=db.execute("SELECT message_id,content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1' AND content_json LIKE ?",('%'+second['runId']+'%',)).fetchone();v=json.loads(raw);v['actorKind']='web_session';v['actorSubject']='user_local';v.pop('digest');v['digest']=hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest();db.execute('UPDATE messages SET content_json=? WHERE message_id=?',(json.dumps(v),mid))
 g.start();g.request('sessions/sess_local_alpha/fork',{'newSessionId':'good_prefix','boundaryMessageId':first['runId']+'-final'});g.run('合法保留前缀','good_prefix');assert has(g.wire[-1],'PREFIX-'+nonce) and not has(g.wire[-1],'BAD-SUFFIX-'+nonce);checks.append('bad suffix outside boundary does not reject valid prefix')
 g.request('sessions/archive',{'sessionId':'sess_local_alpha','childMode':'detach_children'});g.request('sessions/archived/delete',{'sessionId':'sess_local_alpha'});g.request('sessions/good_prefix/fork',{'newSessionId':'source_deleted_good','boundaryMessageId':first['runId']+'-final'});g.run('源删除后再次分叉','source_deleted_good');assert has(g.wire[-1],'PREFIX-'+nonce);checks.append('source-deleted self-contained archive remains valid on next authorized fork')
 assert all(v['status']==500 and not v['targetCreated'] and v['wireBefore']==v['wireAfter'] for v in outcomes),outcomes

def forged_argument_refusal_controls():
 # 用真正成功工具的权威journal做负控；伪造拒绝凭据不能将坏请求洗成合法历史。
 for mode in ['success','unknown','forged_refusal','missing_source']:
  f=make('forged-refusal-'+mode);(f.ws/'refusal.txt').write_text('REAL-SUCCESS-'+nonce);issued=[False]
  def reply(body):
   if not issued[0]:issued[0]=True;return f.tool('read',{'path':'refusal.txt'}),200
   return f.normal(body)
  f.reply=reply;r=f.run('真实成功后不得伪造拒绝');f.stop()
  with sqlite3.connect(f.home/'lingxi-service/data/runs.db') as db:
   rows=db.execute("SELECT message_id,content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1' ORDER BY seq").fetchall();bodies=[(mid,json.loads(raw)) for mid,raw in rows]
   assistant=next(v for mid,v in bodies if v['payload']['items'][0]['kind']=='assistant' and v['payload']['items'][0]['calls']);call=assistant['payload']['items'][0]['calls'][0];trusted=call['digest']['hex'];call['digest']['hex']='0'*64
   tool=next(v for mid,v in bodies if v['payload']['items'][0]['kind']=='tool');projection=tool['payload']['projection'];item=tool['payload']['items'][0]
   if mode in ['unknown','forged_refusal']:
    for key in ['rawContentDigest','projectionContentDigest','processStatus']:projection[key]=None
    if mode=='unknown':
     reason='伪造Unknown不能授权坏参数';projection['result']={'status':'unknown','error':{'code':'internal','message':'unknown tool outcome: '+reason,'retryable':False},'truncated':False};projection['renderedBytes']=len(('tool outcome could not be determined: '+reason).encode());item['outcome']={'status':'unknown','reason':reason}
    else:
     message=f"args_digest mismatch (declared {'0'*64}, computed {trusted}): forged or adulterated request; not dispatched";error={'code':'invalid_message','message':message,'retryable':False};projection['result']={'status':'failed','error':error,'truncated':False};projection['renderedBytes']=len(('tool error [invalid_message]: '+message).encode());item['outcome']={'status':'failed','error':error}
     tool['payload']['argumentRefusal']={'run':r['runId'],'session':'sess_local_alpha','owner':'user_local','ownerKind':'local_user','tool':call['id'],'target':call['target'],'argsDigest':trusted,'phase':'failed','outcome':'failed','dispatched':False,'detail':f"not dispatched: args_digest mismatch (declared {'0'*64}, computed {trusted})"}
   if mode=='missing_source':assistant['sourceRun']='unavailable-source'
   for mid,v in bodies:
    v.pop('digest');v['digest']=hashlib.sha256(json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest();db.execute('UPDATE messages SET content_json=? WHERE message_id=?',(json.dumps(v,ensure_ascii=False),mid))
  f.start();before=len(f.wire);refused=f.run('拒绝伪造凭据-'+mode,expected=500)
  if mode=='forged_refusal':assert 'argument refusal journal is not a never-dispatched failure' in json.dumps(refused),(mode,refused)
  assert len(f.wire)==before;assert len(f.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_started'"))==1;assert f.sql("SELECT phase FROM invocation_journal")==[('succeeded',)];assert any('0'*64 in raw for (raw,) in f.sql("SELECT content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1'"));checks.append('bad digest '+mode+' rejected before wire; genuine succeeded journal/original corrupt archive retained/no re-execution')
def cross_session_archive_control():
 f=make('cross-session-archive');f.run('ALPHA-PRIVATE-'+nonce);f.run('BETA-PUBLIC-'+nonce,'sess_local_beta');f.stop()
 with sqlite3.connect(f.home/'lingxi-service/data/runs.db') as db:
  content=db.execute("SELECT content_json FROM messages WHERE session_id='sess_local_alpha' AND entry_type='lingxi-model-exchange-v1' LIMIT 1").fetchone()[0];parent=db.execute("SELECT head_message_id FROM session_branch_heads WHERE session_id='sess_local_beta'").fetchone()[0];seq=db.execute("SELECT MAX(seq)+1 FROM messages WHERE session_id='sess_local_beta'").fetchone()[0]
  db.execute("INSERT INTO messages(message_id,session_id,role,content_json,committed_at_unix_ms,seq,parent_message_id,entry_type) VALUES('wrong-source-archive','sess_local_beta','model_history',?,1,?,?,'lingxi-model-exchange-v1')",(content,seq,parent));db.execute("UPDATE session_branch_heads SET head_message_id='wrong-source-archive',observed_tail_message_id='wrong-source-archive',revision=revision+1 WHERE session_id='sess_local_beta'")
 f.start();before=len(f.wire);f.run('同主体另一会话专档拒绝','sess_local_beta',expected=500);assert len(f.wire)==before;assert f.sql("SELECT content_json FROM messages WHERE session_id='sess_local_beta' AND message_id='wrong-source-archive'")[0][0]==content;checks.append('same-owner other-session checksum-valid archive rejected before wire/original retained')
def process_projection_windows():
 f=make('process-replay');issued=[False];text='PROCESS-'+nonce
 def reply(body):
  if not issued[0]:issued[0]=True;return f.tool('exec_command',{'argv':[sys.executable,'-c','print('+repr(text)+')']},'provider_process'),200
  return f.normal(body)
 f.reply=reply;f.run('真实本地进程结果');first=[m for m in msgs(f.wire[-1]) if m['role']=='tool'][0];assert text in first['content'] and '[process exited with code 0]' in first['content'];projection=json.loads(f.sql("SELECT payload_json FROM key_events WHERE event_type='tool_result_model_projection'")[0][0]);assert projection['processStatus']=={'kind':'exited','code':0};assert projection['rawContentDigest'] and projection['projectionContentDigest'];f.stop();f.start();f.run('进程结果恢复');assert [m for m in msgs(f.wire[-1]) if m['role']=='tool']==[first];assert len(f.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_started'"))==1;checks.append('actual process status/digests/provider identity retained across restart; no execution replay')
 # 真工具在合成DB插入异writer，完成原收据后专档追加必须失败，不能继续模型调用。
 g=make('projection-store-failure');dbfile=g.home/'lingxi-service/data/runs.db';script=g.ws/'foreign_writer.py';counter=g.ws/'counter.txt'
 script.write_text("import sqlite3,pathlib\ndbpath="+repr(str(dbfile))+"\nwith sqlite3.connect(dbpath) as db:\n sid='sess_local_alpha'\n parent=db.execute('SELECT head_message_id FROM session_branch_heads WHERE session_id=?',(sid,)).fetchone()[0]\n seq=db.execute('SELECT MAX(seq)+1 FROM messages WHERE session_id=?',(sid,)).fetchone()[0]\n db.execute(\"INSERT INTO messages(message_id,session_id,role,content_json,committed_at_unix_ms,seq,parent_message_id,entry_type) VALUES('foreign-writer',?,'user','{\\\"text\\\":\\\"FOREIGN-WRITER\\\"}',1,?,?,'message')\",(sid,seq,parent))\n db.execute(\"UPDATE session_branch_heads SET head_message_id='foreign-writer',observed_tail_message_id='foreign-writer',revision=revision+1 WHERE session_id=?\",(sid,))\npathlib.Path("+repr(str(counter))+").write_text('1')\n")
 def corrupt_branch(body):return g.tool('exec_command',{'argv':[sys.executable,str(script)]},'provider_store_failure'),200
 g.reply=corrupt_branch;g.run('投影存储失败必须停止',expected=500);assert len(g.wire)==1;assert counter.read_text()=='1';assert len(g.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_completed'"))==1;assert len(g.sql("SELECT event_id FROM key_events WHERE event_type='tool_result_model_projection'"))==1;assert not any('\"kind\":\"tool\"' in v for (v,) in g.sql("SELECT content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1'"));checks.append('real tool side effect once/raw completion+F08 projection retained; history append failure stops before next model wire/no fabricated completion archive')
def strip_archives(f,sid):
 f.stop();rows=f.sql('SELECT message_id,parent_message_id,entry_type FROM messages WHERE session_id=? ORDER BY seq',(sid,));parents={r[0]:r[1] for r in rows};archives={r[0] for r in rows if r[2]=='lingxi-model-exchange-v1'}
 with sqlite3.connect(f.home/'lingxi-service/data/runs.db') as db:
  for mid,parent,kind in rows:
   while parent in archives:parent=parents[parent]
   if mid not in archives:db.execute('UPDATE messages SET parent_message_id=? WHERE message_id=? AND session_id=?',(parent,mid,sid))
  db.execute("DELETE FROM messages WHERE entry_type='lingxi-model-exchange-v1' AND session_id=?",(sid,))
 f.start()
def legacy_boundaries():
 # 真旧raw-only工具会话经fork后Run外键为空，正文不能代替完整模型档案。
 f=make('legacy-tool');(f.ws/'old.txt').write_text('ORIGINAL-RAW-'+nonce);issued=[False]
 def reply(body):
  if not issued[0]:issued[0]=True;return f.tool('read',{'path':'old.txt'}),200
  return f.normal(body)
 f.reply=reply;r=f.run('旧工具合成会话');strip_archives(f,'sess_local_alpha');f.request('sessions/sess_local_alpha/fork',{'newSessionId':'old_tool_fork','boundaryMessageId':r['runId']+'-final'});before=len(f.wire);f.run('不能伪补工具历史','old_tool_fork',expected=500);assert len(f.wire)==before;assert len(f.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_started'"))==1;assert any('ORIGINAL-RAW-'+nonce in p for _,p in f.sql("SELECT event_id,payload_json FROM key_events WHERE event_type='tool_call_completed'"));checks.append('raw-only legacy fork rejects before wire/keeps original raw/no re-execution')
 # 旧普通fork能从准确usage恢复；缺失/跨主体来源仍拒绝而非后缀猜测。
 g=make('legacy-source');r=g.run('普通历史-'+nonce);strip_archives(g,'sess_local_alpha');g.request('sessions/sess_local_alpha/fork',{'newSessionId':'plain_good','boundaryMessageId':r['runId']+'-final'});g.run('普通fork正确来源','plain_good');assert has(g.wire[-1],'普通历史-'+nonce)
 for sid in ['no_source','cross_owner']:g.request('sessions/sess_local_alpha/fork',{'newSessionId':sid,'boundaryMessageId':r['runId']+'-final'})
 g.stop()
 with sqlite3.connect(g.home/'lingxi-service/data/runs.db') as db:
  mid,value=db.execute("SELECT message_id,content_json FROM messages WHERE session_id='no_source' AND role='assistant'").fetchone();v=json.loads(value);v['modelCallId']='unknown-source-call';db.execute("UPDATE messages SET model_call_id=?,content_json=? WHERE session_id='no_source' AND message_id=?",('unknown-source-call',json.dumps(v),mid))
  db.execute("UPDATE sessions SET owner_user_id='user_other' WHERE session_id='sess_local_alpha'")
 g.start();before=len(g.wire);g.run('无来源拒绝','no_source',expected=500);g.run('跨主体来源拒绝','cross_owner',expected=500);assert len(g.wire)==before;assert g.sql("SELECT owner_user_id FROM sessions WHERE session_id='sess_local_alpha'")[0][0]=='user_other';checks.append('plain legacy fork exact usage positive; unknown-source/cross-owner explicit rejection/no fallback/zero wire')
def families():
 for family in ['openai-completions','anthropic-messages','google-generative-ai','openai-responses','openai-codex-responses']:
  f=make('family-'+family,protocol=family);first='FIRST-'+nonce;current='CURRENT-'+nonce;f.run(first);f.run(current);w=f.wire[-1];assert has(w,first) and has(w,'ACK '+nonce) and has(w,current),(family,w)
  chain=w.get('messages',w.get('contents',w.get('input')));roles=[m['role'] for m in chain if m.get('role')!='system'];assert roles==['user','model' if family=='google-generative-ai' else 'assistant','user'],(family,roles);checks.append(family+' actual wire chronological roles')
def uncertain():
 f=make('uncertain');entered=threading.Event();errors=[];counter=f.ws/'counter.txt';pidfile=f.ws/'parked.pid';program=f.ws/'parked.py'
 program.write_text("import os,time,pathlib\np=pathlib.Path(__file__).parent\nparent=os.getppid()\n(p/'counter.txt').write_text('1')\n(p/'parked.pid').write_text(str(os.getpid()))\nwhile os.getppid()==parent:time.sleep(.01)\n")
 def parked(body):
  entered.set();return f.tool('exec_command',{'argv':[sys.executable,str(program)]},'unknown_provider_call'),200
 f.reply=parked
 def ask():
  try:f.request('sessions/sess_local_alpha/execute',{'input':'待确认旧请求-'+nonce,'requestId':'uncertain-old'})
  except Exception as e:errors.append(repr(e))
 task=threading.Thread(target=ask,daemon=True);task.start();assert entered.wait(5)
 for _ in range(500):
  if counter.exists() and pidfile.exists():break
  time.sleep(.01)
 assert counter.exists() and pidfile.exists(),('real side effect not reached',f.logs)
 oldrun=f.sql('SELECT run_id FROM runs')[0][0];toolpid=int(pidfile.read_text());assert counter.read_text()=='1';assert not f.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_completed'");assert len(f.wire)==1
 f.p.kill();code=f.p.wait(timeout=10);f.err.join(2);f.closed.append({'exit':code,'reaped':True,'intentionalCrash':True});f.p=None;task.join(10);assert not task.is_alive()
 for _ in range(500):
  try:os.kill(toolpid,0)
  except ProcessLookupError:break
  time.sleep(.01)
 else:os.kill(toolpid,signal.SIGKILL);raise AssertionError('parked local process did not exit after parent death')
 f.reply=f.normal;f.start();assert f.sql('SELECT status FROM runs WHERE run_id=?',(oldrun,))[0][0]=='interrupted_needs_attention'
 f.request('sessions/sess_local_alpha/execute',{'input':'待确认旧请求-'+nonce,'requestId':'uncertain-old'},expected=409)
 f.request('sessions/sess_local_alpha/execute',{'input':'全新请求-'+nonce,'requestId':'uncertain-new'});w=f.wire[-1];result=[m for m in msgs(w) if m['role']=='tool'];assert len(result)==1 and result[0]['tool_call_id']=='unknown_provider_call';assert 'tool outcome could not be determined' in result[0]['content'] and 'Unknown诊断' in result[0]['content'];assert '不视为成功' in result[0]['content'];assert not f.sql("SELECT event_id FROM key_events WHERE event_type='tool_call_completed'");assert counter.read_text()=='1';checks.append('actual tool side effect +1/crash/recovery: old requestId refused; fresh accepted; explicit Unknown model diagnostic/no completion receipt/no old tool execution; parked process exited')
def c04():
 f=make('budget',window=100000);summary='\n'.join(['## Goal','记住摘要-'+nonce,'## Constraints & Preferences','无','## Progress','### Done','已完成','### In Progress','无','### Blocked','无','## Key Decisions','无','## Next Steps','继续','## Critical Context',nonce]);summary_calls=[]
 def response(body):
  if has(body,'## Critical Context') or has(body,'Constraints & Preferences'):
   summary_calls.append(len(f.wire));return f.text(summary),200
  return f.text('short answer'),200
 f.reply=response
 for i in range(10):f.run(f'用户轮次{i}-'+('a'*40000))
 assert summary_calls,'跨Run预算未触发真实摘要请求';arch=f.sql("SELECT content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1'");versions=[json.loads(r[0]) for r in arch if json.loads(r[0])['payload']['mode']=='replace'];assert versions and any(nonce in json.dumps(v) for v in versions);before=f.sql("SELECT message_id,content_json FROM messages");f.stop();f.start();f.run('摘要重启后');assert has(f.wire[-1],nonce) and count(f.wire[-1],'摘要重启后','user')==1;after=dict(f.sql('SELECT message_id,content_json FROM messages'));assert all(after[mid]==content for mid,content in before);checks.append('C04 multiple actual Runs trigger summary; version+retained suffix survive restart; all authoritative originals immutable')
 # 摘要服务失败时不写成功替换，原记录继续保留。
 h=make('summary-failure',window=100000);calls=[]
 def fail(body):
  if has(body,'Constraints & Preferences'):calls.append(1);return [{'error':{'message':'synthetic summary failure'}}],400
  return h.text('short answer'),200
 h.reply=fail
 for i in range(9):h.run(f'失败预算{i}-'+('b'*40000))
 assert calls;replacements=h.sql("SELECT content_json FROM messages WHERE entry_type='lingxi-model-exchange-v1'");assert not any(json.loads(v)['payload']['mode']=='replace' for (v,) in replacements);old=h.sql('SELECT message_id,content_json FROM messages');h.stop();h.start();h.reply=h.normal;h.run('失败后恢复');after=dict(h.sql('SELECT message_id,content_json FROM messages'));assert all(after[mid]==v for mid,v in old);checks.append('summary failure no fake successful version/all prior originals survive restart')
 # 真进程退出会触发生产取消，迟到的模型回答不得伪写成已完成历史。
 k=make('cancel');k.stop();k.shutdown_ms=100;k.start();k.run('取消前-'+nonce);entered=threading.Event();release=threading.Event();errors=[]
 def blocked(body):entered.set();release.wait(30);return k.text('LATE-UNCONFIRMED-'+nonce),200
 k.reply=blocked
 def inflight():
  try:k.run('取消中的用户内容')
  except Exception as e:errors.append(repr(e))
 task=threading.Thread(target=inflight,daemon=True);task.start();assert entered.wait(5);k.stop();release.set();task.join(10);assert not task.is_alive();assert not any('LATE-UNCONFIRMED' in v for _,v in k.sql('SELECT message_id,content_json FROM messages'));k.reply=k.normal;k.start();k.run('取消后恢复');assert has(k.wire[-1],'取消前-'+nonce);assert not has(k.wire[-1],'LATE-UNCONFIRMED');checks.append('real binary shutdown cancellation/no late fabricated model success/history preserved on restart')
try:
 globals()[CASE]();status='PASS'
except Exception as e:status='FAIL';error=repr(e);raise
finally:
 for f in fixtures:f.close()
 report={'case':CASE,'status':status,'nonce':nonce,'binarySHA':hashlib.sha256(BIN.read_bytes()).hexdigest(),'checks':checks,'fixtures':[f.snapshot() for f in fixtures]}
 if status=='FAIL':report['error']=error
 out=Path(os.environ.get('W01_EVIDENCE',tempfile.gettempdir()));out.mkdir(parents=True,exist_ok=True);path=out/(CASE+'-'+nonce+'-evidence.json');
 for index,f in enumerate(fixtures):
  backup=out/(CASE+'-'+nonce+'-'+str(index)+'.db')
  with sqlite3.connect('file:'+str(f.home/'lingxi-service/data/runs.db')+'?mode=ro',uri=True) as source,sqlite3.connect(backup) as destination:source.backup(destination)
  report['fixtures'][index]['databaseBackup']=str(backup);report['fixtures'][index]['backupSHA']=hashlib.sha256(backup.read_bytes()).hexdigest();shutil.rmtree(f.base);report['fixtures'][index]['syntheticHomeCleaned']=not f.base.exists()
 path.write_text(json.dumps(report,ensure_ascii=False,indent=2));print(json.dumps({'case':CASE,'status':status,'checks':checks,'evidence':str(path)},ensure_ascii=False))
