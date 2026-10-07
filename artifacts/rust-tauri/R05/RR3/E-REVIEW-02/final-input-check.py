from pathlib import Path
import os,json,hashlib,datetime
root=Path.cwd();r=Path(__file__).resolve().parent;expected=json.loads((root/'artifacts/rust-tauri/R05/RR3/E-02/inputhash-before.json').read_text());actual={}
for base in ['rust','scripts/rust-tauri']:
 for parent,dirs,files in os.walk(root/base):
  dirs[:]=[x for x in dirs if x not in ['target','.git'] and (base!='scripts/rust-tauri' or x!='__pycache__')]
  for n in files:
   p=Path(parent)/n;actual[str(p.relative_to(root))]=hashlib.sha256(p.read_bytes()).hexdigest()
for n in ['rust-toolchain.toml','shared/contract-versions.json']:actual[n]=hashlib.sha256((root/n).read_bytes()).hexdigest()
changes=[x for x in set(actual)|set(expected['files']) if actual.get(x)!=expected['files'].get(x)];digest=hashlib.sha256(''.join(h+'  '+p+'\n' for p,h in sorted(actual.items())).encode()).hexdigest()
out={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'count':len(actual),'digest':digest,'newDeletedChangedPaths':changes,'files':actual,'scope':expected['scope']};(r/'source-reenumerated.json').write_text(json.dumps(out,indent=2)+'\n');assert not changes and len(actual)==423 and digest==expected['digest']
print('independent re-enumeration 423 files / no new, deleted or changed inputs / digest',digest)
