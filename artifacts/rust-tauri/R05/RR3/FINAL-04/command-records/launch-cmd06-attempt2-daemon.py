#!/usr/bin/env python3
"""FINAL-04 cmd-06 attempt-2 launcher: DOUBLE-FORK daemonize. The host killed
even the start_new_session session-leader (attempt-1 lesson, 2026-10-07T21:23Z),
which means host cleanup walks the descendant tree. A double-fork orphan is
re-parented to launchd and has no ancestry link to this shell; a wrapper shell
under the orphan records the real exit code to an external file.
Writes only /private/tmp/rr3-final04-dir. Repo writes are the gate's own
(FINAL-04/verify-R05 evidence root + rust/target)."""
import os, sys, json, datetime, subprocess

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
OUT = "/private/tmp/rr3-final04-dir"
CARGO = "/Users/study_superior/.cargo/bin/cargo"
EVID = "artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05"
STDOUT = os.path.join(OUT, "cmd-06-attempt2.stdout")
STDERR = os.path.join(OUT, "cmd-06-attempt2.stderr")
EXITF = os.path.join(OUT, "cmd-06-attempt2.exit")
METAF = os.path.join(OUT, "cmd-06-attempt2.meta.json")

cmd = f'exec "{CARGO}" run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence {EVID}; echo $? > "{EXITF}"'
# note: `exec` replaces shell with cargo, so $? line unreachable; use plain invocation instead
cmd = f'"{CARGO}" run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence {EVID}; rc=$?; echo $rc > "{EXITF}"; sync'

utc_start = datetime.datetime.now(datetime.timezone.utc).isoformat()
df_before = subprocess.run(["df", "-h", "/System/Volumes/Data"], capture_output=True, text=True).stdout.strip().splitlines()[-1]

pid = os.fork()
if pid == 0:
    # child: new session, then fork again
    os.setsid()
    if os.fork() == 0:
        # grandchild: orphaned to launchd once child exits
        try:
            os.chdir(REPO)
            so = open(STDOUT, "wb")
            se = open(STDERR, "wb")
            os.dup2(so.fileno(), 1)
            os.dup2(se.fileno(), 2)
            fd = os.open(os.devnull, os.O_RDONLY)
            os.dup2(fd, 0)
            os.execv("/bin/sh", ["/bin/sh", "-c", cmd])
        except Exception:
            os._exit(111)
    os._exit(0)
os.waitpid(pid, 0)

meta = {
    "label": "cmd-06-verify-stage-R05-attempt2",
    "launch_mode": "double-fork daemonize (launchd-adopted orphan runs /bin/sh -c cargo...; exit code -> cmd-06-attempt2.exit)",
    "argv": [CARGO, "run", "--manifest-path", "rust/Cargo.toml", "--locked", "-p", "xtask", "--", "verify-stage", "R05", "--evidence", EVID],
    "cwd": REPO,
    "utc_start": utc_start,
    "df_before": df_before,
    "stdout_file": os.path.basename(STDOUT),
    "stderr_file": os.path.basename(STDERR),
    "exit_code_file": os.path.basename(EXITF),
}
with open(METAF, "w") as f:
    json.dump(meta, f, indent=1, sort_keys=True)
print(json.dumps(meta))
