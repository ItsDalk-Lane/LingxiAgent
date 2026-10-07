import sys, os, json, shutil, hashlib, subprocess, importlib.util
from pathlib import Path
from review_driver import ROOT,OUT,SNAP,PREV,run,save,sha,utc,loadreg,shrecord

def setup_e0s():
 source=(ROOT/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh').read_text()
 start=source.index('# ── E5 classification machinery'); finish=source.index('# ── E1: default-entry-not-switched proofs')
 body=source[start:finish]
 old=(PREV/'snapshot/scripts/rust-tauri/r02_t08_legacy_entry_regression.sh').read_text()
 classifier=source[start:source.index('# ── E0s:')]
 oldclassifier=old[old.index('# ── E5 classification machinery'):old.index('# ── E0s:')]
 binder=source[source.index('bind_worktree() {'):source.index("\n# The gate's own evidence subtree")]
 oldbinder=old[old.index('bind_worktree() {'):old.index("\n# The gate's own evidence subtree")]
 assert classifier==oldclassifier and binder==oldbinder
 cmp_lines=[x for x in source.splitlines() if x.startswith('cmp ')]; oldcmp=[x for x in old.splitlines() if x.startswith('cmp ')]; assert cmp_lines==oldcmp
 ev=OUT/'e0s'; ev.mkdir()
 script=OUT/'e0s-extracted.sh'
 import shlex
 pre='set -euo pipefail\nMAIN_REPO='+shlex.quote(str(ROOT))+'\nEVIDENCE_DIR='+shlex.quote(str(ev))+'\nWORK=$(mktemp -d)\ntrap \'rm -rf -- "$WORK"\' EXIT\nfail() { printf "FAIL: %s\\n" "$*" >&2; exit 1; }\nnote() { printf "%s\\n" "$*"; }\n'
 script.write_text(pre+loadreg().functions(ROOT/'scripts/rust-tauri/run_output_sinks.py')+'\n'+body)
 save(OUT/'e0s-preservation.json',dict(classifierEqualBytes=True,classifierSha256=hashlib.sha256(classifier.encode()).hexdigest(),binderEqualBytes=True,binderSha256=hashlib.sha256(binder.encode()).hexdigest(),cmpLinesEqual=True,cmpLines=cmp_lines,extractedFullBodySha256=hashlib.sha256(body.encode()).hexdigest(),wrapperBoundary='原生产四函数、完整classifier及E0s；只补路径/note/fail/临时目录，不是E1-E5/npm完整门禁'))
 run('e0s',['/bin/bash',script])
 recount_e0s()

def recount_e0s():
 lines=(OUT/'e0s/e0s-self-checks.log').read_text().splitlines()
 fixtures=[x for x in lines if x.startswith('fixture ') and 'expected=[' in x]
 assert len(fixtures)==61 and all(x.split('expected=[',1)[1].split(']')[0]==x.split('actual=[',1)[1].split(']')[0] for x in fixtures)
 assert sum(x.startswith('fixture missing-block-rows: UNRECOGNIZED OK') for x in lines)==1
 assert sum(x.startswith('fixture green-log:') for x in lines)==1
 save(OUT/'e0s-counts.json',dict(actualClassifierFixtures=len(fixtures)+1,expectedActualFixtureRows=len(fixtures),missingBlockRowsNegative=1,greenPositiveControl=1,passed=len(fixtures)+1,ignored=0,filtered=0,explanation='独立逐行统计：61条expected/actual判定+1缺块拒绝=原62；另1合法全绿对照'))

def independent_validators():
 root=OUT/'independent-validator-repo'; root.mkdir(); run('independent-validator-init',['/usr/bin/git','-C',root,'init','-q'])
 tracked='artifacts/rust-tauri/R05/tracked'; fresh='artifacts/rust-tauri/R05/fresh'; link='artifacts/rust-tauri/R05/linked'
 for rel in [tracked,fresh]: (root/rel).mkdir(parents=True)
 (root/tracked/'registered.rs').write_text('registered'); run('independent-validator-index',['/usr/bin/git','-C',root,'add',tracked+'/registered.rs'])
 (root/link).symlink_to(root/fresh,target_is_directory=True)
 lib=OUT/'independent-validator-functions.sh'; lib.write_text(loadreg().functions(ROOT/'scripts/rust-tauri/run_output_sinks.py'))
 fake=OUT/'independent-validator-bin'; fake.mkdir(); proxy=fake/'git'
 # 信号终止、exit0二进制stderr、无法执行，分别覆盖回归四种异常之外的边界。
 proxy.write_text('#!'+sys.executable+'\nimport os,sys,signal\nmode=os.environ["FAULT"]\nif mode=="signal": os.kill(os.getpid(),signal.SIGTERM)\nif mode=="binary-stderr":\n os.write(2,b"independent diagnostic: \\xff\\x00\\n");sys.exit(0)\n')
 proxy.chmod(0o755); records=[]
 for function in ['validate_run_output_unit','validate_declared_run_root']:
  for kind,rel in [('fresh',fresh),('tracked',tracked)]:
   for fault in ['normal','signal','binary-stderr','unexecutable','restored']:
    proxy.chmod(0o644 if fault=='unexecutable' else 0o755)
    # PATH不能回落到另一个Git；Python/bash仍由隔离目录提供，其他工具不参与校验。
    if not (fake/'python3').exists(): (fake/'python3').symlink_to(sys.executable)
    env={'FAULT':fault,'PATH':str(fake)} if fault not in ['normal','restored'] else {}
    expected=1 if kind=='tracked' or fault not in ['normal','restored'] else 0
    name='independent-validator-'+function+'-'+kind+'-'+fault
    shrecord(name,'source "$1"; "$2" "$3" "$4"',[lib,function,root,rel],expected,env)
    log=(OUT/(name+'.log')).read_bytes(); reason=b'git ls-files query failed' in log if fault not in ['normal','restored'] else (b'INDEX-TRACKED' in log if kind=='tracked' else log==b'')
    assert reason,(name,log); records.append(dict(function=function,kind=kind,fault=fault,exit=expected,reasonValid=reason))
  for rel in [link,'.','artifacts','artifacts/rust-tauri/R05/missing/../fresh','../outside']:
   # 缺失路径只针对declared；两函数对本组均应拒绝。
   if function=='validate_run_output_unit' and rel=='artifacts': continue
   name='independent-root-'+function+'-'+str(len(records))
   shrecord(name,'source "$1"; "$2" "$3" "$4"',[lib,function,root,rel],1)
   records.append(dict(function=function,illegalRoot=rel,exit=1))
 save(OUT/'independent-validator-results.json',dict(actual=len(records),passed=len(records),cases=records,substituteBoundary='真实Git索引/文件系统；仅隔离PATH目标Git信号终止、成功码二进制诊断、无法执行；正常恢复用真实Git'))


def reuse():
 items=json.loads((OUT.parent/'A-02/a1-reuse-input-equality.json').read_text())['files']; files=[]
 original=json.loads((PREV/'input-manifest.json').read_text())['files']; orig={x['path']:x for x in original}
 for x in items:
  rel=x['path']; current=(ROOT/rel).read_bytes(); prior=(PREV/'snapshot'/rel).read_bytes(); assert current==prior and sha(ROOT/rel)==orig[rel]['sha256']
  files.append(dict(path=rel,bytes=len(current),sha256=sha(ROOT/rel),equalBytes=True,priorManifestSha256=orig[rel]['sha256']))
 assert len(files)==17
 # 亲读所有层而非复制作者统计。
 idx=[]
 for mode in ['nested-internal','nested-external']:
  for path in sorted((PREV/mode).rglob('verify-stage-result.json')):
   obj=json.loads(path.read_text()); b=obj['candidateSourceBinding']; cps=b['checkpointAfterEveryCommand']; assert obj['overall']=='PASS' and b['stable'] is True
   assert all(c['stable'] and not c.get('changedPathBytesHex',[]) and c.get('error') is None for c in cps)
   assert all(c['status']=='PASS' for c in obj['commands'])
   idx.append(dict(mode=mode,path=str(path.relative_to(ROOT)),sha256=sha(path),count=len(cps),stable=True,overall='PASS',commandStatuses=[c['status'] for c in obj['commands']],binding=b))
  counts=sorted(x['count'] for x in idx if x['mode']==mode); assert counts==[7,8,15,20],counts
 alltest=(PREV/'xtask-all.log').read_text(); assert '121 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' in alltest
 helperlogs=[]
 for log,expected in [('helper-baseline-02.log',0),('helper-mismatch-red.log',101),('helper-restored-green.log',0)]:
  p=PREV/log; text=p.read_text(); helperlogs.append(dict(path=str(p.relative_to(ROOT)),sha256=sha(p),expectedExit=expected))
  if expected==101: assert 'run_output_sinks.py' in text and 'FAILED' in text
 commands=json.loads((PREV/'commands.json').read_text()); selected=[c for c in commands if c['caseId'] in ['xtask-all','nested-external','helper-baseline-02','helper-mismatch-red','helper-restored-green']]
 assert len(selected)==5
 for c in selected:
  p=PREV/(c['caseId']+'.log'); assert sha(p)==c['logSha256'] and c['exitCode']==c['expectedExitCode']
 main=(ROOT/'rust/crates/xtask/src/main.rs').read_text(); pos=main.index('if !stable {'); fragment=main[pos:pos+650]; assert 'report["overall"] = serde_json::Value::String("FAIL".into())' in fragment
 save(OUT/'a1-reuse-input-equality.json',dict(actual=len(files),files=files,priorManifestSha256=sha(PREV/'input-manifest.json'),testLogSha256=sha(PREV/'xtask-all.log'),selectedCommands=selected,helperLogs=helperlogs,stableFailFragment=fragment,shellAndPermanentRegressionReused=False))
 save(OUT/'a1-reused-checkpoints.json',idx)

if __name__=='__main__':
 {'e0s':setup_e0s,'e0s-counts':recount_e0s,'validators':independent_validators,'reuse':reuse}[sys.argv[1]]()
