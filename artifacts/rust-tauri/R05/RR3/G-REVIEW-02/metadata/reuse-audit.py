import json,hashlib,datetime
from pathlib import Path
R=Path('/Users/study_superior/Desktop/Code/LingxiAgent');B=R/'artifacts/rust-tauri/R05/RR3';O=Path('/private/tmp/rr3-g-review-02-20261007')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def compare(n,m):
 out=[]
 for p,h in m.items():
  q=R/p;cur=sha(q) if q.is_file() else None;out.append(dict(path=p,prior=h,current=cur,equal=h==cur))
 return dict(reference=n,referenceSha256=sha(B/n),count=len(out),equal=sum(x['equal'] for x in out),changed=[x for x in out if not x['equal']],files=out)
res=[]
for name,kind in [('A-REVIEW-02/input-manifest.json','list'),('A-REVIEW-02/a1-reuse-input-equality.json','list'),('H-REVIEW-02/FINAL_SOURCE_BINDING.json','hashes'),('I-REVIEW-01/source-after.json','dict'),('J-REVIEW-02/main-sources-before.json','plain')]:
 x=json.loads((B/name).read_text());m={r['path']:r['sha256'] for r in x['files']} if kind=='list' else x['inputHashes'] if kind=='hashes' else x['files'] if kind=='dict' else x
 res.append(compare(name,m))
(O/'reference-input-comparison.json').write_text(json.dumps(dict(at=datetime.datetime.now(datetime.timezone.utc).isoformat(),comparisons=res),ensure_ascii=False,indent=2)+'\n')
for x in res:print(x['reference'],x['equal'],x['count'],[i['path'] for i in x['changed']])
