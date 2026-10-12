"""R02 证据生产者的进程组信号归属检查。"""

from __future__ import annotations

import os
import signal
import subprocess


def signal_owned_group(process: subprocess.Popen, sig: signal.Signals) -> bool:
    """仅在本次 Popen 的会话领袖仍未回收时向其进程组发信号。"""
    # poll 返回 None 时子进程仍未被回收；即使随即退出，PID 也不会在本
    # Popen 执行 wait/communicate 前被系统复用。会话与进程组都须同号。
    if process.poll() is not None:
        return False
    try:
        if os.getpgid(process.pid) != process.pid or os.getsid(process.pid) != process.pid:
            return False
        os.killpg(process.pid, sig)
        return True
    except ProcessLookupError:
        return False

