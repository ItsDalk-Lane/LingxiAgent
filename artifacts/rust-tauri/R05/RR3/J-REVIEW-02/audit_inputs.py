import json,hashlib,sys,os
from pathlib import Path
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent');EV=ROOT/'artifacts/rust-tauri/R05/RR3/J-REVIEW-02'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(EV/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
# 原作者归档逐项实际读取；不把其结果算本轮亲跑。
author=ROOT/'artifacts/rust-tauri/R05/RR3/J-02';m=json.loads((author/'manifest.json').read_text());rows={}
for name,expected in m['files'].items():
 p=author/name;actual=sha(p) if p.is_file() else None
 rows[name]={'expected':expected['sha256'],'actual':actual,'equal':actual==expected['sha256']}
save('author-evidence-audit.json',{'count':len(rows),'different':[k for k,v in rows.items() if not v['equal']],'rows':rows})
prior=ROOT/'artifacts/rust-tauri/R05/RR3/J-REVIEW-01'
x=json.loads((prior/'main-sources-before.json').read_text());entries={p:{'expected':v,'actual':sha(ROOT/p)} for p,v in x.items()};save('prior-504-inputs.json',{'count':len(entries),'different':[p for p,v in entries.items() if v['actual']!=v['expected']],'entries':entries})
# 三段原逻辑直接由前后实际文件比较，独立于作者保持性JSON。
checks={}
def compare(name,old,new,start,end=None):
 a=old[old.index(start):];b=new[new.index(start):]
 if end:a=a[:a.index(end)];b=b[:b.index(end)]
 checks[name]={'equal':a==b,'oldSha256':hashlib.sha256(a).hexdigest(),'newSha256':hashlib.sha256(b).hexdigest(),'bytes':len(b)}
negative=(ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_bytes();old=(author/'r05_t08_negative_gate.sh.before').read_bytes()
compare('negative-pristine-through-final',old,negative,b'# Pristine copies')
legacy=(ROOT/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh').read_bytes();old=(author/'r02_t08_legacy_entry_regression.sh.before').read_bytes()
compare('legacy-binder-discovery-candidate-copy',old,legacy,b'bind_worktree() {',b'\n# Baseline copy:' if b'\n# Baseline copy:' in legacy else b'\n# \xe5\x8e\x86\xe5\x8f\xb2\xe5\x9f\xba\xe7\xba\xbf') if False else None
start=b'bind_worktree() {'; a=old[old.index(start):old.index(b'# Baseline copy:')];b=legacy[legacy.index(start):legacy.index(b'# \xe5\x8e\x86\xe5\x8f\xb2\xe5\x9f\xba\xe7\xba\xbf')];checks['legacy-binder-discovery-candidate-copy']={'equal':a==b,'oldSha256':hashlib.sha256(a).hexdigest(),'newSha256':hashlib.sha256(b).hexdigest()}
compare('legacy-purity-E0s-E1-E5',old,legacy,b'[ "$(git -C "$BASE_COPY" rev-parse HEAD)" = "$BASE_SHA" ]')
for name in ['r05_t08_prepare_node.py','r02_run_output_regression.py']:
 a=(author/(name+'.before')).read_bytes();b=(ROOT/'scripts/rust-tauri'/name).read_bytes();checks[name]={'equal':a==b,'sha256':hashlib.sha256(b).hexdigest()}
save('preserved-contracts-independent.json',checks)
print('author entries',len(rows),'differences',sum(not r['equal'] for r in rows.values()),'contracts',checks)
