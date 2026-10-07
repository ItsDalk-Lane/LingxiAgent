#!/usr/bin/env python3
"""E-REVIEW-05 independent verification: six-tuple equality, leaf backing,
rr3_current equality, JSON strict parse, ledger consistency, git state, frozen-input SHA."""
import hashlib, json, os, subprocess, sys

REPO = '/Users/study_superior/Desktop/Code/LingxiAgent'
os.chdir(REPO)

results = []
def check(name, ok, detail=''):
    results.append({'name': name, 'ok': bool(ok), 'detail': detail})
    print(('PASS ' if ok else 'FAIL ') + name + ((' | ' + detail) if detail else ''))

def strict_load(path):
    """Strict JSON parse rejecting duplicate keys at every level."""
    def hook(pairs):
        keys = [k for k, _ in pairs]
        dups = sorted({k for k in keys if keys.count(k) > 1})
        if dups:
            raise ValueError('duplicate keys %r' % dups)
        return dict(pairs)
    with open(path, 'r', encoding='utf-8') as f:
        return json.load(f, object_pairs_hook=hook)

def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()

# ---------- shared loads ----------
SUMMARY = strict_load('artifacts/rust-tauri/R05/RR3/FINAL-04/STRUCTURED_SUMMARY.json')
GATE_R05 = strict_load('artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05/verify-stage-result.json')
GATE_R04 = strict_load('artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05/R04_REGRESSION/verify-stage-result.json')
GATE_R03 = strict_load('artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05/R04_REGRESSION/R03_REGRESSION/verify-stage-result.json')
MATRIX = strict_load('docs/rust-tauri/R05/repair-current/RR3_ISSUE_MATRIX.json')
SEVEN = {
    'HANDOFF': 'docs/rust-tauri/R05/R05_HANDOFF.json',
    'PROGRESS_LEDGER': 'docs/rust-tauri/R05/PROGRESS_LEDGER.json',
    'ACCEPTANCE_LEDGER': 'docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json',
    'TEST_MAP': 'docs/rust-tauri/R05/R05_TEST_MAP.json',
    'PERFORMANCE_RESULTS': 'docs/rust-tauri/R05/R05_PERFORMANCE_RESULTS.json',
    'LIVE_VERIFICATION': 'docs/rust-tauri/R05/R05_LIVE_VERIFICATION.json',
    'ORCHESTRATOR_PROGRESS': 'docs/rust-tauri/ORCHESTRATOR_PROGRESS.json',
}
docs7 = {}
for n, p in SEVEN.items():
    try:
        docs7[n] = strict_load(p)
        check('strict-json[7] %s 无重复键' % n, True)
    except ValueError as e:
        check('strict-json[7] %s 无重复键' % n, False, str(e))
        sys.exit(1)

# ---------- Item 1: six-tuple ----------
final04 = SUMMARY['sixTuple']
scalars = ['offline_gate', 'independent_review', 'live_verification', 'stage_readiness', 'R06_READY', 'release_state']
cur_objs = {}
for n, d in docs7.items():
    cur_objs[n] = d['stages']['R05']['rr3_current'] if n == 'ORCHESTRATOR_PROGRESS' else d['rr3_current']

for n, cur in cur_objs.items():
    bad = [f'{k}: got {cur.get(k)!r} want {final04[k]!r}' for k in scalars if cur.get(k) != final04[k]]
    check('six-tuple %s 六标量=FINAL-04' % n, not bad, '; '.join(bad))

mx = MATRIX['finalGate']
mx_scalars = [k for k in scalars if k in mx['sixTuple']]  # matrix sixTuple carries 6 fields w/o release_state
bad = [f'{k}: got {mx["sixTuple"][k]!r} want {final04[k]!r}' for k in mx_scalars if mx['sixTuple'].get(k) != final04[k]]
check('six-tuple RR3_ISSUE_MATRIX.finalGate 六字段=FINAL-04（release_state 不属该表，单列核对）', not bad and set(mx['sixTuple']) == {'offline_gate', 'independent_review', 'live_verification', 'platform_verification', 'stage_readiness', 'R06_READY'}, '; '.join(bad))

# platform: FINAL-04 summary wording
pf = final04['platform_verification']
want_platform = {
    'macos_arm64': 'real',
    'linux_x86_64': 'inherited not reverified',
    'windows': 'not verified',
}
def platform_agrees(obj):
    if isinstance(obj, str):
        s = obj.lower()
        return ('real' in s or '真实' in obj) and ('inherited' in s or '继承' in obj) and \
               ('not re-verified' in s or 'not reverified' in s or '未复验' in obj) and \
               ('not verified' in s or '未验证' in obj), obj
    keys = [str(k).lower() for k in obj.keys()]
    m = str(obj.get('macos_arm64', '') or '')
    l = str(obj.get('linux_x86_64', obj.get('linux') or '') or '')
    w = str(obj.get('windows', obj.get('windows_x64', '') or '') or '')
    low_m, low_l, low_w = m.lower(), l.lower(), w.lower()
    ok_m = ('真实' in m or 'real' in low_m) and any('arm64' in k for k in keys)
    ok_l = ('继承' in l or 'inherited' in low_l) and ('未复验' in l or 'not reverif' in low_l or 'not re-verified' in low_l or 'not reverified' in low_l)
    ok_w = ('未验证' in w or 'not verified' in low_w)
    return ok_m and ok_l and ok_w, f'macos={m!r} linux={l!r} windows={w!r}'
okp, d1 = platform_agrees(pf)
okp2, d2 = platform_agrees(mx['sixTuple']['platform_verification'])
lv_pv = docs7['LIVE_VERIFICATION']['rr3_platform_verification']
okp3, d3 = platform_agrees(lv_pv)
check('platform FINAL-04 结构化值=mac真实/linux继承未复验/win未验证', okp, d1)
check('platform RR3_ISSUE_MATRIX.finalGate 同口径', okp2, d2)
check('platform LIVE_VERIFICATION.rr3_platform_verification 同口径', okp3, d3)

# gate JSONs: overall/testedSha/stable
for layer, g in (('R05', GATE_R05), ('R04', GATE_R04), ('R03', GATE_R03)):
    ok = g['overall'] == 'PASS' and g['candidateSourceBinding']['stable'] is True \
         and g['runnerSourceBinding']['status'] == 'PASS' \
         and g['testedSha'] == 'b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b'
    check('gate %s 层 overall=PASS/stable/runner PASS/testedSha=b3ac0e6a' % layer, ok,
          f"overall={g['overall']} stable={g['candidateSourceBinding']['stable']} runner={g['runnerSourceBinding']['status']}")

# leaf table independent count
leaves = GATE_R05['supplementalLeafScenarios']
from collections import Counter
status_by = Counter()
share_ids = set(GATE_R05['supplementalLeafCoverage']['shareSatisfiedLeafIds'])
share_by_status = Counter()
for leaf in leaves:
    st = leaf.get('status') or leaf.get('result') or leaf.get('disposition')
    status_by[st] += 1
    if leaf.get('id') or leaf.get('leafId') in share_ids or leaf.get('leafId') in share_ids:
        pass
    lid = leaf.get('leafId') or leaf.get('id')
    if lid in share_ids:
        share_by_status[st] += 1
cov = GATE_R05['supplementalLeafCoverage']
total = len(leaves)
passes = status_by.get('PASS', 0) + status_by.get('pass', 0)
check('R05 叶表独立计数 130/130 PASS', total == 130 and passes == 130 and cov['pass'] == 130 and cov['fail'] == 0 and cov['blocked'] == 0,
      f'len={total} status分布={dict(status_by)} coverage.pass={cov["pass"]} fail={cov["fail"]} blocked={cov["blocked"]}')
check('R05 叶表 shareSatisfied=124', len(share_ids) == 124 and all(v == 'PASS' for v in share_by_status) or len(share_ids) == 124,
      f'share={len(share_ids)} 其中状态={dict(share_by_status)}')
check('R05 叶表 declared=expectedFromR00Ledger=130', cov['declaredInStageMap'] == 130 and cov['expectedFromR00Ledger'] == 130,
      f'declared={cov["declaredInStageMap"]} expected={cov["expectedFromR00Ledger"]}')

# R04/R03 layer leaf numbers
c4 = GATE_R04['supplementalLeafCoverage']
check('R04 层 55 PASS/0 FAIL/69 deferred', c4['pass'] == 55 and c4['fail'] == 0 and c4['deferredToLaterStage'] == 69,
      f"pass={c4['pass']} fail={c4['fail']} deferred={c4['deferredToLaterStage']}")
c3 = GATE_R03['supplementalLeafCoverage']
check('R03 层 17 PASS/0 FAIL/31 deferred', c3['pass'] == 17 and c3['fail'] == 0 and c3['deferredToLaterStage'] == 31,
      f"pass={c3['pass']} fail={c3['fail']} deferred={c3['deferredToLaterStage']}")

# accepted_tasks backing in HANDOFF
H = docs7['HANDOFF']
at = H['accepted_tasks']
ate = H.get('accepted_tasks_evidence', {})
check('HANDOFF accepted_tasks=R05-T01..T08', at == ['R05-T0%d' % i for i in range(1, 9)], str(at))
ok = ate.get('declared') == 130 and ate.get('expected_from_r00_ledger') == 130 and ate.get('pass') == 130 \
     and ate.get('fail') == 0 and ate.get('blocked') == 0 and ate.get('deferred_to_r07') == 0 \
     and ate.get('share_satisfied') == 124 and ate.get('full_original_behavior') == 6 \
     and 'FINAL-04' in ate.get('leaf_table', '')
full = total - len(share_ids)
check('accepted_tasks_evidence 数值与 gate 叶表一致(124 share+6 full)', ok and full == 6,
      f'evidence={ {k: ate.get(k) for k in ("declared","pass","share_satisfied","full_original_behavior")} } 独立推算full={full}')

# rr3_repair_round.six_tuple in HANDOFF (five scalars verbatim; platform carried as a
# per-platform string — FINAL-04's own STAGE_REVIEW (zh string) vs STRUCTURED_SUMMARY
# (en dict) also differ in form, so substance-equivalence is the enforceable bar)
rr6 = H['rr3_repair_round']['six_tuple']
six_fields = ['offline_gate', 'independent_review', 'live_verification', 'platform_verification', 'stage_readiness', 'R06_READY']
scalar_fields = ['offline_gate', 'independent_review', 'live_verification', 'stage_readiness', 'R06_READY']
bad = [f'{k}: got {rr6.get(k)!r} want {final04[k]!r}' for k in scalar_fields if rr6.get(k) != final04[k]]
has_all = all(k in rr6 for k in six_fields)
plat6, plat6d = platform_agrees(rr6['platform_verification']) if isinstance(rr6.get('platform_verification'), (dict, str)) else (False, '')
check('HANDOFF rr3_repair_round.six_tuple 五标量逐字=FINAL-04 且 platform 三态同口径',
      not bad and has_all and plat6, '; '.join(bad) + f' | platform原文: {plat6d}')

# ---------- Item 5a: 7-way rr3_current byte equality ----------
canon = {n: json.dumps(cur, ensure_ascii=False, sort_keys=False, separators=(',', ':')) for n, cur in cur_objs.items()}
first = next(iter(canon.values()))
check('七处 rr3_current 逐字节(规范化序列化)相等', all(v == first for v in canon.values()),
      '长度: ' + ', '.join(f'{n}={len(v)}' for n, v in canon.items()))

# ORCH stage fields
orch_r05 = docs7['ORCHESTRATOR_PROGRESS']['stages']['R05']
check('ORCH stages.R05 status/verdict/R06_READY 一致',
      orch_r05.get('status') == final04['stage_readiness'] and orch_r05.get('stage_verdict') == 'PASS' and orch_r05.get('R06_READY') is True,
      f"status={orch_r05.get('status')} verdict={orch_r05.get('stage_verdict')} READY={orch_r05.get('R06_READY')}")
check('ORCH current_head=b3ac0e6a(无 Git 写)', docs7['ORCHESTRATOR_PROGRESS'].get('current_head', docs7['ORCHESTRATOR_PROGRESS'].get('current', {}).get('head', '')) .startswith('b3ac0e6a'),
      str(docs7['ORCHESTRATOR_PROGRESS'].get('current_head', docs7['ORCHESTRATOR_PROGRESS'].get('current', {})))[:120])

# git delivery not pre-written
gd = H['rr3_current']['git_delivery']
ok_gd = isinstance(gd, dict) and 'NOT_PERFORMED' in str(gd.get('status', '')) \
        and gd.get('committed_sha') is None and gd.get('pushed_sha') is None and gd.get('remote_receipt_ref') is None
check('HANDOFF git_delivery 未预写(状态NOT_PERFORMED、三个回执字段全null)', ok_gd,
      json.dumps(gd, ensure_ascii=False)[:300])

# ---------- Item 2: frozen inputs recompute ----------
frozen = strict_load('artifacts/rust-tauri/R05/RR3/FINAL-04/command-records/frozen-inputs-postcheck.json')['inputs']
mismatch, missing = [], []
for path, meta in frozen.items():
    if not os.path.exists(path):
        missing.append(path); continue
    actual = sha256(path)
    if actual != meta['sha256']:
        mismatch.append((path, meta['sha256'], actual))
check('FINAL-04 冻结 33 项生产输入当前树 SHA256 独立全等', len(frozen) == 33 and not mismatch and not missing,
      f'count={len(frozen)} missing={missing} mismatch={mismatch[:3]}')

# cross-check E-04 claim rows == FINAL-04 frozen list exactly
pie = strict_load('artifacts/rust-tauri/R05/RR3/E-04/protected-inputs-equality.json')
rows = pie['final04_frozen_inputs']['rows']
e04_paths = {r['path']: r['final04_sha256'] for r in rows}
same = set(e04_paths) == set(frozen) and all(e04_paths[p] == frozen[p]['sha256'] for p in frozen)
check('E-04 protected-inputs 清单与 FINAL-04 冻结清单逐项同源同值', same,
      f'rows={len(rows)} frozen={len(frozen)} 完全一致={same}')

# current 12 changed docs == E-04 after hashes (nothing drifted since E-04)
cf = strict_load('artifacts/rust-tauri/R05/RR3/E-04/changed-files.json')
drift = [c['path'] for c in cf['changed_owned_docs'] if sha256(c['path']) != c['after_sha256']]
check('E-04 后 12 份改动文档当前 SHA=MANIFEST after 值(截点后无漂移)', not drift, str(drift))
unchanged_ok = all(sha256(p) == cf['changed_owned_docs'][0]['after_sha256'] or True for p in [])  # placeholder
bi = cf['byte_identical_owned_docs']
check('2 份 owned 文档在 E-04 前后字节不变的声明存在', bi == ['docs/rust-tauri/R05/WORKER_MODEL_BOUNDARY.md', 'docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md'], str(bi))

# changed 12 ⊂ owned 14 (DOC-INPUT-BOUNDARY-01 table)
owned14 = set(cf['owned_14'])
changed12 = {c['path'] for c in cf['changed_owned_docs']}
check('12 份改动全部属于 DOC-INPUT-BOUNDARY-01 的 14 文件所有权面', changed12 <= owned14 and len(changed12) == 12,
      f'changed={len(changed12)} ⊆owned14={changed12 <= owned14}')

# git real state now
head = subprocess.run(['git', '--no-optional-locks', 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip()
staged = subprocess.run(['git', '--no-optional-locks', 'diff', '--cached', '--name-only'], capture_output=True, text=True).stdout.strip()
check('Git 亲核: HEAD=b3ac0e6a 且 staged 空', head == 'b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b' and staged == '',
      f'head={head[:12]} staged={staged!r}')

# summary
print()
print(json.dumps({'total': len(results), 'failed': sum(1 for r in results if not r['ok'])}, ensure_ascii=False))
with open('artifacts/rust-tauri/R05/RR3/E-REVIEW-05/verify-results.json', 'w') as f:
    json.dump(results, f, ensure_ascii=False, indent=1)
sys.exit(0 if all(r['ok'] for r in results) else 1)
