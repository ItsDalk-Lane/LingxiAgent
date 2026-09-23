#!/usr/bin/env python3
"""R00-A01 复测：真实目录内容摘要、真实只读哨兵和隔离服务。"""

import hashlib
import ctypes
import errno
import sys
import json
import os
import pathlib
import platform
import secrets
import shutil
import signal
import stat
import subprocess
import tempfile
import time
import urllib.request
from datetime import datetime, timezone


ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = pathlib.Path(__file__).resolve().parent
REAL_HOME = pathlib.Path.home()
REAL_DIRS = {"production": REAL_HOME / ".lingxi", "development": REAL_HOME / ".lingxi-dev"}
SENTINEL_BODY = b"R00-A01 temporary read-only sentinel; owned by this probe.\n"


def now():
    return datetime.now(timezone.utc).isoformat()


def sha_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


NOFOLLOW = getattr(os, "O_NOFOLLOW", 0)


def open_directory(path):
    before = path.lstat()
    if not stat.S_ISDIR(before.st_mode) or stat.S_ISLNK(before.st_mode):
        raise RuntimeError("directory is not an ordinary directory")
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | NOFOLLOW)
    opened = os.fstat(fd)
    if (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
        os.close(fd)
        raise RuntimeError("directory changed while opening")
    return fd


def checked_file(parent_fd, name, expected=None):
    before = os.stat(name, dir_fd=parent_fd, follow_symlinks=False)
    if not stat.S_ISREG(before.st_mode):
        raise RuntimeError("entry is not an ordinary file")
    fd = os.open(name, os.O_RDONLY | NOFOLLOW, dir_fd=parent_fd)
    opened = os.fstat(fd)
    if (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino) or (
        expected and (opened.st_dev, opened.st_ino) != (expected["dev"], expected["ino"])
    ):
        os.close(fd)
        raise RuntimeError("file changed while opening")
    return fd, before


def digest_fd(fd):
    digest = hashlib.sha256()
    while block := os.read(fd, 1024 * 1024):
        digest.update(block)
    return digest.hexdigest()


def tree_digest(root):
    """只输出摘要和统计；不保存真实文件名、内容、链接目标。"""
    digest = hashlib.sha256()
    contents = hashlib.sha256()
    counts = {"files": 0, "directories": 0, "symlinks": 0, "other": 0, "bytes": 0, "errors": 0}
    errors = []
    root_fd = open_directory(root)
    root_stat = os.fstat(root_fd)
    digest.update(f"ROOT\0{root_stat.st_dev}\0{root_stat.st_ino}\0{stat.S_IMODE(root_stat.st_mode)}\0"
                  f"{root_stat.st_mtime_ns}\0{root_stat.st_ctime_ns}\n".encode())

    def visit(folder_fd, prefix):
        initial = os.fstat(folder_fd)
        try:
            names = sorted(os.listdir(folder_fd))
        except OSError as error:
            counts["errors"] += 1
            errors.append(f"scan:{error.errno}")
            return
        for name in names:
            relative = f"{prefix}/{name}" if prefix else name
            try:
                before = os.stat(name, dir_fd=folder_fd, follow_symlinks=False)
                mode = before.st_mode
                if stat.S_ISDIR(mode):
                    kind = "directory"
                    counts["directories"] += 1
                    content_hash = ""
                elif stat.S_ISREG(mode):
                    kind = "file"
                    counts["files"] += 1
                    counts["bytes"] += before.st_size
                    fd, _ = checked_file(folder_fd, name, {"dev": before.st_dev, "ino": before.st_ino})
                    try:
                        content_hash = digest_fd(fd)
                        after = os.fstat(fd)
                        if (before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
                            after.st_size, after.st_mtime_ns, after.st_ctime_ns
                        ):
                            raise RuntimeError("file changed while reading")
                    finally:
                        os.close(fd)
                elif stat.S_ISLNK(mode):
                    kind = "symlink"
                    counts["symlinks"] += 1
                    content_hash = hashlib.sha256(os.fsencode(os.readlink(name, dir_fd=folder_fd))).hexdigest()
                    linked_after = os.stat(name, dir_fd=folder_fd, follow_symlinks=False)
                    if (linked_after.st_dev, linked_after.st_ino, linked_after.st_mtime_ns,
                        linked_after.st_ctime_ns) != (before.st_dev, before.st_ino, before.st_mtime_ns,
                                                     before.st_ctime_ns):
                        raise RuntimeError("symlink changed while reading")
                else:
                    kind = "other"
                    counts["other"] += 1
                    content_hash = ""
                digest.update(
                    f"{relative}\0{kind}\0{before.st_dev}\0{before.st_ino}\0"
                    f"{stat.S_IMODE(mode)}\0{before.st_size}\0{before.st_mtime_ns}\0"
                    f"{before.st_ctime_ns}\0{content_hash}\n".encode()
                )
                contents.update(f"{relative}\0{kind}\0{content_hash}\n".encode())
                if kind == "directory":
                    child_fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | NOFOLLOW, dir_fd=folder_fd)
                    try:
                        opened = os.fstat(child_fd)
                        if (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
                            raise RuntimeError("directory changed while opening")
                        visit(child_fd, relative)
                    finally:
                        os.close(child_fd)
            except (OSError, RuntimeError) as error:
                counts["errors"] += 1
                errors.append(f"entry:{getattr(error, 'errno', None)}:{type(error).__name__}")
        final = os.fstat(folder_fd)
        if (initial.st_dev, initial.st_ino, initial.st_mtime_ns, initial.st_ctime_ns) != (
            final.st_dev, final.st_ino, final.st_mtime_ns, final.st_ctime_ns
        ):
            counts["errors"] += 1
            errors.append("directory changed during scan")

    try:
        visit(root_fd, "")
        current = root.lstat()
        if (current.st_dev, current.st_ino) != (root_stat.st_dev, root_stat.st_ino):
            counts["errors"] += 1
            errors.append("root path changed during scan")
    finally:
        os.close(root_fd)
    return {"sha256": digest.hexdigest(), "content_sha256": contents.hexdigest(),
            **counts, "error_kinds": sorted(set(errors))}


# macOS 的 RENAME_EXCL 为 0x4；启动前会在同卷隔离目录内实测其语义。
RENAME_EXCL = 0x4
_LEASES = {}
_CREATE_FAILURES = []


def rename_exclusive(source_fd, source_name, target_fd, target_name):
    if sys.platform != "darwin":
        raise OSError(errno.ENOTSUP, "renameatx_np requires macOS")
    libc = ctypes.CDLL(None, use_errno=True)
    try:
        call = libc.renameatx_np
    except AttributeError as error:
        raise OSError(errno.ENOTSUP, "renameatx_np unavailable") from error
    call.argtypes = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint)
    call.restype = ctypes.c_int
    if call(source_fd, os.fsencode(source_name), target_fd, os.fsencode(target_name), RENAME_EXCL):
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code))


def _entry_state(folder_fd, name):
    before = os.stat(name, dir_fd=folder_fd, follow_symlinks=False)
    if not stat.S_ISREG(before.st_mode):
        raise RuntimeError("entry is not an ordinary file")
    fd, _ = checked_file(folder_fd, name, {"dev": before.st_dev, "ino": before.st_ino})
    try:
        content = digest_fd(fd)
        after = os.fstat(fd)
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (
            after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns
        ):
            raise RuntimeError("entry changed while reading")
        return {"mode": oct(stat.S_IMODE(after.st_mode)), "sha256": content,
                "dev": after.st_dev, "ino": after.st_ino}
    finally:
        os.close(fd)


def _same_entry(folder_fd, name, identity):
    try:
        current = os.stat(name, dir_fd=folder_fd, follow_symlinks=False)
    except FileNotFoundError:
        return False
    return (current.st_dev, current.st_ino) == identity


def _lease_identity_ok(lease):
    root_fd, quarantine_fd = lease["root_fd"], lease["quarantine_fd"]
    root = os.fstat(root_fd)
    quarantine = os.fstat(quarantine_fd)
    if (root.st_dev, root.st_ino) != lease["root_identity"]:
        return False
    if (quarantine.st_dev, quarantine.st_ino) != lease["quarantine_identity"]:
        return False
    if root.st_dev != quarantine.st_dev or stat.S_IMODE(quarantine.st_mode) != 0o700:
        return False
    try:
        path_root = lease["root"].lstat()
        path_quarantine = os.stat(lease["quarantine_name"], dir_fd=root_fd, follow_symlinks=False)
    except (OSError, RuntimeError):
        return False
    return ((path_root.st_dev, path_root.st_ino) == lease["root_identity"]
            and (path_quarantine.st_dev, path_quarantine.st_ino) == lease["quarantine_identity"]
            and stat.S_ISDIR(path_quarantine.st_mode))


def _probe_exclusive_rename(folder_fd):
    source = f".capability-source-{secrets.token_hex(8)}"
    target = f".capability-target-{secrets.token_hex(8)}"
    renamed = f".capability-renamed-{secrets.token_hex(8)}"
    opened_fd = None
    opened_identity = None
    try:
        for name in (source, target):
            opened_fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | NOFOLLOW, 0o600, dir_fd=folder_fd)
            opened = os.fstat(opened_fd)
            opened_identity = (opened.st_dev, opened.st_ino)
            os.close(opened_fd)
            opened_fd = None
        source_state = os.stat(source, dir_fd=folder_fd, follow_symlinks=False)
        target_state = os.stat(target, dir_fd=folder_fd, follow_symlinks=False)
        try:
            rename_exclusive(folder_fd, source, folder_fd, target)
        except OSError as error:
            if error.errno != errno.EEXIST:
                raise
        else:
            raise RuntimeError("exclusive rename replaced existing target")
        source_after = os.stat(source, dir_fd=folder_fd, follow_symlinks=False)
        target_after = os.stat(target, dir_fd=folder_fd, follow_symlinks=False)
        if ((source_after.st_dev, source_after.st_ino) != (source_state.st_dev, source_state.st_ino) or
            (target_after.st_dev, target_after.st_ino) != (target_state.st_dev, target_state.st_ino)):
            raise RuntimeError("exclusive rename modified entries on EEXIST")
        rename_exclusive(folder_fd, source, folder_fd, renamed)
        moved = os.stat(renamed, dir_fd=folder_fd, follow_symlinks=False)
        if ((moved.st_dev, moved.st_ino) != (source_state.st_dev, source_state.st_ino) or
            _entry_state(folder_fd, renamed)["mode"] != "0o600"):
            raise RuntimeError("exclusive rename capability probe failed")
        try:
            os.stat(source, dir_fd=folder_fd, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            raise RuntimeError("exclusive rename left source entry")
    finally:
        cleanup_errors = []
        if opened_fd is not None and opened_identity is not None:
            issue = _close_if_still_ours(opened_fd, opened_identity)
            if issue:
                cleanup_errors.append(issue)
        for name in (source, target, renamed):
            try:
                os.unlink(name, dir_fd=folder_fd)
            except FileNotFoundError:
                pass
            except OSError as error:
                cleanup_errors.append(f"capability entry cleanup failed: {error.errno}")
        if cleanup_errors:
            raise RuntimeError("exclusive rename probe cleanup uncertain: " + "; ".join(cleanup_errors))


def _prepare_lease(root, path):
    root_fd = open_directory(root)
    name = f".r00-a01-quarantine-{secrets.token_hex(16)}"
    quarantine_fd = None
    made = False
    try:
        root_state = os.fstat(root_fd)
        os.mkdir(name, 0o700, dir_fd=root_fd)
        made = True
        quarantine_fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | NOFOLLOW, dir_fd=root_fd)
        quarantine = os.fstat(quarantine_fd)
        if root_state.st_dev != quarantine.st_dev or stat.S_IMODE(quarantine.st_mode) != 0o700:
            raise RuntimeError("quarantine must be 0700 on sentinel filesystem")
        lease = {"root": root, "path": path, "root_fd": root_fd, "root_identity": (root_state.st_dev, root_state.st_ino),
                 "quarantine_fd": quarantine_fd, "quarantine_name": name,
                 "quarantine_identity": (quarantine.st_dev, quarantine.st_ino), "sentinel_fd": None,
                 "sentinel_identity": None, "created": False}
        if not _lease_identity_ok(lease):
            raise RuntimeError("quarantine identity changed")
        _probe_exclusive_rename(quarantine_fd)
        return lease
    except BaseException as error:
        cleanup_errors = []
        if made:
            try:
                if quarantine_fd is None:
                    raise RuntimeError("quarantine identity unavailable")
                held = os.fstat(quarantine_fd)
                current = os.stat(name, dir_fd=root_fd, follow_symlinks=False)
                if (held.st_dev, held.st_ino) != (current.st_dev, current.st_ino):
                    raise RuntimeError("quarantine replaced during setup")
                if os.listdir(quarantine_fd):
                    raise RuntimeError("quarantine is not empty")
                os.rmdir(name, dir_fd=root_fd)
            except (OSError, RuntimeError) as cleanup_error:
                cleanup_errors.append(f"quarantine setup cleanup: {cleanup_error}")
        for held_fd in (quarantine_fd, root_fd):
            if held_fd is not None:
                try:
                    os.close(held_fd)
                except OSError as cleanup_error:
                    cleanup_errors.append(f"descriptor close: {cleanup_error}")
        if cleanup_errors:
            raise RuntimeError("quarantine setup failed; cleanup uncertain: " + "; ".join(cleanup_errors)) from error
        raise


def _close_lease(lease, cleanup):
    errors = []
    try:
        lease_identity_ok = _lease_identity_ok(lease)
    except (OSError, RuntimeError):
        lease_identity_ok = False
    if lease_identity_ok:
        try:
            if os.listdir(lease["quarantine_fd"]):
                raise RuntimeError("quarantine is not empty")
            os.rmdir(lease["quarantine_name"], dir_fd=lease["root_fd"])
            cleanup["quarantine_removed"] = True
        except (OSError, RuntimeError) as error:
            cleanup["quarantine_removed"] = False
            errors.append(f"quarantine removal: {type(error).__name__}:{getattr(error, 'errno', None)}")
    else:
        cleanup["quarantine_removed"] = False
    for key in ("sentinel_fd", "quarantine_fd", "root_fd"):
        fd = lease.get(key)
        if fd is not None:
            try:
                os.close(fd)
            except OSError as error:
                errors.append(f"{key} close: {error.errno}")
            lease[key] = None
    _LEASES.pop(lease["path"], None)
    if errors or not cleanup["quarantine_removed"]:
        cleanup["removed"] = False
        cleanup["reason"] = "; ".join(errors) or "quarantine cleanup uncertain"
    return cleanup


def _close_if_still_ours(fd, identity):
    if fd is None:
        return None
    try:
        current = os.fstat(fd)
    except OSError as error:
        if error.errno == errno.EBADF:
            return None
        return f"descriptor state unknown: {error.errno}"
    if (current.st_dev, current.st_ino) != identity:
        return "descriptor identity changed; not closed"
    try:
        os.close(fd)
    except OSError as error:
        return f"descriptor close failed: {error.errno}"
    return None


def create_sentinel(root, run_id):
    """创建与最后一次父目录关闭均在同一清理事务内。"""
    path = root / f".r00-a01-readonly-{run_id}"
    if path in _LEASES:
        raise FileExistsError("sentinel transaction already exists")
    lease = _prepare_lease(root, path)
    _LEASES[path] = lease
    parent_fd = None
    fd = None
    written = 0
    try:
        parent_fd = os.dup(lease["root_fd"])
        fd = os.open(path.name, os.O_RDWR | os.O_CREAT | os.O_EXCL | NOFOLLOW, 0o400, dir_fd=parent_fd)
        lease["created"] = True
        identity = os.fstat(fd)
        lease["sentinel_identity"] = (identity.st_dev, identity.st_ino)
        lease["sentinel_fd"] = os.dup(fd)
        while written < len(SENTINEL_BODY):
            count = os.write(fd, SENTINEL_BODY[written:])
            if count <= 0:
                raise OSError("short sentinel write")
            written += count
        os.fchmod(fd, 0o400)
        os.fsync(fd)
        os.lseek(fd, 0, os.SEEK_SET)
        if digest_fd(fd) != hashlib.sha256(SENTINEL_BODY).hexdigest():
            raise RuntimeError("sentinel readback mismatch")
        final = os.fstat(fd)
        if (final.st_dev, final.st_ino) != lease["sentinel_identity"]:
            raise RuntimeError("sentinel identity changed")
        os.close(fd)
        fd = None
        created = {"created_utc": now(), "mode": "0o400", "sha256": hashlib.sha256(SENTINEL_BODY).hexdigest(),
                   "dev": identity.st_dev, "ino": identity.st_ino}
        if _entry_state(parent_fd, path.name) != {key: created[key] for key in ("mode", "sha256", "dev", "ino")}:
            raise RuntimeError("sentinel path changed after close")
        os.close(parent_fd)
        parent_fd = None
        return path, created
    except BaseException as error:
        if lease["created"] and lease["sentinel_identity"] is None and fd is not None:
            try:
                identity = os.fstat(fd)
                lease["sentinel_identity"] = (identity.st_dev, identity.st_ino)
                lease["sentinel_fd"] = os.dup(fd)
            except OSError:
                pass
        close_errors = []
        if fd is not None and lease["sentinel_identity"] is not None:
            issue = _close_if_still_ours(fd, lease["sentinel_identity"])
            if issue:
                close_errors.append(issue)
        if parent_fd is not None:
            issue = _close_if_still_ours(parent_fd, lease["root_identity"])
            if issue:
                close_errors.append(issue)
        if lease["created"] and lease["sentinel_identity"] is not None:
            partial = {"dev": lease["sentinel_identity"][0], "ino": lease["sentinel_identity"][1],
                       "mode": "0o400", "sha256": hashlib.sha256(SENTINEL_BODY[:written]).hexdigest()}
            cleanup = delete_owned_sentinel(path, partial)
            _CREATE_FAILURES.append({"sentinel_name": path.name, "cleanup": cleanup,
                                     "cause": type(error).__name__, "close_errors": close_errors})
            if not cleanup["removed"] or close_errors:
                raise RuntimeError(f"sentinel creation failed; cleanup uncertain: {cleanup}; {close_errors}") from error
        elif lease["created"]:
            cleanup = _close_lease(lease, {"removed": False, "reason": "identity unknown"})
            _CREATE_FAILURES.append({"sentinel_name": path.name, "cleanup": cleanup,
                                     "cause": type(error).__name__, "close_errors": close_errors})
            raise RuntimeError("sentinel creation failed before identity could be recorded; cleanup uncertain") from error
        else:
            cleanup = _close_lease(lease, {"removed": True})
            _CREATE_FAILURES.append({"sentinel_name": path.name, "cleanup": cleanup,
                                     "cause": type(error).__name__, "close_errors": close_errors})
        raise


def sentinel_state(path):
    parent_fd = open_directory(path.parent)
    try:
        return _entry_state(parent_fd, path.name)
    finally:
        os.close(parent_fd)


def delete_owned_sentinel(path, original):
    """排他移动后核对对象；外来项尽力无覆盖恢复，绝不删除。"""
    lease = _LEASES.get(path)
    if lease is None:
        return {"removed": False, "reason": "no recorded ownership transaction"}
    result = {"removed": False, "quarantine_path": str(path.parent / lease["quarantine_name"]),
              "reason": "cleanup incomplete"}
    target_name = f"held-{secrets.token_hex(16)}"
    root_fd, quarantine_fd = lease["root_fd"], lease["quarantine_fd"]
    move_error = None
    try:
        if not _lease_identity_ok(lease):
            raise RuntimeError("root or quarantine identity changed")
        if lease["sentinel_identity"] is None or lease["sentinel_fd"] is None:
            raise RuntimeError("sentinel identity unavailable")
        held = os.fstat(lease["sentinel_fd"])
        if (held.st_dev, held.st_ino) != lease["sentinel_identity"]:
            raise RuntimeError("recorded sentinel descriptor changed")
        try:
            before = _entry_state(root_fd, path.name)
        except (OSError, RuntimeError):
            before = None
        # 即使初检通过，移动前也允许名字被替换；移动后才决定是否删除。
        try:
            rename_exclusive(root_fd, path.name, quarantine_fd, target_name)
        except OSError as error:
            move_error = error
        try:
            os.stat(target_name, dir_fd=quarantine_fd, follow_symlinks=False)
        except FileNotFoundError:
            raise RuntimeError(f"exclusive move did not yield entry: {getattr(move_error, 'errno', None)}")
        try:
            moved = _entry_state(quarantine_fd, target_name)
        except (OSError, RuntimeError):
            moved = None
        owned = (moved == {key: original[key] for key in ("mode", "sha256", "dev", "ino")}
                 and (moved["dev"], moved["ino"]) == lease["sentinel_identity"])
        if not owned:
            try:
                os.stat(path.name, dir_fd=root_fd, follow_symlinks=False)
            except FileNotFoundError:
                try:
                    rename_exclusive(quarantine_fd, target_name, root_fd, path.name)
                    result["foreign_restored"] = True
                except OSError as error:
                    result["restore_errno"] = error.errno
            result["reason"] = "moved entry is not owned sentinel; foreign entry retained or restored"
            return result
        if move_error is not None:
            result["reason"] = f"exclusive move raised after possible effect: {move_error.errno}"
            # 已确认自有对象可收尾，但此尝试仍失败。
        os.unlink(target_name, dir_fd=quarantine_fd)
        try:
            os.stat(target_name, dir_fd=quarantine_fd, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            raise RuntimeError("owned sentinel remains in quarantine")
        try:
            os.stat(path.name, dir_fd=root_fd, follow_symlinks=False)
        except FileNotFoundError:
            source_absent = True
        else:
            source_absent = False
        result.update({"removed": source_absent and move_error is None,
                       "absent_after": source_absent, "removed_utc": now(),
                       "reason": None if source_absent and move_error is None else "source recreated or move error"})
    except (OSError, RuntimeError) as error:
        result["reason"] = f"{type(error).__name__}:{getattr(error, 'errno', None)}"
        result["errno"] = getattr(error, "errno", None)
    finally:
        _close_lease(lease, result)
    return result

def ps_args(pid):
    return subprocess.run(["ps", "-p", str(pid), "-o", "pid=,ppid=,command="],
                          capture_output=True, text=True).stdout.strip()


def app_pids():
    output = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True, check=True).stdout
    return [int(line.lstrip().split(" ", 1)[0]) for line in output.splitlines()
            if "/Applications/Lingxi.app/Contents/MacOS/Lingxi" in line]


def quiet_window_state():
    """保守检查：存在相关进程、打开句柄或检查错误时均不声称安静。"""
    process = subprocess.run(["ps", "-axo", "pid=,command="], capture_output=True, text=True, check=True)
    related = []
    for line in process.stdout.splitlines():
        if any(marker in line for marker in ("/Applications/Lingxi.app/", "server/bootstrap.ts", "server/main-full.ts")):
            if "startup_probe_r2.py" not in line and "ps -axo" not in line:
                related.append(int(line.strip().split(None, 1)[0]))
    handles = subprocess.run(["lsof", "-w", "-Fpf", "+D", str(REAL_DIRS["production"]),
                              "+D", str(REAL_DIRS["development"])],
                             capture_output=True, text=True, timeout=30)
    # lsof 的 1 表示没有命中；其他错误不可作为无写者证据。
    if handles.returncode not in (0, 1) or handles.stderr.strip():
        raise RuntimeError("real directory open-handle scan failed")
    opened = sorted({int(line[1:]) for line in handles.stdout.splitlines()
                     if line.startswith("p") and line[1:].isdigit()})
    return {"related_process_pids": related, "open_handle_pids": opened,
            "quiet": not related and not opened, "checked_utc": now()}


def aggregate_attempts(attempts, current_epoch):
    """同一环境中的失败不能由后续稳定观察掩盖。"""
    current = [item for item in attempts if item["environment_epoch"] == current_epoch]
    if not current or any(item["status"] != "OBSERVED_STABLE" or not item["quiet_window"]
                          or not item["cleanup_safe"] or not item["all_checks"] for item in current):
        return "BLOCKED"
    return "PASS"


def health(port, token):
    request = urllib.request.Request(f"http://127.0.0.1:{port}/api/health",
                                     headers={"Authorization": f"Bearer {token}"})
    with urllib.request.urlopen(request, timeout=3) as response:
        body = json.load(response)
        return {"http_status": response.status, "body_status": body.get("status")}


def stop_child(child, result):
    """只停止本探针启动的进程组；失败也继续执行哨兵清理。"""
    try:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=20)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)
            result["forced_shutdown"] = True
        result["server_exit_code"] = child.returncode
    except (OSError, subprocess.SubprocessError) as error:
        result["errors"].append({"type": type(error).__name__, "during": "own_server_shutdown",
                                 "errno": getattr(error, "errno", None)})


def denied_open(path, profile):
    code = "import os,sys; os.open(sys.argv[1], os.O_WRONLY)"
    proc = subprocess.run(["sandbox-exec", "-f", str(profile), shutil.which("python3") or "python3",
                           "-c", code, str(path)], capture_output=True, text=True)
    # 只保留异常类别和 errno，不保存用户路径或真实文件内容。
    return {"exit_code": proc.returncode, "errno_eperm": "PermissionError: [Errno 1]" in proc.stderr,
            "stderr_sha256": hashlib.sha256(proc.stderr.encode()).hexdigest()}


def main():
    _CREATE_FAILURES.clear()
    OUT.mkdir(parents=True, exist_ok=True)
    run_id = secrets.token_hex(12)
    result = {"run_id": run_id, "started_utc": now(), "tested_sha": subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "platform": platform.platform(), "probe_sha256": sha_file(pathlib.Path(__file__)),
        "real_directories": {}, "cleanup": {}, "errors": []}
    test_root = pathlib.Path(tempfile.mkdtemp(prefix="lingxi-r00-a01-r2-", dir="/tmp"))
    isolated = test_root / "isolated"
    (isolated / "tmp").mkdir(parents=True)
    actual_home = isolated / "lingxi-home"
    result["test_root"] = str(test_root)
    result["isolated_home"] = str(actual_home)
    result["lingxi_app_pids_before"] = app_pids()
    result["attempt_kind"] = "single observation; scenario result is recorded separately"
    result["evidence_files"] = {"probe": f"startup-probe-r3-{run_id}.json",
                                "stdout": f"server-stdout-r3-{run_id}.txt",
                                "stderr": f"server-stderr-r3-{run_id}.txt"}
    sentinels = {}
    child = None
    try:
        result["quiet_before"] = quiet_window_state()
        if not result["quiet_before"]["quiet"]:
            raise RuntimeError("real directories do not have a quiet window")
        for label, root in REAL_DIRS.items():
            if not root.is_dir() or root.is_symlink():
                raise RuntimeError(f"{label} real directory missing or symlinked")
            path, created = create_sentinel(root, run_id)
            sentinels[label] = (path, created)
            result["real_directories"][label] = {"sentinel_name": path.name, "sentinel_created": created}
        for label, root in REAL_DIRS.items():
            result["real_directories"][label]["before"] = tree_digest(root)

        profile = test_root / "probe.sb"
        profile.write_text(f'(version 1)\n(allow default)\n(deny file-write* (subpath "{REAL_HOME}"))\n',
                           encoding="utf-8")
        result["sandbox_profile_sha256"] = sha_file(profile)
        for label, (path, _) in sentinels.items():
            result["real_directories"][label]["denied_open"] = denied_open(path, profile)

        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LANG": os.environ.get("LANG", "en_US.UTF-8"),
               "HOME": str(isolated), "TMPDIR": str(isolated / "tmp"), "LINGXI_HOME": str(actual_home),
               "LINGXI_ROOT": str(ROOT), "LINGXI_SERVER_ENTRY": str(ROOT / "server" / "main-full.ts"),
               "LINGXI_PORT": "0", "LINGXI_CREATE_STARTUP_SESSION": "0"}
        command = ["sandbox-exec", "-f", str(profile), shutil.which("node") or "node", "server/bootstrap.ts"]
        result["command"] = command
        result["env"] = {key: env[key] for key in ("HOME", "TMPDIR", "LINGXI_HOME", "LINGXI_ROOT", "LINGXI_SERVER_ENTRY", "LINGXI_PORT")}
        info_path = actual_home / "server-info.json"
        with (OUT / result["evidence_files"]["stdout"]).open("x") as stdout, \
             (OUT / result["evidence_files"]["stderr"]).open("x") as stderr:
            child = subprocess.Popen(command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr, start_new_session=True)
            result["pid"] = child.pid
            result["process_args"] = ps_args(child.pid)
            deadline = time.monotonic() + 90
            info = None
            while time.monotonic() < deadline:
                if child.poll() is not None:
                    result["startup_error"] = f"server exited before ready: {child.returncode}"
                    break
                try:
                    info = json.loads(info_path.read_text(encoding="utf-8"))
                    break
                except (OSError, json.JSONDecodeError):
                    time.sleep(0.2)
            if info is None and "startup_error" not in result:
                result["startup_error"] = "server-info timeout"
            if info:
                result["server_info_observed_in_isolated_home"] = True
                result["server_pid"] = info.get("pid")
                result["server_process_args"] = ps_args(info["pid"])
                result["health"] = health(info["port"], info["token"])
                result["isolated_during"] = tree_digest(isolated)
            stop_child(child, result)
            result["server_info_removed"] = not info_path.exists()

        for label, root in REAL_DIRS.items():
            result["real_directories"][label]["after"] = tree_digest(root)
            result["real_directories"][label]["sentinel_after"] = sentinel_state(sentinels[label][0])
        result["isolated_after"] = tree_digest(isolated)
        result["isolated_logs_present"] = (actual_home / "logs").is_dir()
        result["actual_home_logged"] = f"lingxiHome={actual_home}" in (
            OUT / result["evidence_files"]["stdout"]).read_text(errors="replace")
    except Exception as error:
        result["errors"].append({"type": type(error).__name__, "errno": getattr(error, "errno", None)})
    finally:
        if child is not None and child.poll() is None:
            stop_child(child, result)
        for label, (path, created) in sentinels.items():
            try:
                result["cleanup"][label] = delete_owned_sentinel(path, created)
            except Exception as error:
                result["cleanup"][label] = {"removed": False, "error_type": type(error).__name__,
                                            "errno": getattr(error, "errno", None)}
        result["creation_failures"] = list(_CREATE_FAILURES)
        try:
            result["lingxi_app_pids_after"] = app_pids()
        except Exception as error:
            result["errors"].append({"type": type(error).__name__, "during": "app_process_scan"})
            result["lingxi_app_pids_after"] = None
        try:
            result["quiet_after"] = quiet_window_state()
        except Exception as error:
            result["errors"].append({"type": type(error).__name__, "during": "quiet_after"})
        result["ended_utc"] = now()
        checks = {}
        for label, data in result["real_directories"].items():
            checks[f"{label}_full_content_unchanged"] = (
                data.get("before") == data.get("after") and data.get("before", {}).get("errors") == 0)
            checks[f"{label}_sentinel_unchanged"] = (
                data.get("sentinel_after") == {key: data["sentinel_created"][key]
                                                for key in ("mode", "sha256", "dev", "ino")})
            checks[f"{label}_denied_by_os"] = data.get("denied_open", {}).get("errno_eperm") is True
            checks[f"{label}_cleanup"] = (result["cleanup"].get(label, {}).get("removed") is True
                                            and result["cleanup"].get(label, {}).get("quarantine_removed") is True)
        checks.update({"service_health": result.get("health") == {"http_status": 200, "body_status": "ok"},
                       "service_closed": result.get("server_exit_code") == 0 and result.get("server_info_removed") is True,
                       "isolated_home_effective": result.get("actual_home_logged") is True
                       and result.get("server_info_observed_in_isolated_home") is True
                       and result.get("health", {}).get("http_status") == 200
                       and result.get("server_info_removed") is True,
                       "isolated_data_and_logs": result.get("isolated_after", {}).get("files", 0) > 0
                       and result.get("isolated_after", {}).get("errors") == 0
                       and result.get("isolated_logs_present") is True,
                       "quiet_window": result.get("quiet_before", {}).get("quiet") is True
                       and result.get("quiet_after", {}).get("quiet") is True,
                       "real_app_absent": result.get("lingxi_app_pids_before") == []
                       and result.get("lingxi_app_pids_after") == []})
        result["acceptance_checks"] = checks
        result["status"] = "OBSERVED_STABLE" if not result["errors"] and checks and all(checks.values()) else "BLOCKED"
        with (OUT / result["evidence_files"]["probe"]).open("x", encoding="utf-8") as stream:
            stream.write(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    return 0 if result["status"] == "OBSERVED_STABLE" else 1


if __name__ == "__main__":
    raise SystemExit(main())
