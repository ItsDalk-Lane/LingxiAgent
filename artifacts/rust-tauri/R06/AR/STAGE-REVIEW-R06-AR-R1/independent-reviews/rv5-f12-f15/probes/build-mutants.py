#!/usr/bin/env python3
"""RV5 F15 adversarial mutants — 3 NEW self-made mutations of the fresh positive evidence."""
import json, shutil, hashlib, copy, pathlib

BASE = pathlib.Path('/tmp/rv5-env/f15rt')
OUT = BASE / 'mutants'
OUT.mkdir(exist_ok=True)
REC = [json.loads(l) for l in open(BASE / 'out/records.jsonl')]
FAC = [json.loads(l) for l in open(BASE / 'raw/facts.jsonl')]
sha = lambda b: hashlib.sha256(b).hexdigest()

def write(case, recs, facs):
    d = OUT / case
    d.mkdir(parents=True, exist_ok=True)
    (d / 'records.jsonl').write_text('\n'.join(json.dumps(r, ensure_ascii=False) for r in recs) + '\n')
    (d / 'facts.jsonl').write_text('\n'.join(json.dumps(f, ensure_ascii=False) for f in facs) + '\n')
    return d

# ── M1: BE-03 self-consistent retained-file forgery (disk + recovery record + facts digest) ──
ws = pathlib.Path('/tmp/rv5-env/f15rt/tmp/lingxi-r06t08-be03-ws-2998-19')
retained_name = '.lingxi-recovery-9359c85d038cb69b45e421bbe5952bbf.original'
retained = ws / retained_name
rec_json = BASE / 'tmp/lingxi-r06t08-be03-home-2998-12/file-recovery-records/9359c85d038cb69b45e421bbe5952bbf.json'
pristine_retained = retained.read_bytes()
pristine_rec = rec_json.read_text()
tampered = pristine_retained + b'\nTAMPERED-BY-RV5'
tampered_sha = sha(tampered)
fac1 = copy.deepcopy(FAC)
for f in fac1:
    if f['kind'] == 'world_final' and 'be03' in json.dumps(f['value']):
        f['value']['workspaceListing'] = [[n, tampered_sha if n == retained_name else h]
                                          for n, h in f['value']['workspaceListing']]
rec_obj = json.loads(pristine_rec)
rec_obj['expected']['sha256'] = tampered_sha          # forger aligns the record with tampered bytes
rec_obj['expected']['size'] = len(tampered)
retained.write_bytes(tampered)
rec_json.write_text(json.dumps(rec_obj, ensure_ascii=False))
d1 = write('m1-recovery-selfconsistent-forgery', REC, fac1)
# restore pristine disk state immediately (facts copy carries the forgery)
retained.write_bytes(pristine_retained)
rec_json.write_text(pristine_rec)
print('M1 built:', d1, 'tampered_sha:', tampered_sha[:16])

# ── M2: BE-04 numeric drift agreed across record+facts SQL+wire; only the real production DB disagrees ──
rec2 = copy.deepcopy(REC)
fac2 = copy.deepcopy(FAC)
be04 = [r for r in rec2 if r['taskId'] == 'BE-04'][0]
run1 = 'run_000001a1386e2cda_000001'
def bump(v):
    if v == 40: return 41
    return v
# facts: SQL readback rows + wire usage frames
for f in fac2:
    v = f.get('value', {})
    if f['kind'] == 'usage_sql_readback' and v.get('runId') == run1:
        for r in v['rows']:
            r['input_tokens'] = bump(r['input_tokens'])
    if f['kind'] == 'provider_exchange':
        frames_ = v.get('responseFrames') or v.get('frames') or []
        for fr in frames_:
            if isinstance(fr, dict) and isinstance(fr.get('usage'), dict) and fr['usage'].get('prompt_tokens') == 40:
                fr['usage']['prompt_tokens'] = 41
# record: align totals with the forged 41
t = be04['tokens']
t['inputTokens'] = 101
t['inputCoverage']['total'] = 101
t['inputCoverage']['knownSubtotal'] = 101
t['byLedgerOutcome']['succeeded']['input']['total'] = 41
t['byLedgerOutcome']['succeeded']['input']['knownSubtotal'] = 41
for row in t['ledgerRows']:
    row['inputTokens'] = bump(row['inputTokens'])
d2 = write('m2-be04-numeric-drift-vs-real-db', rec2, fac2)
print('M2 built:', d2)

# ── M3: BE-01 full LIVE claim (label AND flags flipped together) ──
rec3 = copy.deepcopy(REC)
be01 = [r for r in rec3 if r['taskId'] == 'BE-01'][0]
be01['comparisonMode'] = 'REAL_LIVE_PROVIDER_VERIFIED'
be01['liveProviderExecuted'] = True
be01['incumbentEngineExecuted'] = True
d3 = write('m3-be01-live-claim-flags-true', rec3, copy.deepcopy(FAC))
print('M3 built:', d3)
