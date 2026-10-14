#!/usr/bin/env python3
"""在合成 FIFO 中实际尝试非预期 reader；比较修前可进入与修后 EACCES。"""
import argparse
import errno
import json
import os
from pathlib import Path
import sqlite3
import stat
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--root', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--expect', choices=['allowed', 'denied'], required=True)
args = parser.parse_args()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=False)
home, temporary, evidence = (output / name for name in ('home', 'tmp', 'evidence'))
for directory in (home, temporary, evidence):
    directory.mkdir()
env = dict(os.environ, LINGXI_HOME=str(home), TMPDIR=str(temporary),
           W11_EVIDENCE=str(evidence), LINGXI_PRODUCT_DIR=str(args.root.resolve() / 'lib'))
test = 'c03_actual_driver_cancel_native_fifo_keeps_paid_ledger_and_physical_lease'
reader = None
attempt = None
process = None
try:
    with (output / 'stdout.log').open('x') as log:
        process = subprocess.Popen([str(args.binary.resolve()), test, '--exact', '--nocapture'],
                                   cwd=args.root.resolve(), env=env, stdout=log, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 12
        while process.poll() is None and time.monotonic() < deadline:
            if attempt is None:
                for path in home.rglob('*.pending'):
                    try:
                        mode = path.stat().st_mode
                    except FileNotFoundError:
                        # 普通临时文件可能在枚举后已 rename；不能冒认已经尝试过 FIFO。
                        continue
                    if not stat.S_ISFIFO(mode):
                        continue
                    database = next(home.rglob('runs.db'))
                    with sqlite3.connect(f'file:{database}?mode=ro', uri=True) as connection:
                        paid = connection.execute("SELECT COUNT(*) FROM model_call_usage WHERE purpose='embedding' AND run_id IS NOT NULL").fetchone()[0]
                    if not paid:
                        continue
                    # 在原测试的 50ms 取消前窗口里尝试 reader，保留正常账目/任务链。
                    time.sleep(.025)
                    attempt = {'fifo': str(path), 'fifoMode': oct(stat.S_IMODE(mode)),
                               'paidRowsBeforeReader': paid, 'readerPid': os.getpid(), 'testPid': process.pid}
                    try:
                        reader = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
                    except OSError as error:
                        if error.errno != errno.EACCES:
                            raise
                        attempt.update(readerOpened=False, errno=error.errno, readerDeniedEACCES=True)
                    else:
                        attempt.update(readerOpened=True, errno=None, readerDeniedEACCES=False)
                    break
            time.sleep(.001)
        if process.poll() is None:
            raise TimeoutError('控制实验未在 12 秒内完成')
        exit_code = process.wait()
    assert attempt is not None, '必须实际尝试 FIFO reader，不能静默跳过'
    assert attempt['readerOpened'] == (args.expect == 'allowed'), attempt
    assert (exit_code != 0) == (args.expect == 'allowed'), f'非预期 test exit {exit_code}'
    raw = json.loads((evidence / 'c03-native-driver-before-release.json').read_text())
    summary = {**attempt, 'expectedReader': args.expect, 'testExitCode': exit_code,
               'cancelVerdict': raw['cancelVerdict'], 'elapsedMs': raw['elapsedMs'],
               'tasksBeforeRelease': raw['tasks'], 'responseStatus': raw['response'][0],
               'containsRegularFileRefusal': any('not a regular file; paid bytes retained' in str(row) for row in raw['wire'])}
    if args.expect == 'allowed':
        assert summary['cancelVerdict'] == 'None', summary
        assert summary['tasksBeforeRelease'] == [], summary
        assert summary['containsRegularFileRefusal'], summary
    else:
        assert summary['readerDeniedEACCES'] is True, summary
        assert 'StopUnconfirmed' in summary['cancelVerdict'], summary
        assert any(task['exit'] is None for task in summary['tasksBeforeRelease']), summary
    (output / 'reader-receipt.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(summary, ensure_ascii=False, indent=2))
finally:
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    if reader is not None:
        os.close(reader)
