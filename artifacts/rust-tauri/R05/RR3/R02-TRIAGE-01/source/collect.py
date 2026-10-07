import sys, subprocess, hashlib, json, datetime
from pathlib import Path
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
OUT=ROOT/'artifacts/rust-tauri/R05/RR3/R02-TRIAGE-01'
def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def digest(p):
 try:
  b=p.read_bytes(); s=p.stat(); return {'path':str(p),'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b),'mtime_ns':s.st_mtime_ns}
 except Exception as e: return {'path':str(p),'error':str(e)}
def capture(tag, paths):
 records=[]
 for path in paths:
  p=Path(path); p=p if p.is_absolute() else ROOT/p
  a=digest(p)
  try:
   b=p.read_bytes(); dest=OUT/'input'/tag/p.relative_to(ROOT) if p.is_relative_to(ROOT) else OUT/'input'/tag/'external'/str(p).lstrip('/')
   dest.parent.mkdir(parents=True,exist_ok=True); dest.write_bytes(b); snap=hashlib.sha256(b).hexdigest()
  except Exception as e: snap=str(e)
  z=digest(p); records.append({'before':a,'snapshot_sha256':snap,'after':z,'observed_changed':a!=z})
 (OUT/'loghash'/f'{tag}.json').write_text(json.dumps({'time':now(),'records':records},ensure_ascii=False,indent=2))
 return records
def run(tag,args):
 start=now(); r=subprocess.run(args,cwd=ROOT,capture_output=True)
 (OUT/'commands'/f'{tag}.json').write_text(json.dumps({'argv':args,'cwd':str(ROOT),'start':start,'end':now()},ensure_ascii=False,indent=2))
 (OUT/'commands'/f'{tag}.stdout.log').write_bytes(r.stdout); (OUT/'commands'/f'{tag}.stderr.log').write_bytes(r.stderr)
 (OUT/'exit'/f'{tag}.json').write_text(json.dumps({'exit':r.returncode,'time':now()}))
 print(r.stdout.decode(errors='replace')); print(r.stderr.decode(errors='replace'),file=sys.stderr); return r
if __name__=='__main__':
 if sys.argv[1]=='capture': print(json.dumps(capture(sys.argv[2],sys.argv[3:]),ensure_ascii=False,indent=2))
 elif sys.argv[1]=='run': run(sys.argv[2],sys.argv[3:])
