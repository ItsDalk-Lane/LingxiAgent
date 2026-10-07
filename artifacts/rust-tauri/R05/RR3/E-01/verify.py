from pathlib import Path
import json,re,sys,hashlib,collections,os,urllib.parse
ROOT=Path.cwd();E=ROOT/'artifacts/rust-tauri/R05/RR3/E-01';D=ROOT/'docs/rust-tauri/R05'
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def read(p):return (ROOT/p).read_text()
def obj(p):return json.loads(read(p))
before=json.loads((E/'before.json').read_text()); owned=list(before['owned']);checks=[]
def check(ok,label):
 checks.append(label)
 if not ok:raise AssertionError(label)
def fields(p,typ):
 s=read(p);block=s.split('pub struct '+typ+' {',1)[1].split('\n}',1)[0]
 return re.findall(r'^\s*pub\s+(\w+)\s*:',block,re.M)
def walk_strings(d):
 if isinstance(d,dict):
  for v in d.values():yield from walk_strings(v)
 elif isinstance(d,list):
  for v in d:yield from walk_strings(v)
 elif isinstance(d,str):yield d
mode=sys.argv[1]
if mode=='json':
 for p in owned:
  if p.endswith('.json'):
   def unique(pairs):
    result={}
    for k,v in pairs:
     if k in result:raise AssertionError('duplicate JSON key '+k)
     result[k]=v
    return result
   json.loads(read(p),object_pairs_hook=unique);check(True,'JSON '+p)
elif mode=='consistency':
 expected=json.loads((E/'current-state.json').read_text())
 for p in owned:
  if not p.endswith('.json'):continue
  d=obj(p);c=d['stages']['R05']['rr3_current'] if 'ORCHESTRATOR' in p else d['rr3_current']
  check(c==expected,'current state identical '+p)
  check(c['stage_readiness']=='NOT_ACCEPTED' and c['R06_READY'] is False,'no R06 acceptance '+p)
  check(c['packages']['B']['independent_review']=='PASS','B independent only '+p)
  check(c['packages']['A']['independent_review']=='PENDING' and c['packages']['C_F46']['independent_review']=='PENDING','pending A/C '+p)
  check(c['packages']['G']['status']=='NOT RUN' and c['packages']['FINAL']['result_ref'] is None and c['packages']['FINAL']['tested_sha'] is None,'no final result/SHA '+p)
 for name in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md','MODEL_USAGE_SEMANTICS.md','WORKER_MODEL_BOUNDARY.md']:
  s=(D/name).read_text();check('NOT_ACCEPTED' in s and 'R06_READY=false' in s,'Markdown current state '+name)
 h=obj('docs/rust-tauri/R05/R05_HANDOFF.json')
 for k in ['source_sha','working_tree_digest','protocol_version','data_epoch','dependency_locks','accepted_tasks','unresolved_items','allowed_next_scope','artifact_hashes']:check(k in h,'required handoff field '+k)
 check(h['accepted_tasks']==[] and h['allowed_next_scope']['stage']=='R05_ONLY','handoff cannot launch R06')
 check('v7' in ''.join(h['consumer_guidance_r06']['ready_to_consume']) and '台账 v5' not in ''.join(h['consumer_guidance_r06']['ready_to_consume']),'consumer v7')
 check('生产端为 Noop' not in (D/'WORKER_MODEL_BOUNDARY.md').read_text(),'worker production Noop removed')
 check('无 session/run 归属——' not in (D/'MODEL_USAGE_SEMANTICS.md').read_text(),'operation optional context consistent')
 scope=obj('docs/rust-tauri/R05/R05_SCOPE_MATRIX.json')['counts'];check(scope=={'base':16,'supplemental_total':130,'share':119,'full':6,'deferred':5,'single_stage_r05_only':6,'dual_stage_r05_r07':124},'scope authoritative counts')
 rows=[line.split() for line in read('docs/rust-tauri/R05/r05_stage_pins.tsv').splitlines() if line.strip() and not line.startswith('#')]
 check(sum(x[1].startswith(('adp:','svc:')) for x in rows)==27 and sum(x[1].startswith('lib-') for x in rows)==64,'27 suites + 64 lib pins')
 rows=[line.split() for line in read('docs/rust-tauri/R05/r05_required_cids.tsv').splitlines() if line.strip() and not line.startswith('#')]
 check(len(rows)==103,'required CIDs 103 unchanged')
elif mode=='history':
 for name in ['PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_PERFORMANCE_RESULTS.json','R05_LIVE_VERIFICATION.json']:
  rel='docs/rust-tauri/R05/'+name;old=json.loads((E/'before'/rel).read_text());new=obj(rel)
  check(all(new[k]==v for k,v in old.items()),'original JSON fields identical '+name)
  prefix=(E/'before'/rel).read_text().rsplit('}',1)[0].rstrip();check(read(rel).startswith(prefix),'large ledger original byte prefix '+name)
 for name in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md']:
  rel='docs/rust-tauri/R05/'+name;old=(E/'before'/rel).read_text().split('\n',1)[1].lstrip('\n')
  check(old in read(rel),'historical Markdown verbatim '+name)
 rel='docs/rust-tauri/R05/R05_HANDOFF.json';old=json.loads((E/'before'/rel).read_text());new=obj(rel)
 for k in ['baseline','rr1_repair_round','rr2_repair_round','deferred_registrations','interfaces']:check(new[k]==old[k],'handoff history unchanged '+k)
 rel='docs/rust-tauri/ORCHESTRATOR_PROGRESS.json';old=json.loads((E/'before'/rel).read_text());new=obj(rel)
 check(all(new['stages'][k]==v for k,v in old['stages'].items() if k!='R05'),'all non-R05 stages identical')
 check(all(new['tasks'][k]==v for k,v in old['tasks'].items() if not k.startswith('R05-')),'all non-R05 tasks identical')
 changed_root={'current_head','current_head_note','current_task','stages','tasks'}
 check(all(new[k]==v for k,v in old.items() if k not in changed_root),'other ORCHESTRATOR roots identical')
 check(new['stages']['R05']['rr2_repair_round']==old['stages']['R05']['rr2_repair_round'],'R05 historical RR2 unchanged')
 external=[]
 for p,v in json.loads((E/'protected-before.json').read_text()).items():
  actual=sha(ROOT/p)
  if '/dispatch/events.jsonl' in p:
   # 外部CLI日志在运行中追加：验证开工原始字节前缀仍完整，E不写dispatch。
   cumulative=hashlib.sha256();found=False
   for line in (ROOT/p).read_bytes().splitlines(keepends=True):
    cumulative.update(line)
    if cumulative.hexdigest()==v:found=True;break
   check(actual==v or found,'externally appended dispatch original prefix preserved')
  elif '/repair-current/' in p and actual!=v:
   # 总控唯一所有权文件会并发更新，诚实登记观察，不重拍基线、不宣称未变化。
   external.append({'path':p,'beforeSha256':v,'observedSha256':actual,'classification':'EXTERNAL_OWNER_CHANGE_OBSERVED; E未写此路径，不采纳新终审结果'})
  else:check(actual==v,'protected authority/immutable dispatch '+p)
 print(json.dumps({'external_owner_changes':external},ensure_ascii=False))
 check(sha(D/'R05_INTERFACE_EVOLUTION.md')==before['owned']['docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md'],'optional evolution unchanged')
 inputs=json.loads((E/'inputhash-before.json').read_text());after={p:sha(ROOT/p) for p in inputs['files']}
 check(after==inputs['files'],'all 423 scoped source inputs unchanged')
 check(hashlib.sha256(''.join(f'{h}  {p}\n' for p,h in sorted(after.items())).encode()).hexdigest()==inputs['digest'],'scoped input digest unchanged')
elif mode=='source':
 h=obj('docs/rust-tauri/R05/R05_HANDOFF.json');c=h['consumer_contract'];spec=[('gateway','request_fields','request_source','ModelRouteRequest'),('context','fields','source','RunContext'),('model_turn','fields','source','ModelTurnInput'),('auxiliary','request_fields','source','AuxiliaryRequest'),('embedding','request_fields','request_source','EmbeddingRequest'),('embedding','context_fields','source','OperationCallContext'),('usage_query','fields','source','ModelUsageQuery')]
 # 路由请求与端口在同一源文件。
 for key,field,src,typ in spec:
  p=c[key].get(src,c[key]['source']);check(fields(p,typ)==c[key][field],typ+' exact current fields')
 check('WIRE_PROTOCOL_MIN_SUPPORTED: u32 = 1' in read('rust/crates/lingxi-protocol/src/handshake.rs') and 'WIRE_PROTOCOL_MAX_SUPPORTED: u32 = 1' in read('rust/crates/lingxi-protocol/src/handshake.rs'),'wire versions 1/1')
 check('EVENT_SCHEMA_VERSION: u32 = 1' in read('rust/crates/lingxi-protocol/src/wire.rs'),'event schema 1')
 check(obj('shared/contract-versions.json')['DATA_EPOCH']==h['data_epoch']==1,'data epoch 1')
 migrations=read(c['versions_source']);check(max(map(int,re.findall(r'version:\s*(\d+),',migrations)))==h['storage_schema_version']==7,'schema max migration 7')
 check('model_call_usage_rr1_f38_attempts_nullable' in migrations,'migration v7 name')
 for p,v in h['dependency_locks'].items():check(sha(ROOT/p)==v['sha256'],'actual dependency lock '+p)
 check('channel = "1.98.1"' in read('rust-toolchain.toml'),'toolchain pin actual root')
 check('LedgerWorkerCallbackTrace::new(' in read('rust/crates/lingxi-service/src/lib.rs'),'Ledger production composition wired')
 runs=read(c['model_turn']['runtime_source']);check('deadline_unix_ms: call_deadline' in runs and 'remaining_budget_ms(call_deadline)' in runs and 'system_prompt: None' in runs,'runtime deadline/system actual')
 check('.next_turn(&call_ctx, &call_id, &turn_input, &sink)' in runs,'actual driver call signature')
 ports=read(c['model_turn']['port_source']);check('deltas: &\'a dyn TurnDeltaSink' in ports and 'input: &\'a ModelTurnInput' in ports,'next_turn streaming sink argument')
 op=read(c['embedding']['source']);check(bool(re.search(r'pub async fn embed\(\s*&self,\s*request: dialects::embedding::EmbeddingRequest,\s*deadline_unix_ms: Option<u64>,\s*context: Option<&OperationCallContext>',op)),'embedding actual signature')
 check(bool(re.search(r'pub async fn rerank\(\s*&self,\s*request: RerankRequest,\s*deadline_unix_ms: Option<u64>,\s*context: Option<&OperationCallContext>',op)),'rerank actual signature')
 sample=read('rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs').split('async fn rr1_f21_operation_context_carries_session_run_and_cause()',1)[1].split('\n// ──',1)[0]
 for token in ['"sess_rr1_ctx"','"run-rr1-ctx"','"run-rr1-ctx#a1"','"run-rr1-ctx-tc0007"','"context-bound input"','dimensions: Some(2)','context_window: None','input_type: Default::default()','Some(&context)','[0.25,0.75]','"prompt_tokens":5,"total_tokens":9','CallOutcome::Succeeded']:
  check(token in sample,'real normal sample token '+token)
 for test in ['rr1_f21_operation_queue_timeout_never_invents_an_http_attempt','rr1_f22_operation_invalid_usage_must_not_persist_secret_payload']:check(test in read('rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs'),'error/invalid sample source '+test)
 for test,p in [('parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap','rust/crates/lingxi-service/tests/subagent_closeout.rs'),('r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing','rust/crates/lingxi-service/tests/late_result_fence.rs')]:check(test in read(p),'I10 real test source '+test)
 # 不把这些检查当编译或重新运行样例。
elif mode=='evidence':
 h=obj('docs/rust-tauri/R05/R05_HANDOFF.json');c=h['rr3_current']
 for p,v in h['artifact_hashes'].items():check(sha(ROOT/p)==v,'real artifact hash '+p)
 for layer in c['historical_layers']:
  d=obj(layer['result_ref']);check(sha(ROOT/layer['result_ref'])==layer['sha256'],'historical result hash '+layer['stage']);check(d['overall']==layer['overall'],'historical FAIL '+layer['stage'])
 r02=c['historical_r02'];parent=obj(r02['parent_result']);command=next(r for r in parent['commands'] if r['key']=='r02_legacy_regression')
 check(command['status']=='PASS' and command['exitCode']==0 and 'R02_LEGACY_REGRESSION_MODE=directed-no-seal-family' in command['argv'],'real fourth layer R02 directed command')
 text=read(r02['summary_ref']);check('SKIP E5 (full npm + seal-family classification) BY SCOPE' in text and 'RESULT: R02 legacy entry regression DIRECTED (E0–E4.5) ALL GREEN' in text,'R02 real summary keeps E5 scope')
 rr=obj(c['evidence_refs']['resource_receipt']);raw=obj(c['evidence_refs']['resource_raw']);check(rr['exitCode']==0 and rr['summaries']==['test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 335.40s'],'real F46 full receipt')
 check(rr['inputHashesBefore']==rr['inputHashesAfter'],'F46 measured inputs equal')
 for p,v in rr['inputHashesAfter'].items():check(sha(ROOT/p)==v,'F46 receipt bound current input '+p)
 check(rr['artifacts']['f27-resource-series.json']==sha(ROOT/c['evidence_refs']['resource_raw']),'F46 raw hash bound receipt')
 check(len(raw['cycleResults'])==160 and len(raw['series'])==54 and len(raw['ownerResourceSeries'])==61,'raw load and sampling counts')
 check(collections.Counter(x['phase'] for x in raw['cycleResults'])=={'cancel':60,'error':60,'ok':15,'long':10,'worker':15},'original 160 profile')
 check(raw['load']['cancelSettled']==0 and raw['load']['cancelDanglingActive']==60,'408 cannot pretend normal cancel recovery')
 logs=lambda x:len([f for f in x['files']['home'] if re.fullmatch(r'lingxi-service/logs/service-\d+\.log',f['path'])])
 check(all(logs(x)<=3 for x in raw['series']+raw['ownerResourceSeries']),'every raw log point <= original3')
 steady=[x for x in raw['ownerResourceSeries'] if x['phase']=='released-steady'];check(len(steady)==45,'45 owner steady windows')
 check(all(all(v==0 or v==[] for v in x['owners'].values()) for x in steady),'owner release all zero/empty')
 negative=obj(c['evidence_refs']['resource_negative']);check([r['exitCode'] for r in negative['rows']]==[0,101,101,0] and all(r['targetNamed'] for r in negative['rows']),'negative FD/TCP refuse and restored')
 for key,filtered in [('ordinary_cancel',7),('late_result',4)]:
  r=obj(c['evidence_refs'][key]);check(r['exitCode']==0 and any(f'1 passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out' in x for x in r['summaries']),'real I10 receipt '+key)
elif mode=='links':
 warnings=[]
 for rel in owned:
  if not rel.endswith('.md'):continue
  old=(E/'before'/rel).read_text();new=read(rel);oldlinks=set(re.findall(r'\[[^\]]*\]\(([^)]+)\)',old))
  for target in re.findall(r'\[[^\]]*\]\(([^)]+)\)',new):
   if target.startswith(('http:','https:','mailto:')):continue
   target=urllib.parse.unquote(target);path,_,anchor=target.partition('#');dest=((ROOT/rel).parent/path).resolve() if path else ROOT/rel
   good=dest.exists()
   if good and anchor and dest.suffix=='.md':good=f'id="{anchor}"' in dest.read_text() or any(re.sub(r'[^\w\- ]','',line.lstrip('#').strip()).lower().replace(' ','-')==anchor for line in dest.read_text().splitlines() if line.startswith('#'))
   if target in oldlinks:
    if not good:warnings.append({'file':rel,'target':target,'status':'PREEXISTING_HISTORICAL_LINK'})
   else:check(good,'new Markdown link '+rel+' -> '+target)
 h=obj('docs/rust-tauri/R05/R05_HANDOFF.json')
 # 新JSON消费区所有源码/证据引用必须真实存在；历史拟用目录不作为新产物。
 for s in walk_strings(h['consumer_contract']):
  if s.startswith(('rust/','artifacts/','docs/')) and '\n' not in s:
   path=s.split('::',1)[0];check((ROOT/path).is_file(),'consumer source/ref '+path)
 for p in h['rr3_current']['evidence_refs'].values():check((ROOT/p).is_file(),'current evidence ref '+p)
 print(json.dumps({'historical_link_warnings':warnings},ensure_ascii=False))
else:raise ValueError(mode)
print(json.dumps({'check':mode,'assertions':len(checks),'status':'PASS','boundary':'E文档自检，不是独立签收或cargo/正式门禁执行'},ensure_ascii=False))
