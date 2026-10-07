from pathlib import Path
import hashlib,json,datetime,subprocess
R=Path('/Users/study_superior/Desktop/Code/LingxiAgent'); O=Path('/private/tmp/rr3-doc-input-boundary-1pgmn8m7')
names=['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_HANDOFF.json','PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_NEGATIVE_GATE_REPORT.md','R05_PERFORMANCE_RESULTS.json','R05_LIVE_VERIFICATION.json','WORKER_MODEL_BOUNDARY.md','MODEL_USAGE_SEMANTICS.md','R05_INTERFACE_EVOLUTION.md']
paths=['docs/rust-tauri/R05/'+n for n in names]+['docs/rust-tauri/ORCHESTRATOR_PROGRESS.json']
rows=[]
for p in paths:
 b=(R/p).read_bytes(); obj={'path':p,'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b),'readUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
 if p.endswith('.json'):
  d=json.loads(b);obj['keys']=list(d);obj['rr3CurrentKeys']=list((d.get('rr3_current') or d.get('stages',{}).get('R05',{}).get('rr3_current') or {}).keys())
  if 'HANDOFF' in p:obj['selectedFields']={k:d.get(k) for k in ['source_sha','working_tree_digest','artifact_hashes','unresolved_items','git','git_receipts','commit_receipts']};obj['consumerContractKeys']=list(d.get('consumer_contract',{}))
 else:obj['headings']=[x for x in b.decode().splitlines() if x.startswith('#')]
 rows.append(obj)
(O/'document-inputs.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(rows,ensure_ascii=False,indent=2))
graph=[]
for s in ['R05','R04','R03','R02']:
 p='rust/crates/xtask/src/stage_maps/'+s+'.json';b=(R/p).read_bytes();d=json.loads(b);graph.append({'path':p,'sha256':hashlib.sha256(b).hexdigest(),'commands':d['commands']})
(O/'stage-command-graph.json').write_text(json.dumps(graph,ensure_ascii=False,indent=2)+'\n')
for g in graph:
 print(g['path'])
 for name,v in g['commands'].items():print(name, json.dumps(v['argv'],ensure_ascii=False))
