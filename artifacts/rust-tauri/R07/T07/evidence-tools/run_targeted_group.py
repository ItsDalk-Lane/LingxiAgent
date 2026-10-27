#!/usr/bin/env python3
"""运行已声明的受影响范围；不得冒充全阶段门禁。"""
import hashlib,json,pathlib,subprocess,sys
root=pathlib.Path(__file__).resolve().parents[5]
p=pathlib.Path(__file__).resolve().parents[1]
name=sys.argv[1];plan_path=p/f'TARGETED_GATE_PLAN_{name}.json'
plan=json.loads(plan_path.read_text());results={}
for label,command in plan['gates'].items():
 result=subprocess.run([sys.executable,str(p/'evidence-tools/run_candidate.py'),'run',name,label,'--',*command],cwd=root,capture_output=True,text=True)
 receipt=p/f'{name}-{label}.json'
 results[label]={'wrapperExitCode':result.returncode,'productCommandExecuted':receipt.exists(),'receipt':receipt.name if receipt.exists() else None}
 if not receipt.exists():
  error=p/f'{name}-{label}-precheck.json'
  with error.open('x') as f:json.dump({'candidate':name,'label':label,'status':'NOT_RUN_PRECHECK_REJECTED','productCommandExecuted':False,'wrapperExitCode':result.returncode,'stdout':result.stdout,'stderr':result.stderr,'requestedProductCommand':command},f,ensure_ascii=False,indent=2)
 print(json.dumps({'gate':label,**results[label]}),flush=True)
 if result.returncode==99 or not receipt.exists():break
summary={'candidate':name,'scope':'TARGETED_REGRESSION_NOT_FINAL_R07','plan':plan_path.name,'planSha256':hashlib.sha256(plan_path.read_bytes()).hexdigest(),'results':results,'fullStagePassClaimed':False}
with (p/f'{name}-targeted-batch.json').open('x') as f:json.dump(summary,f,ensure_ascii=False,indent=2);f.write('\n')
sys.exit(0 if len(results)==len(plan['gates']) and all(r['wrapperExitCode']==0 for r in results.values()) else 1)
