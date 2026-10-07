from pathlib import Path
import json,subprocess,hashlib
R=Path(__file__).resolve().parent;root=Path.cwd();audit=str(R/'independent-audit.py');runner=str(R/'review-runner.py');H=root/'docs/rust-tauri/R05/R05_HANDOFF.json';W=root/'docs/rust-tauri/R05/WORKER_MODEL_BOUNDARY.md';rows=[]
for kind in ['current','wire','fields','hashes']:
 original=(W if kind=='wire' else H).read_bytes(); p=R/('isolated-'+kind+('.md' if kind=='wire' else '.json'));p.write_bytes(original)
 for phase in ['normal','mutated','restored']:
  if phase=='mutated':
   if kind=='wire':data=original.replace(b'kind=callback + op=model.complete',b'kind=model.complete + op=model.complete',1)
   else:
    d=json.loads(original)
    if kind=='current':d['rr3_current']['packages']['A']['independent_review']='PENDING'
    elif kind=='fields':d['consumer_contract']['model_turn']['fields'].remove('deadline_unix_ms')
    elif kind=='hashes':d['artifact_hashes'][next(iter(d['artifact_hashes']))]='0'*64
    data=(json.dumps(d,ensure_ascii=False,indent=2)+'\n').encode()
   p.write_bytes(data)
  elif phase=='restored':p.write_bytes(original)
  cmd=['python3',runner,'control-'+kind+'-'+phase,'python3',audit,kind,str(p)];q=subprocess.run(cmd,capture_output=True);print(q.stdout.decode().strip());expected=1 if phase=='mutated' else 0
  rows.append({'kind':kind,'phase':phase,'actualExit':q.returncode,'expectedExit':expected,'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'restoredBytesEqual':phase!='restored' or p.read_bytes()==original});assert q.returncode==expected,(kind,phase,q.stdout,q.stderr)
(R/'controls-results.json').write_text(json.dumps(rows,indent=2)+'\n')
print('four isolated controls normal0 -> targeted1 -> exact-restored0')
