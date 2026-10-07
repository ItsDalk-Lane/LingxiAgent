import json,hashlib,re,datetime
from pathlib import Path
R=Path('/Users/study_superior/Desktop/Code/LingxiAgent');B=R/'artifacts/rust-tauri/R05/RR3';O=Path('/private/tmp/rr3-g-review-02-20261007')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
rows=[]
def record(p,expected=None):
 if not p.exists():rows.append(dict(path=str(p),exists=False));return
 h=sha(p);rows.append(dict(path=str(p.relative_to(R)),exists=True,bytes=p.stat().st_size,sha256=h,expected=expected,equal=expected is None or h==expected))
for d in ['A-REVIEW-02','B-REVIEW-01','C-F46-REVIEW-01','D-REVIEW-01','E-REVIEW-02','H-REVIEW-02','I-REVIEW-01','J-REVIEW-02','G-INTERRUPTION-01','DOC-INPUT-BOUNDARY-01']:
 for n in ['REVIEW.md','REPORT.md','I-MAPPING.md']:
  p=B/d/n
  if p.exists():record(p)
for d,idx in [('H-REVIEW-02','COMMAND_INDEX.json'),('A-REVIEW-02','a1-reused-checkpoints.json')]:
 data=json.loads((B/d/idx).read_text());record(B/d/idx)
 for item in data:
  key='record' if 'record' in item else 'path'; p=Path(item[key]);p=p if p.is_absolute() else (B/d/p if key=='record' else R/p)
  record(p,item.get('recordSha256') or item.get('sha256'))
  if key=='record':
   for n in ['stdout.log','stderr.log']:
    if (p.parent/n).exists():record(p.parent/n)
for d in ['A-REVIEW-02','B-REVIEW-01','I-REVIEW-01','J-REVIEW-02']:
 for p in (B/d).rglob('*'):
  if any(x in ['dispatch','snapshot','copy','runner-copy','target','read-input','adversarial'] for x in p.relative_to(B/d).parts):continue
  if p.is_file() and p.suffix in ['.log','.stdout','.stderr'] and p.stat().st_size<5000000:record(p)
for p in (B/'H-REVIEW-02/commands/resources-final').rglob('*resource*json'):
 record(p)
for n in ['H-REVIEW-02/RESOURCE_ANALYSIS-resources-final.json','H-REVIEW-02/F46_RECOMPUTATION.json','H-REVIEW-02/ACCEPTANCE_RECOMPUTATION.json','I-REVIEW-01/permanent-final/result.json','I-REVIEW-01/sync-independent-audit.json','A-REVIEW-02/verified-counts.json','J-REVIEW-02/verified-counts.json','J-REVIEW-02/preserved-contracts-independent.json']:
 record(B/n)
missing=[r for r in rows if not r['exists']];mismatch=[r for r in rows if r.get('equal') is False]
(O/'reference-raw-audit.json').write_text(json.dumps(dict(at=datetime.datetime.now(datetime.timezone.utc).isoformat(),count=len(rows),missing=missing,mismatch=mismatch,files=rows),ensure_ascii=False,indent=2)+'\n')
print(json.dumps(dict(count=len(rows),missing=len(missing),mismatch=len(mismatch))))
