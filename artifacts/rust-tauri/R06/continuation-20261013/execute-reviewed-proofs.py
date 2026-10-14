"""Execute actual registered proofs; the selected scope cannot accept R06."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
IDS = HERE / "reviewed-original-ids.json"
SCRIPT = ROOT / "scripts/rust-tauri/r06_stage_inputs.py"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--evidence", type=Path, default=HERE / "reviewed-execution")
parser.add_argument("--proof-leaf-ids", type=Path, default=IDS)
arguments = parser.parse_args()
ids = arguments.proof_leaf_ids.resolve()
inputs = json.loads((ROOT / "rust/crates/xtask/src/stage_maps/R06_INPUTS.json").read_text())
identifiers = json.loads(ids.read_text())
selected = {
    key for identifier in identifiers for group in inputs["leafBindings"][identifier]
    for key in group["commandRefs"]
}
evidence = arguments.evidence.resolve()
if evidence.exists():
    raise ValueError("Evidence already exists; do not reuse an execution window")
failures = []
for key in sorted(selected):
    binding = inputs["executionBindings"][key]
    env = os.environ.copy()
    if binding["kind"] == "rust_desktop_test":
        env.pop("CARGO_TARGET_DIR", None)
    else:
        # 保留调用者指定的独立构建缓存；未指定时同样放在仓库外。
        env.setdefault("CARGO_TARGET_DIR", str(Path(tempfile.gettempdir()) / "lingxi-r06-reviewed-target"))
    result = subprocess.run([
        sys.executable, "-B", str(SCRIPT), "--binding", key, "--evidence",
        str(evidence / "R06_BINDINGS" / key),
    ], cwd=ROOT, env=env, timeout=binding["timeoutSecs"] + 180)
    print(f"{key}: exit={result.returncode}", flush=True)
    if result.returncode:
        failures.append(key)

rolled_up = subprocess.run([
    sys.executable, "-B", str(SCRIPT), "--leaf-evidence", "--binding-evidence",
    str(evidence / "R06_BINDINGS"), "--proof-leaf-ids", str(ids),
    "--evidence", str(evidence / "R06_LEAVES"),
], cwd=ROOT, timeout=120)
print(json.dumps({
    "scope": f"{len(identifiers)} selected registered original requirements; NOT R06 or R07 acceptance",
    "registeredBindingsExecuted": len(selected),
    "failedBindings": failures,
    "rollupExitCode": rolled_up.returncode,
}), flush=True)
sys.exit(1 if failures or rolled_up.returncode else 0)
