import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time

root = Path.cwd()
out = root / 'artifacts/rust-tauri/R05/RR3/A-01/final-selfcheck-01'
out.mkdir()
cargo = '/Users/study_superior/.cargo/bin/cargo'
checks = [
 ('fmt', [cargo, 'fmt', '--manifest-path', 'rust/Cargo.toml', '--all', '--', '--check'], {}, 0),
 ('clippy-xtask', [cargo, 'clippy', '--manifest-path', 'rust/Cargo.toml', '--locked', '-p', 'xtask', '--all-targets', '--', '-D', 'warnings'], {}, 0),
 ('build-xtask', [cargo, 'build', '--manifest-path', 'rust/Cargo.toml', '--locked', '-p', 'xtask'], {}, 0),
 ('xtask-all', [cargo, 'test', '--manifest-path', 'rust/Cargo.toml', '--locked', '-p', 'xtask'], {'LINGXI_NESTED_ARCHIVE': str(out/'nested-internal')}, 0),
 ('nested-external', [cargo, 'test', '--manifest-path', 'rust/Cargo.toml', '--locked', '-p', 'xtask', 'rr3_real_fd_standard_nested_layout', '--', '--nocapture'], {'LINGXI_NESTED_EXTERNAL': '1', 'LINGXI_NESTED_ARCHIVE': str(out/'nested-external')}, 0),
 ('discovery-green', ['python3', 'scripts/rust-tauri/r02_run_output_regression.py', '--evidence', str(out/'discovery-green')], {}, 0),
 ('discovery-old-red', ['python3', 'scripts/rust-tauri/r02_run_output_regression.py', '--evidence', str(out/'discovery-old-red'), '--discovery', 'artifacts/rust-tauri/R05/RR3/A-01/discover-before.py'], {}, 1),
 ('shell-syntax', ['bash', '-n', 'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh'], {}, 0),
 ('diff-check', ['git', 'diff', '--check'], {}, 0),
]
results=[]
for label, argv, overrides, expected in checks:
 env=dict(os.environ); env.update(overrides)
 begin=datetime.datetime.now(datetime.timezone.utc).isoformat(); start=time.monotonic()
 with (out/(label+'.log')).open('wb') as log:
  result=subprocess.run(argv, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
 end=datetime.datetime.now(datetime.timezone.utc).isoformat()
 record={'caseId': label, 'fId':'F42', 'command':argv,'envOverrides':overrides,'startedAt':begin,'finishedAt':end,'durationSeconds':time.monotonic()-start,'exitCode':result.returncode,'expectedExitCode':expected,'matchesExpected':result.returncode==expected,'logSha256':hashlib.sha256((out/(label+'.log')).read_bytes()).hexdigest()}
 results.append(record)
 (out/(label+'.exit')).write_text(str(result.returncode)+'\n')
 (out/'commands.json').write_text(json.dumps(results,ensure_ascii=False,indent=2)+'\n')
 print(label, result.returncode, 'expected', expected, flush=True)
versions={}
for label,cmd in [('cargo',[cargo,'--version']),('rustc',['/Users/study_superior/.cargo/bin/rustc','--version']),('node',['node','--version']),('python',['python3','--version'])]:
 versions[label]=subprocess.check_output(cmd,text=True).strip()
inputs=['rust-toolchain.toml','rust/Cargo.lock','.gitignore','rust/crates/xtask/src/main.rs','rust/crates/xtask/src/candidate.rs','rust/crates/xtask/src/candidate/tests.rs','rust/crates/xtask/src/runner_identity.rs','rust/crates/xtask/src/verify.rs','rust/crates/xtask/src/verify/runner_tests.rs','scripts/rust-tauri/run_output_sinks.py','scripts/rust-tauri/r02_run_output_regression.py','scripts/rust-tauri/r02_t08_legacy_entry_regression.sh','rust/target/debug/xtask']+[f'rust/crates/xtask/src/stage_maps/{s}.json' for s in ['R02','R03','R04','R05']]
manifest={'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'branch':subprocess.check_output(['git','branch','--show-current'],text=True).strip(),'toolchain':versions,'platform':platform.platform(),'inputs':[{'path':p,'sha256':hashlib.sha256((root/p).read_bytes()).hexdigest()} for p in inputs],'candidateBoundary':'本记录绑定A输入；并行B/C工作树未冻结。完整候选绑定由最终独立阶段运行记录。','allExpected':all(r['matchesExpected'] for r in results)}
(out/'inputs.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
raise SystemExit(0 if manifest['allExpected'] else 1)
