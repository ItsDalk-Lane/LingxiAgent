#!/usr/bin/env python3
"""保存 R08 命令、原始退出码和最小候选绑定；不把退出 0 解释为场景通过。"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('必须给出实际命令')
    output = args.evidence.resolve()
    output.mkdir(parents=True, exist_ok=False)
    # 候选 diff 只绑定源码候选：artifacts/** 是证据输出与历史档案，
    # 其已删大型二进制（git diff --binary 会内嵌完整旧字节，单份可达 11GB，
    # 曾致磁盘写满）不属于受测源码，排除之；源码绑定由下方 sourceBindings 承担。
    diff = subprocess.check_output(['git', 'diff', '--binary', 'HEAD', '--', '.',
        ':(exclude)artifacts/**'], cwd=ROOT)
    (output / 'candidate.diff').write_bytes(diff)
    sources = ['package.json', 'package-lock.json', 'rust-toolchain.toml', 'rust/Cargo.lock',
        'scripts/launch.js', 'scripts/dev-env.js', 'desktop/bootstrap.cjs', 'desktop/main.cjs',
        'desktop/src/shared/single-instance-lock.cjs', 'tests/startup-contract.test.ts',
        'tests/desktop-single-instance-lock.test.ts', 'scripts/rust-tauri/r08_isolation_probe.mjs',
        'scripts/rust-tauri/r08_leaf_map.py', 'scripts/rust-tauri/r08_run.py']
    # 准入修复包含未提交的新模块；仅git diff不能绑定其实际受测内容。
    for crate in ['lingxi-kernel', 'lingxi-service', 'lingxi-protocol', 'lingxi-adapters']:
        base = ROOT / 'rust' / 'crates' / crate
        sources.extend(str(path.relative_to(ROOT)) for path in base.rglob('*')
                       if path.is_file() and (path.suffix in ['.rs', '.inc'] or path.name == 'Cargo.toml'))
    sources.extend(str(path.relative_to(ROOT)) for path in (ROOT / 'scripts/rust-tauri').glob('r08-*.mjs'))
    sources.extend(['lib/ptc/ptc-runtime.ts', 'lib/security/baseline-scanner.ts',
                    'lib/security/secret-rules.ts', 'lib/lsp/jsonrpc-client.ts'])
    sources = sorted(set(sources))
    bindings = {name: digest(ROOT / name) for name in sources if (ROOT / name).is_file()}
    report = dict(schema='lingxi.r08.command-receipt.v1', argv=command, cwd=str(ROOT),
        sourceSha=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        candidateDiffSha256=hashlib.sha256(diff).hexdigest(), sourceBindings=bindings,
        startedAt=utc(), platform=sys.platform, globalHomeUnchanged=os.environ.get('HOME'),
        evidenceBoundary='命令回执，不等于逐断言验收；环境不含真实凭证输出')
    log = output / 'output.log'
    with log.open('xb') as stream:
        try:
            proc = subprocess.run(command, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
            code = proc.returncode
        except OSError as error:
            stream.write(str(error).encode())
            code = 127
    report.update(exitCode=code, finishedAt=utc(), logSha256=digest(log))
    report['sourceBindingsStable'] = all((ROOT / name).is_file() and digest(ROOT / name) == sha
                                         for name, sha in bindings.items())
    (output / 'receipt.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(dict(exitCode=code, sourceBindingsStable=report['sourceBindingsStable'], evidence=str(output))))
    return code if code >= 0 else 128 - code

if __name__ == '__main__':
    sys.exit(main())
