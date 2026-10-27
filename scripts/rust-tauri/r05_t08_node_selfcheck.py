#!/usr/bin/env python3
"""真实小Git + 已安装包内容验证默认Node准备，不运行Rust或完整16负测。"""
import argparse
import datetime
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / 'scripts/rust-tauri/r05_t08_prepare_node.py'
spec = importlib.util.spec_from_file_location('prepare_node', HELPER)
prep = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prep)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    ev = args.evidence.resolve()
    ev.mkdir(parents=True, exist_ok=False)
    area = Path(tempfile.mkdtemp(prefix='r05-node-selfcheck-'))
    source = area / 'source'
    source.mkdir()
    input_paths = [HELPER, Path(__file__).resolve(), ROOT / 'scripts/rust-tauri/r05_t08_negative_gate.sh', ROOT / 'scripts/rust-tauri/r02_client_leaf_matrix.py', ROOT / 'scripts/rust-tauri/r02_owned_process_group.py', ROOT / 'package.json', ROOT / 'package-lock.json']
    producer_inputs = {str(p.relative_to(ROOT)): prep.digest(p) for p in input_paths}
    commands = []
    checks = []
    def run(name, argv, cwd=source, expected=0, named=None):
        started = datetime.datetime.now(datetime.timezone.utc).isoformat()
        process = subprocess.run([str(x) for x in argv], cwd=cwd, capture_output=True)
        (ev / f'{name}.stdout.log').write_bytes(process.stdout)
        (ev / f'{name}.stderr.log').write_bytes(process.stderr)
        text = (process.stdout + process.stderr).decode(errors='replace')
        ok = (process.returncode == expected if expected is not None else process.returncode != 0) and (named is None or named in text)
        commands.append({'name': name, 'argv': [str(x) for x in argv], 'cwd': str(cwd), 'startedUtc': started,
                         'finishedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'exitCode': process.returncode,
                         'stdoutSha256': prep.digest(ev / f'{name}.stdout.log'), 'stderrSha256': prep.digest(ev / f'{name}.stderr.log'), 'ok': ok})
        (ev / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        assert ok, (name, process.returncode, text[-2000:])
        return text
    def check(name, condition):
        checks.append({'name': name, 'ok': bool(condition)})
        assert condition, name
    # 只选真实ws/vitest/semver及其已安装闭包，不复制主树整个node_modules。
    hidden = json.loads((ROOT / 'node_modules/.package-lock.json').read_text())['packages']
    selected = set()
    def locate(name, parent=''):
        directory = ROOT / parent
        for ancestor in [directory, *directory.parents]:
            if not ancestor.is_relative_to(ROOT):
                break
            candidate = (ancestor / 'node_modules' / name).relative_to(ROOT).as_posix()
            if candidate in hidden and (ROOT / candidate).is_dir():
                return candidate
        return None
    def visit(key):
        if key in selected:
            return
        selected.add(key)
        package = json.loads((ROOT / key / 'package.json').read_text())
        for kind in ('dependencies', 'optionalDependencies', 'peerDependencies'):
            for name in package.get(kind, {}):
                found = locate(name, key)
                if found:
                    visit(found)
    for name in ('ws', 'vitest', 'semver'):
        visit(locate(name))
    for key in sorted(selected, key=lambda k: (len(k), k)):
        target = source / key
        if target.exists():
            continue
        shutil.copytree(ROOT / key, target, symlinks=True)
    (source / 'node_modules/.bin').mkdir(exist_ok=True)
    for key in sorted(selected):
        package = json.loads((source / key / 'package.json').read_text())
        bins = package.get('bin', {})
        if isinstance(bins, str):
            bins = {package['name'].split('/')[-1]: bins}
        if key.count('node_modules/') == 1:
            for name, rel in bins.items():
                link = source / 'node_modules/.bin' / name
                if not link.exists():
                    link.symlink_to(os.path.relpath(source / key / rel, link.parent))
    package = {'name': 'r05-node-preparation-fixture', 'version': '1.0.0', 'type': 'module',
               'engines': {'node': '>=24.12.0 <25'},
               'dependencies': {name: hidden['node_modules/' + name]['version'] for name in ('ws', 'vitest', 'semver')}}
    entries = {key: hidden[key] for key in selected}
    (source / 'package.json').write_text(json.dumps(package, indent=2) + '\n')
    (source / 'package-lock.json').write_text(json.dumps({'name': package['name'], 'version': package['version'], 'lockfileVersion': 3,
                                                        'packages': {'': package, **entries}}, indent=2) + '\n')
    (source / 'node_modules/.package-lock.json').write_text(json.dumps({'version': 'old-root-version', 'lockfileVersion': 3, 'packages': entries}, indent=2) + '\n')
    (source / '.gitignore').write_text('node_modules/\n')
    (source / '.npmrc').write_text('min-release-age=1\n')
    for directory in ('cli', 'core', 'shared'):
        shutil.copytree(ROOT / directory, source / directory)
    (source / 'tests').mkdir()
    shutil.copy2(ROOT / 'tests/cli-args.test.ts', source / 'tests/cli-args.test.ts')
    (source / 'scripts/rust-tauri').mkdir(parents=True)
    for name in ('r02_client_leaf_matrix.py', 'r02_owned_process_group.py'):
        shutil.copy2(ROOT / 'scripts/rust-tauri' / name, source / 'scripts/rust-tauri' / name)
    run('git-init', ['git', 'init', '-q', '-b', 'codex/rust-tauri-migration'])
    run('git-add', ['git', 'add', '.'])
    run('git-commit', ['git', '-c', 'user.email=fixture@invalid', '-c', 'user.name=Fixture', 'commit', '-qm', 'fixture'])
    # 合法未提交的根输入必须进入copy，不能盲用HEAD。
    package['description'] = 'legitimate dirty candidate input'
    (source / 'package.json').write_text(json.dumps(package, indent=2) + '\n')
    source_manifest = prep.inventory(source / 'node_modules')
    (ev / 'fixture-dependency-inputs.json').write_text(json.dumps({'selectedPackages': sorted(selected), 'files': source_manifest}, sort_keys=True) + '\n')
    old = area / 'old-default-copy'
    run('old-clone', ['git', 'clone', '-q', '--no-hardlinks', source, old])
    run('old-real-cli-red', ['node', 'cli/entry.ts', 'help'], old, expected=None, named="Cannot find package 'ws'")
    run('old-real-vitest-red', ['node', 'node_modules/vitest/vitest.mjs', '--version'], old, expected=None, named='MODULE_NOT_FOUND')
    # 直接提取默认脚本的真实准备调用，变量由夹具提供，不另写理想准备器。
    shell = (ROOT / 'scripts/rust-tauri/r05_t08_negative_gate.sh').read_text()
    start = shell.index('python3 "$ROOT/scripts/rust-tauri/r05_t08_prepare_node.py"')
    end = shell.index('\n# The copy', start)
    block = shell[start:end]
    check('preparation-before-pristine-and-gates', start < shell.index('snapshot_pristine()') < shell.index('cargo_in_copy test'))
    source_git_before = prep.inventory(source / '.git')
    check('default-shared-clone', 'clone --shared --quiet --branch codex/rust-tauri-migration' in shell)
    def prepare_case(name, expected=0, named=None, copy_override=None):
        copy = area / name
        run(name + '-clone', ['git', '-C', source, 'clone', '--shared', '--quiet', '--branch', 'codex/rust-tauri-migration', source, copy])
        out = ev / name
        out.mkdir()
        # 生产ROOT指向夹具，让helper和实际候选输入使用同一来源。
        shutil.copy2(HELPER, source / 'scripts/rust-tauri/r05_t08_prepare_node.py')
        command = 'ROOT="$1"; COPY="$2"; EV="$3"; fail() { echo "FAIL: $*" >&2; exit 1; };\n' + block
        run(name, ['bash', '-c', command, 'node-prepare', source, copy_override or copy, out], expected=expected, named=named)
        return copy, out / 'node-preparation/result.json'
    good, receipt = prepare_case('prepared')
    check('dirty-package-byte-equal', prep.read_inputs(source) == prep.read_inputs(good))
    run('prepared-real-cli', ['node', 'cli/entry.ts', 'help'], good, named='Usage:')
    run('prepared-real-vitest', [good / 'node_modules/.bin/vitest', 'run', 'tests/cli-args.test.ts'], good, named='Tests')
    cli_code = "import sys,pathlib,json;sys.path.insert(0,'scripts/rust-tauri');import r02_client_leaf_matrix as m;o=pathlib.Path(sys.argv[1]);o.mkdir();rows=[m.run_cli_case(o,*x) for x in m.CLI_CASES];print(json.dumps(rows));assert all(x['ok'] for x in rows)"
    run('production-client-cli-cases', ['python3', '-B', '-c', cli_code, ev / 'client-cli-cases'], good)
    run('verify-normal', ['python3', '-B', HELPER, '--verify', receipt, '--evidence', ev / 'verify-normal'])
    # 写时复制和独立目录使缓存、甚至包文件改写均不能污染来源；后者必须被核验拒绝。
    (good / 'node_modules/.vite').mkdir(exist_ok=True)
    (good / 'node_modules/.vite/selfcheck').write_text('candidate-only cache')
    run('verify-own-cache', ['python3', '-B', HELPER, '--verify', receipt, '--evidence', ev / 'verify-own-cache'])
    file = good / 'node_modules/ws/index.js'; original = file.read_bytes(); file.write_bytes(original + b'\n// selfcheck\n')
    check('source-not-mutated-by-copy-write', prep.inventory(source / 'node_modules') == source_manifest)
    run('verify-content-red', ['python3', '-B', HELPER, '--verify', receipt, '--evidence', ev / 'verify-content-red'], expected=None, named='copy dependencies changed')
    file.write_bytes(original)
    run('verify-content-restored', ['python3', '-B', HELPER, '--verify', receipt, '--evidence', ev / 'verify-content-restored'])
    def mutate_case(name, relative, change, named):
        path = source / relative; original = path.read_bytes()
        try:
            change(path)
            prepare_case(name, expected=None, named=named)
        finally:
            path.write_bytes(original)
    mutate_case('wrong-lock', 'package-lock.json', lambda p: p.write_text(p.read_text().replace('"ws": "8.', '"ws": "9.', 1)), 'package/lock mismatch')
    mutate_case('wrong-installed-version', 'node_modules/ws/package.json', lambda p: p.write_text(p.read_text().replace('"version": "8.', '"version": "9.', 1)), 'installed version mismatch')
    mutate_case('wrong-node-range', 'package.json', lambda p: p.write_text(p.read_text().replace('>=24.12.0 <25', '>=99')), 'package/lock mismatch')
    # 实际Node不满足合法但不同的候选引擎范围，须抵达工具链检查后拒绝。
    saved_package = (source / 'package.json').read_bytes()
    saved_lock = (source / 'package-lock.json').read_bytes()
    try:
        for name in ('package.json', 'package-lock.json'):
            p = source / name
            p.write_text(p.read_text().replace('>=24.12.0 <25', '>=99'))
        prepare_case('actual-node-version-refused', expected=None, named='command failed')
    finally:
        (source / 'package.json').write_bytes(saved_package)
        (source / 'package-lock.json').write_bytes(saved_lock)
    mutate_case('hidden-lock-mismatch', 'node_modules/.package-lock.json', lambda p: p.write_text(p.read_text().replace('sha512-', 'sha511-', 1)), 'installed lock mismatch')
    missing = source / 'node_modules/ws/index.js'; saved = missing.read_bytes()
    missing.unlink()
    try:
        prepare_case('incomplete-content', expected=None, named='command failed')
    finally:
        missing.write_bytes(saved)
    missing = source / 'node_modules/ws/package.json'; saved = missing.read_bytes(); missing.unlink()
    try:
        prepare_case('missing-package', expected=None, named='package content missing')
    finally:
        missing.write_bytes(saved)
    external = source / 'node_modules/escape'; external.symlink_to(ROOT / 'package.json')
    try:
        prepare_case('external-link', expected=None, named='dependency link escapes source')
    finally:
        external.unlink()
    prepare_case('destination-failure', expected=None, named='copy node_modules must not exist', copy_override=good)
    # 真正的文件系统拒写，不替换生产复制函数或制造假成功。
    blocked = area / 'read-only-copy-parent'
    blocked.mkdir()
    blocked.chmod(0o555)
    copy_code = "import importlib.util,pathlib,sys;s=importlib.util.spec_from_file_location('p',sys.argv[1]);m=importlib.util.module_from_spec(s);s.loader.exec_module(m);src=pathlib.Path(sys.argv[2]);m.clone_dependencies(src,pathlib.Path(sys.argv[3]),m.inventory(src))"
    try:
        run('real-copy-failure', ['python3', '-B', '-c', copy_code, HELPER, source / 'node_modules/ws', blocked / 'ws'], expected=None, named='PermissionError')
    finally:
        blocked.chmod(0o755)
    run('real-copy-restored', ['python3', '-B', '-c', copy_code, HELPER, source / 'node_modules/ws', blocked / 'ws'])
    shared = area / 'shared-object-check'
    run('shared-clone', ['git', '-C', source, 'clone', '--shared', '--quiet', '--branch', 'codex/rust-tauri-migration', source, shared])
    alternate = shared / '.git/objects/info/alternates'
    original = alternate.read_bytes()
    check('shared-object-source', Path(original.decode().strip()).resolve() == (source / '.git/objects').resolve())
    alternate.write_text(str(area / 'missing-object-store') + '\n')
    run('shared-objects-unavailable', ['python3', '-B', HELPER, '--source', source, '--copy', shared, '--evidence', ev / 'shared-objects-unavailable'], expected=None, named='command failed')
    alternate.write_bytes(original)
    run('shared-objects-restored', ['python3', '-B', HELPER, '--source', source, '--copy', shared, '--evidence', ev / 'shared-objects-restored'])
    check('shared-clone-main-git-unchanged', prep.inventory(source / '.git') == source_git_before)
    restored, _ = prepare_case('restored')
    run('restored-real-cli', ['node', 'cli/entry.ts', 'help'], restored, named='Usage:')
    run('restored-real-vitest', [restored / 'node_modules/.bin/vitest', 'run', 'tests/cli-args.test.ts'], restored, named='Tests')
    check('source-dependencies-unchanged', prep.inventory(source / 'node_modules') == source_manifest)
    check('producer-inputs-unchanged', all(prep.digest(ROOT / p) == sha for p, sha in producer_inputs.items()))
    result = {'status': 'PASS', 'producerInputsSha256': producer_inputs, 'fixtureRoot': str(area), 'commandCount': len(commands), 'checks': checks,
              'scope': '真实生产准备+真实Node/Git/CLI+Vitest；小依赖闭包替身；未跑默认16/完整R02/完整主依赖复制',
              'selectedPackages': len(selected)}
    (ev / 'result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
