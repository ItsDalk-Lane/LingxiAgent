from capture import *
import re,copy,collections,urllib.parse
checks=[]
def check(ok,label):
 checks.append({'check':label,'ok':bool(ok)})
 if not ok:raise AssertionError(label)
def unique(pairs):
 d={}
 for k,v in pairs:
  if k in d:raise ValueError('duplicate JSON key '+k)
  d[k]=v
 return d
def strict(p):return json.loads(Path(p).read_text(),object_pairs_hook=unique)
def strings(x):
 if isinstance(x,dict):
  for v in x.values():yield from strings(v)
 elif isinstance(x,list):
  for v in x:yield from strings(v)
 elif isinstance(x,str):yield x
def fields(p,typ):
 s=(ROOT/p).read_text(); block=s.split('pub struct '+typ+' {',1)[1].split('\n}',1)[0]
 return re.findall(r'^\s*pub\s+(\w+)\s*:',block,re.M)
def current_assert(h):
 c=h['rr3_current']; p=c['packages']; a=(ROOT/c['evidence_refs']['a_review']).read_text();cf=strict(ROOT/c['evidence_refs']['cf46_result'])
 check('PASS（A1/A2 同包）' in a,'A actual independent report PASS')
 check(cf['verdict']=='PASS' and cf['mustFix']==[],'C/F46 actual independent result PASS')
 for k in ['A','B','C_F46']:check(p[k]['status']=='CLOSED' and p[k]['independent_review']=='PASS','actual current closed '+k)
 check(p['A']['review_sha256']==sha(ROOT/c['evidence_refs']['a_review']),'A report real SHA')
 check(p['C_F46']['review_sha256']==cf['reviewSha256']==sha(ROOT/c['evidence_refs']['cf46_review']),'C/F46 report real SHA')
 check({x['id'] for x in h['unresolved_items']}=={'D','E','G','FINAL'},'unresolved only remaining packages')
 check(h['unresolved_items']==[{'id':k,**v} for k,v in p.items() if v['status']!='CLOSED'],'unresolved consumes latest packages')
 d=strict(ROOT/c['evidence_refs']['d_current_receipt']);identity=strict(ROOT/c['evidence_refs']['d_current_object'])
 check(d['actualExitCode']==p['D']['exit_code']==101 and not d['supervisorTimedOut'],'D real natural exit101 remains')
 check(p['D']['status']=='BLOCKED' and p['D']['required_gate']=='FAIL','D is not granted environment PASS')
 check(p['D']['binary_identity']=={k:identity[k] for k in ['absolutePath','sha256','cdhash']},'D new identity from raw prepared object')
 check(p['E']['status']=='SELF_CHECKED' and p['E']['independent_review']=='PENDING' and p['E']['round']==2,'E no independent self-sign')
 check(p['E']['historical_reviews'][0]['verdict']=='FAIL' and p['E']['historical_reviews'][0]['mustFix']==['MF-E01','MF-E02'],'E first independent FAIL retained')
 check(c['R06_READY'] is False and c['stage_readiness']=='NOT_ACCEPTED' and h['accepted_tasks']==[],'no stage/R06 acceptance')
 check(p['FINAL']['status']=='NOT RUN' and p['FINAL']['tested_sha'] is None and p['FINAL']['result_ref'] is None,'FINAL tested SHA and result empty')
 g=ROOT/R/'G-REVIEW-01';complete=[q for q in [g/'REVIEW.md',g/'REPORT.md',g/'RESULT.json'] if q.exists()]
 check(not complete and p['G']['status']=='RUNNING' and p['G']['result_ref'] is None,'G not finished; real running no invented result')
 check(h['allowed_next_scope']['stage']=='R05_ONLY' and all('A补修新独立' not in x and 'C/F46完整资源新联合独立验收' not in x for x in h['allowed_next_scope']['allowed']),'no stale re-review obligations')
 for k in ['source_sha','working_tree_digest','protocol_version','data_epoch','dependency_locks','accepted_tasks','unresolved_items','allowed_next_scope','artifact_hashes']:check(k in h,'6.2 field '+k)
def wire_assert(text):
 src=(ROOT/'rust/crates/lingxi-service/src/workerrpc.rs').read_text();fx=(ROOT/'rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs').read_text()
 check('kind=callback + op=model.complete' in text and 'kind=model.complete +' not in text,'wire two separate actual fields')
 block=re.search(r'```json\n(.*?)\n```',text,re.S);check(block is not None,'wire JSON example exists');example=json.loads(block.group(1),object_pairs_hook=unique)
 check(list(example)==fields('rust/crates/lingxi-service/src/workerrpc.rs','WorkerCallbackLine'),'wire declaration all six ordered fields')
 check(example['kind']=='callback' and example['op']=='model.complete','wire example kind/op exact')
 check('match parsed.get("kind").and_then(|k| k.as_str())' in src and 'Some("callback") => {' in src and 'unexpected line kind' in src and 'WorkerFailure::ProtocolViolation' in src,'production actual kind dispatcher and rejection')
 check('let cb: WorkerCallbackLine = match serde_json::from_value(parsed)' in src and 'self.spec.model.complete(' in src,'actual callback deserializes and dispatches host complete')
 fixture=fx.split('"ask_model_tagged" => {',1)[1].split('let reply = read_line_stdin()',1)[0]
 for token in ['"kind": "callback"','"cb_id": "cb-1"','"op": "model.complete"','"purpose": "summarize"','"max_output_tokens": 64']:check(token in fixture,'real normal fixture '+token)
 check('cb.op' not in src and '没有另行按`cb.op`匹配' in text,'no invented op validation claim')
mode=sys.argv[1]
try:
 if mode=='json':
  for p in OWNED:
   if p.endswith('.json'):strict(ROOT/p);check(True,'JSON no duplicate '+p)
  for p in [E/'inputhash-before.json',E/'manifest-consumption.json']:strict(p);check(True,'evidence JSON '+p.name)
  for bad in ['{"a":1,"a":2}','{"x":{"a":1,"a":2}}']:
   try:json.loads(bad,object_pairs_hook=unique)
   except ValueError:check(True,'duplicate negative rejected')
   else:check(False,'duplicate negative rejected')
 elif mode=='current':
  h=strict(D/'R05_HANDOFF.json');current_assert(h)
  for p in OWNED:
   if not p.endswith('.json'):continue
   x=strict(ROOT/p);c=x['stages']['R05']['rr3_current'] if 'ORCHESTRATOR' in p else x['rr3_current'];check(c==h['rr3_current'],'current equal '+p)
  for n in ['R05_REPORT.md','R05_BLOCKERS.md','R05_INDEPENDENT_REVIEW.md','R05_NEGATIVE_GATE_REPORT.md','MODEL_USAGE_SEMANTICS.md']:
   s=(D/n).read_text();current_text=s[s.rfind('## 11.'):] if n=='MODEL_USAGE_SEMANTICS.md' else s[s.rfind('## RR3 '):] if n in ['R05_NEGATIVE_GATE_REPORT.md','R05_INDEPENDENT_REVIEW.md'] else s[s.rfind('## 8. RR3'):] if n=='R05_BLOCKERS.md' else s[s.index('## 11.'):]
   check('PENDING、C/F46' not in current_text and '独立结论PENDING' not in current_text and 'A2独立复验PENDING' not in current_text,'no stale current A/C pending '+n)
  check('G RUNNING' in (D/'R05_REPORT.md').read_text() or '| G默认16 | RUNNING' in (D/'R05_REPORT.md').read_text(),'report current G running')
 elif mode=='wire':wire_assert((D/'WORKER_MODEL_BOUNDARY.md').read_text())
 elif mode=='history':
  allowed={'R05_HANDOFF.json':{'generated_by','generated_at','rr3_current','historical_record_notice','unresolved_items','allowed_next_scope','artifact_hashes','working_tree_digest_scope','review_status'},'R05_TEST_MAP.json':{'rr3_current','historical_record_notice','rr3_I10_mapping'},'R05_PERFORMANCE_RESULTS.json':{'rr3_current','historical_record_notice','rr3_resource_selfcheck'},'R05_LIVE_VERIFICATION.json':{'rr3_current','historical_record_notice','rr3_platform_verification'}}
  diffs={}
  for p in OWNED:
   oldpath=E/'before'/p
   if p.endswith('.json') and 'ORCHESTRATOR' not in p:
    old=strict(oldpath);new=strict(ROOT/p);changes=[k for k,v in old.items() if new.get(k)!=v];diffs[p]=changes
    check(set(changes)<=allowed.get(Path(p).name,{'rr3_current','historical_record_notice'}),'only planned current fields '+p)
    check(new['rr3_current_history'][0]['snapshot']==old['rr3_current'],'E01 current snapshot retained historical '+p)
    for k in old:
     if k not in changes:check(new[k]==old[k],'full historical field equal '+p+' '+k)
    if Path(p).name in ['PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json']:
     prefix=oldpath.read_text().split('  "historical_record_notice":',1)[0];check((ROOT/p).read_text().startswith(prefix),'original large ledger byte prefix '+p)
  p='docs/rust-tauri/ORCHESTRATOR_PROGRESS.json';old=strict(E/'before'/p);new=strict(ROOT/p)
  check(old['tasks']==new['tasks'],'ORCH all task records preserved')
  check({k:v for k,v in old.items() if k not in ['stages','current_task']}=={k:v for k,v in new.items() if k not in ['stages','current_task']},'ORCH all other root fields preserved')
  check({k:v for k,v in old['stages'].items() if k!='R05'}=={k:v for k,v in new['stages'].items() if k!='R05'},'ORCH non-R05 stages preserved')
  check(new['stages']['R05']['rr3_current_history'][0]['snapshot']==old['stages']['R05']['rr3_current'],'ORCH E01 historical snapshot preserved')
  for n,marker in [('R05_REPORT.md','<a id="rr3-current"></a>'),('R05_BLOCKERS.md','## 8. RR3'),('R05_INDEPENDENT_REVIEW.md','## RR3 文档实施注记'),('R05_NEGATIVE_GATE_REPORT.md','## RR3 当前負测')]:
   marker='## RR3 当前负测' if n=='R05_NEGATIVE_GATE_REPORT.md' else marker
   old=(E/'before'/'docs/rust-tauri/R05'/n).read_text();new=(D/n).read_text();history=old.split(marker,1)[0].split('\n\n',2)[2]
   if n=='R05_REPORT.md':history=history.split('\n',1)[1]
   check(history in new,'old Markdown historical body verbatim '+n)
  check(sha(D/'R05_INTERFACE_EVOLUTION.md')==load(E/'before.json')['owned']['docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md'],'interface evolution unchanged')
  h=strict(D/'R05_HANDOFF.json');old=strict(E/'before'/'docs/rust-tauri/R05/R05_HANDOFF.json');check(h['consumer_contract']==old['consumer_contract'],'6.2 all interface fields/sample/error/unknown/cancel/recovery/budget preserved')
  authority={k:v for k,v in load(E/'protected-before.json').items() if '/repair-current/' not in k};check(all(sha(ROOT/p)==v for p,v in authority.items()),'all authority scope/pins/CIDs/leaf unchanged')
  external=[{'path':p,'before':v,'after':sha(ROOT/p)} for p,v in load(E/'protected-before.json').items() if '/repair-current/' in p and sha(ROOT/p)!=v]
  dump(E/'history-diff.json',{'changedCurrentFields':diffs,'externalOwnerChangesObserved':external,'scope':'总控并发变化只观察；本E不写，不将其当全树静默。'})
 elif mode=='hashes':
  h=strict(D/'R05_HANDOFF.json')
  for p,v in h['artifact_hashes'].items():check(sha(ROOT/p)==v,'artifact SHA '+p)
  for p,v in h['dependency_locks'].items():check(sha(ROOT/p)==v['sha256'],'lock SHA '+p)
  for folder,record in load(E/'manifest-consumption.json').items():
   check(sha(ROOT/record['manifest'])==record['manifestSha256'],'immutable manifest '+folder)
   for entry in record['checked']:check(sha(ROOT/entry['path'])==entry['sha256'],'consumed raw unchanged '+entry['path'])
  before=load(E/'inputhash-before.json');after=source();check(before['files']==after['files'] and before['digest']==after['digest'],'423 scoped source production inputs unchanged')
  check(h['working_tree_digest']==before['digest'],'handoff scoped digest actual')
  dump(E/'inputhash-after.json',after)
 elif mode=='links':
  warnings=[]
  for rel in OWNED:
   if not rel.endswith('.md'):continue
   old=(E/'before'/rel).read_text();new=(ROOT/rel).read_text();oldlinks=set(re.findall(r'\[[^\]]*\]\(([^)]+)\)',old))
   for target in re.findall(r'\[[^\]]*\]\(([^)]+)\)',new):
    if target.startswith(('http:','https:','mailto:')):continue
    clean=urllib.parse.unquote(target);path,_,anchor=clean.partition('#');dest=((ROOT/rel).parent/path).resolve() if path else ROOT/rel;good=dest.exists()
    if good and anchor and dest.suffix=='.md':
     content=dest.read_text();good=f'id="{anchor}"' in content or any(re.sub(r'[^\w\- ]','',line.lstrip('#').strip()).lower().replace(' ','-')==anchor for line in content.splitlines() if line.startswith('#'))
    if target in oldlinks and not good:warnings.append({'file':rel,'link':target,'status':'historical link warning'})
    else:check(good,'Markdown link '+rel+' -> '+target)
  h=strict(D/'R05_HANDOFF.json')
  for s in strings(h['consumer_contract']):
   if s.startswith(('rust/','artifacts/','docs/')) and '\n' not in s:check((ROOT/s.split('::',1)[0]).is_file(),'consumer source link '+s.split('::',1)[0])
  for v in h['rr3_current']['evidence_refs'].values():check((ROOT/v).is_file(),'current evidence link '+v)
  dump(E/'link-warnings.json',warnings)
 elif mode=='current-fixture':current_assert(strict(Path(sys.argv[2])))
 elif mode=='wire-fixture':wire_assert(Path(sys.argv[2]).read_text())
 else:raise ValueError(mode)
 print(json.dumps({'mode':mode,'status':'SELF_CHECKED','assertions':len(checks),'boundary':'文档自检；非独立PASS，未运行Cargo/worker/FINAL'},ensure_ascii=False))
 if mode not in ['current-fixture','wire-fixture']:dump(E/(mode+'-checks.json'),checks)
except Exception as ex:
 print(json.dumps({'mode':mode,'status':'FAIL','error':str(ex),'assertionsReached':len(checks)},ensure_ascii=False));raise
