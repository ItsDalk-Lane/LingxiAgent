import os, sys, json, hashlib, subprocess, shutil, importlib.util
from pathlib import Path
OUT=Path(__file__).resolve().parent; SNAP=OUT/'snapshot'; SELF=Path(__file__).resolve()
spec=importlib.util.spec_from_file_location('production_regression',SNAP/'scripts/rust-tauri/r02_run_output_regression.py'); reg=importlib.util.module_from_spec(spec); spec.loader.exec_module(reg)
def sh(code,args=(),**kw): return subprocess.run(['bash','-c',code,'bash',*map(str,args)],capture_output=True,**kw)
def bind(lib,root,dest,units):
 p=sh('set -euo pipefail; source "$1"; bind_worktree "$2" "$3" "$4"',[lib,root,dest,units]); assert p.returncode==0,p.stderr; return dest.read_bytes()
def git(root,*argv): subprocess.run(['git','-C',str(root),*argv],check=True,capture_output=True)
def recurse():
 root=Path(sys.argv[2]); ev=Path(sys.argv[3]); level=int(sys.argv[4]); lib=Path(sys.argv[5]); run=root/'artifacts/rust-tauri/R05/run001'; path=run.joinpath(*(['child']*level))
 if level<2:
  child=path/'child'
  with (child/'stdout.log').open('wb') as stdout,(child/'stderr.log').open('wb') as stderr:
   p=subprocess.run([sys.executable,str(SELF),'recurse',str(root),str(ev),str(level+1),str(lib)],stdout=stdout,stderr=stderr)
  sys.exit(p.returncode)
 p=sh('set -euo pipefail; source "$1"; MAIN_REPO="$2"; EVIDENCE_DIR="$3"; (discover_run_output_sinks) > "$3/sinks.txt"',[lib,root,ev]); assert p.returncode==0,p.stderr
 (ev/'discover-stderr.log').write_bytes(p.stderr)
 units='\n'.join(line.split(' ',1)[1] for line in (ev/'sinks.txt').read_text().splitlines() if line.startswith(('DIR ','FILE ')))
 if ev.is_relative_to(root): units+='\n'+str(ev.relative_to(root))
 (ev/'units.txt').write_text(units+'\n'); before=bind(lib,root,ev/'before.tsv',units)
 copy=ev/'source-copy'; shutil.copytree(root,copy,ignore=shutil.ignore_patterns('own-evidence')); copied=bind(lib,copy,ev/'copy-before.tsv',units); assert before==copied
 for level in range(3):
  for name in ['stdout.log','stderr.log']:
   sink=run.joinpath(*(['child']*level))/name
   with sink.open('ab') as f: f.write(b'growth')
 assert bind(lib,root,ev/'growth.tsv',units)==before
 controls=[]
 for rel in ['source.rs','scripts/check.sh','config.json','stage-map.json','untracked-new.rs','artifacts/rust-tauri/R05/static-old.json']:
  path=root/rel; original=path.read_bytes() if path.exists() else None; path.parent.mkdir(parents=True,exist_ok=True); path.write_bytes(b'changed')
  after=bind(lib,root,ev/'mutation.tsv',units); assert after!=before,rel
  cp=copy/rel; cp.parent.mkdir(parents=True,exist_ok=True); cp.write_bytes(b'changed'); ca=bind(lib,copy,ev/'copy-mutation.tsv',units); assert ca==after,rel
  if original is None: path.unlink(); cp.unlink()
  else: path.write_bytes(original); cp.write_bytes(original)
  assert bind(lib,root,ev/'restored.tsv',units)==before,rel
  controls.append(rel)
 src=root/'source.rs'; original=src.read_bytes(); src.unlink(); assert bind(lib,root,ev/'deleted.tsv',units)!=before; src.write_bytes(original); src.rename(root/'renamed.rs'); assert bind(lib,root,ev/'renamed.tsv',units)!=before; (root/'renamed.rs').rename(src); assert bind(lib,root,ev/'final-restored.tsv',units)==before
 old=run/'child/child/old-evidence.json'
 if old.exists():
  old.write_text('changed'); assert bind(lib,root,ev/'old-after.tsv',units)!=before; old.write_text('old'); assert bind(lib,root,ev/'old-restored.tsv',units)==before
 (ev/'checks.json').write_text(json.dumps({'actualControls':len(controls)+2+(1 if old.exists() else 0),'logFds':6,'sourceCopyEqual':True,'mutations':controls,'restored':True},indent=2))
 shutil.rmtree(copy)
if len(sys.argv)>1 and sys.argv[1]=='recurse': recurse(); sys.exit(0)
BASE=OUT/os.environ.get('EXTRA_CASE_DIR','extra'); BASE.mkdir(); results=[]
for mode in ['internal','external']:
 for oldmode in ['fresh','old-untracked-grandchild','old-tracked-grandchild']:
  case=BASE/(mode+'-'+oldmode); case.mkdir(); root=case/'repo'; root.mkdir(); git(root,'init','-q'); shutil.copyfile(SNAP/'.gitignore',root/'.gitignore')
  for rel in ['source.rs','scripts/check.sh','config.json','stage-map.json']:
   p=root/rel; p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(b'original')
  git(root,'add','.gitignore','source.rs','scripts','config.json','stage-map.json')
  (root/'untracked-dirty.txt').write_text('legal untracked'); (root/'source.rs').write_bytes(b'legal dirty')
  run=root/'artifacts/rust-tauri/R05/run001'; (run/'child/child').mkdir(parents=True)
  if oldmode!='fresh':
   old=run/'child/child/old-evidence.json'; old.write_text('old')
   if oldmode=='old-tracked-grandchild': git(root,'add',str(old.relative_to(root)))
  ev=run/'child/child/own-evidence' if mode=='internal' else case/'evidence'; ev.mkdir()
  lib=case/'production-functions.sh'; lib.write_text(reg.functions(SNAP/'scripts/rust-tauri/run_output_sinks.py'))
  with (run/'stdout.log').open('wb') as stdout,(run/'stderr.log').open('wb') as stderr:
   p=subprocess.run([sys.executable,str(SELF),'recurse',str(root),str(ev),'0',str(lib)],stdout=stdout,stderr=stderr)
  results.append({'case':case.name,'exit':p.returncode,'checks':json.loads((ev/'checks.json').read_text()) if (ev/'checks.json').exists() else None}); assert p.returncode==0,(case, (run/'child/child/stderr.log').read_text())
# OS命令故障拒绝，除命令故障外保持发现器源码不变。
for command in ['ps','lsof','git']:
 fake=BASE/('bad-'+command); fake.mkdir(); exe=fake/command; exe.write_text('#!/bin/sh\nprintf controlled-query-failure >&2\nexit 2\n'); exe.chmod(0o755)
 root=BASE/'external-fresh/repo'; ev=BASE/'external-fresh/evidence'; env=dict(os.environ,PATH=str(fake)+os.pathsep+os.environ['PATH'])
 # git异常必须有真实仓库内sink才会到达tracked查询。
 sink=root/'artifacts/rust-tauri/R05/run001/stdout.log'
 with sink.open('ab') as stdout:
  p=subprocess.run(['python3',str(SNAP/'scripts/rust-tauri/run_output_sinks.py'),str(root),str(ev)],stdout=stdout,stderr=subprocess.PIPE,env=env)
 (fake/'stderr.log').write_bytes(p.stderr); results.append({'case':'query-failure-'+command,'exit':p.returncode}); assert p.returncode!=0
# 新增链接根拒绝并精确恢复。
root=BASE/'external-fresh/repo'; lib=BASE/'external-fresh/production-functions.sh'; target=root/'artifacts/rust-tauri/R05/run001'; link=root/'artifacts/rust-tauri/R05/symlink-root'; link.symlink_to(target,target_is_directory=True)
p=sh('source "$1"; validate_declared_run_root "$2" "$3"',[lib,root,str(link.relative_to(root))]); (BASE/'link-rejection.log').write_bytes(p.stdout+p.stderr); assert p.returncode!=0; results.append({'case':'link-root-rejection','exit':p.returncode})
(BASE/'results.json').write_text(json.dumps(results,indent=2)+'\n'); print('PASS independent',len(results),'cases')
