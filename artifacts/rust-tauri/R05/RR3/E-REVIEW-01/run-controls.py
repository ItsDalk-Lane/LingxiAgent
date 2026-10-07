import json,subprocess,sys,hashlib
from pathlib import Path
P=Path(__file__).resolve().parent; rows=[]
for kind in ['current','fields','hash','history']:
 source=Path('docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json' if kind=='history' else 'docs/rust-tauri/R05/R05_HANDOFF.json');j=json.loads(source.read_text())
 if kind=='current':
  for key in ['A','C_F46']:j['rr3_current']['packages'][key]['status']='CLOSED';j['rr3_current']['packages'][key]['independent_review']='PASS'
  # 仅独立检查器的人工正常对照；被审文档绝不写入。
 pristine=(json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode();fixture=P/('isolated-'+kind+'.json');fixture.write_bytes(pristine)
 for phase,expected in [('normal',0),('mutated',1),('restored',0)]:
  if phase=='mutated':
   j=json.loads(pristine)
   if kind=='current':j['rr3_current']['packages']['A']['independent_review']='PENDING'
   elif kind=='fields':j['consumer_contract']['model_turn']['fields'].remove('deadline_unix_ms')
   elif kind=='hash':j['artifact_hashes'][next(iter(j['artifact_hashes']))]='0'*64
   else:del j['acceptances']
   fixture.write_text(json.dumps(j,ensure_ascii=False,indent=2)+'\n')
  if phase=='restored':fixture.write_bytes(pristine)
  cmd=[sys.executable,str(P/'review-runner.py'),'control-'+kind+'-'+phase,sys.executable,str(P/'fixture-check.py'),kind,str(fixture)]
  r=subprocess.run(cmd,capture_output=True,text=True);print(r.stdout);rows.append({'kind':kind,'phase':phase,'expected_exit':expected,'actual_exit':r.returncode,'fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest()})
 if fixture.read_bytes()!=pristine:raise RuntimeError('not restored')
(P/'controls-results.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n');sys.exit(any(x['actual_exit']!=x['expected_exit'] for x in rows))
