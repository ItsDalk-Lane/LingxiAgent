import sys, json, hashlib, shutil, os
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from review_driver import run, COPY, OUT, CARGO, sha
TARGET=Path('/tmp/lingxi-rr3-a-review-01-target')
TEST=TARGET/'debug/deps/xtask-ad766de8ea03e62c'
(OUT/'binary-manifest-before.json').write_text(json.dumps([{'path':str(p),'sha256':sha(p)} for p in [TARGET/'debug/xtask',TEST]],indent=2)+'\n')
argv=[str(TEST),'--exact','verify::runner_tests::rr3_nested_process_probe','--nocapture']
assert run('helper-baseline-02',argv)==0
helper=COPY/'scripts/rust-tauri/run_output_sinks.py'; original=helper.read_bytes(); helper.write_bytes(original+'\n# 独立验收的身份失配注入\n'.encode())
(OUT/'helper-mutation.json').write_text(json.dumps({'path':str(helper),'oldSha256':hashlib.sha256(original).hexdigest(),'newSha256':sha(helper),'testBinarySha256':sha(TEST),'mutationCount':1},indent=2)+'\n')
try:
 assert run('helper-mismatch-red',argv,expected=101)==101
 assert 'run_output_sinks.py' in (OUT/'helper-mismatch-red.log').read_text()
finally: helper.write_bytes(original)
assert run('helper-restored-green',argv)==0
# 精确短路新增输出排除，回到A1旧行为；其余Scope与生产verify编排均保持。
candidate=COPY/'rust/crates/xtask/src/candidate.rs'; original=candidate.read_bytes(); token=b'                || self.output_files.contains(&relative)\n'; assert original.count(token)==1; candidate.write_bytes(original.replace(token,b''))
(OUT/'scope-mutation.json').write_text(json.dumps({'path':str(candidate),'oldSha256':hashlib.sha256(original).hexdigest(),'newSha256':sha(candidate),'mutationCount':1,'boundary':'仅删除新增精确FILE排除条件，生产Scope/verify其余不变'},indent=2)+'\n')
known=set(Path('/tmp').glob('lingxi-xtask-test-rr3-nested-*'))
try:
 assert run('scope-ancestor-old-red',[CARGO,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','rr3_real_fd_standard_nested_layout','--','--nocapture'],{'CARGO_TARGET_DIR':str(TARGET)},expected=101)==101
 new=set(Path('/tmp').glob('lingxi-xtask-test-rr3-nested-*'))-known
 for i,root in enumerate(new):
  if (root/'artifacts').exists(): shutil.copytree(root/'artifacts',OUT/f'scope-old-red-artifacts-{i}')
finally: candidate.write_bytes(original)
assert run('scope-restored-green',[CARGO,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','rr3_real_fd_standard_nested_layout','--','--nocapture'],{'CARGO_TARGET_DIR':str(TARGET),'LINGXI_NESTED_ARCHIVE':str(OUT/'scope-restored-nested')})==0
