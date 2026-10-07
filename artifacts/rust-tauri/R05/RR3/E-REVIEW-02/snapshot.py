from pathlib import Path
import json,hashlib,sys,os,datetime
r=Path(__file__).resolve().parent;root=Path.cwd();s=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
e=json.loads((root/'artifacts/rust-tauri/R05/RR3/E-02/inputhash-before.json').read_text());files=dict(e['files'])
owned=json.loads((root/'artifacts/rust-tauri/R05/RR3/E-02/before.json').read_text())['owned']
for p in owned:files[p]=s(root/p)
for d,n in [('E-01','manifest.json'),('E-REVIEW-01','manifest.json'),('E-02','manifest.json'),('A-REVIEW-02','manifest.json'),('C-F46-REVIEW-01','MANIFEST.json'),('B-REVIEW-01','REVIEW.md'),('D-REVIEW-01','evidence-manifest.json')]:
 p='artifacts/rust-tauri/R05/RR3/'+d+'/'+n;files[p]=s(root/p)
files={p:s(root/p) for p in files}
out={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'423 E production observation inputs + 14 owned docs + seven frozen external report/manifest inputs; excludes G/coordination/target and all other tree','files':files,'count':len(files)}
(r/('input-'+sys.argv[1]+'.json')).write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')
if sys.argv[1]=='after':
 old=json.loads((r/'input-before.json').read_text());changes=[p for p in set(files)|set(old['files']) if files.get(p)!=old['files'].get(p)];print({'count':len(files),'changes':changes});assert not changes
else:print({'count':len(files)})
