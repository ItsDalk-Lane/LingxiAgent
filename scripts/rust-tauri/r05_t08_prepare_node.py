#!/usr/bin/env python3
"""为负测准备有来源记录、独立可写的 Node 依赖；不安装或修改来源。"""
import argparse
import ctypes
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys

INPUTS = ('package.json', 'package-lock.json', '.npmrc')
# 这些仅为依赖工具的可重建输出，不是包内容；副本自行生成，绝不链接来源。
CACHES = {'.cache', '.vite', '.vite-temp'}


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inventory(root):
    require(root.is_dir() and not root.is_symlink(), f'dependency directory missing or linked: {root}')
    rows = {}
    for base, dirs, files in os.walk(root, followlinks=False):
        parent = Path(base)
        if parent == root:
            dirs[:] = sorted(d for d in dirs if d not in CACHES)
        for name in sorted(dirs + files):
            path = parent / name
            rel = path.relative_to(root).as_posix()
            info = path.lstat()
            if path.is_symlink():
                target = os.readlink(path)
                require(not os.path.isabs(target) and path.resolve(strict=True).is_relative_to(root.resolve()),
                        f'dependency link escapes source: {rel}')
                rows[rel] = {'link': target}
            elif path.is_file():
                rows[rel] = {'sha256': digest(path), 'size': info.st_size, 'mode': stat.S_IMODE(info.st_mode)}
            else:
                require(path.is_dir(), f'unsupported dependency file: {rel}')
    return rows


def read_inputs(root):
    result = {}
    for name in INPUTS:
        path = root / name
        if name == '.npmrc' and not path.exists():
            result[name] = None
            continue
        require(path.is_file() and not path.is_symlink(), f'input missing or linked: {name}')
        result[name] = digest(path)
    return result


def validate_metadata(root):
    package = json.loads((root / 'package.json').read_text())
    lock = json.loads((root / 'package-lock.json').read_text())
    hidden = json.loads((root / 'node_modules/.package-lock.json').read_text())
    require(lock.get('lockfileVersion') == 3 and hidden.get('lockfileVersion') == 3, 'lockfile version must be 3')
    entries, installed = lock['packages'], hidden['packages']
    for key in ('name', 'version', 'dependencies', 'devDependencies', 'optionalDependencies', 'engines'):
        require(package.get(key, {}) == entries[''].get(key, {}), f'package/lock mismatch: {key}')
    missing_optional = []
    obsolete_entries = []
    for key, entry in entries.items():
        if not key:
            continue
        if not key.startswith('node_modules/'):
            require(entry.get('extraneous') is True and not package.get('workspaces') and not (root / key).exists(), f'unsupported local dependency: {key}')
            obsolete_entries.append(key)
            continue
        require('..' not in Path(key).parts, f'invalid lock path: {key}')
        if key not in installed:
            require(entry.get('optional') is True and not (root / key).exists(), f'locked dependency missing: {key}')
            missing_optional.append(key)
    for key, entry in installed.items():
        if key in obsolete_entries:
            require(entry == entries[key], f'obsolete lock metadata mismatch: {key}')
            continue
        require(key in entries and key.startswith('node_modules/'), f'unlocked dependency: {key}')
        for field in ('version', 'resolved', 'integrity', 'dependencies', 'optionalDependencies', 'bin'):
            require(entry.get(field) == entries[key].get(field), f'installed lock mismatch: {key}/{field}')
        path = root / key / 'package.json'
        require(path.is_file() and not path.is_symlink(), f'package content missing: {key}/package.json')
        actual = json.loads(path.read_text())
        require(actual.get('version') == entry.get('version'), f'installed version mismatch: {key}')
        bins = actual.get('bin', {})
        if isinstance(bins, str):
            bins = {actual['name']: bins}
        for value in bins.values():
            require((path.parent / value).is_file(), f'package executable missing: {key}/{value}')
    return {'installedLockEntries': len(installed), 'installedPackages': len(installed) - len(obsolete_entries), 'absentOptionalPackages': missing_optional,
            'obsoleteExtraneousMetadata': obsolete_entries,
            'installedLockRootVersion': hidden.get('version'), 'candidateVersion': package['version']}


def run(argv, cwd, evidence, commands):
    index = len(commands)
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    result = subprocess.run(argv, cwd=cwd, capture_output=True)
    stdout, stderr = evidence / f'command-{index:02}.stdout.log', evidence / f'command-{index:02}.stderr.log'
    stdout.write_bytes(result.stdout)
    stderr.write_bytes(result.stderr)
    commands.append({'argv': [str(v) for v in argv], 'cwd': str(cwd), 'startedUtc': started,
                     'finishedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'exitCode': result.returncode,
                     'stdoutSha256': digest(stdout), 'stderrSha256': digest(stderr)})
    require(result.returncode == 0, f'command failed ({result.returncode}): {argv}; see {stderr}')
    return result.stdout.decode().strip()


def clone_dependencies(source, dest, rows):
    # 优先系统写时复制。不能支持时，只有空间足够才做真正独立复制；不使用硬链接。
    total = sum(row.get('size', 0) for row in rows.values())
    ordinary_allowed = shutil.disk_usage(dest.parent).free > total + 1024 ** 3
    counts = {'cowFiles': 0, 'copiedFiles': 0, 'links': 0, 'bytes': total}
    libc = ctypes.CDLL(None, use_errno=True) if sys.platform == 'darwin' else None
    def copy_file(src, dst):
        succeeded = False
        if libc is not None:
            succeeded = libc.clonefile(os.fsencode(src), os.fsencode(dst), 0) == 0
        elif sys.platform.startswith('linux'):
            import fcntl
            try:
                with open(src, 'rb') as inp, open(dst, 'xb') as out:
                    fcntl.ioctl(out.fileno(), 0x40049409, inp.fileno())
                succeeded = True
            except OSError:
                if os.path.exists(dst):
                    os.unlink(dst)
        if succeeded:
            counts['cowFiles'] += 1
            shutil.copystat(src, dst)
        else:
            require(ordinary_allowed, 'independent dependency copy failed: CoW unavailable and insufficient free space')
            shutil.copy2(src, dst)
            counts['copiedFiles'] += 1
        require(os.stat(src).st_ino != os.stat(dst).st_ino or os.stat(src).st_dev != os.stat(dst).st_dev,
                'dependency copy unexpectedly shares inode')
        return str(dst)
    def ignore(directory, names):
        return CACHES.intersection(names) if Path(directory) == source else set()
    shutil.copytree(source, dest, symlinks=True, copy_function=copy_file, ignore=ignore)
    counts['links'] = sum('link' in row for row in rows.values())
    return counts


def prepare(source, copy, evidence, report):
    require(source != copy and not copy.is_relative_to(source / 'node_modules'), 'source and copy must be isolated')
    require(copy.is_dir() and not (copy / 'node_modules').exists() and not (copy / 'node_modules').is_symlink(),
            'copy node_modules must not exist')
    commands = report['commands']
    for root in (source, copy):
        top = run(['git', 'rev-parse', '--show-toplevel'], root, evidence, commands)
        require(Path(top).resolve() == root, 'source/copy must be real Git roots')
    report['sourceHead'] = run(['git', 'rev-parse', 'HEAD'], source, evidence, commands)
    report['copyHead'] = run(['git', 'rev-parse', 'HEAD'], copy, evidence, commands)
    require(report['sourceHead'] == report['copyHead'], 'source/copy HEAD mismatch')
    run(['git', 'fsck', '--connectivity-only', '--no-dangling'], copy, evidence, commands)
    report['inputs'] = read_inputs(source)
    report['metadata'] = validate_metadata(source)
    report['node'] = run(['node', '--version'], source, evidence, commands)
    report['npm'] = run(['npm', '--version'], source, evidence, commands)
    # 依照候选声明检查Node；npm下限来自仓库.npmrc的min-release-age要求。
    run(['node', '-e', "const s=require('./node_modules/semver');const p=require('./package.json');if(!s.satisfies(process.version,p.engines.node)||!s.satisfies(process.argv[1],'>=11.10.0'))process.exit(1)", report['npm']], source, evidence, commands)
    report['tools'] = {name: {'path': str(Path(shutil.which(name)).resolve()),
                            'sha256': digest(Path(shutil.which(name)).resolve())} for name in ('node', 'npm')}
    before = inventory(source / 'node_modules')
    report['dependencyManifest'] = 'dependency-files.json'
    (evidence / 'dependency-files.json').write_text(json.dumps(before, ensure_ascii=False, sort_keys=True) + '\n')
    report['dependencyManifestSha256'] = digest(evidence / 'dependency-files.json')
    for name in INPUTS:
        target = copy / name
        require(not target.is_symlink(), f'copy input is linked: {name}')
        if report['inputs'][name] is None:
            if target.exists():
                require(target.is_file(), f'copy input is not a file: {name}')
                target.unlink()
        else:
            shutil.copy2(source / name, target)
    report['copyMethod'] = clone_dependencies(source / 'node_modules', copy / 'node_modules', before)
    require(inventory(copy / 'node_modules') == before, 'copied dependency content differs')
    require(inventory(source / 'node_modules') == before and read_inputs(source) == report['inputs'], 'source changed during preparation')
    require(read_inputs(copy) == report['inputs'], 'copy package inputs differ')
    validate_metadata(copy)
    # 真正加载ws及启动vitest，缺入口/传递依赖/本机二进制均会明确报错。
    run(['node', '--input-type=module', '-e', "import WebSocket from 'ws';import {createRequire} from 'node:module';const r=createRequire(process.cwd()+'/package.json');console.log(JSON.stringify({ws:r.resolve('ws'),type:typeof WebSocket}));if(typeof WebSocket!=='function')process.exit(1)"], copy, evidence, commands)
    run([str(copy / 'node_modules/.bin/vitest'), '--version'], copy, evidence, commands)
    require(inventory(source / 'node_modules') == before, 'source dependencies changed by probes')
    require(inventory(copy / 'node_modules') == before, 'dependency probes changed package content')
    report['status'] = 'PASS'


def verify(receipt, evidence, report):
    saved = json.loads(receipt.read_text())
    require(saved.get('status') == 'PASS', 'preparation receipt not PASS')
    manifest = receipt.parent / saved['dependencyManifest']
    require(digest(manifest) == saved['dependencyManifestSha256'], 'dependency manifest changed')
    expected = json.loads(manifest.read_text())
    for key in ('source', 'copy'):
        root = Path(saved[key])
        require(read_inputs(root) == saved['inputs'], f'{key} package inputs changed')
        require(inventory(root / 'node_modules') == expected, f'{key} dependencies changed')
    run(['git', 'fsck', '--connectivity-only', '--no-dangling'], Path(saved['copy']), evidence, report['commands'])
    for name, tool in saved['tools'].items():
        require(str(Path(shutil.which(name)).resolve()) == tool['path'] and digest(Path(tool['path'])) == tool['sha256'],
                f'{name} executable changed')
    report.update(status='PASS', receipt=str(receipt), receiptSha256=digest(receipt), checkedFiles=len(expected))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--copy', type=Path)
    parser.add_argument('--verify', type=Path)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=False)
    report = {'status': 'FAIL', 'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'commands': [], 'cacheExclusions': sorted(CACHES)}
    try:
        if args.verify:
            verify(args.verify.resolve(), args.evidence, report)
        else:
            require(args.source is not None and args.copy is not None, '--source and --copy required')
            source, copy = args.source.resolve(), args.copy.resolve()
            report.update(source=str(source), copy=str(copy))
            prepare(source, copy, args.evidence, report)
    except Exception as error:
        report['error'] = str(error)
        print(f'FAIL: Node dependency preparation: {error}', file=sys.stderr)
    finally:
        report['finishedUtc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        (args.evidence / 'result.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    sys.exit(main())
