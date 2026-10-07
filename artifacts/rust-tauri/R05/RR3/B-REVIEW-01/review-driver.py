#!/usr/bin/env python3
"""独立验收记录器：只写本审查证据及隔离副本。"""
import datetime
import hashlib
import json
import os
import pathlib
import re
import subprocess

ROOT = pathlib.Path('/Users/study_superior/Desktop/Code/LingxiAgent')
EV = ROOT / 'artifacts/rust-tauri/R05/RR3/B-REVIEW-01'
CARGO = '/Users/study_superior/.cargo/bin/cargo'
TARGET = 'svc:r05_t01_model_plane'
BFILES = ['scripts/rust-tauri/r05_t08_negative_gate.sh', 'scripts/rust-tauri/r05_t08_mutate_pin.py', 'scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py']
TABLE = 'docs/rust-tauri/R05/r05_stage_pins.tsv'

def sha(data):
    return hashlib.sha256(data).hexdigest()

def save(name, value):
    (EV / name).write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def manifest(root):
    paths = []
    for folder in ['rust', 'scripts/rust-tauri', 'docs/rust-tauri', 'contracts']:
        for base, dirs, files in os.walk(root / folder):
            dirs[:] = [d for d in dirs if d not in ('target', '.git')]
            for name in files:
                path = pathlib.Path(base) / name
                if path.is_file():
                    paths.append(path)
    paths.append(root / 'rust-toolchain.toml')
    items = [{'path': str(p.relative_to(root)), 'sha256': sha(p.read_bytes())} for p in sorted(set(paths))]
    return {'digestSha256': sha(json.dumps(items, sort_keys=True, separators=(',', ':')).encode()), 'files': items}

def run(name, argv, cwd=ROOT, env=None):
    receipt = {'command': argv, 'cwd': str(cwd), 'startedAt': now()}
    with (EV / (name + '.log')).open('w') as out:
        result = subprocess.run(argv, cwd=cwd, env=env, stdout=out, stderr=subprocess.STDOUT)
    receipt.update(endedAt=now(), exitCode=result.returncode, logSha256=sha((EV / (name + '.log')).read_bytes()))
    save(name + '-receipt.json', receipt)
    print(name, result.returncode, flush=True)
    return result.returncode

source_before = manifest(ROOT)
save('source-before.json', source_before)
save('environment.json', {
    'startedAt': now(), 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
    'branch': subprocess.check_output(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], cwd=ROOT, text=True).strip(),
    'gitStatus': subprocess.check_output(['git', 'status', '--short'], cwd=ROOT, text=True),
    'cargo': subprocess.check_output([CARGO, '--version'], cwd=ROOT, text=True).strip(),
    'rustc': subprocess.check_output(['/Users/study_superior/.cargo/bin/rustc', '--version'], cwd=ROOT, text=True).strip(),
    'os': subprocess.check_output(['uname', '-a'], text=True).strip(),
    'node': subprocess.check_output(['node', '--version'], text=True).strip(),
    'npm': subprocess.check_output(['npm', '--version'], text=True).strip(),
    'boundary': '主树只读；官方脚本本地 clone 写自己的新隔离副本，不写主树 Git；仅本目录保存验收材料。其他包并发修改，不代表总树冻结。'
})
assert run('bash-n', ['bash', '-n', BFILES[0]]) == 0
assert run('implementation-selfcheck', ['python3', BFILES[2]]) == 0
assert run('official-n03', ['bash', BFILES[0], str(EV / 'n03'), '--case', 'N03']) == 0
doc = json.loads((EV / 'n03/case-results.json').read_text())
assert doc['scope'] == 'N03' and len(doc['cases']) == 1 and len(doc['unexecutedCases']) == 15
copy = pathlib.Path(doc['isolatedCopyPath'])
assert copy != ROOT and copy.is_dir()
save('copy-before-phases.json', manifest(copy))
source_after = manifest(ROOT)
save('source-after.json', source_after)
before = {r['path']: r['sha256'] for r in source_before['files']}
after = {r['path']: r['sha256'] for r in source_after['files']}
copied = {r['path']: r['sha256'] for r in manifest(copy)['files']}
save('source-copy-boundary.json', {
    'copy': str(copy), 'sourceBeforeDigest': source_before['digestSha256'],
    'sourceAfterDigest': source_after['digestSha256'], 'copyDigest': manifest(copy)['digestSha256'],
    'sourceChangedPaths': [p for p in sorted(before.keys() | after.keys()) if before.get(p) != after.get(p)],
    'copyVsSourceAfterChangedPaths': [p for p in sorted(copied.keys() | after.keys()) if copied.get(p) != after.get(p)],
    'reviewedBFilesUnchanged': all(before[p] == after[p] == copied[p] for p in BFILES + [TABLE]),
    'bFileHashes': {p: copied[p] for p in BFILES + [TABLE]},
})
assert all(before[p] == after[p] == copied[p] for p in BFILES + [TABLE])

env = os.environ.copy()
env['CARGO_NET_OFFLINE'] = 'true'
env['CARGO_TARGET_DIR'] = '/Users/study_superior/.cache/lingxi-r05-neg-target'
test_argv = [CARGO, 'test', '--manifest-path', 'rust/Cargo.toml', '--locked', '-p', 'xtask', '--bin', 'xtask', 'stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites', '--', '--exact']

def phase(name):
    inputs = manifest(copy)
    save(name + '-inputs.json', inputs)
    code = run(name, test_argv, copy, env)
    log = (EV / (name + '.log')).read_text()
    binary = pathlib.Path(re.search(r'Running unittests .* \((.+)\)', log).group(1))
    receipt = json.loads((EV / (name + '-receipt.json')).read_text())
    receipt.update(inputDigest=inputs['digestSha256'], afterInputDigest=manifest(copy)['digestSha256'],
                   binaryPath=str(binary), binarySha256=sha(binary.read_bytes()),
                   lockSha256=sha((copy / 'rust/Cargo.lock').read_bytes()),
                   tableSha256=sha((copy / TABLE).read_bytes()),
                   testCounts=re.findall(r'test result: .*', log),
                   boundary='真实 xtask 注册镜像；运行时读取隔离副本权威 TSV；不模拟 cargo/镜像/注入器。')
    assert receipt['inputDigest'] == receipt['afterInputDigest']
    save(name + '-receipt.json', receipt)
    return code, receipt

pristine = (copy / TABLE).read_bytes()
normal_code, normal = phase('independent-normal')
assert normal_code == 0
assert run('independent-inject', ['python3', str(copy / BFILES[1]), str(copy / TABLE), str(EV / 'independent-mutation.json')], copy) == 0
mut = json.loads((EV / 'independent-mutation.json').read_text())
assert mut['matches'] == mut['mutations'] == 1 and mut['new'] == mut['old'] - 1
mutated_code, mutated = phase('independent-mutated')
assert mutated_code == 101
log = (EV / 'independent-mutated.log').read_text()
assert TARGET in log and 'drifted' in log and '0 passed; 1 failed; 0 ignored' in log
(copy / TABLE).write_bytes(pristine)
assert (copy / TABLE).read_bytes() == pristine
restored_code, restored = phase('independent-restored')
assert restored_code == 0
assert normal['inputDigest'] == restored['inputDigest']
normal_items = {r['path']: r['sha256'] for r in json.loads((EV / 'independent-normal-inputs.json').read_text())['files']}
mutated_items = {r['path']: r['sha256'] for r in json.loads((EV / 'independent-mutated-inputs.json').read_text())['files']}
assert [p for p in normal_items if normal_items[p] != mutated_items[p]] == [TABLE]
save('phases-result.json', {
    'findingId': 'F45', 'normalExit': normal_code, 'mutationExit': 0, 'redExit': mutated_code,
    'restoreExit': restored_code, 'onlyChangedInput': TABLE, 'restoredInputDigestEqual': True,
    'binaryHashes': {k: v['binarySha256'] for k, v in [('normal', normal), ('red', mutated), ('restored', restored)]},
    'copy': str(copy), 'finishedAt': now(), 'status': 'PASS',
})
