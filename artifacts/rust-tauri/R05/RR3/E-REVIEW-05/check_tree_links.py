#!/usr/bin/env python3
"""E-REVIEW-05 item 2/5: independent full-tree comparison vs E-04 after-snapshot,
MD link validity, ledger cross-consistency."""
import hashlib, json, os, re, subprocess, sys
from urllib.parse import unquote

REPO = '/Users/study_superior/Desktop/Code/LingxiAgent'
os.chdir(REPO)
results = []
def check(name, ok, detail=''):
    results.append({'name': name, 'ok': bool(ok), 'detail': str(detail)[:400]})
    print(('PASS ' if ok else 'FAIL ') + name + ((' | ' + str(detail)[:200]) if detail else ''))

def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()

# ---- independent tree enumeration, same semantics as candidate binder / E-04 snapshot ----
out = subprocess.run(['git', '--no-optional-locks', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'],
                     capture_output=True)
assert out.returncode == 0
paths = [p.decode() for p in out.stdout.split(b'\0') if p]
after = json.load(open('artifacts/rust-tauri/R05/RR3/E-04/tree-snapshot-after.json'))['files']
cur = {}
for p in paths:
    if p.startswith('artifacts/rust-tauri/R05/RR3/E-REVIEW-05/'):
        continue  # my own evidence root (post-E-04, expected)
    if p.startswith('artifacts/rust-tauri/R05/RR3/E-04/'):
        continue  # E-04's own evidence root — excluded from its own snapshot by design
    if os.path.islink(p) or not os.path.isfile(p):
        cur[p] = 'NONREGULAR'
    else:
        cur[p] = sha256(p)

added = sorted(set(cur) - set(after))
removed = sorted(set(after) - set(cur))
changed = sorted(p for p in set(cur) & set(after) if cur[p] != after[p]['sha256'])
# post-E-04 writes expected: coordinator's own ledger entry (E-04 must not own it) + the E-REVIEW-05 brief
added_expected = {'docs/rust-tauri/R05/repair-current/RR3_E_R5_REVIEW_BRIEF.md'}
changed_expected = {'docs/rust-tauri/R05/repair-current/RR3_PROGRESS.md'}  # 总控台账, E-04 后新增 E04 行 (07:14 local)
added_unexpected = [p for p in added if p not in added_expected]
changed_unexpected = [p for p in changed if p not in changed_expected]
check('独立全树对比 vs E-04 after 快照: 意外变更=0、删除=0、意外新增=0（预期外仅总控台账 1 改+brief 1 增）',
      not changed_unexpected and not removed and not added_unexpected,
      f'changed={changed} removed={removed[:5]} added={added} unexpected_changed={changed_unexpected} unexpected_added={added_unexpected}')
# confirm the RR3_PROGRESS.md delta is only the appended E04/E-REVIEW-05-dispatch line by 总控
prog = open('docs/rust-tauri/R05/repair-current/RR3_PROGRESS.md', encoding='utf-8').read()
check('RR3_PROGRESS.md 变化内容=E04 SELF_CHECKED 追加行（总控台账职责，非 E-04 所有权文件）',
      prog.rstrip().endswith('待E-REVIEW-05新独立审。') and 'E04实施SELF_CHECKED' in prog, '')

# ---- MD relative link validity over the 5 owned MD docs ----
def md_links(path):
    base = os.path.dirname(path)
    txt = open(path, encoding='utf-8').read()
    links = re.findall(r'\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)', txt)
    rel = []
    for l in links:
        if l.startswith(('http://', 'https://', '#', 'mailto:')):
            continue
        target = unquote(l.split('#')[0])
        if not target:
            continue
        rel.append((l, os.path.normpath(os.path.join(base, target))))
    return rel

total, bad = 0, []
for md in ['docs/rust-tauri/R05/R05_REPORT.md', 'docs/rust-tauri/R05/R05_INDEPENDENT_REVIEW.md',
           'docs/rust-tauri/R05/R05_BLOCKERS.md', 'docs/rust-tauri/R05/R05_NEGATIVE_GATE_REPORT.md',
           'docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md']:
    for l, resolved in md_links(md):
        total += 1
        if not os.path.exists(resolved):
            bad.append((md, l))
check('5 份 owned MD 相对链接全部解析存在', not bad, f'total_links={total} bad={bad[:5]}')

# ---- ledger cross-consistency (matrix / progress / report / HANDOFF) ----
def strict(path):
    def hook(pairs):
        keys = [k for k, _ in pairs]
        dups = sorted({k for k in keys if keys.count(k) > 1})
        if dups: raise ValueError('duplicate keys %r' % dups)
        return dict(pairs)
    return json.load(open(path), object_pairs_hook=hook)

matrix = strict('docs/rust-tauri/R05/repair-current/RR3_ISSUE_MATRIX.json')
handoff = strict('docs/rust-tauri/R05/R05_HANDOFF.json')
progress_txt = open('docs/rust-tauri/R05/repair-current/RR3_PROGRESS.md', encoding='utf-8').read()
report_txt = open('docs/rust-tauri/R05/R05_REPORT.md', encoding='utf-8').read()

fg = matrix['finalGate']
ok = fg['round'] == 'RR3/FINAL-04' and fg['overall'] == 'PASS' and fg['sixTuple']['R06_READY'] is True \
     and matrix['stageStatus'] == 'ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS' \
     and matrix['R06_READY'] is True
check('矩阵: finalGate RR3/FINAL-04 PASS + stageStatus/R06_READY 翻正', ok, '')

h = handoff['rr3_current']
ok = h['offline_gate'] == 'PASS' and h['R06_READY'] is True \
     and h['live_verification'] == 'BLOCKED_NOT_AUTHORIZED' and h['release_state'] == 'NOT_IN_SCOPE'
check('HANDOFF: rr3_current 与矩阵 finalGate 同值', ok, '')

ok = ('E04实施SELF_CHECKED' in progress_txt or 'E04' in progress_txt) and '待E-REVIEW-05' in progress_txt \
     and 'FINAL-04全新阶段终审完成' in progress_txt
check('进度台账: FINAL-04 完成 + E04 SELF_CHECKED + 待 E-REVIEW-05 记录一致', ok, '')

ok = 'ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS' in report_txt and 'R06_READY' in report_txt \
     and '130/130' in report_txt
check('报告: §13 六元组/130/130 与矩阵一致', ok, '')

# F42–F54 all CLOSED in matrix
fids = [i['id'] for i in matrix['issues']]
all_closed = all(i['status'] == 'CLOSED' for i in matrix['issues'] if i['id'].startswith('F') and i['id'] != 'R05-ENV-R00')
r00 = [i for i in matrix['issues'] if i['id'] == 'R05-ENV-R00'][0]
check('矩阵: F42–F54 全部 CLOSED（R05-ENV-R00 为观察属性项）',
      all_closed and r00['status'] == 'OBSERVED_PASS_FOR_CURRENT_INSTANCE_ENV_ATTRIBUTED',
      f'issues={fids}')

print()
print(json.dumps({'total': len(results), 'failed': sum(1 for r in results if not r['ok'])}, ensure_ascii=False))
with open('artifacts/rust-tauri/R05/RR3/E-REVIEW-05/tree-links-results.json', 'w') as f:
    json.dump(results, f, ensure_ascii=False, indent=1)
sys.exit(0 if all(r['ok'] for r in results) else 1)
