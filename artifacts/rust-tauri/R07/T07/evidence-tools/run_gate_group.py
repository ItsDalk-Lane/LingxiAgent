#!/usr/bin/env python3
"""顺序运行同一固定候选的 Cargo 检查；每项仍保留独立原始回执。"""
import json
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[5]
out = Path(__file__).resolve().parents[1]
candidate = sys.argv[1]
plan = json.loads((out / 'GATE_PLAN.json').read_text())['gates']
results = {}
for label in ['fmt', 'clippy', 'protocol', 'protocol-tests', 'kernel', 'boundaries', 'adapter', 'service', 'build']:
    result = subprocess.run([sys.executable, str(Path(__file__).with_name('run_candidate.py')), 'run', candidate, label, '--', *plan[label]], cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    results[label] = result.returncode
    receipt_exists = (out / f'{candidate}-{label}.json').is_file()
    if not receipt_exists:
        precheck = out / f'{candidate}-{label}-precheck.json'
        assert not precheck.exists()
        precheck.write_text(json.dumps({'candidate':candidate, 'gate':label, 'status':'NOT_RUN_PRECHECK_REJECTED', 'productCommandExecuted':False, 'wrapperExitCode':result.returncode, 'stdout':result.stdout, 'requestedProductCommand':plan[label]}, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'gate':label, 'exitCode':result.returncode, 'receipt':f'{candidate}-{label}.json' if receipt_exists else None, 'productCommandExecuted':receipt_exists}, ensure_ascii=False), flush=True)
    if result.returncode == 99 or not receipt_exists:
        print(result.stdout, flush=True)
        break
summary = out / f'{candidate}-rust-batch.json'
assert not summary.exists()
summary.write_text(json.dumps({'candidate':candidate, 'results':results}, ensure_ascii=False, indent=2) + '\n')
sys.exit(0 if len(results)==9 and all(code==0 for code in results.values()) else 1)
