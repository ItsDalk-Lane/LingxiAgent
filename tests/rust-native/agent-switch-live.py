# 真服务、真实HTTP模型替身、数据库与磁盘联合检查；不替换会话创建或工具内核。
import concurrent.futures,hashlib,http.server,json,os,pathlib,re,sqlite3,subprocess,threading,urllib.request,urllib.error
root=pathlib.Path(os.environ['EX12_EVIDENCE']).resolve();root.mkdir(parents=True)
home=root/'home';workspace=root/'workspace';selected=root/'selected';explicit=root/'explicit';extra=root/'extra'
for p in [home,workspace,selected,explicit,extra]:p.mkdir()
for p,name in [(workspace,'WRONG-GLOBAL'),(selected,'CURRENT-SELECTED'),(explicit,'TARGET-EXPLICIT')]:(p/'owned.txt').write_text(name)
requests=[]
class Provider(http.server.BaseHTTPRequestHandler):
 def log_message(self,*_):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body);step=(len(requests)-1)%3
  if step<2:
   name,args=('mcp_describe_tool',{'name':'read'}) if step==0 else ('mcp_call',{'tool':'read','arguments':{'path':'owned.txt'}})
   delta={'role':'assistant','tool_calls':[{'index':0,'id':f'ex12-{len(requests)}','type':'function','function':{'name':name,'arguments':json.dumps(args)}}]};finish='tool_calls'
  else:delta={'role':'assistant','content':'切换后的真实会话完成'};finish='stop'
  frames=[{'choices':[{'index':0,'delta':delta,'finish_reason':None}]},{'choices':[{'index':0,'delta':{},'finish_reason':finish}]},{'choices':[],'usage':{'prompt_tokens':10,'completion_tokens':3}}]
  data=(''.join('data: '+json.dumps(f)+'\n\n' for f in frames)+'data: [DONE]\n\n').encode();self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
provider=http.server.ThreadingHTTPServer(('127.0.0.1',0),Provider);threading.Thread(target=provider.serve_forever,daemon=True).start()
product=root/'product/yuan';product.mkdir(parents=True);(product/'lingxi.md').write_text('你是{{agentName}}。切换归属验证。')
config={'home':str(home),'workspace':str(workspace),'providers':{name:{'protocol':'openai-completions','endpoint':f'http://127.0.0.1:{provider.server_port}/v1','auth':{'kind':'apiKey','apiKey':'synthetic-ex12-'+name}} for name in ['global-provider','target-provider']},'models':{'chat':{'provider':'global-provider','model':'global-model','capabilities':{'tools':True}},'title':{'provider':'target-provider','model':'target-model','capabilities':{'tools':True}}}}

if os.environ.get('EX12_TARGET_ONLY')=='1':config['models'].pop('chat')
if os.environ.get('EX12_EXPECT_NO_MODEL')=='1':config={'home':str(home),'workspace':str(workspace)}
(root/'config.json').write_text(json.dumps(config));log=(root/'service.log').open('w');trace=[]
proc=subprocess.Popen([os.environ['EX12_SERVICE_BIN'],'--config',str(root/'config.json'),'--bind','127.0.0.1:0'],env={**os.environ,'LINGXI_HOME':str(home),'TMPDIR':str(root),'LINGXI_PRODUCT_DIR':str(product.parent)},stdout=subprocess.PIPE,stderr=log,text=True)
try:
 for line in proc.stdout:
  log.write(line);log.flush();m=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
  if m:url='http://'+m[1];break
 token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
 def api(method,path,body=None,status=200,credential=None):
  req=urllib.request.Request(url+path,data=None if body is None else json.dumps(body).encode(),method=method,headers={'Authorization':'Bearer '+(credential or token),'Content-Type':'application/json'})
  try:res=urllib.request.urlopen(req,timeout=30)
  except urllib.error.HTTPError as e:res=e
  data=json.loads(res.read());trace.append({'method':method,'path':path,'body':body,'status':res.status,'data':({**data,'secret':'[omitted]'} if isinstance(data,dict) and 'secret' in data else data)});assert res.status==status,(res.status,data);return data
 for a in ['switch-a','switch-b']:
  api('POST','/lingxi/v1/agents',{'id':a,'name':a,'yuan':'lingxi'});api('PUT',f'/lingxi/v1/agents/{a}/config',{'memory':{'enabled':False}})
  if os.environ.get('EX12_EXPECT_NO_MODEL')!='1' and not (os.environ.get('EX12_TARGET_ONLY')=='1' and a=='switch-a'):api('PUT',f'/lingxi/v1/agents/{a}/config',{'models':{'chat':{'provider':'target-provider' if a=='switch-b' else 'global-provider','id':'target-model' if a=='switch-b' else 'global-model'}}})
 if os.environ.get('EX12_TARGET_ONLY')=='1':
  assert 'chat' not in config['models'] and config['models']['title']['provider']=='target-provider'
  switched=api('POST','/lingxi/v1/agents/switch',{'id':'switch-b','cwd':str(selected)})
  sid=switched['sessionId'];assert switched['currentModelProvider']=='target-provider' and switched['currentModelId']=='target-model'
  run=api('POST',f'/lingxi/v1/sessions/{sid}/execute',{'input':'全局缺省模型为空仍使用目标助手模型'})
  assert len(requests)==3 and all(r['model']=='target-model' for r in requests),requests
  db=sqlite3.connect(f'file:{home}/lingxi-service/data/runs.db?mode=ro',uri=True)
  assert db.execute('SELECT session_id,status FROM runs WHERE run_id=?',(run['runId'],)).fetchone()==(sid,'completed')
  assert db.execute('SELECT DISTINCT provider,model FROM model_call_usage WHERE run_id=?',(run['runId'],)).fetchall()==[('target-provider','target-model')]
  result={'status':'PASS','checks':['explicit_target_model_without_global_chat_route','actual_target_provider_http_and_usage'],'sessionId':sid,'runId':run['runId'],'modelRequests':3};(root/'result.json').write_text(json.dumps(result));print(json.dumps(result));raise SystemExit(0)
 if os.environ.get('EX12_EXPECT_NO_MODEL')=='1':
  prefs=home/'user/preferences.json';before=prefs.read_bytes() if prefs.exists() else None
  db=sqlite3.connect(f'file:{home}/lingxi-service/data/runs.db?mode=ro',uri=True);before_sessions=db.execute('SELECT * FROM sessions ORDER BY session_id').fetchall()
  configs=[(home/f'agents/{a}/config.yaml').read_bytes() for a in ['switch-a','switch-b']]
  answer=api('POST','/lingxi/v1/agents/switch',{'id':'switch-b','cwd':str(selected)},500)
  assert 'model plane unavailable' in json.dumps(answer),answer
  assert (prefs.read_bytes() if prefs.exists() else None)==before
  assert configs==[(home/f'agents/{a}/config.yaml').read_bytes() for a in ['switch-a','switch-b']]
  assert db.execute('SELECT * FROM sessions ORDER BY session_id').fetchall()==before_sessions
  assert not requests
  result={'status':'PASS','checks':['no_model_explicit_failure_zero_session_focus_config_mutation'],'modelRequests':0};(root/'result.json').write_text(json.dumps(result));print(json.dumps(result));raise SystemExit(0)
 existing_a=api('POST','/lingxi/v1/sessions/new',{'agentId':'switch-a','cwd':str(selected),'workspaceFolders':[str(extra)],'permissionMode':'read_only','memoryEnabled':False})['sessionId']
 existing_b=api('POST','/lingxi/v1/sessions/new',{'agentId':'switch-b','cwd':str(workspace),'permissionMode':'operate','memoryEnabled':False})['sessionId']
 api('POST','/lingxi/v1/sessions/switch',{'sessionId':existing_a})
 prefs=home/'user/preferences.json';db=sqlite3.connect(f'file:{home}/lingxi-service/data/runs.db?mode=ro',uri=True)
 if os.environ.get('EX12_EXPECT_DELETE_UNFIXED')=='1':
  deleted=api('DELETE','/lingxi/v1/agents/switch-a',{})
  current=json.loads(prefs.read_text());row=db.execute('SELECT agent_id FROM sessions WHERE session_id=?',(current['current_session_id'],)).fetchone()
  assert current['current_agent_id']=='switch-b' and current['current_session_id']==existing_a and row==('switch-a',)
  result={'status':'EXPECTED_DELETE_GAP_REPRODUCED','deleted':deleted,'focusAfter':current,'oldSessionAgent':row[0],'oldSession':existing_a,'existingReplacementSession':existing_b};(root/'result.json').write_text(json.dumps(result,indent=2));print(json.dumps(result));raise SystemExit(0)
 if os.environ.get('EX12_DELETE_CURRENT')=='1':
  # 替代目录失败必须在逻辑删除和技能清理前终止。
  protected_skill=home/'skills/delete-guard/SKILL.md';protected_skill.parent.mkdir(parents=True);protected_skill.write_text('合成技能，不得误删')
  if os.sys.platform=='darwin':
   protected_before=(prefs.read_bytes(),db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0],(home/'agents/switch-a/config.yaml').read_bytes(),(home/'agents/switch-b/config.yaml').read_bytes(),api('GET','/lingxi/v1/app-events?since=0')['events'])
   subprocess.run(['/usr/bin/chflags','uchg',str(prefs)],check=True)
   try:api('DELETE','/lingxi/v1/agents/switch-a',{'deleteSkills':['delete-guard']},500)
   finally:subprocess.run(['/usr/bin/chflags','nouchg',str(prefs)],check=True)
   assert protected_before==(prefs.read_bytes(),db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0],(home/'agents/switch-a/config.yaml').read_bytes(),(home/'agents/switch-b/config.yaml').read_bytes(),api('GET','/lingxi/v1/app-events?since=0')['events'])
   assert protected_skill.read_text()=='合成技能，不得误删'
  api('PUT','/lingxi/v1/agents/switch-b/config',{'desk':{'home_folder':str(root/'missing-target-home')}})
  before_prefs=prefs.read_bytes();before_count=db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0];source_a=(home/'agents/switch-a/config.yaml').read_bytes();before_events=api('GET','/lingxi/v1/app-events?since=0')['events']
  api('DELETE','/lingxi/v1/agents/switch-a',{'deleteSkills':['delete-guard']},400)
  assert prefs.read_bytes()==before_prefs and db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0]==before_count
  assert (home/'agents/switch-a/config.yaml').read_bytes()==source_a and protected_skill.read_text()=='合成技能，不得误删'
  assert api('GET','/lingxi/v1/app-events?since=0')['events']==before_events
  api('PUT','/lingxi/v1/agents/switch-b/config',{'desk':{'home_folder':str(explicit)}})
  # 真实rename失败允许B已切换、A仍完好；响应必须给出已提交的B上下文。
  if os.sys.platform=='darwin':
   subprocess.run(['/usr/bin/chflags','uchg',str(home/'agents/switch-a')],check=True)
   try:failed=api('DELETE','/lingxi/v1/agents/switch-a',{},500)
   finally:subprocess.run(['/usr/bin/chflags','nouchg',str(home/'agents/switch-a')],check=True)
   partial=failed['replacementSwitchResult'];partial_id=partial['sessionId'];assert partial['agent']['id']=='switch-b' and partial['sessionPath']=='native:'+partial_id
   partial_focus=json.loads(prefs.read_text());assert partial_focus['current_agent_id']=='switch-b' and partial_focus['current_session_id']==partial_id
   assert (home/'agents/switch-a/config.yaml').read_bytes()==source_a
   assert not any(e['type']=='agent-deleted' for e in api('GET','/lingxi/v1/app-events?since=0')['events'])
   api('POST','/lingxi/v1/sessions/switch',{'sessionId':existing_a})
  # 真实只读标记阻止物理清理；逻辑删除后沿原持久收据恢复。
  protected=home/'agents/switch-a/memory/retained.txt';protected.parent.mkdir(exist_ok=True);protected.write_text('原助手待清理合成资料')
  if os.sys.platform=='darwin':subprocess.run(['/usr/bin/chflags','uchg',str(protected)],check=True)
  deleted=api('DELETE','/lingxi/v1/agents/switch-a',{})
  context=deleted['replacementSwitchResult'];sid=context['sessionId'];assert context['agent']['id']=='switch-b' and context['cwd']==str(explicit)
  assert sid not in [existing_a,existing_b] and context['sessionPath']=='native:'+sid and context['permissionMode']=='read_only'
  assert deleted['logicalDeleted'] is True and deleted['replacementAgentId']=='switch-b' and not (home/'agents/switch-a').exists()
  assert db.execute('SELECT agent_id FROM sessions WHERE session_id=?',(existing_a,)).fetchone()==('switch-a',)
  assert db.execute('SELECT agent_id FROM sessions WHERE session_id=?',(existing_b,)).fetchone()==('switch-b',)
  focus=json.loads(prefs.read_text());assert focus['current_agent_id']=='switch-b' and focus['current_session_id']==sid
  events=api('GET','/lingxi/v1/app-events?since=0')['events'];sw=next(e for e in reversed(events) if e['type']=='agent-switched');de=next(e for e in reversed(events) if e['type']=='agent-deleted')
  assert sw['seq']<de['seq'] and sw['payload']['sessionPath']==context['sessionPath'] and sw['payload']['authorizedFolders']==[str(explicit)]
  pending=deleted['cleanupPending']
  if os.sys.platform=='darwin':
   assert pending is True and deleted['referencesPending'] is False
   retry=api('POST','/lingxi/v1/agent-deletions/'+deleted['deletionId']+'/cleanup',{})
   assert retry['cleanupPending'] is True and retry['logicalDeleted'] is True
   staged=home/'lingxi-service/agent-deletions'/deleted['deletionId']/'data/memory/retained.txt'
   subprocess.run(['/usr/bin/chflags','nouchg',str(staged)],check=True)
  proc.terminate();proc.wait(timeout=15)
  proc=subprocess.Popen([os.environ['EX12_SERVICE_BIN'],'--config',str(root/'config.json'),'--bind','127.0.0.1:0'],env={**os.environ,'LINGXI_HOME':str(home),'TMPDIR':str(root),'LINGXI_PRODUCT_DIR':str(product.parent)},stdout=subprocess.PIPE,stderr=log,text=True)
  for line in proc.stdout:
   log.write(line);log.flush();m=re.search(r'LINGXI_SERVICE_READY addr=(\S+)',line)
   if m:url='http://'+m[1];break
  token=json.loads((home/'lingxi-service/local-token.json').read_text())['token']
  receipts=api('GET','/lingxi/v1/agent-deletions')['deletions'];settled=next(r for r in receipts if r['deletionId']==deleted['deletionId']);assert settled['status']=='completed' and settled['cleanupPending'] is False
  focus=json.loads(prefs.read_text());assert focus['current_agent_id']=='switch-b' and focus['current_session_id']==sid
  before_last=(prefs.read_bytes(),db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0],(home/'agents/switch-b/config.yaml').read_bytes())
  api('DELETE','/lingxi/v1/agents/switch-b',{'deleteSkills':['delete-guard']},400)
  assert before_last==(prefs.read_bytes(),db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0],(home/'agents/switch-b/config.yaml').read_bytes())
  assert protected_skill.read_text()=='合成技能，不得误删'
  run=api('POST',f'/lingxi/v1/sessions/{sid}/execute',{'input':'读取替代助手真实工作区'})
  assert db.execute('SELECT session_id,status FROM runs WHERE run_id=?',(run['runId'],)).fetchone()==(sid,'completed')
  assert len(requests)==3 and 'switch-b' in json.dumps(requests[0])
  assert all(r['model']=='target-model' for r in requests),requests
  assert db.execute('SELECT DISTINCT provider,model FROM model_call_usage WHERE run_id=?',(run['runId'],)).fetchall()==[('target-provider','target-model')]
  tools=json.dumps([m for m in requests[-1]['messages'] if m['role']=='tool']);assert 'TARGET-EXPLICIT' in tools and 'CURRENT-SELECTED' not in tools,tools
  result={'status':'PASS','deletedSessionAgentUnchanged':existing_a,'existingReplacementSessionPreserved':existing_b,'replacementSession':sid,'runId':run['runId'],'modelRequests':len(requests),'cleanupWasPending':pending,'checks':['pre_switch_focus_failure_preserves_a_skills_and_old_focus','replacement_validation_before_delete_or_skill_cleanup','failed_rename_preserves_a_after_real_b_switch','current_delete_creates_fresh_b_context','old_a_b_session_ownership_preserved','full_context_event_precedes_delete','real_cleanup_failure_and_receipt_retry','restart_cleanup_preserves_b_focus','last_agent_refusal_zero_mutation','next_run_uses_replacement_agent_session_workspace','replacement_target_provider_model_http_and_ledger']};(root/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2));print(json.dumps(result,ensure_ascii=False));raise SystemExit(0)
 first=api('POST','/lingxi/v1/agents/switch',{'id':'switch-b'})
 if os.environ.get('EX12_EXPECT_UNFIXED')=='1':
  current=json.loads(prefs.read_text());assert first['sessionPath'] is None and current['current_session_id']==existing_a and current['current_agent_id']=='switch-b'
  result={'status':'EXPECTED_GAP_REPRODUCED','response':first,'focusAfter':current,'oldFocus':existing_a,'existingOtherAgentSession':existing_b};(root/'result.json').write_text(json.dumps(result,indent=2));print(json.dumps(result));raise SystemExit(0)
 def check_context(value,agent,cwd,mode):
  sid=value['sessionId'];assert value['sessionPath']=='native:'+sid and value['agent']['id']==agent and value['cwd']==str(cwd),value
  row=db.execute('SELECT agent_id,permission_mode,authorized_folders_json,memory_enabled FROM sessions WHERE session_id=?',(sid,)).fetchone();assert row==(agent,mode,json.dumps([str(cwd)],separators=(',',':')),0),row
  assert value['authorizedFolders']==[str(cwd)] and value['workspaceFolders']==[] and value['memoryMasterEnabled'] is False,value
  return sid
 assert json.loads((home/'chat-input-settings.json').read_text())['sessions'][existing_b]['model']==['global-provider','global-model']
 assert first['currentModelProvider']=='target-provider' and first['currentModelId']=='target-model'
 first_sid=check_context(first,'switch-b',selected,'read_only');assert first_sid not in [existing_a,existing_b]
 # 切换后首次真实Run必须使用新助手/新会话/新工作目录，不能落到已有B会话或全局目录。
 run=api('POST',f'/lingxi/v1/sessions/{first_sid}/execute',{'input':'读取当前工作区文件'})
 assert db.execute('SELECT session_id,status FROM runs WHERE run_id=?',(run['runId'],)).fetchone()==(first_sid,'completed')
 assert len(requests)==3 and all(r['model']=='target-model' for r in requests),requests
 assert db.execute('SELECT DISTINCT provider,model FROM model_call_usage WHERE run_id=?',(run['runId'],)).fetchall()==[('target-provider','target-model')]
 tools=json.dumps([m for m in requests[-1]['messages'] if m['role']=='tool']);assert 'CURRENT-SELECTED' in tools and 'WRONG-GLOBAL' not in tools,tools
 assert 'switch-b' in json.dumps(requests[0]),requests[0]
 api('PUT','/lingxi/v1/agents/switch-a/config',{'desk':{'home_folder':str(explicit)}})
 second=api('POST','/lingxi/v1/agents/switch',{'id':'switch-a'});second_sid=check_context(second,'switch-a',explicit,'read_only');assert second_sid!=existing_a
 third=api('POST','/lingxi/v1/agents/switch',{'id':'switch-a','cwd':str(selected)});check_context(third,'switch-a',selected,'read_only')
 def snapshot():
  return {'count':db.execute('SELECT COUNT(*) FROM sessions').fetchone()[0],'prefs':prefs.read_bytes(),'configs':[(home/f'agents/{a}/config.yaml').read_bytes() for a in ['switch-a','switch-b']],'events':api('GET','/lingxi/v1/app-events?since=0')['events']}
 restricted=api('POST','/lingxi/v1/devices/credentials',{'userId':'restricted','scopes':['settings.read']},201)['secret']
 before=snapshot()
 api('POST','/lingxi/v1/agents/switch',{'id':'switch-b'},403,credential=restricted)
 assert snapshot()==before
 for body,status in [({'id':'missing'},404),({'id':'switch-b','cwd':'relative'},400),({'id':'switch-b','cwd':str(root/'absent')},400)]:
  api('POST','/lingxi/v1/agents/switch',body,status)
  assert snapshot()==before
 # 坏配置在创建前拒绝，焦点、会话、其它配置及广播均保持原样。
 config_b=home/'agents/switch-b/config.yaml';saved=config_b.read_bytes();config_b.write_text('desk: [broken')
 bad_before=snapshot();api('POST','/lingxi/v1/agents/switch',{'id':'switch-b'},400);assert snapshot()==bad_before
 config_b.write_bytes(saved)
 before=snapshot()
 saved_model=config_b.read_bytes();text=saved_model.decode();assert 'target-model' in text
 config_b.write_text(text.replace('target-model','unavailable-model'))
 unknown_before=snapshot();api('POST','/lingxi/v1/agents/switch',{'id':'switch-b'},409);assert snapshot()==unknown_before
 config_b.write_bytes(saved_model)
 before=snapshot()
 # 真实偏好写失败不能遗留新会话或改变焦点。
 if os.sys.platform=='darwin':
  subprocess.run(['/usr/bin/chflags','uchg',str(prefs)],check=True)
  try:api('POST','/lingxi/v1/agents/switch',{'id':'switch-b'},500)
  finally:subprocess.run(['/usr/bin/chflags','nouchg',str(prefs)],check=True)
  after=snapshot();assert after['count']==before['count'] and after['prefs']==before['prefs'] and after['events']==before['events'] and after['configs']==before['configs']
 # 并发两目标有相同显式目录，避免把合法调度顺序误判为目录错误。
 api('PUT','/lingxi/v1/agents/switch-b/config',{'desk':{'home_folder':str(explicit)}})
 # 并发切换必须各自创建真实会话；最终焦点与最后成功广播一致，已有A/B会话完整保留。
 with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
  results=list(pool.map(lambda a:api('POST','/lingxi/v1/agents/switch',{'id':a}),['switch-a','switch-b']))
 ids=[]
 for val in results:ids.append(check_context(val,val['agent']['id'],explicit,'read_only'))
 assert len(set(ids))==2
 events=api('GET','/lingxi/v1/app-events?since=0')['events'];switched=[x['payload'] for x in events if x['type']=='agent-switched'];last=switched[-1];focus=json.loads(prefs.read_text())
 assert focus['current_agent_id']==last['agentId'] and focus['current_session_id']==last['sessionId']
 for val in [first,second,third]+results:
  event=next(e for e in switched if e['sessionId']==val['sessionId']);assert all(event[k]==val[k] for k in ['sessionPath','cwd','workspaceFolders','authorizedFolders','cwdHistory','memoryMasterEnabled'])
 assert db.execute('SELECT agent_id FROM sessions WHERE session_id=?',(existing_a,)).fetchone()==('switch-a',)
 assert db.execute('SELECT agent_id FROM sessions WHERE session_id=?',(existing_b,)).fetchone()==('switch-b',)
 result={'status':'PASS','existingSessions':[existing_a,existing_b],'createdSessions':[first_sid,second_sid,third['sessionId']]+ids,'runId':run['runId'],'modelRequests':len(requests),'checks':['fresh_session_and_current_cwd','next_run_agent_session_cwd','target_provider_model_http_and_ledger','target_home_priority','explicit_cwd','read_only_permission_preserved','target_memory_config','restricted_identity_zero_mutation','corrupt_config_zero_mutation','unknown_target_model_zero_mutation','rejections_zero_mutation','preference_failure_no_ghost_session','concurrent_switch_focus_event_consistency','existing_a_b_sessions_preserved']};(root/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2));print(json.dumps(result,ensure_ascii=False))
finally:
 (root/'trace.json').write_text(json.dumps(trace,ensure_ascii=False,indent=2));(root/'model-requests.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2));proc.terminate();proc.wait(timeout=15);provider.shutdown();log.close()
