#!/usr/bin/env python3
"""从指定 Git 提交建立完整独立副本；同字节文件使用写时复制，不信任脏工作树。"""
import argparse
import ctypes
import datetime
import errno
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def plain_path(path):
    path = Path(os.path.abspath(path))
    for component in [*reversed(path.parents), path]:
        require(not component.is_symlink(), f'path crosses symlink: {component}')
    return path


class Commands:
    def __init__(self, evidence, report):
        self.evidence, self.report = evidence, report
        # 所有来源 Git 查询禁止可选的 index 刷新；清掉会重定向仓库的外部变量。
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        self.env['GIT_OPTIONAL_LOCKS'] = '0'

    def run(self, root, *args, output=None):
        argv = ['git', '-C', str(root), *args]
        number = len(self.report['commands'])
        stdout = self.evidence / f'git-{number:04}.stdout'
        stderr = self.evidence / f'git-{number:04}.stderr'
        started = utc()
        with (output or stdout).open('xb') as out, stderr.open('xb') as err:
            result = subprocess.run(argv, env=self.env, stdout=out, stderr=err)
        self.report['commands'].append(dict(argv=argv, cwd=str(root), startedUtc=started,
            finishedUtc=utc(), exitCode=result.returncode, stdoutSha256=digest(output or stdout),
            stderrSha256=digest(stderr), outputFile=str(output or stdout)))
        require(result.returncode == 0, f'Git command failed ({result.returncode}): {args}; see {stderr}')
        require(not stderr.stat().st_size, f'Git command emitted diagnostics: {args}; see {stderr}')
        return b'' if output else stdout.read_bytes()


def git_state(root, commands):
    require((root / '.git').is_dir() and not (root / '.git').is_symlink(), 'source must have its own .git directory')
    require(os.fsdecode(commands.run(root, 'rev-parse', '--show-toplevel')).strip() == str(root), 'source is not a Git root')
    index = root / '.git/index'
    require(index.is_file() and not index.is_symlink(), 'source index missing or linked')
    return dict(head=commands.run(root, 'rev-parse', 'HEAD').decode().strip(),
                headFileSha256=digest(root / '.git/HEAD'), indexSha256=digest(index))


def tree(root, revision, commands):
    result = []
    for row in commands.run(root, 'ls-tree', '-rlz', revision).split(b'\0'):
        if not row:
            continue
        fields, raw_path = row.split(b'\t', 1)
        mode, kind, oid, size = fields.split()
        require(kind == b'blob' and mode in (b'100644', b'100755', b'120000'), 'unsupported Git entry type')
        path = os.fsdecode(raw_path)
        require(not path.startswith('/') and all(p not in ('', '.', '..', '.git') for p in path.split('/')), 'unsafe Git path')
        result.append(dict(path=path, pathBytesHex=raw_path.hex(), mode=mode.decode(), oid=oid.decode(), size=int(size)))
    return result


def hashes(stream, size, algorithm):
    blob, sha = hashlib.new(algorithm), hashlib.sha256()
    blob.update(f'blob {size}\0'.encode())
    while block := stream.read(1024 * 1024):
        blob.update(block)
        sha.update(block)
    return blob.hexdigest(), sha.hexdigest()


def source_entry(root, relative, algorithm, consume=None):
    # 按目录句柄逐层打开，绝不跟随来源中途被替换的链接去读用户目录。
    flags = os.O_RDONLY | os.O_NOFOLLOW
    directory = os.open(root, flags | os.O_DIRECTORY)
    try:
        parts = relative.split('/')
        for component in parts[:-1]:
            try:
                child = os.open(component, flags | os.O_DIRECTORY, dir_fd=directory)
            except FileNotFoundError:
                return {'kind': 'missing-parent'}
            except OSError as error:
                if error.errno in (errno.ENOTDIR, errno.ELOOP):
                    info = os.stat(component, dir_fd=directory, follow_symlinks=False)
                    require(stat.S_ISLNK(info.st_mode), f'unsupported source parent: {relative}')
                    return {'kind': 'linked-parent', 'link': os.readlink(component, dir_fd=directory)}
                raise
            os.close(directory)
            directory = child
        try:
            info = os.stat(parts[-1], dir_fd=directory, follow_symlinks=False)
        except FileNotFoundError:
            return {'kind': 'missing'}
        if stat.S_ISLNK(info.st_mode):
            return {'kind': 'link', 'link': os.readlink(parts[-1], dir_fd=directory)}
        require(stat.S_ISREG(info.st_mode), f'unsupported source file: {relative}')
        with os.fdopen(os.open(parts[-1], flags, dir_fd=directory), 'rb') as stream:
            before = os.fstat(stream.fileno())
            require((before.st_dev, before.st_ino) == (info.st_dev, info.st_ino), f'source changed while opening: {relative}')
            oid, sha = hashes(stream, before.st_size, algorithm)
            row = dict(kind='file', oid=oid, sha256=sha, size=before.st_size, mode=stat.S_IMODE(before.st_mode),
                       device=before.st_dev, inode=before.st_ino, mtimeNs=before.st_mtime_ns, ctimeNs=before.st_ctime_ns)
            if consume:
                consume(stream, row)
            after = os.fstat(stream.fileno())
            require(all(getattr(before, field) == getattr(after, field) for field in
                        ('st_dev', 'st_ino', 'st_size', 'st_mode', 'st_mtime_ns', 'st_ctime_ns')),
                    f'source changed while reading: {relative}')
            return row
    finally:
        os.close(directory)


def independent_file(stream, dest, size):
    # macOS/Linux 优先独立 CoW；不支持时明确记录普通复制，空间不足直接拒绝。
    succeeded = False
    fallback_errors = (errno.ENOTSUP, errno.EXDEV, errno.ENOSYS, errno.EINVAL, errno.ENOTTY)
    if sys.platform == 'darwin':
        libc = ctypes.CDLL(None, use_errno=True)
        succeeded = libc.fclonefileat(stream.fileno(), -2, os.fsencode(dest), 0) == 0
        if not succeeded:
            error = ctypes.get_errno()
            require(error in fallback_errors, f'CoW copy failed: {os.strerror(error)}')
    elif sys.platform.startswith('linux'):
        import fcntl
        try:
            with dest.open('xb') as out:
                fcntl.ioctl(out.fileno(), 0x40049409, stream.fileno())
            succeeded = True
        except OSError as error:
            if dest.exists():
                dest.unlink()
            require(error.errno in fallback_errors, f'CoW copy failed: {error}')
    if not succeeded:
        require(shutil.disk_usage(dest.parent).free > size + 64 * 1024 ** 2,
                'independent copy needs more free space; CoW unavailable')
        stream.seek(0)
        with dest.open('xb') as out:
            shutil.copyfileobj(stream, out)
    original, copied = os.fstat(stream.fileno()), dest.stat()
    require((original.st_dev, original.st_ino) != (copied.st_dev, copied.st_ino), 'copy shares writable inode')
    return 'cow' if succeeded else 'ordinary'


def prepare(source, copy, revision, branch, evidence, report):
    source, copy, evidence = map(plain_path, (source, copy, evidence))
    require(copy != source and not copy.is_relative_to(source) and not source.is_relative_to(copy), 'source and copy must be disjoint')
    require(not copy.exists() and not copy.is_symlink(), 'destination must not exist')
    require(copy.parent.is_dir(), 'destination parent missing')
    require(not evidence.is_relative_to(copy), 'evidence cannot be inside destination')
    commands = Commands(evidence, report)
    report.update(source=str(source), copy=str(copy), requestedRevision=revision, sourceBefore=git_state(source, commands),
                  freeBytesBefore=shutil.disk_usage(copy.parent).free, helperSha256=digest(Path(__file__)))
    commit = commands.run(source, 'rev-parse', '--verify', revision + '^{commit}').decode().strip()
    algorithm = commands.run(source, 'rev-parse', '--show-object-format').decode().strip()
    require(algorithm in ('sha1', 'sha256'), 'unsupported Git object format')
    entries = tree(source, commit, commands)
    source_rows = {entry['path']: source_entry(source, entry['path'], algorithm) for entry in entries}
    copied_rows = []
    # --no-checkout 避免先展开一次；独立 HEAD/index 均仅在新副本内建立。
    clone = ['clone', '--shared', '--no-checkout', '--quiet']
    if branch:
        clone += ['--branch', branch]
    commands.run(source, *clone, str(source), str(copy))
    if branch:
        require(commands.run(copy, 'rev-parse', 'HEAD').decode().strip() == commit, 'requested branch does not match source revision')
    else:
        commands.run(copy, 'update-ref', '--no-deref', 'HEAD', commit)
    commands.run(copy, 'read-tree', commit)
    commands.run(copy, 'fsck', '--connectivity-only', '--no-dangling')
    report.update(commit=commit, tree=commands.run(copy, 'rev-parse', 'HEAD^{tree}').decode().strip(), objectFormat=algorithm)
    for entry in entries:
        dest = copy / entry['path']
        dest.parent.mkdir(parents=True, exist_ok=True)
        row = dict(entry)
        def consume(stream, observed):
            require(observed == source_rows[entry['path']], f'source changed before copying: {entry["path"]}')
            if entry['mode'] != '120000' and observed['oid'] == entry['oid']:
                row['method'] = independent_file(stream, dest, entry['size'])
        require(source_entry(source, entry['path'], algorithm, consume) == source_rows[entry['path']],
                f'source changed before copying: {entry["path"]}')
        if 'method' not in row:
            if entry['mode'] == '120000':
                value = commands.run(copy, 'cat-file', 'blob', entry['oid'])
                os.symlink(os.fsdecode(value), dest)
                row['method'] = 'git-link'
            else:
                commands.run(copy, 'cat-file', 'blob', entry['oid'], output=dest)
                row['method'] = 'git-blob'
        if entry['mode'] != '120000':
            dest.chmod(0o755 if entry['mode'] == '100755' else 0o644)
            with dest.open('rb') as stream:
                oid, sha = hashes(stream, dest.stat().st_size, algorithm)
            require(oid == entry['oid'], f'materialized blob mismatch: {entry["path"]}')
            row.update(sha256=sha, permissions=stat.S_IMODE(dest.stat().st_mode))
        else:
            value = os.fsencode(os.readlink(dest))
            require(hashlib.new(algorithm, f'blob {len(value)}\0'.encode() + value).hexdigest() == entry['oid'], 'materialized link mismatch')
            row.update(link=os.fsdecode(value), sha256=hashlib.sha256(value).hexdigest())
        copied_rows.append(row)
    require(tree(copy, 'HEAD', commands) == entries, 'copy HEAD tree differs')
    require(commands.run(copy, 'write-tree').decode().strip() == report['tree'], 'copy index differs from commit')
    commands.run(copy, 'diff', '--exit-code', commit, '--')
    require(not commands.run(copy, 'status', '--porcelain=v1', '-z', '--untracked-files=all'), 'copy is not pristine')
    commands.run(copy, 'fsck', '--connectivity-only', '--no-dangling')
    # 所有副本核验完成后再核来源；不能漏掉末尾 Git 核验期间的并发漂移。
    require({entry['path']: source_entry(source, entry['path'], algorithm) for entry in entries} == source_rows,
            'source worktree changed during preparation')
    report['sourceAfter'] = git_state(source, commands)
    require(report['sourceBefore'] == report['sourceAfter'], 'source HEAD/index changed during preparation')
    for name, rows in [('tracked-files.json', copied_rows), ('source-files.json', source_rows)]:
        path = evidence / name
        path.write_text(json.dumps(rows, sort_keys=True) + '\n')
        report[name] = dict(sha256=digest(path), count=len(rows))
    report.update(status='PASS', trackedCount=len(entries), trackedBytes=sum(e['size'] for e in entries),
                  methods={name: sum(row['method'] == name for row in copied_rows) for name in ('cow', 'ordinary', 'git-blob', 'git-link')},
                  copyIndexSha256=digest(copy / '.git/index'), freeBytesAfter=shutil.disk_usage(copy.parent).free)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--copy', type=Path, required=True)
    parser.add_argument('--revision', default='HEAD')
    parser.add_argument('--branch')
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    evidence = plain_path(args.evidence)
    source = plain_path(args.source)
    require(not evidence.is_relative_to(source / '.git'), 'evidence must not modify source Git metadata')
    evidence.mkdir(parents=True, exist_ok=False)
    report = dict(status='FAIL', startedUtc=utc(), commands=[], platform=sys.platform,
                  argv=sys.argv, cwd=str(Path.cwd()), spaceNote='实际文件系统可用空间截点，不能解释为此副本独占物理块')
    try:
        prepare(args.source, args.copy, args.revision, args.branch, evidence, report)
    except Exception as error:
        report['error'] = str(error)
        print(f'FAIL: Git copy preparation: {error}', file=sys.stderr)
    finally:
        report['finishedUtc'] = utc()
        (evidence / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    sys.exit(main())
