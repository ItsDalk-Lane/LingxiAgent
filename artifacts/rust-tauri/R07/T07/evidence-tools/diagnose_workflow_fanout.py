#!/usr/bin/env python3
"""原断言保持不变的重复取证；首个失败即停，不能作为重试后验收。"""
import json,subprocess,sys,datetime
argv=['/Users/study_superior/.cargo/bin/cargo','test','--manifest-path','rust/Cargo.toml','--locked','-p','lingxi-service','--test','r07_workflow','workflow::fanout_dispatch_events_journal_and_completion_delivery','--','--exact','--nocapture']
for iteration in range(1,31):
    print(json.dumps({'diagnosticIteration':iteration,'command':argv,'startedAt':datetime.datetime.now(datetime.timezone.utc).isoformat()}),flush=True)
    result=subprocess.run(argv)
    print(json.dumps({'diagnosticIteration':iteration,'exitCode':result.returncode}),flush=True)
    if result.returncode: sys.exit(result.returncode)
print('DIAGNOSTIC_ONLY: 30 repetitions did not reproduce the previous failure; previous failure is not closed by this result.',flush=True)
