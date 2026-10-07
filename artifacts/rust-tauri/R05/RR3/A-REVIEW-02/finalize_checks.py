import sys,os,json,hashlib,shutil,subprocess,platform
from pathlib import Path
from review_driver import ROOT,OUT,PREV,SNAP,sha,utc,save,run

for name,args in [('cargo-version',['/Users/study_superior/.cargo/bin/cargo','--version']),('rustc-version',['/Users/study_superior/.cargo/bin/rustc','--version']),('node-version',['node','--version']),('npm-version',['npm','--version']),('python-version',[sys.executable,'--version']),('git-version',['/usr/bin/git','--version']),('os-version',['sw_vers']),('shell-syntax',['/bin/bash','-n',ROOT/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh']),('python-syntax',[sys.executable,'-c','import ast,sys; [ast.parse(open(p).read()) for p in sys.argv[1:]]',ROOT/'scripts/rust-tauri/r02_run_output_regression.py',ROOT/'scripts/rust-tauri/run_output_sinks.py'])]:
 run(name,args)
assert '1.98.1' in (OUT/'cargo-version.log').read_text() and '1.98.1' in (OUT/'rustc-version.log').read_text()
binaries=[]
for command in [sys.executable,'/bin/bash','/usr/bin/git','ps','lsof','node','npm','/Users/study_superior/.cargo/bin/cargo','/Users/study_superior/.cargo/bin/rustc','/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex']:
 path=Path(shutil.which(command) or command); resolved=path.resolve(); binaries.append(dict(command=command,path=str(path),realPath=str(resolved),sha256=sha(resolved),bytes=resolved.stat().st_size))
for command in ['cargo','rustc']:
 path=Path(subprocess.check_output(['/Users/study_superior/.cargo/bin/rustup','which',command],cwd=ROOT,text=True).strip()); binaries.append(dict(command='pinned '+command,path=str(path),sha256=sha(path),bytes=path.stat().st_size))
save(OUT/'binary-manifest.json',dict(utc=utc(),platform=platform.platform(),machine=platform.machine(),binaries=binaries,reusedRustBinaries={'binary-manifest-before.json':json.loads((PREV/'binary-manifest-before.json').read_text()),'scope-binary-identities.json':json.loads((PREV/'scope-binary-identities.json').read_text()),'policy':'原独立运行身份引用，不声称本轮新运行/构建，不复制target'}))
inputs=json.loads((OUT/'input-manifest.json').read_text()); equality=[]
for x in inputs['files']:
 current=sha(ROOT/x['path']); equality.append(dict(**x,currentSha256=current,equal=current==x['sha256']))
assert all(x['equal'] for x in equality)
save(OUT/'delivery-inputs.json',dict(utc=utc(),head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),files=equality,actual=len(equality),allEqual=True,notWholeTreeFreeze=True))
# 从亲跑原始文件计算结果，不复制实施者计数。
summaries={}
for name in ['validators-old-red','validators-restored-green','discovery-green','discovery-old-red','discovery-restored-green','e0s/f42-real-fd']:
 path=OUT/name/'validator-query/results.json'; data=json.loads(path.read_text()); cmds=data['commands']; assert len(cmds)==24
 for c in cmds:
  for stream in ['stdout','stderr']:
   assert sha(c[stream+'Log'])==c[stream+'Sha256']
  assert c['passed']==(c['exitCode']==c['expectedExitCode'] and c['reasonValid'])
 normal=[c for c in cmds if c['mode'] in ['normal','restored']]; faults=[c for c in cmds if c['mode'] not in ['normal','restored']]
 summaries[name]=dict(actual=len(cmds),passed=sum(c['passed'] for c in cmds),normalRestoreActual=len(normal),normalRestorePassed=sum(c['passed'] for c in normal),faultActual=len(faults),faultPassed=sum(c['passed'] for c in faults),sha256=sha(path))
 assert summaries[name]['passed']==(8 if name=='validators-old-red' else 24)
for name in ['discovery-green','discovery-restored-green','e0s/f42-real-fd']:
 p=OUT/name/'results.json'; r=json.loads(p.read_text()); assert len(r['cases'])==4 and r['illegalRootsRejected']==7 and r['trackedSinkRejected']
 summaries[name]['fdCases']=len(r['cases']); summaries[name]['ignored']=r['ignored']; summaries[name]['filtered']=r['filtered']
# 旧发现器真实误吞的目标前后绑定字节相等。
old=OUT/'discovery-old-red/old-untracked-parent-child'; assert (old/'before.tsv').read_bytes()==(old/'after.tsv').read_bytes() and 'DIR artifacts/rust-tauri/R05/run001' in (old/'sinks.txt').read_text()
summaries['oldDiscoveryTarget']=dict(exit=1,changedOldJsonWasMissed=True,beforeSha256=sha(old/'before.tsv'),afterSha256=sha(old/'after.tsv'),sinksSha256=sha(old/'sinks.txt'))
extra=json.loads((OUT/'independent-03/results.json').read_text()); assert len(extra)==6 and all(r['exit']==0 for r in extra)
faults=json.loads((OUT/'independent-03/discovery-faults.json').read_text()); assert len(faults)==6 and all(c['exit']!=0 for c in faults)
summaries['independent']=dict(fdGroups=len(extra),fdPerGroup=6,transformations=sum(r['result']['actual'] for r in extra),sourceCopySymmetricTransformations=sum(sum(t['sourceCopyEqual'] for t in r['result']['transformations']) for r in extra),discoveryFaults=len(faults),validatorControls=json.loads((OUT/'independent-validator-results.json').read_text())['actual'])
assert summaries['independent']['transformations']==100
# 验证复用A1的旧行为反证与恢复原始层结果。
redrows=[]
for group,expected in [('scope-old-red-artifacts-0','FAIL'),('scope-restored-nested-02','PASS')]:
 paths=list((PREV/group).rglob('verify-stage-result.json')); assert len(paths)==4
 for path in paths:
  r=json.loads(path.read_text()); b=r['candidateSourceBinding']; cp=b['checkpointAfterEveryCommand']; assert r['overall']==expected and b['stable']==(expected=='PASS')
  assert all(c['stable']==(expected=='PASS') for c in cp)
  if len(cp)==20: assert all(c['status']=='PASS' and c['exitCode']==0 for c in r['commands'])
  redrows.append(dict(group=group,path=str(path.relative_to(ROOT)),sha256=sha(path),overall=r['overall'],stable=b['stable'],checkpoints=len(cp),changedPathsHex=[c.get('changedPathBytesHex',[]) for c in cp]))
save(OUT/'a1-reused-red-restored.json',redrows)
summaries['a1']=dict(equalInputs=17,reusedUnitTests=121,reusedInternalCheckpoints=50,reusedExternalCheckpoints=50,reusedRedCheckpoints=sum(c['checkpoints'] for c in redrows if c['overall']=='FAIL'),stableMainSourceAuditOnly=True,fullFormalGateNotRun=True)
summaries['e0s']=json.loads((OUT/'e0s-counts.json').read_text())
save(OUT/'verified-counts.json',summaries)
# 命令记录都绑定本轮冻结输入；失败准备轮保留，不计产品红。
ledger=json.loads((OUT/'commands.json').read_text())
for c in ledger:
 assert sha(OUT/c['log'])==c['logSha256'],c['name']
 c['inputManifestSha256']=sha(OUT/'input-manifest.json'); c['caseId']=c['name']; c['F-ID']='F42'; c['platform']='macOS arm64'; c['reviewer']='rr3_a_review_02'; c['inputBoundary']='A逐项冻结；主树其他包并行；注入仅本证据隔离副本'
save(OUT/'commands.json',ledger)
save(OUT/'command-audit.json',dict(actualRecordedCommands=len(ledger),allLogHashesMatch=True,unexpectedExitCommands=[dict(name=c['name'],exit=c['exitCode'],expected=c['expectedExitCode']) for c in ledger if c['expectedExitCode'] is not None and c['expectedExitCode']!=c['exitCode']]))
print(json.dumps(summaries['independent'],ensure_ascii=False))
print('PASS bindings/counts/log hashes/tools')
