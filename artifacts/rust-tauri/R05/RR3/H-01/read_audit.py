from capture import *
triage=ROOT/'artifacts/rust-tauri/R05/RR3/R02-TRIAGE-01'
paths=[p for base in ['input','source','manifest','loghash','commands','exit'] for p in (triage/base).rglob('*') if p.is_file()]
paths += [p for p in triage.glob('input/*.json') if p.is_file()]
paths += list((ROOT/'artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/specifications').rglob('*.md'))
paths += [ROOT/'docs/rust-tauri/R00'/p for p in ['ACCEPTANCE_MAP.json','FEATURE_INVENTORY.json','FEATURE_STAGE_ACCEPTANCE.json']]
paths += [ROOT/'docs/rust-tauri/R05/repair-current'/p for p in ['RR1_MASTER_PROMPT_2026-10-04.md','RR2_MASTER_PROMPT_2026-10-06.md','RR3_BRIEF.md','RR3_REVIEW_BRIEF.md','RR3_H_BRIEF.md','RR3_PROGRESS.md','RR3_ISSUE_MATRIX.json','RR3_HANDOFF.md']]
rows=[]
for p in sorted(set(paths)):
    before=sha(p);data=p.read_bytes();after=sha(p)
    rows.append({'path':str(p.relative_to(ROOT)),'UTC':utc(),'bytes':len(data),'before':before,'read':hashlib.sha256(data).hexdigest(),'after':after,'stable':before==after})
write(EV/'read-audit.json',rows)
spec=json.loads((ROOT/'docs/rust-tauri/R00/ACCEPTANCE_MAP.json').read_text())
write(EV/'R00-readonly-requirements.json',spec)
print({'readFiles':len(rows),'readBytes':sum(r['bytes'] for r in rows),'changedWhileReading':[r['path'] for r in rows if not r['stable']]})
