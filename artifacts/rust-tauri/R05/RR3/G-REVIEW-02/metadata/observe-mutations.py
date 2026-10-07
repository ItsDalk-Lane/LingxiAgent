import hashlib,json,os,time,datetime,re
from pathlib import Path
O=Path('/private/tmp/rr3-g-review-02-20261007'); R=Path('/Users/study_superior/Desktop/Code/LingxiAgent'); C=Path('/Users/study_superior/r05t08-work/negcopy.HwNE95')
s=(R/'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text(); names=s.split('MUTATED_FILES=(\n',1)[1].split('\n)',1)[0].split()
def utc():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def dig(p):return hashlib.sha256(p.read_bytes()).hexdigest()
seen=None
with (O/'mutation-observations.jsonl').open('w') as f:
 while not (O/'command.json').exists():
  rows={}
  for n in names:
   p=C/n
   if p.exists():rows[n]={'sha256':dig(p),'mtime_ns':p.stat().st_mtime_ns}
  if rows!=seen:
   f.write(json.dumps({'at':utc(),'files':rows})+'\n');f.flush();seen=rows
  time.sleep(.5)
 f.write(json.dumps({'at':utc(),'status':'STOPPED_AFTER_DEFAULT_EXIT'})+'\n')
print('mutation observer stopped',flush=True)
