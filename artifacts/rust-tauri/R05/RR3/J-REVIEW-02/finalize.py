import json,os,hashlib,datetime,subprocess,shutil,re,platform,sys
from pathlib import Path
R=Path('/Users/study_superior/Desktop/Code/LingxiAgent');E=R/'artifacts/rust-tauri/R05/RR3/J-REVIEW-02'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
source=json.loads((E/'default-prefix/git-preparation/source-files.json').read_text());diff=[]
for name,row in source.items():
 p=R/name
 if row['kind']=='file':
  if not p.is_file() or p.is_symlink() or sha(p)!=row['sha256'] or p.stat().st_mode&0o777!=row['mode']:diff.append(name)
 else:raise AssertionError((name,row))
save('tracked-source-final-preservation.json',{'trackedCount':len(source),'different':diff,'scope':'本轮真实HEAD全部tracked来源文件的字节/模式，非全部untracked输出或全机状态冻结'});assert not diff
records=[json.loads(s) for s in (E/'commands.jsonl').read_text().splitlines()]
for rec in records:
 for kind in ['stdout','stderr']:assert sha(E/(rec['name']+'.'+kind))==rec[kind+'Sha256']
assert json.loads((E/'node-final-verify/result.json').read_text())['status']=='PASS'
assert not json.loads((E/'main-git-differences.json').read_text())['different'];assert not json.loads((E/'main-sources-differences.json').read_text())['different']
counts={'gitSelfcheck':json.loads((E/'git-selfcheck/result.json').read_text())['checkCount'],'gitSelfcheckCommands':json.loads((E/'git-selfcheck/result.json').read_text())['commandCount'],'topLevelRecordedCommands':len(records),'copyAudit':json.loads((E/'independent-full-copy-audit.json').read_text())['counts'],'historicalBase':json.loads((E/'historical-base-independent.json').read_text()),'client':json.loads((E/'CLIENT/summary.json').read_text()),'nodeFinal':json.loads((E/'node-final-verify/result.json').read_text())}
save('verified-counts.json',counts)
legacy=json.loads((E/'legacy-directed/legacy-entry/e0-base-materialization/result.json').read_text());work=Path(legacy['copy']).parent
save('legacy-cleanup.json',{'path':str(work),'existsAfter':work.exists(),'owner':'原legacy本次mktemp目录，原EXIT trap自行清理','checkedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()});assert not work.exists()
# 回执由真实运行输出构成；读版本无安装、无构建、无系统变更。
args=[['git','--version'],['python3','--version'],['uname','-a']];rows=[]
for a in args:
 started=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(a,capture_output=True)
 rows.append({'argv':a,'cwd':str(R),'startedUtc':started,'finishedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exitCode':p.returncode,'stdout':p.stdout.decode(),'stderr':p.stderr.decode()})
rows += [{'tool':t,'path':shutil.which(t),'resolved':str(Path(shutil.which(t)).resolve()),'sha256':sha(Path(shutil.which(t)).resolve())} for t in ['git','python3','node','npm']]
save('toolchain-extra.json',rows)
save('reviewer-preparation-notes.json',{'failures':[{'script':'audit_inputs.initial.py','exitCode':1,'reason':'审查记录器bytes.index误传str，未生成保持性结论；只修本目录记录器后直接比较前后字节成功。','error':'TypeError: argument should be integer or bytes-like object, not str','originalPreserved':True}],'correctedObservations':[{'file':'negative-identities.initial.json','reason':'审查正则漏掉双引号而统计0；按真实record_case行修正，16个原ID各一次。','initialPreserved':True}],'productFailures':[]})
print('Final tracked preservation',len(source),'recorded commands',len(records),'freeBytes',shutil.disk_usage(R).free)
