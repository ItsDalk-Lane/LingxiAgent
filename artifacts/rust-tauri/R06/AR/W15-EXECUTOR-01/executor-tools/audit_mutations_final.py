import json, copy, subprocess,sys,hashlib
from pathlib import Path
base=Path(__file__).parent;root=base/'own.git';out=base/'evidence/audit-mutation-inputs-final';out.mkdir(exist_ok=False)
records=[json.loads(x) for x in (base/'evidence/final-be-battery-03/evaluation.jsonl').read_text().splitlines()]
scope=json.loads((root/'artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01/SCOPE_RECONCILIATION.json').read_text());cases=[]
def add(name,mutate,point):
 r=copy.deepcopy(records);s=copy.deepcopy(scope);mutate(r,s);cases.append((name,r,s,point))
add('false-zero',lambda r,s:next(x for x in r if x['taskId']=='BE-04')['tokens'].update(inputTokens=0),'inputTokens')
add('missing-as-zero',lambda r,s:next(x for x in r if x['taskId']=='BE-06')['tokens'].update(inputTokens=0),'BE-06')
add('remove-negative',lambda r,s:r.__setitem__(slice(None),[x for x in r if x['taskId']!='BE-10']),'BE-10')
add('echo-as-full-leaf',lambda r,s:s['leaves'][0].update(status='PASS',actual_positive_negative={'mechanismEcho':'ok'}),scope['leaves'][0]['id'])
add('missing-as-deferred',lambda r,s:s['leaves'][1].update(status='DEFERRED_R07'),scope['leaves'][1]['id'])
add('deadline-moved',lambda r,s:s['obligations'][0].update(deadline='R10'),scope['obligations'][0]['id'])
add('a15-echo-binding',lambda r,s:s['acceptance_bindings'].update({'R06-A15':['BE-09']}),'A15/A16')
add('stale-candidate',lambda r,s:s['candidate_sources'].update({'rust/crates/lingxi-service/tests/r06_t08_behavior_eval.rs':'0'*64}),'候选漂移')
add('false-stage-accepted',lambda r,s:s.update(current_stage_status='R06_ACCEPTED'),'当前放行状态')
add('semantic-negative-as-pass',lambda r,s:next(x for x in r if x['taskId']=='BE-10')['score'].update(semanticParamsCorrect=True,taskComplete=True,overall='pass'),'R06-A15')
summary=[]
for name,rs,sc,point in cases:
 rp=out/(name+'-evaluation.jsonl');sp=out/(name+'-scope.json');rp.write_text(''.join(json.dumps(x,ensure_ascii=False)+'\n' for x in rs));sp.write_text(json.dumps(sc,ensure_ascii=False))
 cmd=[sys.executable,str(base/'guard_run_v2.py'),'audit-final-mutant-'+name,sys.executable,'scripts/rust-tauri/r06_evaluation_audit.py','--records',str(rp),'--facts',str(base/'evidence/final-be-battery-03/raw/facts.jsonl'),'--obligations','docs/rust-tauri/R06/repair-current/AR_ORIGINAL_OBLIGATIONS.json','--scope',str(sp),'--root','.']
 proc=subprocess.run(cmd,cwd=root,timeout=600,capture_output=True,text=True)
 message=(base/'evidence'/('audit-final-mutant-'+name)/'stdout.log').read_text()
 ok=proc.returncode==1 and point in message
 summary.append(dict(name=name,exit_code=proc.returncode,expected_point=point,actual_message=message.strip(),rejected_as_expected=ok,input_sha256=hashlib.sha256(sp.read_bytes()).hexdigest(),records_sha256=hashlib.sha256(rp.read_bytes()).hexdigest()))
 print(name,proc.returncode,ok,flush=True)
(out/'results.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
raise SystemExit(0 if all(x['rejected_as_expected'] for x in summary) else 1)
