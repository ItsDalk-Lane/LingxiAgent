import pathlib,json,sqlite3,hashlib,subprocess,sys,copy
ROOT=pathlib.Path('/private/tmp/lingxi-r06-w15-reviewer-01');OUT=ROOT/'artifacts/rust-tauri/R06/AR/W15-REVIEWER-01';WINDOW=OUT/'windows/independent-be-battery-02';DEST=pathlib.Path(__import__('os').environ['R06_T08_EVAL_OUT']).parent
records=[json.loads(x) for x in (WINDOW/'evaluation.jsonl').read_text().splitlines()];facts=[json.loads(x) for x in (WINDOW/'raw/facts.jsonl').read_text().splitlines()];scope=json.loads((ROOT/'artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01/SCOPE_RECONCILIATION.json').read_text());original=ROOT/'docs/rust-tauri/R06/repair-current/AR_ORIGINAL_OBLIGATIONS.json'
# 独立从实际生产库读取；不以测试发出的简表替代数据库。
readbacks=[]
for fact in facts:
 if fact['kind']!='usage_sql_readback':continue
 v=fact['value'];path=pathlib.Path(v['databaseHome'])/'lingxi-service/data/runs.db'
 with sqlite3.connect('file:'+str(path)+'?mode=ro',uri=True) as db:
  db.row_factory=sqlite3.Row;actual=[dict(x) for x in db.execute(v['query'],(v['runId'],))]
 readbacks.append({'home':v['databaseHome'],'runId':v['runId'],'query':v['query'],'actualRows':actual,'equalToCapturedFullSql':actual==v['rows'],'dbSha256AfterServiceExit':hashlib.sha256(path.read_bytes()).hexdigest()})
assert all(x['equalToCapturedFullSql'] for x in readbacks)
files=[]
for fact in facts:
 if fact['kind']!='world_final':continue
 v=fact['value'];ws=pathlib.Path(v['workspace']);actual=sorted((str(p.relative_to(ws)),hashlib.sha256(p.read_bytes()).hexdigest()) for p in ws.rglob('*') if p.is_file());expected=[tuple(x) for x in v['workspaceListing']];files.append({'workspace':str(ws),'actual':actual,'equalToCapture':actual==expected})
assert all(x['equalToCapture'] for x in files)
be04=next(x for x in records if x['taskId']=='BE-04');recomputed={'input':sum(x['inputTokens'] for x in be04['tokens']['ledgerRows']),'output':sum(x['outputTokens'] for x in be04['tokens']['ledgerRows']),'byOutcome':{}}
for outcome in set(x['outcome'] for x in be04['tokens']['ledgerRows']):
 rows=[x for x in be04['tokens']['ledgerRows'] if x['outcome']==outcome];recomputed['byOutcome'][outcome]={'rows':len(rows),'input':sum(x['inputTokens'] for x in rows),'output':sum(x['outputTokens'] for x in rows)}
cases=[]
def add(name,fn,label,should_reject=True):
 r=copy.deepcopy(records);s=copy.deepcopy(scope);f=copy.deepcopy(facts);fn(r,s,f);cases.append((name,r,s,f,label,should_reject))
def rec(r,id):return next(x for x in r if x['taskId']==id)
add('c03-echo-full-business',lambda r,s,f:s['obligations'][0].update(status='PASS',runtime_evidence=['echo-ok']),'R06-T01-S01')
add('c03-missing-deferred',lambda r,s,f:s['leaves'][0].update(status='DEFERRED_R07'),scope['leaves'][0]['id'])
add('c03-negative-deleted',lambda r,s,f:r.remove(rec(r,'BE-10')),'BE-10')
add('c03-a15-handshake',lambda r,s,f:s['acceptance_bindings'].update({'R06-A15':['handshake']}),'A15/A16')
add('metrics-false-zero',lambda r,s,f:rec(r,'BE-04')['tokens'].update(inputTokens=0),'BE-04')
add('metrics-be04-home-forgery',lambda r,s,f:rec(r,'BE-04')['tokens']['ledgerRows'][1].update(databaseHome=rec(r,'BE-01')['tokens']['ledgerRows'][0]['databaseHome']),'BE-04')
add('metrics-negative-score-pass',lambda r,s,f:rec(r,'BE-10')['score'].update(semanticParamsCorrect=True,taskComplete=True,overall='pass'),'R06-A15')
# 自提同族反例：真实文件否定完成事实时仍写全维PASS。
add('own-be01-file-false-with-pass',lambda r,s,f:rec(r,'BE-01')['observedBehavior'].update(fileBytesMatch=False),'BE-01',True)
add('own-be03-rejection-effect-false',lambda r,s,f:rec(r,'BE-03')['observedBehavior'].update(nestedEditApplied=False,unknownParamDispatched=True),'BE-03',True)
add('own-be01-expected-calls-999',lambda r,s,f:rec(r,'BE-01').update(modelCallsExpected=999,extraModelCalls=0),'BE-01',True)
add('own-coverage-status-false',lambda r,s,f:rec(r,'BE-01')['tokens']['inputCoverage'].update(measurementStatus='NOT_MEASURED',rowCount=999),'BE-01',True)
add('own-outcome-coverage-false',lambda r,s,f:rec(r,'BE-04')['tokens']['byLedgerOutcome']['failed']['input'].update(knownRows=999,missingRows=999,measurementStatus='MEASURED'),'BE-04',True)
add('own-be07-full-chat-claim',lambda r,s,f:rec(r,'BE-07').update(originalRequirementStatus='FULL_CHAT_SELECTION_PASS'),'BE-07',True)
add('own-be04-session-forgery',lambda r,s,f:next(x['value'] for x in f if x['kind']=='run_accepted' and 'BE04B' in x['value']['input']).update(sessionId='forged-unrelated-session'),'BE-04',True)
add('own-candidate-bindings-deleted',lambda r,s,f:s.update(candidate_sources={}),'候选',True)
add('own-comparison-mode-live-with-false-flags',lambda r,s,f:rec(r,'BE-01').update(comparisonMode='REAL_LIVE_PROVIDER_VERIFIED'),'BE-01',True)
add('own-coverage-invented-outcome',lambda r,s,f:rec(r,'BE-04')['tokens']['byLedgerOutcome'].update({'invented_success':{'rowCount':999,'input':{'total':999},'output':{'total':999}}}),'BE-04',True)
results=[]
for name,r,s,f,label,reject in cases:
 d=DEST/'mutants'/name;d.mkdir(parents=True);rp=d/'records.jsonl';sp=d/'scope.json';fp=d/'facts.jsonl';rp.write_text(''.join(json.dumps(x,ensure_ascii=False)+'\n' for x in r));sp.write_text(json.dumps(s,ensure_ascii=False));fp.write_text(''.join(json.dumps(x,ensure_ascii=False)+'\n' for x in f))
 argv=[sys.executable,str(ROOT/'scripts/rust-tauri/r06_evaluation_audit.py'),'--records',str(rp),'--facts',str(fp),'--obligations',str(original),'--scope',str(sp),'--root',str(ROOT)];p=subprocess.run(argv,capture_output=True,text=True,timeout=600);(d/'stdout.log').write_text(p.stdout);(d/'stderr.log').write_text(p.stderr);results.append({'id':name,'argv':argv,'exitCode':p.returncode,'stdout':p.stdout.strip(),'expectedReject':reject,'pointExpected':label,'rejectedAndNamed':p.returncode!=0 and label in p.stdout,'unsafeAccepted':reject and p.returncode==0})
# 恢复原始输入后正控必须再次通过。
d=DEST/'restored-positive';d.mkdir();rp=d/'records.jsonl';sp=d/'scope.json';fp=d/'facts.jsonl';rp.write_text((WINDOW/'evaluation.jsonl').read_text());sp.write_text(json.dumps(scope));fp.write_text((WINDOW/'raw/facts.jsonl').read_text());p=subprocess.run([sys.executable,str(ROOT/'scripts/rust-tauri/r06_evaluation_audit.py'),'--records',str(rp),'--facts',str(fp),'--obligations',str(original),'--scope',str(sp),'--root',str(ROOT)],capture_output=True,text=True,timeout=600);assert p.returncode==0
report={'actualSqlReadbacks':readbacks,'actualWorkspaceReadbacks':files,'BE04Recomputed':recomputed,'mutations':results,'restoredPositiveExitCode':p.returncode,'restoredPositiveOutput':p.stdout,'newCounterexampleFamilies':['false semantic/file positive','invented expected call budget','coverage metadata','BE07 scope share promoted to full']};(DEST/'REVIEW.json').write_text(json.dumps(report,ensure_ascii=False,indent=2));print(json.dumps({'BE04':recomputed,'mutations':[(x['id'],x['exitCode']) for x in results],'unsafeAccepted':[x['id'] for x in results if x['unsafeAccepted']]},ensure_ascii=False));sys.exit(1 if any(x['unsafeAccepted'] for x in results) else 0)
