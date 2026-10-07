from pathlib import Path
import subprocess,json,hashlib
root=Path.cwd();r=Path(__file__).resolve().parent;original=(root/'docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json').read_bytes();p=r/'isolated-history.json';rows=[]
for phase in ['normal','mutated','restored']:
 data=original
 if phase=='mutated':d=json.loads(original);del d['acceptances'];data=(json.dumps(d,ensure_ascii=False,indent=2)+'\n').encode()
 p.write_bytes(data);q=subprocess.run(['python3',str(r/'review-runner.py'),'control-history-'+phase,'python3',str(r/'history-control-check.py'),str(p)],capture_output=True);expected=1 if phase=='mutated' else 0;print(q.stdout.decode().strip());assert q.returncode==expected
 rows.append({'phase':phase,'exit':q.returncode,'sha256':hashlib.sha256(data).hexdigest(),'restoredBytesEqual':phase!='restored' or data==original})
(r/'history-controls-results.json').write_text(json.dumps(rows,indent=2)+'\n')
