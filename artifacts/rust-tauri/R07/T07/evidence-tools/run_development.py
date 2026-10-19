#!/usr/bin/env python3
"""保留开发期真实命令、失败和前后源码；源变更时不冒称固定候选验收。"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[5]
OUT = Path(__file__).resolve().parents[1]
def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sources():
    paths = sorted(p for p in (ROOT / 'rust').rglob('*') if p.is_file()
                   and 'target' not in p.parts and (p.suffix in ('.rs', '.toml') or p.name == 'Cargo.lock'))
    return [{'path': str(p.relative_to(ROOT)), 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths]
def fingerprint(rows):
    return hashlib.sha256(json.dumps(rows, separators=(',', ':')).encode()).hexdigest()
label = sys.argv[1]
argv = sys.argv[2:]
if argv[:1] == ['--']:
    argv = argv[1:]
assert argv and label.replace('-', '').replace('_', '').isalnum()
receipt = OUT / f'dev-{label}.json'
log = OUT / f'dev-{label}.log'
assert not receipt.exists() and not log.exists(), '保留已有运行，用新标签'
before = sources()
env = os.environ.copy()
provided = {'CARGO_TARGET_DIR': '/private/tmp/lingxi-r07-build', 'CARGO_BUILD_JOBS': '4',
            'LINGXI_HOME': '/private/tmp/lingxi-r07-tests/home', 'TMPDIR': '/private/tmp/lingxi-r07-tests/tmp'}
env.update(provided)
result = {'stage': 'R07-T07', 'purpose': 'development_not_final_acceptance', 'command': argv,
          'cwd': str(ROOT), 'environment': provided, 'startedAt': now(),
          'baseCommit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
          'sourceFingerprintBefore': fingerprint(before), 'sourceFilesBefore': before}
with log.open('wb') as stream:
    done = subprocess.run(argv, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
after = sources()
result.update({'endedAt': now(), 'exitCode': done.returncode, 'sourceFingerprintAfter': fingerprint(after),
               'sourceFilesAfter': after, 'log': log.name, 'logSha256': hashlib.sha256(log.read_bytes()).hexdigest()})
result['sourceUnchanged'] = result['sourceFingerprintBefore'] == result['sourceFingerprintAfter']
result['versionBinding'] = 'UNCHANGED_RUST_SOURCE' if result['sourceUnchanged'] else 'SOURCE_CHANGED_DURING_DEVELOPMENT_NOT_FIXED_CANDIDATE'
receipt.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({k: result[k] for k in ('exitCode', 'sourceUnchanged', 'versionBinding', 'log', 'logSha256')}))
sys.exit(done.returncode)
