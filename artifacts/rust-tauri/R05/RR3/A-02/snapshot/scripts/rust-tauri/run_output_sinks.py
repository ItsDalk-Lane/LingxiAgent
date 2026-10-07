#!/usr/bin/env python3
"""从真实进程 fd 识别本次输出，目录归属必须由下至上完整证明。"""
import os
import subprocess
import sys

repo = os.path.realpath(sys.argv[1])
evidence_dir = os.path.realpath(sys.argv[2])
ev_rel = os.path.relpath(evidence_dir, repo)
if ev_rel == "." or ev_rel == ".." or ev_rel.startswith(".." + os.sep):
    ev_rel = ""                                   # evidence root outside the repo

# this process tree: self up to (excluding) pid 1, cycle- and depth-capped
pids, cur, seen = [], os.getpid(), set()
while cur > 1 and cur not in seen and len(pids) < 64:
    seen.add(cur)
    pids.append(cur)
    try:
        out = subprocess.run(
            ["ps", "-o", "ppid=", "-p", str(cur)],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
        cur = int(out.split()[0]) if out else 1
    except Exception as exc:
        raise RuntimeError("cannot walk run-output ancestor process") from exc

# fd 1/2 file targets per pid — /proc fast path, lsof fallback (darwin)
resolved_any = False
fd_targets = set()
have_proc = os.path.isdir("/proc")
for pid in pids:
    fds = {}
    if have_proc:
        for fd in ("1", "2"):
            try:
                fds[fd] = os.readlink("/proc/%d/fd/%s" % (pid, fd))
            except OSError:
                pass
    if not fds:
        try:
            query = subprocess.run(
                ["lsof", "-a", "-p", str(pid), "-d", "1,2", "-Fpfn"],
                capture_output=True, text=True,
            )
            # 无匹配 fd 可返回 1；命令故障或权限诊断不能吞掉后继续归属。
            if query.returncode not in (0, 1) or query.stderr.strip():
                raise RuntimeError("lsof failed: " + query.stderr.strip())
            out = query.stdout
        except Exception as exc:
            raise RuntimeError("cannot query real OS output descriptors") from exc
        cur_fd = None
        for line in out.splitlines():
            if line.startswith("p"):
                cur_fd = None
            elif line.startswith("f") and line[1:] in ("1", "2"):
                cur_fd = line[1:]
            elif line.startswith("n") and cur_fd is not None:
                fds[cur_fd] = line[1:]
                cur_fd = None
    if fds:
        resolved_any = True
        for target in fds.values():
            fd_targets.add(target)

if len(pids) == 64 and cur > 1:
    raise RuntimeError("run-output ancestor walk exceeded its bound")

if not resolved_any:
    print("DISCOVERY-UNAVAILABLE")
    sys.exit(0)

def tracked(path):
    # index-tracked entries under path (ls-files: also works on an
    # unborn-HEAD scratch repo); an unreadable answer is fail-closed
    try:
        out = subprocess.run(
            ["git", "--literal-pathspecs", "-C", repo, "ls-files", "--", path],
            check=True, capture_output=True, text=True,
        ).stdout
    except Exception as exc:
        raise RuntimeError("cannot inspect tracked sink ownership") from exc
    return out.strip() != ""

sinks = []                                        # repo-relative sink files
for target in sorted(fd_targets):
    if not target.startswith("/"):
        continue                                  # pipe / device / unnamed target
    literal = os.path.abspath(target)
    if literal == repo or not literal.startswith(repo + os.sep):
        continue
    current = repo
    for component in os.path.relpath(literal, repo).split(os.sep):
        current = os.path.join(current, component)
        if os.path.islink(current):
            raise RuntimeError("run-output sink crosses symlink: " + current)
    real = os.path.realpath(target)
    if real == repo or not real.startswith(repo + os.sep):
        continue                                  # outside this repository
    if not os.path.isfile(real):
        continue                                  # directories/devices carry no output
    rel = os.path.relpath(real, repo)
    if "\n" in rel or "\r" in rel:
        raise RuntimeError("run-output sink path contains a line separator")
    sinks.append(rel)

for rel in sinks:
    if tracked(rel):
        print("TRACKED-SINK %s" % rel)

# 目录必须自底向上获证；一个子目录有 sink 并不证明其中旧文件属于本次运行。
attribution_dirs = set(os.path.dirname(r) for r in sinks)
sink_set = set(sinks)
dir_units = set()

def walk_error(error):
    raise error

def dedicated(unit):
    base = os.path.join(repo, unit)
    for root, dirs, files in os.walk(base, onerror=walk_error):
        for name in dirs:
            if os.path.islink(os.path.join(root, name)):
                return False
        for name in files:
            full = os.path.join(root, name)
            if os.path.islink(full):
                return False
            fr = os.path.relpath(full, repo)
            if fr in sink_set:
                continue
            # 只有调用者自己的新鲜 evidence 或已经完整获证的子目录可归属。
            if ev_rel and (fr == ev_rel or fr.startswith(ev_rel + "/")):
                continue
            if any(fr.startswith(child + "/") for child in dir_units):
                continue
            return False
    return True

for cand in sorted(attribution_dirs, key=lambda p: (-p.count("/"), p)):
    if not cand.startswith("artifacts/") or len(cand.split("/")) < 4:
        continue
    if tracked(cand):
        continue
    if dedicated(cand):
        dir_units.add(cand)

emitted_dir = set()
for rel in sinks:
    if tracked(rel):
        continue                                  # already reported as TRACKED-SINK
    du = os.path.dirname(rel)
    if "--files" not in sys.argv[3:] and du in dir_units:
        if du not in emitted_dir:
            emitted_dir.add(du)
            print("DIR %s" % du)
    else:
        print("FILE %s" % rel)
