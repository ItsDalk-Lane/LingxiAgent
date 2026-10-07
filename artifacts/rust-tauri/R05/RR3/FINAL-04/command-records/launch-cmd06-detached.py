#!/usr/bin/env python3
"""FINAL-04 cmd-06 launcher: runs verify-stage R05 fully detached from the
host session via subprocess start_new_session=True (FINAL-03 attempt-1
lesson). Records argv/cwd/UTC/df; raw stdout+stderr go to the external dir.
Evidence root artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05 is created by
the gate itself (pre-verified absent before launch)."""
import subprocess, os, json, datetime, hashlib

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
OUT = "/private/tmp/rr3-final04-dir"
CARGO = "/Users/study_superior/.cargo/bin/cargo"
ARGV = [CARGO, "run", "--manifest-path", "rust/Cargo.toml", "--locked", "-p", "xtask",
        "--", "verify-stage", "R05", "--evidence",
        "artifacts/rust-tauri/R05/RR3/FINAL-04/verify-R05"]

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def df_line():
    r = subprocess.run(["df", "-h", "/System/Volumes/Data"], capture_output=True, text=True)
    return r.stdout.strip().splitlines()[-1]

stdout_p = os.path.join(OUT, "cmd-06-verify-stage-R05.stdout")
stderr_p = os.path.join(OUT, "cmd-06-verify-stage-R05.stderr")

utc_start = now()
df_before = df_line()
so = open(stdout_p, "wb")
se = open(stderr_p, "wb")
# Fully detach: new session -> not in the host's foreground process group,
# immune to host-side task kills that hit FINAL-03 attempt-1.
proc = subprocess.Popen(ARGV, cwd=REPO, stdout=so, stderr=se,
                        stdin=subprocess.DEVNULL, start_new_session=True)
pid = proc.pid
utc_spawn = now()
# Wait for completion (this process stays alive as the detached parent).
rc = proc.wait()
so.close(); se.close()
utc_end = now()
df_after = df_line()

def sha_file(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

meta = {
    "label": "cmd-06-verify-stage-R05",
    "argv": ARGV,
    "cwd": REPO,
    "launch_mode": "python subprocess start_new_session=True (fully detached)",
    "child_pid": pid,
    "utc_start": utc_start,
    "utc_spawn": utc_spawn,
    "utc_end": utc_end,
    "exit": rc,
    "df_before": df_before,
    "df_after": df_after,
    "stdout_file": os.path.basename(stdout_p),
    "stdout_sha256": sha_file(stdout_p),
    "stdout_bytes": os.path.getsize(stdout_p),
    "stderr_file": os.path.basename(stderr_p),
    "stderr_sha256": sha_file(stderr_p),
    "stderr_bytes": os.path.getsize(stderr_p),
}
with open(os.path.join(OUT, "cmd-06-verify-stage-R05.meta.txt"), "w") as f:
    json.dump(meta, f, indent=1, sort_keys=True)
print(json.dumps({k: meta[k] for k in ("child_pid", "exit", "utc_start", "utc_end",
                                       "stdout_bytes", "stderr_bytes")}))
