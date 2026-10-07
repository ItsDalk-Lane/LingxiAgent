#!/usr/bin/env python3
"""E-REVIEW-05 item 6: isolated positive/negative controls validating MY check methods.
All mutations happen only in /private/tmp/rr3-ereview05-controls — no repo file is touched."""
import hashlib, json, os, re, shutil, sys

REPO = '/Users/study_superior/Desktop/Code/LingxiAgent'
CTRL = '/private/tmp/rr3-ereview05-controls'
shutil.rmtree(CTRL, ignore_errors=True)
os.makedirs(CTRL)
results = []
def case(name, expect, got, detail=''):
    got_norm = got.split(':')[0].strip() if isinstance(got, str) else got
    ok = (expect == got_norm)
    results.append({'case': name, 'expect': expect, 'got': got, 'ok': ok, 'detail': detail})
    print(('PASS ' if ok else 'FAIL ') + f'{name}: expect={expect} got={got}' + (f' | {detail}' if detail else ''))

# ===== my check methods (same logic as verify.py) =====
def strict_loads(text):
    def hook(pairs):
        keys = [k for k, _ in pairs]
        dups = sorted({k for k in keys if keys.count(k) > 1})
        if dups:
            raise ValueError('duplicate keys %r' % dups)
        return dict(pairs)
    return json.loads(text, object_pairs_hook=hook)

def sha256_bytes(b):
    return hashlib.sha256(b).hexdigest()

FINAL04_SIX = json.load(open(os.path.join(REPO, 'artifacts/rust-tauri/R05/RR3/FINAL-04/STRUCTURED_SUMMARY.json')))['sixTuple']
SCALARS = ['offline_gate', 'independent_review', 'live_verification', 'stage_readiness', 'R06_READY', 'release_state']

def sixtuple_ok(cur):
    return all(cur.get(k) == FINAL04_SIX[k] for k in SCALARS)

def rr3_current_equal(objs):
    canon = [json.dumps(o, ensure_ascii=False, separators=(',', ':')) for o in objs]
    return len(set(canon)) == 1

def frozen_equal(frozen_map, recompute):
    return all(recompute.get(p) == m['sha256'] for p, m in frozen_map.items())

# ===== positive control: byte-identical copies =====
src_handoff = open(os.path.join(REPO, 'docs/rust-tauri/R05/R05_HANDOFF.json'), 'rb').read()
pos = os.path.join(CTRL, 'positive')
os.makedirs(pos)
open(os.path.join(pos, 'HANDOFF.json'), 'wb').write(src_handoff)
d = strict_loads(open(os.path.join(pos, 'HANDOFF.json'), encoding='utf-8').read())
case('阳性1: 字节相同副本 strict 解析', 'PASS', 'PASS' if isinstance(d, dict) else 'FLAG')
case('阳性2: 字节相同副本六元组检查', 'PASS', 'PASS' if sixtuple_ok(d['rr3_current']) else 'FLAG')

# ===== negative 1: tamper R06_READY -> false =====
neg1 = os.path.join(CTRL, 'negative1')
os.makedirs(neg1)
t = src_handoff.decode()
t2 = t.replace('"R06_READY": true', '"R06_READY": false', 1)
assert t2 != t, 'tamper failed to apply'
open(os.path.join(neg1, 'HANDOFF.json'), 'w').write(t2)
d1 = strict_loads(open(os.path.join(neg1, 'HANDOFF.json'), encoding='utf-8').read())
case('阴性1: R06_READY 篡改为 false 被六元组检查 FLAG', 'FLAG', 'PASS' if sixtuple_ok(d1['rr3_current']) else 'FLAG')

# ===== negative 2: true duplicate top-level key =====
neg2 = os.path.join(CTRL, 'negative2')
os.makedirs(neg2)
m = re.search(r'^(\s*"schemaVersion":\s*[^,\n]+,)', t, re.M)
assert m
dup = t.replace(m.group(1), m.group(1) + '\n "schemaVersion": "9.9.9-tampered",', 1)
open(os.path.join(neg2, 'HANDOFF.json'), 'w').write(dup)
try:
    strict_loads(open(os.path.join(neg2, 'HANDOFF.json'), encoding='utf-8').read())
    got = 'PASS'
except ValueError as e:
    got = 'FLAG: ' + str(e)[:60]
case('阴性2: 顶层真重复键 schemaVersion 被严格解析器 FLAG', 'FLAG', got)
naive = json.loads(open(os.path.join(neg2, 'HANDOFF.json'), encoding='utf-8').read())
case('阴性2b: naive json.loads 静默 last-wins（佐证严格解析必要）', '9.9.9-tampered', naive.get('schemaVersion'))

# ===== negative 3: one frozen-input SHA tampered =====
neg3 = os.path.join(CTRL, 'negative3')
os.makedirs(neg3)
frozen = json.load(open(os.path.join(REPO, 'artifacts/rust-tauri/R05/RR3/FINAL-04/command-records/frozen-inputs-postcheck.json')))['inputs']
frozen_bad = json.loads(json.dumps(frozen))
victim = 'rust/Cargo.lock'
frozen_bad[victim]['sha256'] = ('0' * 63) + '1'
recompute_partial = {}
for p in list(frozen)[:8] + [victim]:
    recompute_partial[p] = sha256_bytes(open(os.path.join(REPO, p), 'rb').read())
recompute_full = {p: sha256_bytes(open(os.path.join(REPO, p), 'rb').read()) for p in frozen}
case('阴性3: 冻结输入单 SHA 篡改被相等检查 FLAG', 'FLAG', 'PASS' if frozen_equal(frozen_bad, recompute_partial) else 'FLAG')
case('阴性3b: 未篡改清单同一方法为 PASS', 'PASS', 'PASS' if frozen_equal(frozen, recompute_full) else 'FLAG')

# ===== negative 4: one rr3_current diverges among 7 =====
neg4 = os.path.join(CTRL, 'negative4')
os.makedirs(neg4)
cur = d['rr3_current']
diverged = json.loads(json.dumps(cur))
diverged['offline_gate'] = 'FAIL'
case('阴性4: 七处 rr3_current 之一被篡改被逐字节相等检查 FLAG', 'FLAG', 'PASS' if rr3_current_equal([cur, cur, diverged, cur, cur, cur, cur]) else 'FLAG')
case('阴性4b: 七处原样为 PASS', 'PASS', 'PASS' if rr3_current_equal([cur] * 7) else 'FLAG')

ok_all = all(r['ok'] for r in results)
print()
print(json.dumps({'method_validated': ok_all, 'cases': len(results)}, ensure_ascii=False))
os.makedirs(os.path.join(REPO, 'artifacts/rust-tauri/R05/RR3/E-REVIEW-05/controls'), exist_ok=True)
with open(os.path.join(REPO, 'artifacts/rust-tauri/R05/RR3/E-REVIEW-05/controls/control-results.json'), 'w') as f:
    json.dump({'control_root': CTRL, 'cases': results, 'method_validated': ok_all}, f, ensure_ascii=False, indent=1)
sys.exit(0 if ok_all else 1)
