import sys,os,json,subprocess,shutil,stat,re
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parent));import review_driver as d
ROOT,EV,COPY=d.ROOT,d.EV,d.COPY
checks=[]
def check(name,v):
 checks.append({'name':name,'ok':bool(v)});d.save('independent-checks.json',checks);assert v,name
# 独立Git工作树/index/HEAD，真实生产绑定器的变异和恢复。
shell=(ROOT/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh').read_text();start=shell.index('bind_worktree() {');end=shell.index('\n}',start)+2;block=shell[start:end]
(EV/'production-bind-worktree.sh').write_text(block)
def bind(name):
 assert d.run(name,['bash','-c',block+'\nbind_worktree "$1" "$2"','review',COPY,EV/(name+'.tsv')])==0
 return d.sha(EV/(name+'.tsv'))
check('independent-index-inode',(ROOT/'.git/index').stat().st_ino!=(COPY/'.git/index').stat().st_ino)
check('independent-head-inode',(ROOT/'.git/HEAD').stat().st_ino!=(COPY/'.git/HEAD').stat().st_ino)
check('index-entries-equal',subprocess.check_output(['git','ls-files','--stage','-z'],cwd=ROOT,env=d.ENV)==subprocess.check_output(['git','ls-files','--stage','-z'],cwd=COPY,env=d.ENV))
check('shared-alternate-exact-main',Path((COPY/'.git/objects/info/alternates').read_text().strip()).resolve()==(ROOT/'.git/objects').resolve())
tracked=json.loads((EV/'cow-source-copy.json').read_text())['trackedFiles']
mode_errors=[]
for name,row in tracked.items():
 src,dst=ROOT/name,COPY/name
 if not src.exists() or not dst.exists():continue
 if not src.is_symlink() and stat.S_IMODE(src.stat().st_mode)!=stat.S_IMODE(dst.stat().st_mode):mode_errors.append(name)
check('tracked-mode-equal',not mode_errors)
# 包/脚本的合法脏内容在原overlay后保留，检查H输入以外所有主目标目录文件。
source=json.loads((EV/'main-sources-before.json').read_text())
changed=[]
for name,h in source.items():
 p=COPY/name
 if not p.is_file() or d.sha(p)!=h:changed.append(name)
check('source-scope-candidate-byte-equal',not changed)
before=bind('binding-normal');file=COPY/'rust/crates/lingxi-kernel/src/lib.rs';original=file.read_bytes();mainsha=d.sha(ROOT/'rust/crates/lingxi-kernel/src/lib.rs')
file.write_bytes(original+'\n// 独立验收：仅本副本的受控变异\n'.encode())
red=bind('binding-mutant');check('real-binder-detects-isolated-mutation',red!=before);check('main-source-no-writeback',d.sha(ROOT/'rust/crates/lingxi-kernel/src/lib.rs')==mainsha)
file.write_bytes(original);check('real-binder-restored',bind('binding-restored')==before)
# 已知依赖普通文件写控与自身缓存写控；拒绝后还原真实字节。
file=COPY/'node_modules/ws/index.js';original=file.read_bytes();srcsha=d.sha(ROOT/'node_modules/ws/index.js')
cache=COPY/'node_modules/.vite/j-review-cache-probe';cache.parent.mkdir(exist_ok=True);cache.write_text('independent writable cache\n');check('cache-no-source-writeback',not (ROOT/'node_modules/.vite/j-review-cache-probe').exists())
file.write_bytes(original+b'\n// isolated dependency write control\n');check('dependency-no-source-writeback',d.sha(ROOT/'node_modules/ws/index.js')==srcsha)
assert d.run('full-content-red',['python3','-B',d.helper,'--verify',EV/'node-preparation/result.json','--evidence',EV/'full-content-red'],expect=1)==1
check('content-red-named','copy dependencies changed' in (EV/'full-content-red.stderr.log').read_text())
file.write_bytes(original)
assert d.run('full-content-restored',['python3','-B',d.helper,'--verify',EV/'node-preparation/result.json','--evidence',EV/'full-content-restored'])==0
# 真正移除副本的共享对象路径；源对象目录不动，生产verify必须准确拒绝。
alt=COPY/'.git/objects/info/alternates';orig=alt.read_bytes();alt.write_text(str(EV/'nonexistent-objects')+'\n')
try:
 assert d.run('full-shared-objects-red',['python3','-B',d.helper,'--verify',EV/'node-preparation/result.json','--evidence',EV/'full-shared-objects-red'],expect=1)==1
 check('shared-object-red-named','command failed' in (EV/'full-shared-objects-red.stderr.log').read_text())
finally:alt.write_bytes(orig)
assert d.run('full-shared-objects-restored',['python3','-B',d.helper,'--verify',EV/'node-preparation/result.json','--evidence',EV/'full-shared-objects-restored'])==0
print('independent controls',len(checks))
