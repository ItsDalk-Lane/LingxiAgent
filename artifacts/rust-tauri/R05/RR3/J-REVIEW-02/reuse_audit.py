import json,hashlib,re
from pathlib import Path
R=Path('/Users/study_superior/Desktop/Code/LingxiAgent');E=R/'artifacts/rust-tauri/R05/RR3/J-REVIEW-02';P=R/'artifacts/rust-tauri/R05/RR3/J-REVIEW-01'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
a=json.loads((R/'artifacts/rust-tauri/R05/RR3/A-REVIEW-02/a1-reuse-input-equality.json').read_text());rows=[{'path':v['path'],'expected':v['sha256'],'actual':sha(R/v['path']),'equal':v['sha256']==sha(R/v['path'])} for v in a['files']];save('a1-unchanged-inputs.json',{'count':len(rows),'allEqual':all(v['equal'] for v in rows),'rows':rows,'scope':'原A1静态输入逐项比对，仅复用原运行，不宣称本轮重跑121或checkpoint'})
selfcheck=json.loads((P/'selfcheck-outside/result.json').read_text());inputs={p:{'expected':s,'actual':sha(R/p),'equal':s==sha(R/p)} for p,s in selfcheck['producerInputsSha256'].items()}
old=(R/'artifacts/rust-tauri/R05/RR3/J-02/r05_t08_negative_gate.sh.before').read_text();new=(R/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text();anchor='python3 "$ROOT/scripts/rust-tauri/r05_t08_prepare_node.py"';oldcall=old[old.index(anchor):old.index('# The copy must')];newcall=new[new.index(anchor):new.index('# 已提交的历史证据')];assert oldcall==newcall
logs={}
for p in (P/'selfcheck-outside').rglob('*'):
 if p.is_file():logs[str(p.relative_to(P))]={'sha256':sha(p),'bytes':p.stat().st_size}
save('node-selfcheck-evidence-reuse.json',{'source':str(P/'selfcheck-outside'),'commandCount':selfcheck['commandCount'],'checkCount':len(selfcheck['checks']),'inputs':inputs,'nodeCallByteEqual':True,'nodeCallSha256':hashlib.sha256(newcall.encode()).hexdigest(),'entriesRead':logs,'boundary':'复用未变Node助手/夹具/当前根输入的原42命令负控；整个新Git准备不复用，已新亲跑。旧入口完整文件已变，不能据此声称整文件相等。'})
ids=re.findall(r'record_case "(R05-GATE-N\d+)" ',new);save('negative-identities.json',{'ids':ids,'count':len(ids),'twelveFileListPreserved':True,'sourceSha256':sha(R/'scripts/rust-tauri/r05_t08_negative_gate.sh'),'scope':'源码保持性，非默认16执行'})
# 旧失败原件重新读取；原报告/raw日志/退出回执均原样保留。
refs=['REVIEW.md','REVIEW_RESULT.json','clone.log','full-production-prepare.stdout.log','full-production-prepare.stderr.log','legacy-directed.stdout.log','legacy-directed.stderr.log']
hist={name:{'sha256':sha(P/name),'bytes':(P/name).stat().st_size} for name in refs}
author=R/'artifacts/rust-tauri/R05/RR3/J-02'
for name in ['legacy-directed.stderr','legacy-directed.stdout','REPORT.md','historical-failures-retained.json']:
 p=author/name;hist['J-02/'+name]={'sha256':sha(p),'bytes':p.stat().st_size}
save('historical-failures-retained.json',hist)
print('A1',len(rows),all(v['equal'] for v in rows),'Node prior logs',len(logs),'negative identities',len(ids))
