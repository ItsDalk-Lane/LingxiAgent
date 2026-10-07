#!/usr/bin/env python3
"""E-REVIEW-05 item 3/4: historical FAIL retention, forbidden phrase, boundary retention, r00 consistency."""
import json, os, re, sys

REPO = '/Users/study_superior/Desktop/Code/LingxiAgent'
os.chdir(REPO)
results = []
def check(name, ok, detail=''):
    results.append({'name': name, 'ok': bool(ok), 'detail': detail})
    print(('PASS ' if ok else 'FAIL ') + name + ((' | ' + str(detail)[:200]) if detail else ''))

def read(p):
    with open(p, encoding='utf-8') as f:
        return f.read()

REPORT = read('docs/rust-tauri/R05/R05_REPORT.md')
REVIEW = read('docs/rust-tauri/R05/R05_INDEPENDENT_REVIEW.md')
BLOCKERS = read('docs/rust-tauri/R05/R05_BLOCKERS.md')
NEG = read('docs/rust-tauri/R05/R05_NEGATIVE_GATE_REPORT.md')
MUS = read('docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md')
HANDOFF_TXT = read('docs/rust-tauri/R05/R05_HANDOFF.json')
ALL_MD = {'REPORT': REPORT, 'REVIEW': REVIEW, 'BLOCKERS': BLOCKERS, 'NEG': NEG, 'MUS': MUS}
# where each historical fact must remain findable (at least one current doc)
HIST = [
    ('FINAL-01 FAIL(56 嵌套.git 夹具→绑定器拒收) 可寻且标历史', r'FINAL-01', [REPORT, REVIEW, BLOCKERS, HANDOFF_TXT]),
    ('FINAL-02 FAIL(唯一 symlink 条目) 可寻且标历史', r'FINAL-02', [REPORT, REVIEW, HANDOFF_TXT]),
    ('FINAL-03 FAIL(F53 flake+F54 46叶分类) 可寻且标历史', r'FINAL-03', [REPORT, REVIEW, HANDOFF_TXT]),
    ('G01 真实默认失败(exit2/15目标红) 保留', r'G-REVIEW-01', [REPORT, NEG, BLOCKERS, HANDOFF_TXT]),
    ('G02 ENOSPC 空间阻断历史保留(BLOCKED_BY_STORAGE)', r'ENOSPC|BLOCKED_BY_STORAGE', [REPORT, NEG, BLOCKERS, HANDOFF_TXT]),
    ('E-REVIEW-01 FAIL(MF-E01/MF-E02) 保留', r'MF-E01', [REPORT, REVIEW, HANDOFF_TXT]),
    ('A-REVIEW-01 FAIL 保留', r'A-REVIEW-01', [REPORT, REVIEW, HANDOFF_TXT]),
    ('RR2 层不稳(R04 8/8、R03 15/15 checkpoint 不稳) 保留', r'15/15|checkpoint.{0,6}不稳|不稳定', [REPORT, REVIEW, HANDOFF_TXT, BLOCKERS]),
]
for name, pat, docs in HIST:
    hits = [i for i, d in enumerate(docs) if re.search(pat, d)]
    check(name, bool(hits), f'matched_in={len(hits)} doc(s)')

# FINAL layer FAIL three-round detail retention
check('RR3/FINAL-01/02/03 三轮 offline_gate=FAIL 原样保留(报告历史层表)',
      all(re.search(pat, REPORT) for pat in (r'FINAL-01', r'FINAL-02', r'FINAL-03')) and ('历史' in REPORT), '')

# 禁语 "只剩ALF"
for n, d in ALL_MD.items():
    occ = [m.start() for m in re.finditer(r'只剩\s*ALF', d)]
    ok = True; ctx = []
    for pos in occ:
        window = d[max(0, pos-40):pos+40]
        ctx.append(window.replace('\n', ' '))
        # allowed only in negation/citation form (不能说/不写/不得/旧 …)
        if not re.search(r'不能说|不写|不得|不再|不是|禁止|否|旧', window):
            ok = False
    check(f'禁语「只剩ALF」不复现于 {n}（否定引用除外）', ok, f'occurrences={len(occ)} ctx={ctx[:2]}')
ok_h = all(re.search(r'不能说|不写|不得|不再|不是|禁止|否|旧', HANDOFF_TXT[max(0,m.start()-40):m.start()+40]) for m in re.finditer(r'只剩\s*ALF', HANDOFF_TXT))
check('禁语「只剩ALF」不复现于 HANDOFF（否定引用除外）', ok_h, f'occurrences={len(re.findall(chr(21482)+chr(21097)+r"ALF", HANDOFF_TXT))}')

# raw npm registered red
for n, d in [('REPORT', REPORT), ('NEG', NEG)]:
    check(f'raw npm 登记 red 不写全绿保留于 {n}',
          re.search(r'raw npm.{0,80}(登记|registered).{0,40}(红|red)|registered-not-formal-green|不写全绿|未写全绿', d) is not None, '')

# directed / E5 legal scope
check('directed(E0–E4.5)与 E5 BY SCOPE SKIP 原许可保留',
      re.search(r'directed', REPORT) and re.search(r'E5', REPORT) and re.search(r'SKIP|跳过', REPORT), '')

# LIVE boundary
for n, d in [('REPORT', REPORT), ('BLOCKERS', BLOCKERS)]:
    check(f'LIVE BLOCKED_NOT_AUTHORIZED(RR-BLK-CREDENTIALS 最迟 R10) 保留于 {n}',
          'BLOCKED_NOT_AUTHORIZED' in d and 'R10' in d, '')

# platform boundary
for n, d in [('REPORT', REPORT), ('BLOCKERS', BLOCKERS)]:
    check(f'平台边界(Linux 继承未复验/Windows 未验证) 保留于 {n}',
          ('Linux' in d and 'Windows' in d and ('未复验' in d or '未验证' in d)), '')

# r00 four-object statements (item 4)
srcs = {
    'FINAL-01': read('artifacts/rust-tauri/R05/RR3/FINAL-01/STAGE_REVIEW.md'),
    'FINAL-02': read('artifacts/rust-tauri/R05/RR3/FINAL-02/STAGE_REVIEW.md'),
    'FINAL-03': read('artifacts/rust-tauri/R05/RR3/FINAL-03/STAGE_REVIEW.md'),
    'FINAL-04': read('artifacts/rust-tauri/R05/RR3/FINAL-04/STAGE_REVIEW.md'),
    'D-REVIEW-01': read('artifacts/rust-tauri/R05/RR3/D-REVIEW-01/REVIEW.md'),
}
check('源事实: D-01 c5975a45/D-R1 9f748902 曾被拦(FINAL-01 记「均被 ALF 阻断(20s 0 字节)」、D-R1 记 20s 内读取 0 字节)',
      'c5975a45' in srcs['FINAL-01'] and '9f748902' in srcs['FINAL-01'] and re.search(r'20s.{0,6}0\s*字节|0\s*字节|20s内读取0字节', srcs['D-REVIEW-01']), '')
check('源事实: 43d95970 连续三轮(FINAL-01/02/03)放行、无用户操作证据',
      all('43d95970' in srcs[k] for k in ('FINAL-01', 'FINAL-02', 'FINAL-03'))
      and all(re.search(r'无证据.{0,6}(需要|表明).{0,8}(用户|防火墙)', srcs[k]) for k in ('FINAL-01', 'FINAL-02', 'FINAL-03')), '')
check('源事实: FINAL-04 cf9bce2f+d57ea731/CDHash 364514be 两新对象 6 次 LAN 全过、ALF 放行',
      'cf9bce2f' in srcs['FINAL-04'] and 'd57ea731' in srcs['FINAL-04'] and '364514be' in srcs['FINAL-04'] and re.search(r'6\s*次.{0,10}(全部)?真实通过|LAN 行为本轮 6 次全部真实通过', srcs['FINAL-04']), '')
check('源事实: 各 STAGE_REVIEW 均保留按实例观察属性措辞(非永久解除)',
      all(re.search(r'按.{0,4}(二进制)?实例.{0,4}偶发|观察属性', srcs[k]) for k in ('FINAL-01', 'FINAL-02', 'FINAL-03', 'FINAL-04')), '')

cur = REPORT + BLOCKERS + HANDOFF_TXT
check('现行文档: 四对象 SHA(c5975a45/9f748902/43d95970/cf9bce2f+d57ea731) 全部可寻',
      all(s in cur for s in ('c5975a45', '9f748902', '43d95970', 'cf9bce2f', 'd57ea731', '364514be')), '')
check('现行文档: 9f748902 明确标历史被拦、未写成放行',
      re.search(r'9f748902[^。]{0,60}(曾|历史).{0,30}(拦|阻断|0/1/0/0)', cur) is not None, '')
check('现行文档: 「无证据需要用户防火墙操作」与 D 终审口径一致',
      re.search(r'无证据(表明)?需要用户防火墙操作', cur) is not None, '')
neg_perm = [m.start() for m in re.finditer(r'永久解除', cur)]
ok_perm = all(re.search(r'不能写成|不.{0,4}写成|未写成', cur[max(0,p-30):p+10]) for p in neg_perm) and neg_perm
check('现行文档: 观察属性未写成永久解除（每处「永久解除」均为否定式）', bool(ok_perm), f'occurrences={len(neg_perm)}')

# BLOCKERS current section state
check('BLOCKERS 当前段: RR3 无未关闭必需缺口 + 登记项(RR-BLK-CREDENTIALS/平台/R05-ENV-R00 观察属性)',
      re.search(r'无未关闭|没有未关闭|RR-BLK-CREDENTIALS', BLOCKERS) and 'R05-ENV-R00' in BLOCKERS and '观察属性' in BLOCKERS, '')

print()
print(json.dumps({'total': len(results), 'failed': sum(1 for r in results if not r['ok'])}, ensure_ascii=False))
with open('artifacts/rust-tauri/R05/RR3/E-REVIEW-05/history-results.json', 'w') as f:
    json.dump(results, f, ensure_ascii=False, indent=1)
sys.exit(0 if all(r['ok'] for r in results) else 1)
