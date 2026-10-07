from capture import *
import re,urllib.parse
commands=load(E/'commands.json')
for x in commands:
 assert sha(ROOT/x['stdout'])==x['stdoutSha256'],x['label']
 assert sha(ROOT/x['stderr'])==x['stderrSha256'],x['label']
 assert x['exitCode'] in [0,1],x['label']
 if x['exitCode']==1:assert x['label'] in ['20-current-target-red','20-wire-target-red','21-raw-source-audit'],x['label']
for p,r in load(E/'changed-files.json')['files'].items():assert sha(ROOT/p)==r['afterSha256'],p
for target in re.findall(r'\[[^\]]*\]\(([^)]+)\)',(E/'REPORT.md').read_text()):
 if target.startswith(('http:','https:')):continue
 clean=urllib.parse.unquote(target);path,_,anchor=clean.partition('#');assert (E/path).exists(),target
h=load(D/'R05_HANDOFF.json')
for p,v in h['artifact_hashes'].items():assert sha(ROOT/p)==v,p
before=load(E/'inputhash-before.json');after=source();assert before['files']==after['files'] and before['digest']==after['digest']
assert h['consumer_contract']==load(E/'before'/'docs/rust-tauri/R05/R05_HANDOFF.json')['consumer_contract']
assert h['accepted_tasks']==[] and h['rr3_current']['R06_READY'] is False
assert h['rr3_current']['packages']['E']['independent_review']=='PENDING'
assert not any((ROOT/R/'G-REVIEW-01'/n).exists() for n in ['REVIEW.md','REPORT.md','RESULT.json','MANIFEST.json','manifest.json'])
check=subprocess.run(['git','diff','--check','--',*OWNED],capture_output=True,text=True);assert check.returncode==0,check.stderr
print(json.dumps({'status':'SELF_CHECKED_PENDING_NEW_INDEPENDENT_REVIEW','priorCommandsChecked':len(commands),'sourceCount':before['count'],'sourceEqual':True,'allReportLinksValid':True,'actualDiffCheckExit':check.returncode,'G':'RUNNING_NO_COMPLETE_CONCLUSION','FINAL':'NOT RUN'},ensure_ascii=False))
