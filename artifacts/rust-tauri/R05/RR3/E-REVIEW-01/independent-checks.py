import json,sys,re,hashlib,difflib,copy,urllib.parse,os
from pathlib import Path
ROOT=Path.cwd(); OUT=Path(__file__).resolve().parent; E=ROOT/'artifacts/rust-tauri/R05/RR3/E-01'; D=ROOT/'docs/rust-tauri/R05'
results=[]
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def check(name,ok,detail=None):results.append({'name':name,'pass':bool(ok),'detail':detail})
def load(p):return json.loads(Path(p).read_text())
def finish(mode):
 data={'mode':mode,'checks':results,'passed':sum(x['pass'] for x in results),'failed':sum(not x['pass'] for x in results)}
 (OUT/(mode+'-results.json')).write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
 print(json.dumps(data,ensure_ascii=False,indent=2));sys.exit(bool(data['failed']))
owned=list(load(E/'before.json')['owned']); hand=load(D/'R05_HANDOFF.json'); mode=sys.argv[1]
if mode=='json':
 def strict(s):
  def pairs(ps):
   d={}
   for k,v in ps:
    if k in d:raise ValueError('duplicate:'+k)
    d[k]=v
   return d
  return json.loads(s,object_pairs_hook=pairs)
 for s in ['{"status":"PASS","status":"FAIL"}','{"x":{"a":1,"a":2}}']:
  try:strict(s);check('重复键反控',False,s)
  except ValueError as e:check('重复键反控',str(e).startswith('duplicate:'),str(e))
 check('JSON正常对照',strict('{"x":{"a":1},"y":{"a":2}}')=={'x':{'a':1},'y':{'a':2}})
 fs=[Path(x) for x in owned if x.endswith('.json')]+list(E.glob('*.json'))+[D/'repair-current/RR3_ISSUE_MATRIX.json']
 for p in fs:
  try:strict(p.read_text());check('严格JSON '+str(p.resolve().relative_to(ROOT)),True)
  except Exception as e:check('严格JSON '+str(p.resolve().relative_to(ROOT)),False,str(e))
elif mode=='links':
 def resolve(p,target):
  target=urllib.parse.unquote(target.split()[0].strip('<>'))
  path,_,anchor=target.partition('#')
  if re.match(r'^[a-zA-Z][a-zA-Z0-9+.-]*:',path):return None
  q=(p.parent/path).resolve() if path else p.resolve()
  if not q.exists():return False
  if anchor and q.suffix=='.md':
   t=q.read_text();anchors=re.findall(r'(?:id|name)=["\x27]([^"\x27]+)',t)
   for h in re.findall(r'^#+\s+(.+)$',t,re.M):anchors.append(re.sub(r'[^\w\- ]','',h.lower()).replace(' ','-'))
   if anchor not in anchors:return False
  return True
 for path in owned:
  p=Path(path)
  if p.suffix!='.md':continue
  old=(E/'before'/p).read_text();new=p.read_text()
  added='\n'.join(x[2:] for x in difflib.ndiff(old.splitlines(),new.splitlines()) if x.startswith('+ '))
  for target in re.findall(r'\[[^\]]*\]\(([^)]+)\)',added):
   ok=resolve(p,target)
   if ok is not None:check('新增链接 '+path+' -> '+target,ok)
 def walk(v,ptr):
  if isinstance(v,dict):
   for k,x in v.items():walk(x,ptr+'/'+k)
  elif isinstance(v,list):
   for i,x in enumerate(v):walk(x,ptr+'/'+str(i))
  elif isinstance(v,str) and re.match(r'^(?:docs|rust|scripts|shared|artifacts)/',v) and '\n' not in v:
   path=re.split(r'(?:::|[:#（]|\s[+]|\s的|：)',v)[0]
   if Path(path).exists():check('JSON引用 '+ptr,True,path)
   elif not any(x in path for x in ['*','{','..']):check('JSON引用 '+ptr,False,path)
 for key in ['rr3_current','consumer_contract','working_tree_digest_scope']:walk(hand[key],key)
 check('缺文件反控',resolve(D/'R05_REPORT.md','no-such-E-review-target.md') is False)
 check('错锚点反控',resolve(D/'R05_REPORT.md','R05_REPORT.md#no-such-E-review-anchor') is False)
elif mode=='history':
 base=load(E/'before.json')
 for p,h in base['owned'].items():check('before原文hash '+p,sha(E/'before'/p)==h)
 for p,h in base['authority'].items():
  if any(x in p for x in ['pins','required_cids','leaf_case_map','SCOPE_MATRIX','stage_cids']):check('权威未改 '+p,sha(p)==h)
 def diff(a,b,ptr=''):
  if isinstance(a,dict) and isinstance(b,dict):
   out=[]
   for k in a:
    if k not in b:out.append([ptr+'/'+k,'DELETED'])
    else:out+=diff(a[k],b[k],ptr+'/'+k)
   return out
  if a==b:return []
  return [[ptr,a,b]]
 for path in owned:
  p=Path(path);old=(E/'before'/p).read_text();new=p.read_text()
  if p.suffix=='.json':
   changes=diff(json.loads(old),json.loads(new));(OUT/(p.stem+'-historical-diff.json')).write_text(json.dumps(changes,ensure_ascii=False,indent=2)+'\n')
   check('旧JSON字段差异已逐项取证 '+path,True,changes if len(str(changes))<1000 else {'count':len(changes),'evidence':p.stem+'-historical-diff.json'})
   if p.name in ['PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_PERFORMANCE_RESULTS.json']:
    check('大型账本旧字段无改 '+path,not changes)
  elif p.name not in ['WORKER_MODEL_BOUNDARY.md','MODEL_USAGE_SEMANTICS.md']:
   it=iter(new.splitlines());check('历史Markdown逐行保留 '+path,all(any(t==line for t in it) for line in old.splitlines()))
 check('INTERFACE_EVOLUTION字节原样',sha(D/'R05_INTERFACE_EVOLUTION.md')==sha(E/'before/docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md'))
 old=load(E/'before/docs/rust-tauri/ORCHESTRATOR_PROGRESS.json');cur=load(ROOT/'docs/rust-tauri/ORCHESTRATOR_PROGRESS.json')
 for group in ['stages','tasks']:
  if isinstance(old[group],list):
   for item in old[group]:
    if 'R05' not in str(item.get('stage_id',item.get('id',''))):check('ORCH非R05保留 '+str(item.get('id',item.get('stage_id'))),item in cur[group])
  else:
   for k,v in old[group].items():
    if not k.startswith('R05'):check('ORCH非R05保留 '+k,cur[group].get(k)==v)
elif mode=='hashes':
 for path,h in hand['dependency_locks'].items():check('真实锁 '+path,sha(path)==h['sha256'])
 for path,h in hand['artifact_hashes'].items():check('实际artifact '+path,Path(path).is_file() and sha(path)==h)
 for row in hand['rr3_current']['historical_layers']:check('层结果hash '+row['stage'],sha(row['result_ref'])==row['sha256'])
 for row in load(E/'manifest.json')['files']:check('冻结E包manifest '+row['path'],sha(E/row['path'])==row['sha256'])
 for name in ['commands.json','commands-02.json','commands-03.json']:
  j=load(E/name)
  for i,c in enumerate(j['commands']):
   for stream in ['stdout','stderr']:check(name+f'/{i+1} '+stream,sha(c[stream])==c[stream+'Sha256'])
   if name=='commands-03.json':check('作者03真实exit '+str(i+1),c['exitCode']==0,{'argv':c['argv'],'exit':c['exitCode']})
 before=load(E/'inputhash-before.json');after=load(E/'inputhash-after.json')
 rows=before['files'];digest=hashlib.sha256(''.join(f'{h}  {p}\n' for p,h in sorted(rows.items())).encode()).hexdigest()
 mismatches=[p for p,h in rows.items() if not Path(p).exists() or sha(p)!=h]
 check('423输入数量',len(rows)==before['count']==hand['working_tree_digest_scope']['count'])
 check('摘要独立重算',digest==before['digest']==hand['working_tree_digest'])
 check('E输入前后原记录相等',rows==after['files'])
 check('当前限定生产输入仍相等',not mismatches,mismatches)
 for s in load(E/'source-evidence.json')['sources']:
  check('源摘录字节 '+s['path']+':'+str(s['line']),sha(s['path'])==s['sha256'] and '\n'.join(Path(s['path']).read_text().splitlines()[s['line']-1:s['endLine']])==s['excerpt'])
elif mode=='current':
 auth=load(D/'repair-current/RR3_ISSUE_MATRIX.json')
 expected={'A':next(x for x in auth['issues'] if x['id']=='F42'),'C_F46':next(x for x in auth['issues'] if x['id']=='F27-RR3')}
 for path in owned:
  if not path.endswith('.json'):continue
  j=load(path)
  def find(x,ptr):
   if isinstance(x,dict):
    if 'rr3_current' in x:
     c=x['rr3_current']
     check(path+ptr+' 阶段未放行',c['stage_readiness']=='NOT_ACCEPTED' and c['R06_READY'] is False)
     for pkg,exp in expected.items():check(path+ptr+' current '+pkg,c['packages'][pkg]['status']==exp['status'] and c['packages'][pkg]['independent_review']=='PASS',{'actual':c['packages'][pkg],'expected':{'status':exp['status'],'review':exp['independentReview']}})
     check(path+ptr+' FINAL未编造',c['packages']['FINAL']['status']=='NOT RUN' and c['packages']['FINAL']['tested_sha'] is None and c['packages']['FINAL']['result_ref'] is None)
    for k,v in x.items():find(v,ptr+'/'+k)
   elif isinstance(x,list):
    for i,v in enumerate(x):find(v,ptr+'/'+str(i))
  find(j,'')
 check('HANDOFF当前unresolved不虚列已关闭A/C',not {'A','C_F46'} & {x['id'] for x in hand['unresolved_items']},[x['id'] for x in hand['unresolved_items']])
 for pkg,report in [('A','A-REVIEW-02'),('C_F46','C-F46-REVIEW-01')]:
  t=(ROOT/'artifacts/rust-tauri/R05/RR3'/report/'REVIEW.md').read_text();check('真实独立报告 '+report,'PASS' in t and 'mustFix' in t,sha(ROOT/'artifacts/rust-tauri/R05/RR3'/report/'REVIEW.md'))
elif mode=='fields':
 def body(path,name,kind='struct'):
  text=Path(path).read_text();m=re.search(r'pub '+kind+r' '+re.escape(name)+r'\s*\{',text);assert m,(path,name)
  depth=1;start=m.end();end=start
  while depth:
   if text[end]=='{':depth+=1
   if text[end]=='}':depth-=1
   end+=1
  return text[start:end-1]
 def fields(path,name):return re.findall(r'^\s*pub\s+(\w+)\s*:',body(path,name),re.M)
 c=hand['consumer_contract']
 specs=[('gateway','request_fields','rust/crates/lingxi-kernel/src/model_exchange.rs','ModelRouteRequest'),('context','fields','rust/crates/lingxi-kernel/src/lib.rs','RunContext'),('model_turn','fields','rust/crates/lingxi-kernel/src/model_exchange.rs','ModelTurnInput'),('auxiliary','request_fields','rust/crates/lingxi-adapters/src/models/auxiliary.rs','AuxiliaryRequest'),('embedding','request_fields','rust/crates/lingxi-adapters/src/models/operations/embedding.rs','EmbeddingRequest'),('embedding','context_fields','rust/crates/lingxi-service/src/operations.rs','OperationCallContext'),('usage_query','fields','rust/crates/lingxi-kernel/src/usage.rs','ModelUsageQuery')]
 for obj,key,path,name in specs:
  actual=fields(path,name);check('独立字段全序相等 '+name,actual==c[obj][key],{'source':path,'actual':actual,'handoff':c[obj][key]})
 enum=body('rust/crates/lingxi-kernel/src/model_exchange.rs','ExchangeItem','enum')
 for variant,key in [('AssistantTurn','assistant_fields'),('ToolResult','tool_result_fields')]:
  segment=enum.split(variant+' {',1)[1].split('\n    }',1)[0];actual=re.findall(r'^\s*(\w+)\s*:',segment,re.M);check('ExchangeItem完整字段 '+variant,actual==c['exchange_and_messages'][key],actual)
 proto=hand['protocol_version']; hs=Path('rust/crates/lingxi-protocol/src/handshake.rs').read_text()
 for key,const in [('min_supported','WIRE_PROTOCOL_MIN_SUPPORTED'),('max_supported','WIRE_PROTOCOL_MAX_SUPPORTED')]:check('wire真实常量 '+const,int(re.search(const+r': u32 = (\d+);',hs)[1])==proto[key])
 check('wire name','pub const WIRE_PROTOCOL_NAME: &str = "'+proto['name']+'";' in hs)
 check('event version',int(re.search(r'EVENT_SCHEMA_VERSION: u32 = (\d+);',Path('rust/crates/lingxi-protocol/src/wire.rs').read_text())[1])==proto['event_schema_version'])
 ver=load('shared/contract-versions.json');check('真实data epoch',hand['data_epoch']==ver['DATA_EPOCH']);check('legacy轴',proto['legacy_preload_api']==ver['PRELOAD_API_VERSION'] and proto['legacy_server_protocol']==ver['SERVER_PROTOCOL_VERSION'])
 migrations=Path('rust/crates/lingxi-adapters/src/storage/migrations.rs').read_text();check('schema版本实际max',hand['storage_schema_version']==max(map(int,re.findall(r'\bversion: (\d+),',migrations))))
 checks=[('kernel gateway','rust/crates/lingxi-kernel/src/model_exchange.rs','fn resolve_route(\n        &self,\n        request: &ModelRouteRequest,\n    ) -> Result<ResolvedModelRoute, ModelGatewayError>'),('credential signature','rust/crates/lingxi-adapters/src/models/credentials.rs','route: &\x27a ResolvedModelRoute'),('credential 401 port','rust/crates/lingxi-adapters/src/models/credentials.rs','fn report_unauthorized'),('runtime input','rust/crates/lingxi-service/src/runs.rs','deadline_unix_ms: call_deadline'),('production trace','rust/crates/lingxi-service/src/lib.rs','Arc::new(workermodel::LedgerWorkerCallbackTrace::new('),('共享配额释放','rust/crates/lingxi-service/src/runs.rs','drop(model_permit);'),('先账后事件','rust/crates/lingxi-service/src/runs.rs','port.record_model_call_usage(record, now_ms)'),('取消只账行','rust/crates/lingxi-service/src/runs.rs','async fn persist_model_call_cancelled_in_flight'),('owner JOIN','rust/crates/lingxi-adapters/src/storage/run_store.rs','JOIN sessions s ON s.session_id = m.session_id'),('owner过滤','rust/crates/lingxi-adapters/src/storage/run_store.rs','WHERE s.owner_user_id'),('未知尝试NULL','rust/crates/lingxi-adapters/src/storage/migrations.rs','transport_attempts  INTEGER,'),('真实普通取消','rust/crates/lingxi-service/tests/subagent_closeout.rs','parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap'),('真实下一run恢复','rust/crates/lingxi-service/tests/late_result_fence.rs','r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing')]
 for name,path,token in checks:check(name,token in Path(path).read_text(),path)
 for fn,req in [('embed','dialects::embedding::EmbeddingRequest'),('rerank','RerankRequest')]:
  op=Path('rust/crates/lingxi-service/src/operations.rs').read_text();m=re.search(r'pub async fn '+fn+r'\([\s\S]*?\) -> Result<[^\n]+',op);check('实际调用签名 '+fn,m is not None and 'request: '+req in m[0] and 'deadline_unix_ms: Option<u64>' in m[0] and 'context: Option<&OperationCallContext>' in m[0],m[0] if m else None)
 for path in ['anthropic_messages.rs','google_generative_ai.rs','openai_responses.rs']:
  t=Path('rust/crates/lingxi-adapters/src/models/'+path).read_text();check('opaque来源真防线 '+path,'origin.authorizes(&route.provider, &route.model)' in t and 'None' in t,t[t.index('fn validate_replay_origin') if 'fn validate_replay_origin' in t else t.index('origin.authorizes')-100:t.index('origin.authorizes')+550])
 origin=body('rust/crates/lingxi-kernel/src/model_exchange.rs','TurnOrigin');check('provider/model两个来源字段',re.findall(r'pub (\w+):',origin)==['provider','model'])
 sample=Path('rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs').read_text().split('async fn rr1_f21_operation_context_carries_session_run_and_cause()',1)[1].split('// ── F21: the query surface',1)[0]
 for token in ['"embedding":[0.25,0.75]','"prompt_tokens":5,"total_tokens":9','dimensions: Some(2)','context_window: None','Some(&context)','Some("sess_rr1_ctx")','Some("run-rr1-ctx")','CallOutcome::Succeeded','started_at_unix_ms.is_some()','row.usage.is_some()']:
  check('正常样例实际源码 '+token,token in sample)
 usage_source=Path('rust/crates/lingxi-adapters/src/models/usage.rs').read_text().split('pub fn decode_operation_usage',1)[1].split('#[cfg(test)]',1)[0]
 check('缺output不由total猜算',all(x in usage_source for x in ['let output = match read("output_tokens")','(input, output) => (input, output)','output_tokens: output']))
 check('§6.2必需字段齐全',all(x in hand for x in ['source_sha','working_tree_digest','protocol_version','data_epoch','dependency_locks','accepted_tasks','unresolved_items','allowed_next_scope','artifact_hashes']))
 check('未accepted无虚列任务',hand['accepted_tasks']==[] and hand['allowed_next_scope']['stage']=='R05_ONLY')
elif mode=='artifacts':
 from collections import Counter
 for row in hand['rr3_current']['historical_layers']:
  j=load(row['result_ref']);bind=j['candidateSourceBinding'];ck=bind['checkpointAfterEveryCommand'];commands=j['commands']
  actual={'overall':j['overall'],'candidate_stable':bind['stable'],'runner_status':j['runnerSourceBinding']['status'],'stable_checkpoints':sum(x['stable'] for x in ck),'checkpoints':len(ck),'commands_passed':sum(x['status']=='PASS' for x in commands),'commands_total':len(commands)}
  check('历史层真实字段 '+row['stage'],all(row[k]==v for k,v in actual.items()),actual)
 perf=load(D/'R05_PERFORMANCE_RESULTS.json')['rr3_resource_selfcheck'];raw=load(perf['raw_series'])
 for key in ['load','thresholds']:check('资源原始字段 '+key,perf[key]==raw[key])
 check('真实160轮',perf['cycles']==len(raw['cycleResults'])==160)
 check('binary点',perf['binary_points']==len(raw['series']))
 check('owner点',perf['owner_points']==len(raw['ownerResourceSeries']))
 for field,outname in [('series','binary_phases'),('ownerResourceSeries','owner_phases')]:check('资源逐相位 '+field,dict(Counter(x['phase'] for x in raw[field]))==perf[outname])
 check('raw资源清理',all(perf['cleanup'][k]==v for k,v in raw['cleanup'].items() if k in perf['cleanup']))
 scope=load(D/'R05_SCOPE_MATRIX.json');check('权威130口径',hand['rr3_current']['scope_counts']['shared']==119 and hand['rr3_current']['scope_counts']['full']==6 and hand['rr3_current']['scope_counts']['deferred']==5,scope['counts'])
 for x in load(D/'R05_TEST_MAP.json')['rr3_I10_mapping']['ordinary_cancel_same_instance']:
  receipt=load(x['receipt']);check('普通取消实际回执 '+x['test'],receipt.get('exitCode',receipt.get('exit_code'))==x['exit_code'],receipt)
 check('预算408仍记running',all(x['status']=='running' and x['httpStatus']==408 for x in raw['cycleResults'] if x['phase']=='cancel') and raw['load']['cancelSettled']==0 and raw['load']['cancelDanglingActive']==60)
 for x in ['raw_npm','directed_E5','permissions']:check('许可与raw红明确 '+x,bool(hand['rr3_current'][x]),hand['rr3_current'][x])

else:raise SystemExit('unknown mode')
finish(mode)
