#!/usr/bin/env python3
"""亲跑默认准备片段及历史基线入口，验证完整 Git/文件系统隔离和失败拒绝。"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / 'scripts/rust-tauri/prepare_git_copy.py'
NEGATIVE = ROOT / 'scripts/rust-tauri/r05_t08_negative_gate.sh'
LEGACY = ROOT / 'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh'


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def manifest(root):
    rows = {}
    for base, dirs, files in os.walk(root, followlinks=False):
        for name in sorted(dirs + files):
            path = Path(base) / name
            if path.is_symlink():
                rows[str(path.relative_to(root))] = {'link': os.readlink(path)}
            elif path.is_file():
                rows[str(path.relative_to(root))] = {'sha256': sha(path), 'mode': path.stat().st_mode & 0o777}
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    scratch = Path(tempfile.mkdtemp(prefix='lingxi-git-copy-', dir='/private/tmp' if sys.platform == 'darwin' else None)).resolve()
    commands, checks = [], []
    inputs = {str(p): sha(p) for p in (HELPER, NEGATIVE, LEGACY, Path(__file__))}
    def run(name, argv, cwd=scratch, expected=0, env=None):
        start = datetime.datetime.now(datetime.timezone.utc).isoformat()
        result = subprocess.run([str(v) for v in argv], cwd=cwd, env=env, capture_output=True)
        stdout, stderr = evidence / (name + '.stdout'), evidence / (name + '.stderr')
        stdout.write_bytes(result.stdout); stderr.write_bytes(result.stderr)
        commands.append(dict(name=name, argv=[str(v) for v in argv], cwd=str(cwd), startedUtc=start,
            finishedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), exitCode=result.returncode,
            expectedExit=expected, stdoutSha256=sha(stdout), stderrSha256=sha(stderr)))
        (evidence / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        assert (result.returncode != 0 if expected == 'nonzero' else result.returncode == expected), (name, result.returncode, result.stderr.decode(errors='replace'))
        return result.stdout
    def check(name, truth):
        assert truth, name
        checks.append(name)
    source = scratch / 'source'; source.mkdir()
    def git(name, *argv):
        return run(name, ['git', '-C', source, *argv])
    git('init', 'init', '-q', '-b', 'codex/rust-tauri-migration')
    git('name', 'config', 'user.name', 'Fixture')
    git('email', 'config', 'user.email', 'fixture@example.invalid')
    for name, data in {'plain': b'HEAD\n', 'missing': b'from Git\n', 'executable': b'#!/bin/sh\nexit 0\n',
                       'artifacts/old evidence/keep.json': b'{"old":true}\n',
                       'odd \t\n\\\u4e2d name': b'special path\n', 'parent/child': b'never read outside\n'}.items():
        path = source / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(data)
    (source / 'executable').chmod(0o755)
    (source / 'link').symlink_to('../outside-no-follow')
    for name in ('rust', 'scripts/rust-tauri', 'docs/rust-tauri'):
        (source / name).mkdir(parents=True, exist_ok=True)
    shutil.copy2(HELPER, source / 'scripts/rust-tauri/prepare_git_copy.py')
    git('add', 'add', '.')
    git('commit-base', 'commit', '-qm', 'fixture base')
    base = git('base', 'rev-parse', 'HEAD').decode().strip()
    (source / 'plain').write_text('second commit\n')
    git('add-head', 'add', 'plain'); git('commit-head', 'commit', '-qm', 'fixture head')
    head = git('head', 'rev-parse', 'HEAD').decode().strip()
    # 合法 staged + unstaged + 缺文件；历史物化均不得借用错误字节。
    (source / 'plain').write_text('staged\n'); git('stage-dirty', 'add', 'plain')
    (source / 'plain').write_text('unstaged\n'); (source / 'missing').unlink()
    (source / 'untracked').write_text('candidate only\n')
    (source / 'rust/dirty.rs').write_text('candidate overlay\n')
    initial_git = manifest(source / '.git')
    initial_source = manifest(source)
    shell = NEGATIVE.read_text()
    start = shell.index('rmdir "$COPY"')
    end = shell.index('# Overlay the uncommitted candidate:', start)
    production = shell[start:end]
    prefix = 'set -euo pipefail; ROOT="$1"; COPY="$2"; EV="$3"; fail(){ echo "$*" >&2; exit 1; };\n'
    (evidence / 'default-production-fragment.sh').write_text(prefix + production)
    def prepare(name, revision=None, env=None, copy=None, expected=0):
        destination = copy or scratch / name
        out = evidence / name; out.mkdir()
        if revision is None:
            if not destination.exists(): destination.mkdir()
            argv = ['bash', evidence / 'default-production-fragment.sh', source, destination, out]
        else:
            argv = ['python3', '-B', source / 'scripts/rust-tauri/prepare_git_copy.py', '--source', source,
                    '--copy', destination, '--revision', revision, '--evidence', out / 'git-preparation']
        run(name, argv, expected=expected, env=env)
        receipt = json.loads((out / 'git-preparation/result.json').read_text()) if (out / 'git-preparation/result.json').exists() else None
        return destination, receipt
    copy, receipt = prepare('default-head')
    check('default HEAD correct with staged and unstaged source', (copy / 'plain').read_text() == 'second commit\n' and receipt['commit'] == head)
    check('missing source restored from exact Git blob', (copy / 'missing').read_text() == 'from Git\n')
    check('all historical artifacts retained', (copy / 'artifacts/old evidence/keep.json').read_bytes() == (source / 'artifacts/old evidence/keep.json').read_bytes())
    check('executable and original external symlink target preserved', (copy / 'executable').stat().st_mode & 0o777 == 0o755 and os.readlink(copy / 'link') == '../outside-no-follow')
    check('candidate-only untracked content not presented as HEAD', not (copy / 'untracked').exists())
    check('source Git files unchanged by preparation', manifest(source / '.git') == initial_git)
    # 对照普通 Git checkout 的 HEAD/tree/index/内容；目录全部保留。
    reference = scratch / 'reference'
    run('normal-clone', ['git', 'clone', '--quiet', '--no-hardlinks', source, reference])
    for name, argv in [('head', ['rev-parse', 'HEAD']), ('tree', ['rev-parse', 'HEAD^{tree}']), ('index', ['ls-files', '-sz']), ('status', ['status', '--porcelain=v1', '-z'])]:
        left = run('copy-' + name, ['git', '-C', copy, *argv])
        right = run('reference-' + name, ['git', '-C', reference, *argv])
        check('normal clone agrees: ' + name, left == right)
    check('normal clone complete file bytes and modes agree', {k:v for k,v in manifest(copy).items() if not k.startswith('.git/')} == {k:v for k,v in manifest(reference).items() if not k.startswith('.git/')})
    # 真实独立文件/index/HEAD 写控；来源前后所有内容一致。
    (copy / 'plain').write_text('copy-only mutation\n')
    run('copy-index-write', ['git', '-C', copy, 'add', 'plain'])
    run('copy-head-write', ['git', '-C', copy, 'update-ref', '--no-deref', 'HEAD', base])
    check('copy file/index/HEAD writes cannot change source', manifest(source) == initial_source)
    # 历史 BASE 使用真实生产调用，保留原纯净断言。
    legacy = LEGACY.read_text(); first = legacy.index('BASE_COPY="$(cd "$WORK" && pwd -P)/repo-base"')
    last = legacy.index('cp -Rc "$MAIN_REPO/node_modules"', first)
    base_work = scratch / 'base-work'; base_work.mkdir()
    base_copy = base_work / 'repo-base'; base_ev = evidence / 'base-copy'; base_ev.mkdir()
    # macOS 的真实 /tmp 别名覆盖系统 TMPDIR 同类形状，不放宽助手的链接拒绝。
    work_arg = str(base_work).replace('/private/tmp/', '/tmp/', 1) if sys.platform == 'darwin' else str(base_work)
    base_code = 'set -euo pipefail; MAIN_REPO="$1"; WORK="$2"; BASE_SHA="$3"; EVIDENCE_DIR="$4"; fail(){ echo "$*" >&2; exit 1; };\n' + legacy[first:last]
    run('legacy-base', ['bash', '-c', base_code, 'base', source, work_arg, base, base_ev])
    check('historical BASE bytes differ correctly from candidate HEAD', (base_copy / 'plain').read_text() == 'HEAD\n')
    check('historical BASE pristine', run('base-status', ['git', '-C', base_copy, 'status', '--porcelain=v1', '-z']) == b'')
    run('base-index-purity', ['git', '-C', base_copy, 'diff', '--cached', '--exit-code'])
    run('base-content-purity', ['git', '-C', base_copy, 'diff', '--exit-code', base])
    # 外部命令错误、复制权限失败和复制期间漂移，均通过实际生产函数。
    proxy_dir = scratch / 'bin'; proxy_dir.mkdir()
    proxy = proxy_dir / 'git'
    real_git = shutil.which('git')
    proxy.write_text('''#!/usr/bin/env python3
import os, subprocess, sys
from pathlib import Path
mode=os.environ.get('COPY_CHECK_FAULT','normal'); args=sys.argv[1:]
if 'ls-tree' in args and mode in ('query-failure','query-stderr'):
    sys.stderr.write('controlled Git query failure\\n'); sys.exit(2 if mode=='query-failure' else 0)
code=subprocess.call([os.environ['COPY_CHECK_REAL_GIT'],*args])
if code==0 and 'read-tree' in args and mode=='copy-permission':
    Path(args[1]).chmod(0o555)
if code==0 and 'clone' in args and mode=='source-drift':
    (Path(os.environ['COPY_CHECK_SOURCE'])/'plain').write_text('concurrent changed source\\n')
if code==0 and 'write-tree' in args and mode=='source-late-drift':
    (Path(os.environ['COPY_CHECK_SOURCE'])/'plain').write_text('late concurrent changed source\\n')
sys.exit(code)
''')
    proxy.chmod(0o755)
    for fault, message in [('query-failure','Git command failed'), ('query-stderr','Git command emitted diagnostics'),
                           ('copy-permission','Permission denied'), ('source-drift','source changed'),
                           ('source-late-drift','source worktree changed')]:
        env = dict(os.environ, PATH=str(proxy_dir) + os.pathsep + os.environ['PATH'], COPY_CHECK_FAULT=fault,
                   COPY_CHECK_REAL_GIT=real_git, COPY_CHECK_SOURCE=str(source))
        destination, result = prepare(fault, env=env, expected=1)
        check(fault + ' rejected at target', result is not None and message in result['error'])
        if destination.exists(): destination.chmod(0o755)
        (source / 'plain').write_text('unstaged\n')
        prepare(fault + '-restored')
    # 真实缺对象不是假 Git 结果。移走本夹具的提交对象后还原。
    object_path = source / '.git/objects' / head[:2] / head[2:]
    parked = scratch / 'parked-object'; object_path.rename(parked)
    try:
        _, broken = prepare('missing-object', expected=1)
        check('missing Git object refused', 'Git command failed' in broken['error'])
    finally:
        parked.rename(object_path)
    prepare('missing-object-restored')
    # 共享对象失效会被 Git 拒绝，恢复后仍可达。
    alternate = base_copy / '.git/objects/info/alternates'; original = alternate.read_bytes()
    alternate.write_text(str(scratch / 'nonexistent-objects') + '\n')
    run('shared-object-loss', ['git', '-C', base_copy, 'fsck', '--connectivity-only', '--no-dangling'], expected='nonzero')
    check('shared object loss refuses execution', commands[-1]['exitCode'] != 0)
    alternate.write_bytes(original)
    run('shared-objects-restored', ['git', '-C', base_copy, 'fsck', '--connectivity-only', '--no-dangling'])
    # 目标同根/祖先/子目录/已存在/链接全部拒绝，不能清理调用者内容。
    unsafe = scratch / 'nonempty'; unsafe.mkdir(); (unsafe / 'keep').write_text('keep')
    alias = scratch / 'alias'; alias.symlink_to(scratch / 'source')
    for name, dest in [('same',source), ('ancestor',scratch), ('nested',source/'new'), ('nonempty',unsafe), ('linked',alias/'new')]:
        _, result = prepare('bad-' + name, revision=head, copy=dest, expected=1)
        check('invalid destination rejected: ' + name, result['status'] == 'FAIL')
    check('existing destination unchanged', (unsafe / 'keep').read_text() == 'keep')
    # 来源特殊文件明确拒绝；父链接仅读链接目标字符串，不能越界读取。
    os.mkfifo(source / 'missing')
    _, result = prepare('special-file', expected=1)
    check('special source file refused', 'unsupported source file' in result['error'])
    (source / 'missing').unlink(); prepare('special-file-restored')
    shutil.rmtree(source / 'parent'); (source / 'parent').symlink_to('../outside-no-follow')
    linked, result = prepare('source-linked-parent')
    check('source linked parent never followed; accurate Git content restored', (linked / 'parent/child').read_text() == 'never read outside\n')
    (source / 'parent').unlink(); (source / 'parent').mkdir(); (source / 'parent/child').write_text('never read outside\n')
    prepare('final-restored')
    check('production inputs unchanged during selfcheck', all(sha(Path(p)) == value for p,value in inputs.items()))
    check('source Git unchanged after all controls', manifest(source / '.git') == initial_git)
    (evidence / 'result.json').write_text(json.dumps(dict(status='PASS', checks=checks, checkCount=len(checks), commandCount=len(commands),
        inputs=inputs, fixture=str(scratch), sourceGit=initial_git, ignored=0, filtered=0,
        boundary='小真实 Git 仓库；生产默认/历史调用片段、Git、CoW 和文件系统真实。只有故障腿代理 Git 命令注入查询失败/权限/并发漂移。'), indent=2) + '\n')
    # 只清理本次创建的可重建夹具；所有真实命令、结果及输入摘要已保存。
    shutil.rmtree(scratch)
    (evidence / 'cleanup.json').write_text(json.dumps(dict(path=str(scratch), existsAfter=scratch.exists(),
        reason='本次自有小仓库和故障副本，保留命令/日志/输入/结果'), indent=2) + '\n')
    print(f'PASS {len(checks)} checks, {len(commands)} actual commands')


if __name__ == '__main__':
    main()
