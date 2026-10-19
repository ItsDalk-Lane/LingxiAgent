#!/usr/bin/env python3
"""顺序运行冻结T08的Rust与原生检查，每项分别保存原始回执。"""
import hashlib,json,subprocess,sys
from pathlib import Path
root=Path(__file__).resolve().parents[5]
out=Path(__file__).resolve().parents[1]
candidate=sys.argv[1]
plan_path=out/'GATE_PLAN.json'
plan=json.loads(plan_path.read_text())['gates']
labels=[label for label in plan if label not in {'ui','types','build','card-navigation-desktop','card-navigation-mobile'}]+['build']
summary_path=out/f'{candidate}-rust-batch.json'
if summary_path.exists():raise SystemExit('已有批次结果，拒绝覆盖')
results={}
for label in labels:
 print(json.dumps({'gate':label,'status':'STARTING'},ensure_ascii=False),flush=True)
 process=subprocess.run([sys.executable,str(Path(__file__).with_name('run_candidate.py')),'run',candidate,label,'--',*plan[label]],cwd=root,capture_output=True,text=True)
 receipt=out/f'{candidate}-{label}.json'
 results[label]={'wrapperExitCode':process.returncode,'productCommandExecuted':receipt.exists(),'receipt':receipt.name if receipt.exists() else None}
 if not receipt.exists():
  with (out/f'{candidate}-{label}-precheck.json').open('x') as f:
   json.dump({'candidate':candidate,'gate':label,'status':'NOT_RUN_PRECHECK_REJECTED','productCommandExecuted':False,'wrapperExitCode':process.returncode,'stdout':process.stdout,'stderr':process.stderr,'requestedProductCommand':plan[label]},f,ensure_ascii=False,indent=2)
   f.write('\n')
 print(json.dumps({'gate':label,**results[label]},ensure_ascii=False),flush=True)
 if process.returncode!=0 or not receipt.exists():break
remaining=[label for label in labels if label not in results]
summary={'candidate':candidate,'scope':'T08_RELATED_CHECKS_NOT_FINAL_R07','runnerSha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'planSha256':hashlib.sha256(plan_path.read_bytes()).hexdigest(),'results':results,'notRun':remaining,'fullStagePassClaimed':False}
with summary_path.open('x') as f:json.dump(summary,f,ensure_ascii=False,indent=2);f.write('\n')
sys.exit(0 if not remaining and all(r['wrapperExitCode']==0 for r in results.values()) else 1)
