#!/usr/bin/env python3
"""手写独立输入，直接运行被验注入器与主脚本汇总函数。"""
import datetime
import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path('/Users/study_superior/Desktop/Code/LingxiAgent')
EV = ROOT / 'artifacts/rust-tauri/R05/RR3/I-01/b41-new'
EV.mkdir()
HELPER = ROOT / 'scripts/rust-tauri/r05_t08_mutate_pin.py'
GATE = ROOT / 'scripts/rust-tauri/r05_t08_negative_gate.sh'
TARGET = 'svc:r05_t01_model_plane'
rows = [
    ('old-fixed-seven', b'pin svc:r05_t01_model_plane 7 r05_t01_model_plane\n', b'pin svc:r05_t01_model_plane 6 r05_t01_model_plane\n'),
    ('future-count-forty-two', b'pin svc:r05_t01_model_plane 42 r05_t01_model_plane\n', b'pin svc:r05_t01_model_plane 41 r05_t01_model_plane\n'),
    ('spacing-crlf-other-rows', b'# pin svc:r05_t01_model_plane 999 ignored\r\npin other:suite 8 x\r\n  pin\tsvc:r05_t01_model_plane\t17\t r05_t01_model_plane  \r\n', b'# pin svc:r05_t01_model_plane 999 ignored\r\npin other:suite 8 x\r\n  pin\tsvc:r05_t01_model_plane\t16\t r05_t01_model_plane  \r\n'),
    ('missing', b'pin other:suite 9 all\n', None),
    ('duplicate', b'pin svc:r05_t01_model_plane 7 all\npin svc:r05_t01_model_plane 7 all\n', None),
    ('conflicting', b'pin svc:r05_t01_model_plane 7 all\npin svc:r05_t01_model_plane 24 all\n', None),
    ('non-decimal', b'pin svc:r05_t01_model_plane bad all\n', None),
    ('fraction', b'pin svc:r05_t01_model_plane 7.5 all\n', None),
    ('signed', b'pin svc:r05_t01_model_plane +7 all\n', None),
    ('negative', b'pin svc:r05_t01_model_plane -7 all\n', None),
    ('zero', b'pin svc:r05_t01_model_plane 0 all\n', None),
    ('one', b'pin svc:r05_t01_model_plane 1 all\n', None),
    ('missing-filter', b'pin svc:r05_t01_model_plane 7\n', None),
    ('extra-field', b'pin svc:r05_t01_model_plane 7 all extra\n', None),
    ('non-ascii-count', 'pin svc:r05_t01_model_plane ７ all\n'.encode(), None),
]
results = []
for name, given, expected in rows:
    folder = EV / name
    folder.mkdir()
    table = folder / 'input.tsv'
    table.write_bytes(given)
    (folder / 'before.tsv').write_bytes(given)
    receipt = folder / 'mutation.json'
    argv = ['python3', str(HELPER), str(table), str(receipt)]
    start = datetime.datetime.now(datetime.timezone.utc).isoformat()
    run = subprocess.run(argv, capture_output=True, text=True)
    (folder / 'stdout.log').write_text(run.stdout)
    (folder / 'stderr.log').write_text(run.stderr)
    if expected is None:
        assert run.returncode == 1 and table.read_bytes() == given and not receipt.exists(), name
        assert TARGET in run.stderr, name
    else:
        assert run.returncode == 0 and table.read_bytes() == expected, name
        record = json.loads(receipt.read_text())
        assert record['matches'] == record['mutations'] == 1, name
        assert record['old'] - record['new'] == 1, name
        assert record['beforeSha256'] == hashlib.sha256(given).hexdigest(), name
        assert record['afterSha256'] == hashlib.sha256(expected).hexdigest(), name
    row = {'case': name, 'command': argv, 'startedAt': start, 'endedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'exitCode': run.returncode, 'expectedExit': 1 if expected is None else 0, 'beforeSha256': hashlib.sha256(given).hexdigest(), 'afterSha256': hashlib.sha256(table.read_bytes()).hexdigest(), 'receiptPresent': receipt.exists(), 'status': 'PASS'}
    (folder / 'receipt.json').write_text(json.dumps(row, indent=2) + '\n')
    results.append(row)

source = GATE.read_text()
summary = source[source.index('write_results() {'):source.index('\n# ── controls:')]
summary_file = EV / 'summary-function.sh'
summary_file.write_text(summary)
assert 'CASE_SCOPE=ALL\n' in source
assert source.count('run_n03\n') == 2
identities = [f'R05-GATE-N{i:02d}' for i in range(1, 17)]
actual = __import__('re').findall(r'record_case "(R05-GATE-N\d+)"', source)
assert sorted(actual) == identities and len(actual) == 16
full_rows = [f'{ident}\t101\tOK\tOK\tindependent fixture\n' for ident in identities]
checks = [('single-only', 'N03', [full_rows[2]], 0), ('single-empty', 'N03', [], 1),
          ('single-other', 'N03', [full_rows[0]], 1), ('single-duplicate', 'N03', [full_rows[2]] * 2, 1),
          ('full-sixteen', 'ALL', full_rows, 0), ('full-one-is-not-sixteen', 'ALL', [full_rows[2]], 1),
          ('full-empty', 'ALL', [], 1), ('full-duplicate', 'ALL', full_rows + [full_rows[2]], 1),
          ('full-bad-verdict', 'ALL', [r.replace('\tOK\tindependent', '\tBAD\tindependent') if i == 2 else r for i, r in enumerate(full_rows)], 1),
          ('full-wrong-identity', 'ALL', [r.replace('N03', 'N99') for r in full_rows], 1)]
checks += [(f'full-missing-N{i+1:02d}', 'ALL', full_rows[:i] + full_rows[i+1:], 1) for i in range(16)]
for name, scope, tsv, expected_exit in checks:
    folder = EV / name
    folder.mkdir()
    for control in ['control-xtask', 'control-binwiring', 'n03-restored']:
        (folder / control).mkdir()
        (folder / control / 'exit-code.txt').write_text('0\n')
    (folder / 'case-results.tsv').write_text(''.join(tsv))
    argv = ['bash', '-c', 'source "$1"; EV="$2"; CASE_SCOPE="$3"; COPY="fixture only"; fail() { exit 1; }; write_results', 'independent-summary', str(summary_file), str(folder), scope]
    start = datetime.datetime.now(datetime.timezone.utc).isoformat()
    run = subprocess.run(argv, capture_output=True, text=True)
    (folder / 'stdout.log').write_text(run.stdout)
    (folder / 'stderr.log').write_text(run.stderr)
    assert run.returncode == expected_exit, (name, run.stdout, run.stderr)
    doc = json.loads((folder / 'case-results.json').read_text())
    assert doc['allRefused'] == (expected_exit == 0), name
    if name == 'single-only':
        assert doc['expectedCases'] == ['R05-GATE-N03'] and len(doc['cases']) == 1 and len(doc['unexecutedCases']) == 15
    if name == 'full-sixteen':
        assert doc['expectedCases'] == identities and doc['unexecutedCases'] == []
    row = {'case': name, 'command': argv, 'startedAt': start, 'endedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'exitCode': run.returncode, 'expectedExit': expected_exit, 'status': 'PASS', 'boundary': '只测试正式脚本原汇总函数；TSV/controls是手写结果夹具，不表示真实执行这16项负测。'}
    (folder / 'receipt.json').write_text(json.dumps(row, indent=2, ensure_ascii=False) + '\n')
    results.append(row)
(EV / 'result.json').write_text(json.dumps({'status': 'PASS', 'checks': len(results), 'results': results, 'registeredIdentities': actual, 'defaultScope': 'ALL', 'implementationHelperSha256': hashlib.sha256(HELPER.read_bytes()).hexdigest(), 'gateSha256': hashlib.sha256(GATE.read_bytes()).hexdigest()}, indent=2, ensure_ascii=False) + '\n')
print(json.dumps({'status': 'PASS', 'checks': len(results)}))
