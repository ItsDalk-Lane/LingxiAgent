from pathlib import Path
import json,re,hashlib,sys,datetime,os,subprocess,difflib
ROOT=Path.cwd(); OUT=Path(__file__).resolve().parent; RR=ROOT/'artifacts/rust-tauri/R05/RR3'; D=ROOT/'docs/rust-tauri/R05'
checks=[]
def ck(name,ok,detail=None): checks.append({'check':name,'ok':bool(ok),'detail':detail})
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def pairs(ps):
 d={}
 for k,v in ps:
  if k in d: raise ValueError('duplicate key '+k)
  d[k]=v
 return d
def j(p):return json.loads(Path(p).read_text(),object_pairs_hook=pairs)
def save(n,d):(OUT/n).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def fields(path,name):
 s=Path(path).read_text(); start=re.search(r'pub (?:struct|enum) '+name+r'\b[^\{]*\{',s).end(); level=1;i=start
 while level:
  if s[i]=='{':level+=1
  elif s[i]=='}':level-=1
  i+=1
 body=s[start:i-1];return re.findall(r'^\s*pub\s+(\w+)\s*:',body,re.M),body
OWNED=list(j(RR/'E-02/before.json')['owned']); H=j(D/'R05_HANDOFF.json'); C=H['rr3_current']; mode=sys.argv[1]
if mode=='read':
 paths=[ROOT/p for p in OWNED]
 paths+=list((D/'repair-current').glob('RR*MASTER*'))+[D/'repair-current'/p for p in ['RR3_BRIEF.md','RR3_REVIEW_BRIEF.md','RR3_E_BRIEF.md','RR3_E_R2_BRIEF.md','RR3_E_R2_REVIEW_BRIEF.md','RR3_ISSUE_MATRIX.json','RR3_PROGRESS.md','RR3_HANDOFF.md','RR2_BRIEF.md','RR2_ISSUE_MATRIX.json','RR2_PROGRESS.md','RR2_HANDOFF.md']]
 paths+=list((ROOT/'artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/specifications').rglob('*.md'))
 for pack,report in [('E-01','REPORT.md'),('E-REVIEW-01','REVIEW.md'),('E-02','REPORT.md'),('A-REVIEW-02','REVIEW.md'),('B-REVIEW-01','REVIEW.md'),('C-F46-REVIEW-01','REVIEW.md'),('D-REVIEW-01','REVIEW.md')]:paths.append(RR/pack/report)
 rows=[]
 for p in paths:
  data=p.read_bytes(); dest=OUT/'read-input'/p.relative_to(ROOT);dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
  rows.append({'path':str(p.relative_to(ROOT)),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'read':'full bytes / JSON parsed where applicable'})
 save('reading.json',rows);ck('full input read',True,len(rows))
elif mode=='json':
 for p in OWNED:
  if p.endswith('.json'):ck('strict '+p,isinstance(j(ROOT/p),dict))
 for text in ['{"x":1,"x":2}','{"nested":{"x":1,"x":2}}']:
  try:json.loads(text,object_pairs_hook=pairs);good=False
  except ValueError:good=True
  ck('nested duplicate refusal',good,text)
 ck('valid nested zero retained',json.loads('{"nested":{"x":0}}',object_pairs_hook=pairs)=={'nested':{'x':0}})
elif mode=='current':
 if len(sys.argv)>2:H=j(sys.argv[2]);C=H['rr3_current']
 for p in OWNED:
  if p.endswith('.json'):
   obj=j(ROOT/p);curr=obj['stages']['R05']['rr3_current'] if 'stages' in obj else obj['rr3_current'];ck('same current '+p,curr==C)
 for pkg,report in [('A','A-REVIEW-02'),('B','B-REVIEW-01'),('C_F46','C-F46-REVIEW-01')]:
  v=C['packages'][pkg];ck('closed latest '+pkg,v['status']=='CLOSED' and v['independent_review']=='PASS' and report in v['evidence_ref'])
  ck('real PASS report '+pkg,'PASS' in (ROOT/v['evidence_ref']).read_text()[:600])
  if 'review_sha256' in v:ck('real review SHA '+pkg,sha(ROOT/v['evidence_ref'])==v['review_sha256'])
 ck('closed A/C removed from unresolved',not {'A','C_F46','F42','F27-RR3','F46'}&{x['id'] for x in H['unresolved_items']})
 ck('not accepted / empty accepted',C['stage_readiness']=='NOT_ACCEPTED' and C['R06_READY'] is False and H['accepted_tasks']==[])
 ck('only R05 next scope',H['allowed_next_scope']['stage']=='R05_ONLY' and any('R05必需' in x for x in H['allowed_next_scope']['forbidden']))
 ck('E correct pending independent',C['packages']['E']['status']=='SELF_CHECKED' and C['packages']['E']['independent_review']=='PENDING' and C['packages']['E']['round']==2)
 er=C['packages']['E']['historical_reviews'][0];ck('E first FAIL preserved',er['verdict']=='FAIL' and er['mustFix']==['MF-E01','MF-E02'] and sha(ROOT/er['evidence_ref'])==er['sha256'])
 for pkg in ['G','FINAL']:
  v=C['packages'][pkg];ck(pkg+' no fake result/SHA',v.get('result_ref') is None and v.get('tested_sha') is None)
 ck('G real running observed',C['packages']['G']['status']=='RUNNING' and not (RR/'G-REVIEW-01/REVIEW.md').exists())
 ck('FINAL not run',C['packages']['FINAL']['status']=='NOT RUN' and not (RR/'FINAL-01/verify-R05/verify-stage-result.json').exists())
 v=C['packages']['D'];dr=j(RR/'D-REVIEW-01/r00-formal-01.json');identity=dr['observedBinaryProcesses'][0]['identity'];ck('D preparation vs gate distinction',v['status']=='BLOCKED' and v['required_gate']=='FAIL' and v['exit_code']==101 and dr['actualExitCode']==101 and dr['supervisorTimedOut'] is False)
 ck('D identity precise',v['binary_identity']['sha256']==identity['sha256']=='9f7489029c91d1c232e3c204236854bd53c69cee2fef33d6fd778a27f44696d3' and v['binary_identity']['cdhash']==identity['cdhash'])
 ck('D disk object hash',sha(v['binary_identity']['absolutePath'])==identity['sha256'])
 for f in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md','MODEL_USAGE_SEMANTICS.md']:
  s=(D/f).read_text(); ck('current latest reports '+f,('A-REVIEW-02' in s or ('A/F42' in s and '均新独立PASS' in s)) if f!='MODEL_USAGE_SEMANTICS.md' else 'C-F46-REVIEW-01' in s)
 ck('history snapshot explicit',H['rr3_current_history'][0]['historical_only'] is True)
 save('current-observation.json',{'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'GReportExists':(RR/'G-REVIEW-01/REVIEW.md').exists(),'GLog':(RR/'G-REVIEW-01/default-console.log').read_text(),'coordination':j(D/'repair-current/RR3_ISSUE_MATRIX.json')})
elif mode=='wire':
 p=Path(sys.argv[2]) if len(sys.argv)>2 else D/'WORKER_MODEL_BOUNDARY.md';s=p.read_text();src=ROOT/'rust/crates/lingxi-service/src/workerrpc.rs';actual,body=fields(src,'WorkerCallbackLine');source=src.read_text();fixture=(ROOT/'rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs').read_text()
 ck('two different actual fields',bool(re.search(r'行协议: kind=callback \+ op=model.complete',s)))
 example=json.loads(re.search(r'```json\s*(.*?)\s*```',s,re.S).group(1),object_pairs_hook=pairs)
 ck('example ordered actual fields',list(example)==actual==['kind','cb_id','op','purpose','prompt','max_output_tokens'])
 ck('kind and op example',example['kind']=='callback' and example['op']=='model.complete')
 ck('source actual kind dispatch','match parsed.get("kind").and_then' in source and 'Some("callback") => {' in source)
 ck('source required op and default fields',body.count('#[serde(default)]')==3 and 'pub op: String' in body)
 ck('actual unexpected kind rejected','unexpected line kind {other:?} (only callback/result are' in source and 'ProtocolViolation { detail }' in source)
 match=re.search(r'"ask_model_tagged"\s*=>\s*\{(.*?)\n\s*\}',fixture,re.S).group(1)
 ck('fixture separate fields and normal purpose',all(x in match for x in ['"kind": "callback"','"op": "model.complete"','"purpose": "summarize"','"max_output_tokens": 64']))
 ck('no imagined op validation','cb.op' not in source and '没有另行按`cb.op`匹配' in s)
 ck('host authority preserved',all(x in source for x in ['FORBIDDEN_CALLBACK_IDENTITY_KEYS','allowed_model_purposes.contains(&cb.purpose)','self.spec.model.complete(','&request_id','tokio::time::timeout_at(']))
 save('worker-source.json',{'path':str(src.relative_to(ROOT)),'sha256':sha(src),'fields':actual,'declaration':body,'fixtureMode':match,'sourceFullSnapshot':'source-input/rust/crates/lingxi-service/src/workerrpc.rs'})
elif mode=='fields':
 if len(sys.argv)>2:H=j(sys.argv[2])
 c=H['consumer_contract']; cases=[('gateway','request_fields','rust/crates/lingxi-kernel/src/model_exchange.rs','ModelRouteRequest'),('context','fields','rust/crates/lingxi-kernel/src/lib.rs','RunContext'),('model_turn','fields','rust/crates/lingxi-kernel/src/model_exchange.rs','ModelTurnInput'),('auxiliary','request_fields','rust/crates/lingxi-adapters/src/models/auxiliary.rs','AuxiliaryRequest'),('embedding','request_fields','rust/crates/lingxi-adapters/src/models/operations/embedding.rs','EmbeddingRequest'),('embedding','context_fields','rust/crates/lingxi-service/src/operations.rs','OperationCallContext'),('usage_query','fields','rust/crates/lingxi-kernel/src/usage.rs','ModelUsageQuery')]
 for group,key,p,name in cases:
  actual,body=fields(ROOT/p,name);ck('full order '+name,actual==c[group][key],{'actual':actual,'document':c[group][key],'declaration':body})
 _,ex=fields(ROOT/'rust/crates/lingxi-kernel/src/model_exchange.rs','ExchangeItem')
 for name,key in [('AssistantTurn','assistant_fields'),('ToolResult','tool_result_fields')]:
  b=re.search(name+r'\s*\{(.*?)\n\s*\}',ex,re.S).group(1);actual=re.findall(r'^\s*(\w+)\s*:',b,re.M);ck('full enum field order '+name,actual==c['exchange_and_messages'][key],actual)
 ms=(ROOT/'rust/crates/lingxi-adapters/src/storage/migrations.rs').read_text();versions=re.findall(r'version:\s*(\d+)',ms);ck('schema actual max7',max(map(int,versions))==H['storage_schema_version']==7)
 vers=j(ROOT/'shared/contract-versions.json');ck('data epoch real1',H['data_epoch']==1);save('actual-contract-versions.json',vers)
 hand=(ROOT/'rust/crates/lingxi-protocol/src/handshake.rs').read_text();wire=(ROOT/'rust/crates/lingxi-protocol/src/wire.rs').read_text();lib=(ROOT/'rust/crates/lingxi-protocol/src/lib.rs').read_text()
 ck('wire actual1',all(re.search(r'const '+n+r'[^=]*=\s*1\s*;',hand+lib) for n in ['WIRE_PROTOCOL_MIN_SUPPORTED','WIRE_PROTOCOL_MAX_SUPPORTED']) and H['protocol_version']['min_supported']==H['protocol_version']['max_supported']==1)
 ck('all mandatory handoff keys',all(k in H for k in ['source_sha','working_tree_digest','protocol_version','data_epoch','dependency_locks','accepted_tasks','unresolved_items','allowed_next_scope','artifact_hashes']))
 creds=(ROOT/c['credentials']['source']).read_text();ck('real credentials port resolve unauthorized','pub trait ProviderCredentialPort' in creds and 'fn resolve<' in creds and 'fn report_unauthorized<' in creds)
 ops=(ROOT/c['embedding']['source']).read_text()
 for name in ['embed','rerank']:
  sig=re.search(r'pub async fn '+name+r'\((.*?)\) -> ([^{]+)',ops,re.S).group(0);ck('operation real signature '+name,'deadline_unix_ms: Option<u64>' in sig and 'context: Option<&OperationCallContext>' in sig,sig)
 sample=(ROOT/'rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs').read_text();start=sample.index('async fn rr1_f21_operation_context_carries_session_run_and_cause');end=sample.find('#[tokio::test]',start);sample=sample[start:end]
 ck('normal embedding real complete sample',all(x in sample for x in ['"context-bound input"','dimensions: Some(2)','[0.25,0.75]','"prompt_tokens":5','"total_tokens":9','Some(&context)','"run-rr1-ctx-tc0007"']))
 ck('normal usage does not infer missing output','不能用total9减5猜4' in c['embedding']['normal_processing'])
 emb=(ROOT/'rust/crates/lingxi-adapters/src/models/operations/embedding.rs').read_text();usage=(ROOT/'rust/crates/lingxi-adapters/src/models/usage.rs').read_text();decode=usage[usage.index('pub fn decode_operation_usage'):usage.index('pub fn decode_operation_usage')+4800];ck('decoder output not total difference','let output = match read("output_tokens")' in decode and '(input, output) => (input, output)' in decode)
 runs=(ROOT/'rust/crates/lingxi-service/src/runs.rs').read_text();wl=(ROOT/'rust/crates/lingxi-service/src/workermodel.rs').read_text();boot=(ROOT/'rust/crates/lingxi-service/src/lib.rs').read_text();store=(ROOT/'rust/crates/lingxi-adapters/src/storage/run_store.rs').read_text()
 ck('actual main deadline','deadline_unix_ms: call_deadline' in runs)
 ck('actual LedgerWorkerCallbackTrace wired','LedgerWorkerCallbackTrace::new' in boot and 'record_model_call_usage' in wl)
 ck('unknown and cancelled ledger facts','record_cancelled_model_usage' in runs or 'Cancelled' in runs)
 ck('real owner and filter query',all(x in store for x in ['owner_user_id','recorded_from_unix_ms','recorded_to_unix_ms','query_model_call_usage']))
 ck('opaque provenance exact source match','provider' in fields(ROOT/'rust/crates/lingxi-kernel/src/model_exchange.rs','TurnOrigin')[0] and 'model' in fields(ROOT/'rust/crates/lingxi-kernel/src/model_exchange.rs','TurnOrigin')[0])
 ck('error unknown cancel recovery budget present',all(x in c['error_unknown_cancel_recovery_budget'] for x in ['error','unknown','cancel','restart_recovery','budget','ordinary_recovery_refs']))
 save('embedding-sample-source.json',{'file':'rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs','sha256':sha(ROOT/'rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs'),'sample':sample})
elif mode=='hashes':
 if len(sys.argv)>2:H=j(sys.argv[2])
 for p,h in H['artifact_hashes'].items():ck('artifact '+p,sha(ROOT/p)==h)
 for p,v in H['dependency_locks'].items():ck('dependency '+p,sha(ROOT/p)==v['sha256'])
 audits=[]
 for pack,name in [('A-REVIEW-02','manifest.json'),('C-F46-REVIEW-01','MANIFEST.json'),('D-REVIEW-01','evidence-manifest.json'),('E-01','manifest.json'),('E-REVIEW-01','manifest.json'),('E-02','manifest.json')]:
  base=RR/pack;man=j(base/name);fs=man['files'];rows=fs if isinstance(fs,list) else [dict(v,path=p) for p,v in fs.items()]
  for row in rows:
   p=base/row['path'];ck('manifest '+pack+'/'+row['path'],sha(p)==row['sha256'] and p.stat().st_size==row.get('bytes',p.stat().st_size))
  audits.append({'pack':pack,'count':len(rows),'manifestSha256':sha(base/name)})
 e=j(RR/'E-02/inputhash-before.json');actual={p:sha(ROOT/p) for p in e['files']};digest=hashlib.sha256(''.join(h+'  '+p+'\n' for p,h in sorted(actual.items())).encode()).hexdigest()
 ck('423 inputs individually same',actual==e['files']);ck('423 digest exact',digest==e['digest']==H['working_tree_digest']);ck('E2 frozen owned matches',all(sha(ROOT/p)==h for p,h in j(RR/'E-02/manifest.json')['ownedFilesAfter'].items()))
 a=j(RR/'A-REVIEW-02/input-manifest.json')['files'];ck('A all19 production inputs same',all(sha(ROOT/x['path'])==x['sha256'] for x in a),len(a))
 f=j(RR/'C-F46-REVIEW-01/FINAL_SOURCE_BINDING.json');fs=f['after']['files'];ck('C broad final mismatch preserved',f['fullSnapshotBeforeAfterEqual'] is False and f['comparisonToF46RuntimeInputs']['equal'] is False)
 save('manifest-audit.json',audits);save('c-binding-read.json',{'afterFileShape':str(type(fs)), 'binding':f})
elif mode=='history':
 changes={}
 def diff(a,b,p=''):
  if isinstance(a,dict) and isinstance(b,dict):
   out=[]
   for k in a:
    if k not in b:out.append(p+'/'+k+' MISSING')
    else:out+=diff(a[k],b[k],p+'/'+k)
   return out
  return [] if a==b else [p]
 for f in ['PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_PERFORMANCE_RESULTS.json','R05_LIVE_VERIFICATION.json']:
  before=j(RR/'E-01/before/docs/rust-tauri/R05'/f);after=j(D/f);ds=diff(before,after);ck('original historical fields untouched '+f,not ds,ds)
 for f in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md']:
  old=(RR/'E-01/before/docs/rust-tauri/R05'/f).read_text();new=(D/f).read_text();lines=new.splitlines();pos=0;missing=[]
  for line in old.splitlines():
   if line.startswith('# R05_REPORT —') or line.startswith('# R05 独立终审报告') or line.startswith('- 生成：R05-T08-执行者'):continue
   try:pos=lines.index(line,pos)+1
   except ValueError:missing.append(line)
  ck('historical MD in order '+f,not missing,missing)
 b=j(RR/'E-02/before/docs/rust-tauri/ORCHESTRATOR_PROGRESS.json');n=j(ROOT/'docs/rust-tauri/ORCHESTRATOR_PROGRESS.json');ck('ORCH non R05 stages unchanged',all(n['stages'][k]==v for k,v in b['stages'].items() if k!='R05'));ck('ORCH all tasks unchanged',b['tasks']==n['tasks'])
 for f in ['PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json']:
  before=j(RR/'E-02/before/docs/rust-tauri/R05'/f);after=j(D/f);delta=diff(before,after);ck('large ledger second round only current/history notice '+f,all(x.startswith(('/rr3_current','/rr3_I10_mapping','/historical_record_notice')) for x in delta),delta);changes[f]=delta
 ck('interface evolution no change',sha(D/'R05_INTERFACE_EVOLUTION.md')==j(RR/'E-02/before.json')['owned']['docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md'])
 save('historical-diff.json',changes);save('historical-metadata-exceptions.json',{'allowed':'current titles and generated header consumer pointer explicitly updated; original snapshots preserved in immutable E01/E02 before','onlyLines':['R05_REPORT title','R05_REPORT generated metadata points from old current10 to current11','R05_INDEPENDENT_REVIEW title']})
elif mode=='links':
 links=[]
 for p in OWNED:
  if not p.endswith('.md'):continue
  for target in re.findall(r'\]\(([^\s)]+)\)',(ROOT/p).read_text()):
   if re.match(r'https?://',target):continue
   path,_,anchor=target.partition('#');resolved=(ROOT/p).parent/path if path else ROOT/p;exists=resolved.exists();ok=exists
   if anchor and exists:
    s=resolved.read_text();anchors=re.findall(r'<a\s+id="([^"]+)"',s);anchors += [re.sub(r'[^\w\- ]','',re.sub(r'`','',h).lower()).replace(' ','-') for h in re.findall(r'^#+\s+(.*)',s,re.M)];ok=anchor in anchors
   ck('local MD link '+p+' '+target,ok);links.append({'file':p,'target':target,'ok':ok})
 def walk(v,p=''):
  if isinstance(v,dict):
   for k,x in v.items():walk(x,p+'/'+k)
  elif isinstance(v,list):
   for i,x in enumerate(v):walk(x,p+'/'+str(i))
  elif isinstance(v,str) and re.match(r'^(rust/|docs/|artifacts/|shared/)',v) and '\n' not in v and ' ' not in v and '+' not in v:
   x=v.split('::')[0];ck('JSON source/evidence path '+p,(ROOT/x).exists(),x)
 for k in ['consumer_contract','artifact_hashes']:walk(H[k],k)
 walk(C['evidence_refs'],'current/evidence_refs');save('links.json',links)
else:raise ValueError(mode)
save(mode+(('-'+Path(sys.argv[2]).stem) if len(sys.argv)>2 else '')+'-results.json',{'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'mode':mode,'checks':checks,'passed':sum(x['ok'] for x in checks),'failed':sum(not x['ok'] for x in checks),'boundary':'independent document/source/evidence assertions; no Cargo tests executed'})
print(json.dumps({'mode':mode,'passed':sum(x['ok'] for x in checks),'failed':[x for x in checks if not x['ok']]},ensure_ascii=False));sys.exit(1 if any(not x['ok'] for x in checks) else 0)
