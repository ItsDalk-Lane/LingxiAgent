from pathlib import Path
import json,sys,hashlib,os,re,shutil
sys.path.insert(0,str(Path(__file__).resolve().parent))
from review_driver import run,OUT,COPY,CARGO,sha
TARGET=Path('/tmp/lingxi-rr3-a-review-01-target'); binary=TARGET/'debug/deps/xtask-ad766de8ea03e62c'; src=COPY/'rust/crates/xtask/src/candidate.rs'; orig=src.read_bytes(); token=b'                || self.output_files.contains(&relative)\n'; assert orig.count(token)==1
src.write_bytes(orig.replace(token,b'')); records=[]
try:
 assert run('scope-old-build-02',[CARGO,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--no-run'],{'CARGO_TARGET_DIR':str(TARGET)})==0
 records.append({'phase':'scope-ancestor-old-red-02','path':str(binary),'sha256':sha(binary),'candidateSourceSha256':sha(src)})
 assert run('scope-ancestor-old-red-02',[str(binary),'--exact','verify::runner_tests::rr3_real_fd_standard_nested_layout_has_stable_checkpoints_at_every_layer','--nocapture'],expected=101)==101
finally: src.write_bytes(orig)
assert run('scope-restored-build-02',[CARGO,'test','--manifest-path','rust/Cargo.toml','--locked','-p','xtask','--no-run'],{'CARGO_TARGET_DIR':str(TARGET)})==0
records.append({'phase':'scope-restored-green-02','path':str(binary),'sha256':sha(binary),'candidateSourceSha256':sha(src)})
assert run('scope-restored-green-02',[str(binary),'--exact','verify::runner_tests::rr3_real_fd_standard_nested_layout_has_stable_checkpoints_at_every_layer','--nocapture'],{'LINGXI_NESTED_ARCHIVE':str(OUT/'scope-restored-nested-02')})==0
(OUT/'scope-binary-identities.json').write_text(json.dumps(records,indent=2)+'\n')
