#!/usr/bin/env python3
"""FINAL-04 §5.3 command runner: records argv/cwd/UTC start+end/exit/df
before+after/raw stdout+stderr to the external dir /private/tmp/rr3-final04-dir.
Does not swallow exits; does not filter output."""
import subprocess, sys, os, json, datetime, hashlib

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
OUT = "/private/tmp/rr3-final04-dir"
CARGO = "/Users/study_superior/.cargo/bin/cargo"

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def df_line():
    r = subprocess.run(["df", "-h", "/System/Volumes/Data"], capture_output=True, text=True)
    return r.stdout.strip().splitlines()[-1]

def sha_file(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()

label = sys.argv[1]
argv = sys.argv[2:]
stdout_p = os.path.join(OUT, f"{label}.stdout")
stderr_p = os.path.join(OUT, f"{label}.stderr")
meta_p = os.path.join(OUT, f"{label}.meta.txt")

utc_start = now()
df_before = df_line()
with open(stdout_p, "wb") as so, open(stderr_p, "wb") as se:
    proc = subprocess.run(argv, cwd=REPO, stdout=so, stderr=se)
utc_end = now()
df_after = df_line()

meta = {
    "label": label,
    "argv": argv,
    "cwd": REPO,
    "utc_start": utc_start,
    "utc_end": utc_end,
    "exit": proc.returncode,
    "df_before": df_before,
    "df_after": df_after,
    "stdout_file": os.path.basename(stdout_p),
    "stdout_sha256": sha_file(stdout_p),
    "stdout_bytes": os.path.getsize(stdout_p),
    "stderr_file": os.path.basename(stderr_p),
    "stderr_sha256": sha_file(stderr_p),
    "stderr_bytes": os.path.getsize(stderr_p),
}
with open(meta_p, "w") as f:
    json.dump(meta, f, indent=1, sort_keys=True)
print(json.dumps({k: meta[k] for k in ("label", "exit", "utc_start", "utc_end",
                                       "stdout_bytes", "stderr_bytes")}))
sys.exit(proc.returncode)
