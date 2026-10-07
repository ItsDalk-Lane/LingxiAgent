#!/usr/bin/python3
"""仅记录本工作包命令及真实退出；不修改系统策略。"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
spec = json.loads(sys.argv[1])
name = spec['name']
start = datetime.datetime.now().astimezone().isoformat()
t0 = time.monotonic()
log = ROOT / (name + '.log')
snap = ROOT / (name + '-listeners.log')
timed_out = False
with log.open('wb') as output:
    process = subprocess.Popen(spec['argv'], stdout=output, stderr=subprocess.STDOUT,
                               start_new_session=True)
    monitor_at = 0.0
    while process.poll() is None:
        if spec.get('monitor') and time.monotonic() >= monitor_at:
            with snap.open('ab') as monitor:
                monitor.write((datetime.datetime.now().astimezone().isoformat() + '\n').encode())
                for argv in [['/bin/ps', '-axo', 'pid,ppid,lstart,comm'],
                             ['/usr/sbin/lsof', '-nP', '-iTCP', '-sTCP:LISTEN']]:
                    sampled = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                    # 仅保存被测测试进程和其监听，避免记录无关应用信息。
                    lines = sampled.stdout.splitlines()
                    selected = [line for line in lines if b'r00_management' in line or b'r00_manag' in line]
                    monitor.write(('COMMAND ' + repr(argv) + ' EXIT ' + str(sampled.returncode) + '\n').encode())
                    monitor.write(b'\n'.join(selected) + b'\n')
            monitor_at = time.monotonic() + 1
        if time.monotonic() - t0 > spec.get('timeout', 600):
            timed_out = True
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            break
        time.sleep(0.2)
code = process.wait()
result = dict(spec, cwd=str(Path.cwd()), start=start,
              end=datetime.datetime.now().astimezone().isoformat(),
              elapsedSeconds=round(time.monotonic() - t0, 3), pid=process.pid,
              actualExitCode=code, supervisorTimedOut=timed_out,
              supervisorExitCode=124 if timed_out else code,
              log=str(log), logSha256=hashlib.sha256(log.read_bytes()).hexdigest())
(ROOT / (name + '.json')).write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
print(json.dumps(result, ensure_ascii=False))
sys.exit(124 if timed_out else code)
