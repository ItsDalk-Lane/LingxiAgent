#!/usr/bin/env python3
"""round3（C01-C03）验证命令证据 runner — 与 round2/run-evidence.py 同构。

用法：
  python3 artifacts/f1-f12-repair/round3-c01-c03/run-evidence.py <label> <command...>

行为：快照源码 manifest（排除 artifacts 非 .py 与 *.patch）→ 执行命令 →
日志脱敏（仓库根 → <WORKTREE>，真实 home → <USER_HOME>）落盘 → 追加
COMMAND_RESULTS.json（含命令前后 manifest 漂移标记）。
"""
import os, sys, json, hashlib, subprocess, platform, datetime, pathlib, re

root = pathlib.Path(__file__).resolve().parents[3]
out = root / 'artifacts/f1-f12-repair/round3-c01-c03'
BASE = '67dee5d2de9d3b9fc75ec5ef5c555e93c65b3ccd'

def digest(b): return hashlib.sha256(b).hexdigest()

def manifest():
    paths = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=root).decode().split('\0')
    rows = []
    for p in sorted(set(paths)):
        if not p: continue
        if p.startswith('artifacts/f1-f12-repair/') and not p.endswith('.py'): continue
        if p.endswith('.patch'): continue
        f = root / p
        if f.is_file():
            b = f.read_bytes(); rows.append({'path': p, 'bytes': len(b), 'sha256': digest(b)})
    return {
        'sourceIdentity': {'kind': 'worktree', 'base': BASE},
        'exclusions': [
            'artifacts/f1-f12-repair/** except *.py (generated evidence, reports, delivery files and synthetic test home)',
            '*.patch (delivery patches)',
            'git ignored generated files; git ls-files --cached --others --exclude-standard',
        ],
        'files': rows,
    }

m = manifest()
raw = (json.dumps(m, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
h = digest(raw)
(out / 'manifests').mkdir(exist_ok=True)
(out / 'manifests' / f'{h}.json').write_bytes(raw)
(out / 'SOURCE_MANIFEST.json').write_bytes(raw)

if len(sys.argv) < 3:
    print(h); sys.exit(0)

label = sys.argv[1]; cmd = sys.argv[2:]
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
env = os.environ.copy()
env['LINGXI_HOME'] = str(out / 'synthetic-home')
env['HANA_HOME'] = env['LINGXI_HOME']
env['NO_COLOR'] = '1'
p = subprocess.run(cmd, cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
log = p.stdout.replace(str(root).encode(), b'<WORKTREE>').replace(str(pathlib.Path.home()).encode(), b'<USER_HOME>')
f = out / 'logs' / f'{label}.log'
f.parent.mkdir(exist_ok=True)
f.write_bytes(log)
clean = re.sub(r'\x1b\[[0-9;]*m', '', log.decode(errors='replace'))
rows = re.findall(r'^\s*Tests\s+(.+)$', clean, re.M)
counts = None
if rows:
    counts = {}
    for k in ['passed', 'failed', 'skipped', 'todo']:
        match = re.search(r'(\d+) ' + k, rows[-1])
        counts[k] = int(match.group(1)) if match else 0
after = (json.dumps(manifest(), ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
record = {
    'command': cmd, 'cwd': '<WORKTREE>', 'platform': platform.system(), 'arch': platform.machine(),
    'node': subprocess.check_output(['node', '--version']).decode().strip(),
    'npm': subprocess.check_output(['npm', '--version']).decode().strip(),
    'start': start, 'end': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'exitCode': p.returncode, 'counts': counts,
    'sourceChangedDuringCommand': digest(after) != h,
    'endSourceManifestHash': digest(after),
    'log': f'logs/{f.name}', 'logSha256': digest(log), 'sourceManifestHash': h,
}
results = out / 'COMMAND_RESULTS.json'
data = json.loads(results.read_text()) if results.exists() else []
data.append(record)
results.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
print(log.decode(errors='replace')[-6000:])
print('EVIDENCE', label, 'EXIT', p.returncode, 'MANIFEST', h)
sys.exit(p.returncode)
