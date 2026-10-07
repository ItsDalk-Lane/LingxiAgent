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
            capture_output=True, text=True,
        ).stdout.strip()
        cur = int(out.split()[0]) if out else 1
    except Exception:
        cur = 1

# fd 1/2 file targets per pid — /proc fast path, lsof fallback (darwin)
resolved_any = False
fd_targets = set()
have_proc = os.path.isdir("/proc")
for pid in pids:
    fds = {}
    if have_proc:
        for fd in ("1", "2"):
            try:
                fds[fd] = os.path.realpath(os.readlink("/proc/%d/fd/%s" % (pid, fd)))
            except OSError:
                pass
    if not fds:
        try:
            out = subprocess.run(
                ["lsof", "-a", "-p", str(pid), "-d", "1,2", "-Fpfn"],
                capture_output=True, text=True,
            ).stdout
        except Exception:
            out = ""
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

if not resolved_any:
    print("DISCOVERY-UNAVAILABLE")
    sys.exit(0)

def tracked(path):
    # index-tracked entries under path (ls-files: also works on an
    # unborn-HEAD scratch repo); an unreadable answer is fail-closed
    try:
        out = subprocess.run(
            ["git", "-C", repo, "ls-files", "--", path],
            check=True, capture_output=True, text=True,
        ).stdout
    except Exception:
        return True
    return out.strip() != ""

sinks = []                                        # repo-relative sink files
for target in sorted(fd_targets):
    if not target.startswith("/"):
        continue                                  # pipe / device / unnamed target
    real = os.path.realpath(target)
    if real == repo or not real.startswith(repo + os.sep):
        continue                                  # outside this repository
    if not os.path.isfile(real):
        continue                                  # directories/devices carry no output
    sinks.append(os.path.relpath(real, repo))

for rel in sinks:
    if tracked(rel):
        print("TRACKED-SINK %s" % rel)

# DIR-unit candidacy, under the F42 fences. Candidate attribution dirs:
# every sink's directory plus the gate's own evidence dir — a file under
# a candidate DIR unit is attributable to THIS run only when it is itself
# a sink, or some directory STRICTLY BELOW the unit (never the unit
# itself — loose foreign files directly under a non-dedicated dir must
# not inherit its candidacy) is an attribution dir.
attribution_dirs = set(os.path.dirname(r) for r in sinks)
if ev_rel:
    attribution_dirs.add(ev_rel)
sink_set = set(sinks)

def dedicated(unit):
    base = os.path.join(repo, unit)
    if not os.path.isdir(base):
        return False
    for root, dirs, files in os.walk(base):
        for d in dirs:
            if os.path.islink(os.path.join(root, d)):
                return False                      # a symlinked dir could shadow content
        for f in files:
            fr = os.path.relpath(os.path.join(root, f), repo)
            if fr in sink_set:
                continue                          # itself a discovered sink
            d_ = os.path.dirname(fr)
            attributed = False
            while d_ and d_ != unit:
                if d_ in attribution_dirs:
                    attributed = True
                    break
                d_ = os.path.dirname(d_)
            if not attributed:
                return False                      # foreign/pre-existing file — not dedicated
    return True

dir_units = set()
for cand in sorted(attribution_dirs):
    if not cand or not cand.startswith("artifacts/"):
        continue                                  # DIR units only inside the evidence tree
    if len(cand.split("/")) < 4:
        continue                                  # artifacts / artifacts/<area> / a bare stage dir can never be a unit
    if tracked(cand):
        continue                                  # tracked content must stay bound
    if cand == ev_rel or dedicated(cand):
        dir_units.add(cand)

emitted_dir = set()
for rel in sinks:
    if tracked(rel):
        continue                                  # already reported as TRACKED-SINK
    du = os.path.dirname(rel)
    if du in dir_units:
        if du not in emitted_dir:
            emitted_dir.add(du)
            print("DIR %s" % du)
    else:
        print("FILE %s" % rel)
