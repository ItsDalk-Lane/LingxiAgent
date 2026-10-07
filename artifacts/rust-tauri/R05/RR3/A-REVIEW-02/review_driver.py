import os, sys, json, hashlib, shutil, subprocess, importlib.util, signal
from pathlib import Path
from datetime import datetime, timezone
ROOT=Path(__file__).resolve().parents[5]
OUT=Path(__file__).resolve().parent
PREV=OUT.parent/'A-REVIEW-01'
SNAP=OUT/'snapshot'
def utc(): return datetime.now(timezone.utc).isoformat()
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(p,v): Path(p).write_text(json.dumps(v,indent=2,ensure_ascii=False)+'\n')
def run(name,args,expected=0,env=None,cwd=ROOT):
 start=utc(); log=OUT/(name+'.log'); actualenv=dict(os.environ,**(env or {}))
 with log.open('wb') as f: p=subprocess.run(list(map(str,args)),cwd=cwd,env=actualenv,stdout=f,stderr=subprocess.STDOUT)
 item=dict(name=name,argv=list(map(str,args)),cwd=str(cwd),environmentOverrides=env or {},startedAt=start,endedAt=utc(),exitCode=p.returncode,expectedExitCode=expected,log=str(log.relative_to(OUT)),logSha256=sha(log))
 ledger=OUT/'commands.json'; rows=json.loads(ledger.read_text()) if ledger.exists() else []; rows.append(item); save(ledger,rows)
 print(name,p.returncode,flush=True)
 if expected is not None: assert p.returncode==expected,item
 return p

def loadreg():
 spec=importlib.util.spec_from_file_location('production_regression',SNAP/'scripts/rust-tauri/r02_run_output_regression.py'); reg=importlib.util.module_from_spec(spec); spec.loader.exec_module(reg); return reg

def prepare():
 SNAP.mkdir()
 rows=json.loads((OUT.parent/'A-02/input-manifest.json').read_text())
 rows=rows['files'] if isinstance(rows,dict) else rows
 files=[]
 for item in rows:
  rel=item['path']; src=ROOT/rel; files.append(dict(path=rel,sha256=sha(src),bytes=src.stat().st_size))
 for rel in ['.gitignore','scripts/rust-tauri/r02_t08_legacy_entry_regression.sh','scripts/rust-tauri/r02_run_output_regression.py','scripts/rust-tauri/run_output_sinks.py']:
  dest=SNAP/rel; dest.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(ROOT/rel,dest)
 save(OUT/'input-manifest.json',dict(utc=utc(),head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),branch=subprocess.check_output(['git','branch','--show-current'],cwd=ROOT,text=True).strip(),files=files,scope='逐项A输入冻结，其他包并行，不宣称总树冻结'))
 shutil.copyfile(PREV/'discover-old.py',OUT/'discover-old.py')
 save(OUT/'historical-red-inputs.json',dict(validatorFaultResultsSha256=sha(PREV/'validator-fault-results.json'),oldShellSha256=sha(PREV/'snapshot/scripts/rust-tauri/r02_t08_legacy_entry_regression.sh'),oldDiscoverySha256=sha(OUT/'discover-old.py')))


def old_new():
 gate=SNAP/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh'; original=gate.read_bytes(); old=(PREV/'snapshot/scripts/rust-tauri/r02_t08_legacy_entry_regression.sh').read_text(); source=original.decode()
 mutations=[]
 for start,end in [('validate_run_output_unit() {','\n# discover_run_output_sinks:'),('validate_declared_run_root() {','\n# Assemble the exclusion-unit set')]:
  a=source.index(start); b=source.index(end,a); oa=old.index(start); ob=old.index(end,oa)
  mutations.append(dict(function=start,oldSha256=hashlib.sha256(old[oa:ob].encode()).hexdigest(),currentSha256=hashlib.sha256(source[a:b].encode()).hexdigest()))
  source=source[:a]+old[oa:ob]+source[b:]
 gate.write_text(source); save(OUT/'validator-mutation.json',dict(originalSha256=hashlib.sha256(original).hexdigest(),mutantSha256=sha(gate),mutations=mutations,boundary='仅隔离快照还原旧两校验，永久回归保持当前正确断言'))
 run('validators-old-red',[sys.executable,SNAP/'scripts/rust-tauri/r02_run_output_regression.py','--evidence',OUT/'validators-old-red','--validators-only'],1)
 gate.write_bytes(original); assert sha(gate)==sha(ROOT/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh')
 run('validators-restored-green',[sys.executable,SNAP/'scripts/rust-tauri/r02_run_output_regression.py','--evidence',OUT/'validators-restored-green','--validators-only'])
 run('discovery-green',[sys.executable,ROOT/'scripts/rust-tauri/r02_run_output_regression.py','--evidence',OUT/'discovery-green'])
 run('discovery-old-red',[sys.executable,SNAP/'scripts/rust-tauri/r02_run_output_regression.py','--evidence',OUT/'discovery-old-red','--discovery',OUT/'discover-old.py'],1)
 run('discovery-restored-green',[sys.executable,ROOT/'scripts/rust-tauri/r02_run_output_regression.py','--evidence',OUT/'discovery-restored-green'])

def shrecord(name,code,args,expected=0,env=None): return run(name,['/bin/bash','-c',code,'bash',*args],expected,env)
def bind(lib,root,out,units):
 shrecord(str(out.relative_to(OUT)).replace('/','__'),'set -euo pipefail; source "$1"; bind_worktree "$2" "$3" "$4"',[lib,root,out,units]); return out.read_bytes()
def child():
 reg=loadreg(); root=Path(sys.argv[2]); ev=Path(sys.argv[3]); level=int(sys.argv[4]); lib=Path(sys.argv[5]); runroot=root/'artifacts/rust-tauri/R05/run-review'; here=runroot.joinpath(*(['child']*level))
 if level<2:
  with (here/'child/stdout.log').open('wb') as so,(here/'child/stderr.log').open('wb') as se:
   p=subprocess.run([sys.executable,__file__,'child',str(root),str(ev),str(level+1),str(lib)],stdout=so,stderr=se)
  sys.exit(p.returncode)
 shrecord(str(root.parent.relative_to(OUT)).replace('/','__')+'-discover','set -euo pipefail; source "$1"; MAIN_REPO="$2"; EVIDENCE_DIR="$3"; (discover_run_output_sinks) > "$3/sinks.txt"',[lib,root,ev])
 units='\n'.join(x.split(' ',1)[1] for x in (ev/'sinks.txt').read_text().splitlines() if x.startswith(('DIR ','FILE ')))
 if ev.is_relative_to(root): units+='\n'+str(ev.relative_to(root))
 (ev/'units.txt').write_text(units+'\n')
 base=bind(lib,root,ev/'baseline.tsv',units); copy=root.parent/'candidate-copy'; shutil.copytree(root,copy,symlinks=True,ignore=shutil.ignore_patterns('own-evidence'))
 assert bind(lib,copy,ev/'baseline-copy.tsv',units)==base
 for depth in range(3):
  for stream in ['stdout.log','stderr.log']:
   with runroot.joinpath(*(['child']*depth),stream).open('ab') as f: f.write(b'new run output\n')
 assert bind(lib,root,ev/'growth.tsv',units)==base
 transformations=[]
 for rel in ['source.rs','scripts/run.sh','config.json','stage-map.json']:
  src=root/rel; csrc=copy/rel; original=src.read_bytes()
  for action in ['content','add','delete','rename']:
   added=src.with_name(src.name+'.new'); cadded=csrc.with_name(csrc.name+'.new'); renamed=src.with_name(src.name+'.renamed'); crenamed=csrc.with_name(csrc.name+'.renamed')
   if action=='content': src.write_bytes(b'independent changed input'); csrc.write_bytes(b'independent changed input')
   if action=='add': added.write_bytes(b'new input'); cadded.write_bytes(b'new input')
   if action=='delete': src.unlink(); csrc.unlink()
   if action=='rename': src.rename(renamed); csrc.rename(crenamed)
   tag=rel.replace('/','_')+'-'+action
   changed=bind(lib,root,ev/(tag+'.tsv'),units); copied=bind(lib,copy,ev/(tag+'-copy.tsv'),units); assert changed!=base and copied==changed,(rel,action)
   if action in ['content','delete']: src.write_bytes(original); csrc.write_bytes(original)
   if action=='add': added.unlink(); cadded.unlink()
   if action=='rename': renamed.rename(src); crenamed.rename(csrc)
   assert bind(lib,root,ev/(tag+'-restore.tsv'),units)==base
   assert bind(lib,copy,ev/(tag+'-copy-restore.tsv'),units)==base
   transformations.append(dict(path=rel,action=action,detected=True,sourceCopyEqual=True,restored=True))
 old=runroot/'child/child/old-evidence.json'
 if old.exists():
  cold=copy/old.relative_to(root); old.write_text('modified historical evidence'); cold.write_text('modified historical evidence')
  changed=bind(lib,root,ev/'old-json.tsv',units); assert changed!=base and bind(lib,copy,ev/'old-json-copy.tsv',units)==changed
  old.write_text('old'); cold.write_text('old'); assert bind(lib,root,ev/'old-json-restored.tsv',units)==base and bind(lib,copy,ev/'old-json-copy-restored.tsv',units)==base
  assert all('DIR '+str(runroot.joinpath(*(['child']*d)).relative_to(root)) not in (ev/'sinks.txt').read_text().splitlines() for d in range(3)), '旧孙文件时祖先DIR仍被扩大'
  transformations.append(dict(path=str(old.relative_to(root)),action='old-content',detected=True,sourceCopyEqual=True,restored=True))
 save(ev/'results.json',dict(fdCount=6,stableDuringGrowth=True,transformations=transformations,units=units.splitlines(),actual=len(transformations)))


def extra():
 reg=loadreg(); base=OUT/os.environ.get('EXTRA_DIR','independent'); base.mkdir(); results=[]
 for mode in ['internal','external']:
  for oldmode in ['fresh','untracked','tracked']:
   case=base/(mode+'-'+oldmode); case.mkdir(); root=case/'repo'; root.mkdir(); run('init-'+base.name+'-'+case.name,['git','-C',root,'init','-q']); shutil.copyfile(SNAP/'.gitignore',root/'.gitignore')
   for rel in ['source.rs','scripts/run.sh','config.json','stage-map.json']:
    p=root/rel; p.parent.mkdir(parents=True,exist_ok=True); p.write_text('original')
   run('index-'+base.name+'-'+case.name,['git','-C',root,'add','.gitignore','source.rs','scripts','config.json','stage-map.json'])
   (root/'source.rs').write_text('legal dirty tracked'); (root/'untracked-dirty.txt').write_text('legal dirty untracked')
   runroot=root/'artifacts/rust-tauri/R05/run-review'; (runroot/'child/child').mkdir(parents=True)
   if oldmode!='fresh':
    old=runroot/'child/child/old-evidence.json'; old.write_text('old')
    if oldmode=='tracked': run('index-old-'+base.name+'-'+case.name,['git','-C',root,'add',str(old.relative_to(root))])
   ev=runroot/'child/child/own-evidence' if mode=='internal' else case/'evidence'; ev.mkdir(); lib=case/'production-functions.sh'; lib.write_text(reg.functions(SNAP/'scripts/rust-tauri/run_output_sinks.py'))
   with (runroot/'stdout.log').open('wb') as so,(runroot/'stderr.log').open('wb') as se:
    started=utc(); p=subprocess.run([sys.executable,__file__,'child',str(root),str(ev),'0',str(lib)],stdout=so,stderr=se)
   results.append(dict(case=case.name,startedAt=started,endedAt=utc(),exit=p.returncode,result=json.loads((ev/'results.json').read_text()) if (ev/'results.json').exists() else None)); save(base/'results.json',results); assert p.returncode==0,(case,(runroot/'child/child/stderr.log').read_text())
 # 额外独立异常：三个命令各无stderr的非零、误导stdout+非零；真实sink确保到达归属检查。
 root=base/'external-fresh/repo'; ev=base/'external-fresh/evidence'; faultrecords=[]
 for command in ['ps','lsof','git']:
  for fault in ['silent-exit7','stdout-exit9']:
   fake=base/(command+'-'+fault); fake.mkdir(); exe=fake/command; exe.write_text('#!/bin/sh\n'+('printf misleading-output\n' if fault=='stdout-exit9' else '')+'exit '+('7' if fault=='silent-exit7' else '9')+'\n'); exe.chmod(0o755)
   env=dict(os.environ,PATH=str(fake)+os.pathsep+os.environ['PATH']); started=utc()
   with (root/'artifacts/rust-tauri/R05/run-review/stdout.log').open('ab') as so:
    p=subprocess.run([sys.executable,SNAP/'scripts/rust-tauri/run_output_sinks.py',root,ev],env=env,stdout=so,stderr=subprocess.PIPE)
   (fake/'stderr.log').write_bytes(p.stderr); faultrecords.append(dict(command=command,fault=fault,exit=p.returncode,startedAt=started,endedAt=utc(),stderrSha256=hashlib.sha256(p.stderr).hexdigest())); assert p.returncode!=0
 save(base/'discovery-faults.json',faultrecords)

if __name__=='__main__':
 mode=sys.argv[1]
 if mode=='prepare': prepare()
 elif mode=='core': old_new()
 elif mode=='extra': extra()
 elif mode=='child': child()
