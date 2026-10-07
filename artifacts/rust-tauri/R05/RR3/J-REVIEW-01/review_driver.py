import os,sys,json,hashlib,subprocess,datetime,shutil,importlib.util
from pathlib import Path
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
EV=ROOT/'artifacts/rust-tauri/R05/RR3/J-REVIEW-01'
COPY=EV/'copy'
ENV=os.environ.copy(); ENV['GIT_OPTIONAL_LOCKS']='0'; ENV['PYTHONDONTWRITEBYTECODE']='1'; ENV['PATH']='/Users/study_superior/.cargo/bin:'+ENV['PATH']; ENV['CARGO_TARGET_DIR']=str(ROOT/'rust/target'); ENV['CARGO_NET_OFFLINE']='true'
def sha(p):
 with open(p,'rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(name,v): (EV/name).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def run(name,args,cwd=ROOT,env=None,expect=0):
 start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with (EV/(name+'.stdout.log')).open('wb') as out,(EV/(name+'.stderr.log')).open('wb') as err:
  p=subprocess.run([str(a) for a in args],cwd=cwd,env=env or ENV,stdout=out,stderr=err)
 r={'name':name,'argv':[str(a) for a in args],'cwd':str(cwd),'startedUtc':start,'finishedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exitCode':p.returncode,'expectedExit':expect,'stdoutSha256':sha(EV/(name+'.stdout.log')),'stderrSha256':sha(EV/(name+'.stderr.log'))}
 with (EV/'commands.jsonl').open('a') as f:f.write(json.dumps(r)+'\n')
 print(name,p.returncode,flush=True)
 return p.returncode
helper=ROOT/'scripts/rust-tauri/r05_t08_prepare_node.py'
spec=importlib.util.spec_from_file_location('prep',helper);prep=importlib.util.module_from_spec(spec);spec.loader.exec_module(prep)
def manifest():
 paths=[p for d in ['rust/crates','scripts/rust-tauri','cli','shared'] for p in (ROOT/d).rglob('*') if p.is_file() and '__pycache__' not in p.parts]
 paths += [ROOT/p for p in ['package.json','package-lock.json','.npmrc','rust/Cargo.lock','rust/Cargo.toml','rust-toolchain.toml']]
 return {str(p.relative_to(ROOT)):sha(p) for p in paths}
if sys.argv[1]=='prepare':
 save('main-sources-before.json',manifest());save('main-git-before.json',prep.inventory(ROOT/'.git'))
 save('space-before.json',dict(shutil.disk_usage(EV)._asdict()))
 run('identity',['bash','-c','git rev-parse HEAD; git branch --show-current; node --version; npm --version; /Users/study_superior/.cargo/bin/rustc --version; /Users/study_superior/.cargo/bin/cargo --version; shasum -a 256 rust/target/debug/lingxi-service'])
 binary=ROOT/'rust/target/debug/lingxi-service';assert sha(binary)=='7fa13a3bc7ad8d8fde1d55d33242f0eca8b17913172daf8fe513d899c224cd7e'
 shell=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text();block=shell[shell.index('SOURCE_HEAD='):shell.index('# The copy must')]
 (EV/'production-preparation-block.sh').write_text(block)
 command='set -uo pipefail; ROOT="$1"; COPY="$2"; EV="$3"; note(){ echo "$*"; }; fail(){ echo "FAIL: $*" >&2; exit 1; };\n'+block
 assert run('full-production-prepare',['bash','-c',command,'review',ROOT,COPY,EV])==0
 save('space-after-prepare.json',dict(shutil.disk_usage(EV)._asdict()))
 save('main-git-after-prepare.json',prep.inventory(ROOT/'.git'))
 assert json.loads((EV/'main-git-before.json').read_text())==json.loads((EV/'main-git-after-prepare.json').read_text())
 save('copy-git.json',{k:subprocess.check_output(['git','-C',str(COPY),*args],env=ENV,text=True).strip() for k,args in {'head':['rev-parse','HEAD'],'tree':['rev-parse','HEAD^{tree}'],'index':['ls-files','--stage'],'status':['status','--porcelain'],'alternate':['rev-parse','--git-dir']}.items()})
elif sys.argv[1]=='business':
 run('client',['python3','-B','scripts/rust-tauri/r02_client_leaf_matrix.py',EV/'CLIENT'],COPY)
 run('sessions-auth',['python3','-B','scripts/rust-tauri/r02_cli_sessions_leaf_matrix.py',EV/'CLI_SESSIONS'],COPY)
 run('binary-after-business',['shasum','-a','256',ROOT/'rust/target/debug/lingxi-service'])
elif sys.argv[1]=='selfcheck':
 tmp=EV/'tmp';tmp.mkdir(exist_ok=True); env=ENV.copy();env['TMPDIR']=str(tmp)
 run('selfcheck',['python3','-B',ROOT/'scripts/rust-tauri/r05_t08_node_selfcheck.py','--evidence',EV/'selfcheck'],env=env)
elif sys.argv[1]=='final':
 run('full-verify',['python3','-B',helper,'--verify',EV/'node-preparation/result.json','--evidence',EV/'full-verify'])
 save('main-sources-after.json',manifest());save('main-git-after.json',prep.inventory(ROOT/'.git'));save('space-final.json',dict(shutil.disk_usage(EV)._asdict()))
if sys.argv[1]=='cow-prepare':
 assert not COPY.exists()
 assert run('shared-no-checkout',['git','clone','--shared','--no-checkout','--quiet','--branch','codex/rust-tauri-migration',ROOT,COPY])==0
 tracked=subprocess.check_output(['git','ls-files','-z'],cwd=ROOT,env=ENV).split(b'\0')
 import ctypes
 libc=ctypes.CDLL(None,use_errno=True)
 copied={};missing=[]
 for raw in tracked:
  if not raw:continue
  name=os.fsdecode(raw);src=ROOT/name;dst=COPY/name
  if not src.exists() and not src.is_symlink():missing.append(name);continue
  dst.parent.mkdir(parents=True,exist_ok=True)
  if src.is_symlink():dst.symlink_to(os.readlink(src));copied[name]={'link':os.readlink(src)}
  elif src.is_file():
   assert libc.clonefile(os.fsencode(src),os.fsencode(dst),0)==0,(name,ctypes.get_errno())
   assert src.stat().st_ino!=dst.stat().st_ino
   assert sha(src)==sha(dst),name
   copied[name]={'sha256':sha(src),'size':src.stat().st_size}
  else:raise RuntimeError(name)
 shutil.copy2(ROOT/'.git/index',COPY/'.git/index')
 save('cow-source-copy.json',{'scope':'reviewer setup fallback after actual production clone ENOSPC; production helper unchanged','trackedFiles':copied,'missingTracked':missing,'bytes':sum(v.get('size',0) for v in copied.values())})
 for sub in ['rust','scripts','docs/rust-tauri']:
  assert run('overlay-'+sub.replace('/','-'),['rsync','-a','--delete','--exclude','target/',str(ROOT/sub)+'/',str(COPY/sub)+'/'])==0
 shell=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text();block=shell[shell.index('python3 "$ROOT/scripts/rust-tauri/r05_t08_prepare_node.py"'):shell.index('# The copy must')]
 command='set -uo pipefail; ROOT="$1"; COPY="$2"; EV="$3"; fail(){ echo "FAIL: $*" >&2; exit 1; };\n'+block
 assert run('full-production-node-prepare',['bash','-c',command,'review',ROOT,COPY,EV])==0
 save('space-after-prepare.json',dict(shutil.disk_usage(EV)._asdict()))
 save('main-git-after-prepare.json',prep.inventory(ROOT/'.git'))
 assert json.loads((EV/'main-git-before.json').read_text())==json.loads((EV/'main-git-after-prepare.json').read_text())
