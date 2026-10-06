"""Read-only review of R05 semantic coverage; probes touch scratch only.
The assembler probe reuses historical logs as fixtures; it is NOT a test rerun.
"""
import json, pathlib, hashlib, shutil, subprocess, sys
from collections import Counter
ROOT=pathlib.Path('/workspace/scratch/9b4397d87a13/LingxiAgent')
OUT=pathlib.Path('/workspace/scratch/9b4397d87a13/audit/gate')
stage=json.loads((ROOT/'rust/crates/xtask/src/stage_maps/R05.json').read_text())
scope=json.loads((ROOT/'docs/rust-tauri/R05/R05_SCOPE_MATRIX.json').read_text())
scopes={x['id']:x for x in scope['supplemental_leaves']}
ev=ROOT/'artifacts/rust-tauri/R05/FINAL-WFR2-1/verify-R05'
result=json.loads((ev/'verify-stage-result.json').read_text())
results={x['id']:x for x in result['supplementalLeafScenarios']}
leaf_map={x.split()[1]:x.split()[2] for x in (ROOT/'docs/rust-tauri/R05/r05_leaf_case_map.tsv').read_text().splitlines() if x.startswith('leafcase ')}
rows=[]
for leaf in stage['supplementalLeafScenarios']:
 if leaf['r00ExecutionStageIds']==['R05']:
  r=results[leaf['id']]
  case=leaf['assertionContract']['cases'][0]['case']
  rows.append({'id':leaf['id'],'required_then':leaf['r00Then'],'scope_share':scopes[leaf['id']]['r05_share'],'declared_later_share':leaf['laterShare'],'mapped_run':leaf_map[case],'recorded_status':r['status'],'deferred_to_stages':r['deferredToStages'],'original_assertion_cases':r['originalAssertionCases'],'recorded_reason':r['reason']})
(OUT/'six_r05_only_leaves.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2))
# Execute the checked-in Python assembler verbatim against copied log fixtures.
# The sole mutation is a C-ID name in the copied cid table, leaving all tests,
# test counts, passing logs and source files untouched.
text=(ROOT/'scripts/rust-tauri/r05_t08_stage_suites.sh').read_text()
start=text.index("python3 - \"$EVIDENCE_DIR\" << 'PYEOF' || fail \"case assembly failed\"")+len("python3 - \"$EVIDENCE_DIR\" << 'PYEOF' || fail \"case assembly failed\"")
code=text[start:].split('\nPYEOF',1)[0]
source=ev/'R05_SUITES'
probe=OUT/'cid_rename_fixture'
probe.mkdir(exist_ok=True)
for name in ['cases.jsonl','pin-table.txt','cid-table.txt']:
 shutil.copyfile(source/name,probe/name)
for line in (source/'pin-table.txt').read_text().splitlines():
 run=line.split()[1];name=run.replace(':','_').replace('/','_')+'.log'
 shutil.copyfile(source/name,probe/name)
table=(probe/'cid-table.txt').read_text()
assert 'R05-T02-C01' in table
(probe/'cid-table.txt').write_text(table.replace('R05-T02-C01','R05-T99-C99'))
completed=subprocess.run([sys.executable,'-',str(probe)],input=code,text=True,cwd=ROOT,capture_output=True)
(probe/'assembler-output.txt').write_text(completed.stdout+completed.stderr)
# Check every condition of stage_map.rs:3229-3297 on this sole name mutation.
pins={l.split()[1]:int(l.split()[2]) for l in (probe/'pin-table.txt').read_text().splitlines()}
rows_cid=[l.split() for l in (probe/'cid-table.txt').read_text().splitlines()]
ids={r[1] for r in rows_cid}
required={'R05-T01-C02','R05-T02-C12','R05-T03-C12','R05-T04-C16','R05-T05-C13','R05-T06-C12','R05-T07-C10','R05-T08-C01','R05-T08-C12'}
owned=[(r[2],n) for r in rows_cid for n in r[3].split('+')]
checks={'r05_prefix':all(x.startswith('R05-T') for x in ids),'exactly_91_ids':len(ids)==91,'nine_pinned_ids':required<=ids,'all_runs_registered':all(r[2] in pins for r in rows_cid),'one_owner_per_test':len(set(owned))==len(owned),'all_tests_owned':all(sum(r==run for r,_ in owned)==count for run,count in pins.items())}
assembled=json.loads((probe/'r05-cases.json').read_text()) if (probe/'r05-cases.json').exists() else {}
probe_report={'kind':'actual checked-in Python assembler executed on historical log fixtures (not Cargo rerun)','mutation':'rename R05-T02-C01 to non-existent R05-T99-C99 only in scratch cid-table.txt','assembler_exit':completed.returncode,'stdout':completed.stdout.strip(),'mirror_conditions_recomputed':checks,'original_id_present':any(c['case']=='R05-T02-C01' for c in assembled.get('cases',[])),'fabricated_id_present':any(c['case']=='R05-T99-C99' for c in assembled.get('cases',[])),'allCasesOk':assembled.get('allCasesOk'),'allSuitesOk':assembled.get('allSuitesOk'),'limitation':'This script exercises the original Python assembler on historical-log fixtures only. Separately performed actual Rust 1.98.1 mirror control and mutation runs are in cid-mirror-control.log and cid-mirror-mutated.log (both 7/7 PASS). It does not run full verify-stage.'}
(OUT/'cid_rename_probe.json').write_text(json.dumps(probe_report,ensure_ascii=False,indent=2))
print(json.dumps({'r05_only_leaves':len(rows),'all_registered_pass':all(r['recorded_status']=='PASS' for r in rows),'all_no_remaining_stage':all(r['deferred_to_stages']==[] for r in rows),'cid_rename_probe':probe_report},ensure_ascii=False,indent=2))
