import collections, copy, difflib, gzip, json, pathlib, re, sys
from audit import ROOT, EV, D, G, H, DOCS, load, sha, save, atom, entry, snapshot, compare_g, compare_h, strict_pairs, now

checks=[]
def ck(name,ok,detail=None):
    checks.append(dict(name=name,passed=bool(ok),detail=detail))
    if not ok:print('FAIL',name,detail)
before=json.loads(gzip.decompress((EV/'before-documents.json.gz').read_bytes()))
current=load(EV/'current-state.json'); hand=load(D/'R05_HANDOFF.json')
for path in DOCS:
    if path.endswith('.json'):
        value=json.loads((ROOT/path).read_text(),object_pairs_hook=strict_pairs)
        c=value['stages']['R05']['rr3_current'] if path.endswith('ORCHESTRATOR_PROGRESS.json') else value['rr3_current']
        ck('same current '+path,c==current)
        ck('strict JSON '+path,True)
try:json.loads('{"a":{"x":1,"x":2}}',object_pairs_hook=strict_pairs)
except ValueError:ck('nested duplicate keys refused',True)
else:ck('nested duplicate keys refused',False)
ck('R05 false/accepted empty',current['stage_readiness']=='NOT_ACCEPTED' and current['R06_READY'] is False and hand['accepted_tasks']==[])
ck('all closed packages consume actual reports',all(current['packages'][k]['status']=='CLOSED' and current['packages'][k]['independent_review']=='PASS' for k in ['A','B','C_F46','H','I','J']))
ck('E03 only selfcheck and historical E2 closed',current['packages']['E']['round']==3 and current['packages']['E']['independent_review']=='PENDING' and current['packages']['E']['historical_reviews'][-1]['closed']==['MF-E01','MF-E02'])
g=current['packages']['G'];ck('G02 exact incomplete boundary',g['status']=='BLOCKED_BY_STORAGE' and g['default_shell_exit']=='UNKNOWN' and g['N02']['target_reached'] is False and g['unexecuted_cases']==[f'N{x:02d}' for x in range(3,17)] and g['full_R02_runs']==0)
ck('FINAL actual absent',current['packages']['FINAL']['result_ref'] is None and current['packages']['FINAL']['tested_sha'] is None and not (ROOT/'artifacts/rust-tauri/R05/RR3/FINAL-01').exists())
ck('old D object not current',current['packages']['D']['current_final_binary_identity'] is None and '历史' in current['packages']['D']['binary_identity']['identity_scope'])
ck('I01-I11 boundary exact',all(current['I01_I11'][f'I{i:02d}']['status']=='PENDING_CURRENT_COMBINATION' for i in range(1,10)) and current['I01_I11']['I10']['status']=='PASS_LIMITED_REUSE' and current['I01_I11']['I11']['status']=='INCOMPLETE')
ck('Git genuinely not performed',hand['git_delivery_receipt_contract']['committed_sha'] is None and hand['git_delivery_receipt_contract']['receipt_ref'] is None and hand['evidence_delivery_boundary']['final_inventory_ref'] is None)
ck('four platform axes',all(k in load(D/'R05_LIVE_VERIFICATION.json')['rr3_platform_verification'] for k in ['macOS arm64','macOS x64','Windows x64','Linux x64']))

# 原账本与接口保留，精确到旧顶层字段，较大字段还核原字节前缀。
allowed={'generated_by','generated_at','historical_record_notice','rr3_current','rr3_current_history','review_status','source_sha','source_sha_semantics','working_tree_digest','working_tree_digest_scope','unresolved_items','allowed_next_scope','artifact_hashes','consumer_contract','rr3_I10_mapping','rr3_resource_independent_review','rr3_platform_verification'}
for path in DOCS:
    if not path.endswith('.json') or path.endswith('ORCHESTRATOR_PROGRESS.json'):continue
    old=json.loads(before[path]);new=load(ROOT/path)
    for key,val in old.items():
        if key not in allowed:ck('historical field '+path+'/'+key,new.get(key)==val)
    ck('E02 current retained '+path,new['rr3_current_history'][-1]['snapshot']==old['rr3_current'])
    ck('earlier current history retained '+path,new['rr3_current_history'][:-1]==old['rr3_current_history'])
for name in ['PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json']:
    p='docs/rust-tauri/R05/'+name; marker='  "historical_record_notice":';ck('large ledger original byte prefix '+name,before[p].split(marker)[0]==(ROOT/p).read_text().split(marker)[0])
op='docs/rust-tauri/ORCHESTRATOR_PROGRESS.json';old=json.loads(before[op]);new=load(ROOT/op)
ck('other stages untouched',all(v==new['stages'][k] for k,v in old['stages'].items() if k!='R05'))
ck('all tasks untouched',old['tasks']==new['tasks'])
ck('other orchestration roots untouched',all(v==new[k] for k,v in old.items() if k not in ['stages','current_task']))
oldc=json.loads(before['docs/rust-tauri/R05/R05_HANDOFF.json'])['consumer_contract'];newc=copy.deepcopy(hand['consumer_contract']);newc['error_unknown_cancel_recovery_budget']['ordinary_recovery_refs']=oldc['error_unknown_cancel_recovery_budget']['ordinary_recovery_refs'];ck('consumer contract only two current evidence refs changed',oldc==newc)
for name in ['WORKER_MODEL_BOUNDARY.md','R05_INTERFACE_EVOLUTION.md']:
    p='docs/rust-tauri/R05/'+name;ck('unneeded interface unchanged '+name,before[p]==(ROOT/p).read_text())
for name in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md','MODEL_USAGE_SEMANTICS.md']:
    path='docs/rust-tauri/R05/'+name;old=before[path].splitlines();new=(ROOT/path).read_text().splitlines();i=0;missing=[]
    for line in old:
        if line.startswith(('# R05_REPORT —','> **RR3 当前（','- 生成：R05-T08-执行者','## 11. RR3 当前','## RR3 文档实施','## 8. RR3 当前','## RR3 当前负测','## 11. RR3 消费')) or '<a id="rr3-current">' in line:continue
        try:i=new.index(line,i)+1
        except ValueError:missing.append(line)
    ck('historical Markdown line order '+name,not missing,missing)
for name in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md']:
    banner=(D/name).read_text().splitlines()[2];ck('current banner '+name,'BLOCKED' in banner and 'G RUNNING' not in banner and 'E-03' in banner)

# 真实源码字段与普通调用；不执行旧作者检查器或Cargo样例。
def struct(path,name):
    s=(ROOT/path).read_text();m=re.search(r'pub (?:struct|enum) '+name+r'(?:<[^\n]+>)?\s*\{',s);assert m,name
    start=m.end();depth=1;i=start
    while depth:
        if s[i]=='{':depth+=1
        elif s[i]=='}':depth-=1
        i+=1
    body=s[start:i-1];return re.findall(r'^\s*pub\s+(\w+)\s*:',body,re.M),body
c=hand['consumer_contract']
cases=[('gateway','request_fields','rust/crates/lingxi-kernel/src/model_exchange.rs','ModelRouteRequest'),('context','fields','rust/crates/lingxi-kernel/src/lib.rs','RunContext'),('model_turn','fields','rust/crates/lingxi-kernel/src/model_exchange.rs','ModelTurnInput'),('auxiliary','request_fields','rust/crates/lingxi-adapters/src/models/auxiliary.rs','AuxiliaryRequest'),('embedding','request_fields','rust/crates/lingxi-adapters/src/models/operations/embedding.rs','EmbeddingRequest'),('embedding','context_fields','rust/crates/lingxi-service/src/operations.rs','OperationCallContext'),('usage_query','fields','rust/crates/lingxi-kernel/src/usage.rs','ModelUsageQuery')]
for group,key,p,name in cases:ck('actual complete field order '+name,struct(p,name)[0]==c[group][key])
_,ex=struct('rust/crates/lingxi-kernel/src/model_exchange.rs','ExchangeItem')
for name,key in [('AssistantTurn','assistant_fields'),('ToolResult','tool_result_fields')]:
    body=re.search(name+r'\s*\{(.*?)\n\s*\}',ex,re.S).group(1);ck('exchange fields '+name,re.findall(r'^\s*(\w+)\s*:',body,re.M)==c['exchange_and_messages'][key])
worker=(D/'WORKER_MODEL_BOUNDARY.md').read_text();wire=json.loads(re.search(r'```json\s*(.*?)\s*```',worker,re.S).group(1));fields,_=struct('rust/crates/lingxi-service/src/workerrpc.rs','WorkerCallbackLine');ck('worker six actual fields',list(wire)==fields and wire['kind']=='callback' and wire['op']=='model.complete')
rpc=(ROOT/'rust/crates/lingxi-service/src/workerrpc.rs').read_text();ck('worker kind actual dispatch','Some("callback") =>' in rpc)
migrations=(ROOT/'rust/crates/lingxi-adapters/src/storage/migrations.rs').read_text();ck('storage schema7',max(map(int,re.findall(r'version:\s*(\d+)',migrations)))==hand['storage_schema_version']==7)
ck('wire event epoch axes',hand['data_epoch']==1 and hand['protocol_version']['min_supported']==hand['protocol_version']['max_supported']==1)
ops=(ROOT/c['embedding']['source']).read_text()
for name in ['embed','rerank']:
    signature=re.search(r'pub async fn '+name+r'\((.*?)\) -> ([^{]+)',ops,re.S).group(0);ck('operation context actual '+name,'context: Option<&OperationCallContext>' in signature and 'deadline_unix_ms: Option<u64>' in signature)
sample=(ROOT/'rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs').read_text();start=sample.index('async fn rr1_f21_operation_context_carries_session_run_and_cause');sample=sample[start:sample.find('#[tokio::test]',start)];ck('real normal embedding sample',all(x in sample for x in ['"context-bound input"','dimensions: Some(2)','[0.25,0.75]','"prompt_tokens":5','"total_tokens":9','Some(&context)','"run-rr1-ctx-tc0007"']))
ck('unknown output preserved','不能用total9减5猜4' in c['embedding']['normal_processing'])
ck('callback trace real wiring','LedgerWorkerCallbackTrace::new' in (ROOT/'rust/crates/lingxi-service/src/lib.rs').read_text())
ck('mandatory consumer keys',all(k in hand for k in ['source_sha','working_tree_digest','protocol_version','data_epoch','dependency_locks','accepted_tasks','unresolved_items','allowed_next_scope','artifact_hashes']))
ck('all lifecycle descriptions',all(k in c['error_unknown_cancel_recovery_budget'] for k in ['error','unknown','cancel','ordinary_recovery_refs','restart_recovery','budget']))

# 从H实际序列独立重算，不把作者汇总当采样本身。
perf=load(D/'R05_PERFORMANCE_RESULTS.json')['rr3_resource_independent_review'];raw=load(ROOT/perf['raw_series']);a=load(H/'RESOURCE_ANALYSIS-resources-final.json');series=raw['series'];owners=raw['ownerResourceSeries'];cycles=raw['cycleResults']
ck('raw exact SHA',sha((ROOT/perf['raw_series']).read_bytes())==perf['raw_sha256']==a['rawSha256'])
ck('all original counts',len(series)==54 and len(owners)==61 and len(cycles)==160)
ck('original distribution',dict(collections.Counter(x['phase'] for x in cycles))=={'cancel':60,'error':60,'ok':15,'long':10,'worker':15})
ck('original thresholds',raw['thresholds']==perf['original_thresholds']==dict(fdBound=400,fdGrowthBound=64,logFilesBound=3,registeredBeforeRun=True,rssBoundKiB=409600,rssGrowthBoundKiB=153600))
def ranges(rows,key):return [min(x[key] for x in rows),max(x[key] for x in rows)]
trees=[x['serviceTree'] for x in series];otrees=[x['processTree'] for x in owners]
for key in ['rssKiB','fds']:
    ck('service ranges '+key,ranges(series,key)==perf['ranges'][key]);ck('tree ranges '+key,ranges(trees,key)==perf['tree_ranges'][key]);ck('owner ranges '+key,ranges(otrees,key)==perf['owner_tree_ranges'][key])
for tree in trees+otrees:
    ck('tree sums PID '+str(tree['rootPid']),all(sum(p[k] for p in tree['processes'])==tree[k] for k in ['rssKiB','fds']) and sum(len(p['establishedTcp']) for p in tree['processes'])==tree['establishedTcpCount'])
ck('15 live workers',all(len(x['serviceTree']['processes'])==2 and x['serviceTree']['establishedTcpCount']>=2 for x in series if x['phase']=='worker-live') and sum(x['phase']=='worker-live' for x in series)==15)
ck('15 released workers',all(len(x['serviceTree']['processes'])==1 and x['serviceTree']['establishedTcpCount']==0 for x in series if x['phase']=='worker-released') and sum(x['phase']=='worker-released' for x in series)==15)
steady=[x for x in owners if x['phase']=='released-steady'];ck('45 owner steady zero',len(steady)==45 and all(all(v==0 or v==[] for v in x['owners'].values()) for x in steady))
ck('all 115 log limits',all(sum(z['path'].startswith('lingxi-service/logs/service-') for z in x['files']['home'])<=3 for x in series+owners))
ck('steady tmp clear',all(not z['path'].endswith('.tmp') for x in series+owners for items in x['files'].values() for z in items))
ck('budget restart separate',raw['load']['cancelDanglingActive']==60 and raw['load']['cancelSettled']==0 and all(x['httpStatus']==408 and x['providerConnectionReclaimed'] for x in cycles if x['phase']=='cancel'))
ck('cleanup preserved',perf['cleanup']==raw['cleanup'] and all(raw['cleanup'][k] for k in ['homeRemoved','workspaceRemoved','configRemoved','serviceStopped','equipmentStubStopped']) and raw['cleanup']['lastServiceExit']==0)
negative=load(ROOT/perf['negative_control']);ck('real sampler control sequence',[x['exitCode'] for x in negative['rows']]==[0,101,101,0] and all(x['targetNamed'] and x['copyHash']==perf['equipment_sha256'] for x in negative['rows']))
for p,filtered in zip(perf['ordinary_cancel_receipts'],[7,4]):
    r=load(ROOT/p);ck('ordinary actual receipt '+p,r['exit']==0 and any(f'1 passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out' in c for c in r['counts']))
r=load(ROOT/perf['receipt']);ck('resource actual command',r['exit']==0 and '2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 336.02s' in r['counts'][0]);bind=load(ROOT/perf['runtime_binding_ref']);ck('actual runtime binary',any(x.get('binarySha256')==perf['service_sha256'] for x in bind['rows']))

# 当前所引用的小证据及工具输入按逐项SHA保存；不复制binary或大证据树。
refs=set(hand['artifact_hashes'])
for p,want in hand['artifact_hashes'].items():ck('artifact hash '+p,sha((ROOT/p).read_bytes())==want)
for p,v in hand['dependency_locks'].items():ck('dependency lock '+p,sha((ROOT/p).read_bytes())==v['sha256'])
for p in current['evidence_refs'].values():
    if p and not p.startswith('artifacts/rust-tauri/R05/RR3/E-03/'):refs.add(p)
for p in [perf['receipt'],perf['raw_series'],perf['runtime_binding_ref'],perf['negative_control'],*perf['ordinary_cancel_receipts']]:refs.add(p)
for base in ['H-REVIEW-02','I-REVIEW-01','J-02','J-REVIEW-02','DOC-INPUT-BOUNDARY-01','DELIVERY-PREP-02','STORAGE-03']:
    for p in (ROOT/'artifacts/rust-tauri/R05/RR3'/base).glob('*.py'):refs.add(p.relative_to(ROOT).as_posix())
for p in (G/'metadata').glob('*.py'):refs.add(p.relative_to(ROOT).as_posix())
for row in load(G/'metadata/authority-files.json')['files']:
    if 'specifications/' in row['path']:refs.add(row['path']);ck('original authority unchanged '+row['path'],sha((ROOT/row['path']).read_bytes())==row['sha256'])
save('read-inputs.json',dict(at=now(),files=[entry(ROOT/p) for p in sorted(refs)],scope='Actual report/raw/authority/source-driver bytes read for document consumption. Historical drivers were not executed. Not a complete rehash of old giant manifests.'))
storage=load(ROOT/'artifacts/rust-tauri/R05/RR3/STORAGE-03/MANIFEST.json');ck('storage formal archive every file',all(sha((ROOT/'artifacts/rust-tauri/R05/RR3/STORAGE-03'/x['path']).read_bytes())==x['sha256'] for x in storage['files']))
for name in ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_NEGATIVE_GATE_REPORT.md','MODEL_USAGE_SEMANTICS.md','WORKER_MODEL_BOUNDARY.md']:
    p=D/name;s=p.read_text();bad=[]
    for target in re.findall(r'\]\(([^)]+)\)',s):
        if target.startswith(('https:','http:','mailto:')):continue
        target=target.strip('<>');bare=target.split('#')[0]
        if bare and not (p.parent/bare).exists():bad.append(target)
        if target.endswith('#rr3-current'):ck('current explicit anchor '+name,'<a id="rr3-current"></a>' in (p.parent/bare).read_text())
    ck('local markdown links '+name,not bad,bad)

after=snapshot();first=load(EV/'semantic-inputs-before.json');save('semantic-inputs-after.json',after)
ck('semantic paths and each byte mode equal',first['files']==after['files'])
ck('semantic digest same',first['digest']==after['digest']==hand['working_tree_digest'])
hh=compare_h();save('h02-current-after-comparison.json',hh);ck('H375 current equals tested actual input',hh['all_equal'] and hh['count']==375)
gg=compare_g();save('g02-current-after-comparison.json',gg)
changed=[p for p in DOCS if sha(before[p].encode())!=sha((ROOT/p).read_bytes())]
save('documents-after.json',[entry(ROOT/p) for p in DOCS]);save('changed-files.json',dict(count=len(changed),paths=changed,unchanged=[p for p in DOCS if p not in changed]))
diff=''.join(''.join(difflib.unified_diff(before[p].splitlines(True),(ROOT/p).read_text().splitlines(True),fromfile='before/'+p,tofile='after/'+p)) for p in changed);atom(EV/'current-documents.diff',diff.encode())
save('SELF_CHECK.json',dict(at=now(),status='PASS' if all(x['passed'] for x in checks) else 'FAIL',kind='AUTHOR_DOCUMENT_SELF_CHECK_NOT_INDEPENDENT_REVIEW',assertions=len(checks),failed=[x for x in checks if not x['passed']],checks=checks,semantic_count=after['count'],semantic_digest=after['digest'],documents_changed=len(changed),g_inventory_changes=len(gg['changed']),whole_candidate_equal=False))
print('document assertions',len(checks),'failures',sum(not x['passed'] for x in checks),'changed docs',len(changed),'protected inputs',after['count'],'G inventory changed',len(gg['changed']))
sys.exit(0 if all(x['passed'] for x in checks) else 1)
