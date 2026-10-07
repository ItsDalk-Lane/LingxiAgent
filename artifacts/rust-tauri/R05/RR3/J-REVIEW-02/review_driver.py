import os, sys, json, hashlib, subprocess, datetime, shutil, tempfile
from pathlib import Path
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
EV=ROOT/'artifacts/rust-tauri/R05/RR3/J-REVIEW-02'
ENV=dict(os.environ,GIT_OPTIONAL_LOCKS='0',PYTHONDONTWRITEBYTECODE='1',CARGO_NET_OFFLINE='true')
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(name,v): (EV/name).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def inventory(root):
 rows={}
 for base,dirs,files in os.walk(root,followlinks=False):
  for name in sorted(dirs+files):
   p=Path(base)/name
   if p.is_symlink():rows[str(p.relative_to(root))]={'link':os.readlink(p)}
   elif p.is_file():rows[str(p.relative_to(root))]={'sha256':sha(p),'mode':p.stat().st_mode&0o777}
 return rows
def sources():
 paths=[p for d in ['rust/crates','scripts/rust-tauri','cli','shared','contracts','docs/rust-tauri'] for p in (ROOT/d).rglob('*') if p.is_file() and '__pycache__' not in p.parts]
 paths += [ROOT/p for p in ['package.json','package-lock.json','.npmrc','rust/Cargo.lock','rust/Cargo.toml','rust-toolchain.toml']]
 return {str(p.relative_to(ROOT)):sha(p) for p in paths}
def run(name,argv,cwd=ROOT,env=None):
 rec=dict(name=name,argv=[str(a) for a in argv],cwd=str(cwd),startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),freeBefore=shutil.disk_usage(ROOT).free)
 with (EV/(name+'.stdout')).open('wb') as out,(EV/(name+'.stderr')).open('wb') as err:
  p=subprocess.run(rec['argv'],cwd=cwd,env=env or ENV,stdout=out,stderr=err)
 rec.update(exitCode=p.returncode,finishedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),freeAfter=shutil.disk_usage(ROOT).free,stdoutSha256=sha(EV/(name+'.stdout')),stderrSha256=sha(EV/(name+'.stderr')))
 with (EV/'commands.jsonl').open('a') as f:f.write(json.dumps(rec)+'\n')
 print(name,p.returncode,flush=True)
 return p.returncode
if sys.argv[1]=='prepare':
 save('main-sources-before.json',sources());save('main-git-before.json',inventory(ROOT/'.git'))
 for name,args in [('head',['git','rev-parse','HEAD']),('branch',['git','branch','--show-current']),('status',['git','status','--porcelain=v1','--untracked-files=all']),('node',['node','--version']),('npm',['npm','--version']),('rustc',['/Users/study_superior/.cargo/bin/rustc','--version']),('cargo',['/Users/study_superior/.cargo/bin/cargo','--version'])]: assert run(name,args)==0
 ev=EV/'default-prefix';ev.mkdir()
 work=Path.home()/'r05t08-work'
 copy=Path(tempfile.mkdtemp(prefix='j-review02-',dir=work))
 (EV/'copy-path.txt').write_text(str(copy)+'\n')
 shell=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text();start=shell.index('SOURCE_HEAD=');end=shell.index('\n# Pristine copies',start)
 fragment='set -uo pipefail; ROOT="$1"; COPY="$2"; EV="$3"; note(){ echo "$*"; }; fail(){ echo "FAIL: $*" >&2; exit 1; };\n'+shell[start:end]
 (EV/'production-prefix.sh').write_text(fragment)
 rc=run('full-production-prefix',['bash',EV/'production-prefix.sh',ROOT,copy,ev])
 save('main-git-after-prepare.json',inventory(ROOT/'.git'));save('main-sources-after-prepare.json',sources())
 raise SystemExit(rc)
if sys.argv[1]=='checks':
 for name,args in [('git-selfcheck',['python3','-B',ROOT/'scripts/rust-tauri/prepare_git_copy_selfcheck.py','--evidence',EV/'git-selfcheck']),('fd-selfcheck',['python3','-B',ROOT/'scripts/rust-tauri/r02_run_output_regression.py','--evidence',EV/'fd-selfcheck']),('i-restore',['python3','-B',ROOT/'scripts/rust-tauri/r05_t08_restore_selfcheck.py','--evidence',EV/'i-restore']),('b-negative',['python3','-B',ROOT/'scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py'])]:
  assert run(name,args)==0,name
if sys.argv[1]=='legacy':
 copy=Path((EV/'copy-path.txt').read_text().strip())
 rc=run('legacy-directed',['bash',copy/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh',EV/'legacy-directed'],copy,dict(ENV,R02_LEGACY_REGRESSION_MODE='directed-no-seal-family'))
 raise SystemExit(rc)
if sys.argv[1]=='verify':
 raise SystemExit(run('node-final-verify',['python3','-B',ROOT/'scripts/rust-tauri/r05_t08_prepare_node.py','--verify',EV/'default-prefix/node-preparation/result.json','--evidence',EV/'node-final-verify']))
