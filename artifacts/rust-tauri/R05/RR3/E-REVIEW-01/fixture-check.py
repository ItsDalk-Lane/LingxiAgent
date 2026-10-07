import json,sys,re,hashlib
from pathlib import Path
kind,p=sys.argv[1:];p=Path(p)
if kind=='current':
 j=json.loads(p.read_text());packages=j['rr3_current']['packages']; errors=[]
 for key in ['A','C_F46']:
  if packages[key]['status']!='CLOSED' or packages[key]['independent_review']!='PASS':errors.append('current '+key+' contradicts closed independent PASS')
elif kind=='fields':
 j=json.loads(p.read_text());src=Path('rust/crates/lingxi-kernel/src/model_exchange.rs').read_text();b=src.split('pub struct ModelTurnInput {',1)[1].split('\n}',1)[0];actual=re.findall(r'^\s*pub (\w+):',b,re.M);errors=[] if actual==j['consumer_contract']['model_turn']['fields'] else ['ModelTurnInput complete field list differs: '+str(actual)]
elif kind=='hash':
 j=json.loads(p.read_text());errors=[x+' artifact digest differs' for x,h in j['artifact_hashes'].items() if hashlib.sha256(Path(x).read_bytes()).hexdigest()!=h]
elif kind=='history':
 j=json.loads(p.read_text());old=json.loads(Path('artifacts/rust-tauri/R05/RR3/E-01/before/docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json').read_text());errors=[]
 def compare(a,b,path):
  if isinstance(a,dict) and isinstance(b,dict):
   for k,v in a.items():
    if k not in b:errors.append(path+'/'+k+' deleted')
    else:compare(v,b[k],path+'/'+k)
  elif a!=b:errors.append(path+' historical evidence changed')
 compare(old,j,'')
else:raise SystemExit('unknown fixture check')
print(json.dumps({'kind':kind,'file':str(p),'status':'FAIL' if errors else 'PASS','errors':errors},ensure_ascii=False));sys.exit(bool(errors))
