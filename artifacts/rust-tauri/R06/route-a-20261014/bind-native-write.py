"""真实沙盒不可用拒绝证明补齐 write 原叶，不修改任何原断言。"""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
spec = importlib.util.spec_from_file_location(
    "anchors", HERE.parent / "continuation-20261013" / "review-proof-anchors.py"
)
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
row = next(x for x in inventory["items"] if x["id"] == "R00-T02-LA-4F0466F7C090")
original = {"then": row["then"], "assertions": list(row["assertions"])}
suite = "r04_t04_file_tools"
test = "native_write_refuses_missing_or_untrusted_sandbox_without_creating_or_overwriting"
location = anchors.test_location(suite, test)
group = next(g for g in row["groups"] if g["index"] == 2)
reference = {"package": "lingxi-service", "suite": suite, "test": test}
if reference not in group["tests"]:
    group["tests"].append(reference)
    group["proofLocations"].append(location)
group.update(
    coverage="FULL", missingClauses=[],
    meaning="既有实际冲突/越权/符号链接拒绝证明保留；新实际注册网关write在系统助手"
            "缺失或伪造时Failed，旧字节与变更记录不变、不创建父目录或临时残留。"
            "真实系统助手恢复时实际写入成功，再不可用时重新拒绝；read仍可用，edit也拒绝绕过。",
)
assert [g["index"] for g in row["groups"]] == list(range(len(row["assertions"])))
assert all(g["coverage"] == "FULL" and not g["missingClauses"] for g in row["groups"])
assert {"then": row["then"], "assertions": row["assertions"]} == original
report = HERE / "native-write-proof-review.json"
row.update(status="READY", gaps=[], reviewSource=report.relative_to(ROOT).as_posix())
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "EXECUTED_TEST_BOUND_NOT_STAGE_ACCEPTANCE", "leaf": row["id"],
    "proof": location,
    "scope": "补齐OS沙盒不可用拒绝这一条款；文件范围/原子性由既有FileTools/FileSafety证明。"
             "不宣称父进程文件写入在OS隔离子进程中执行。",
    "environmentCorrection": "r04_t06拒写sentinel位于编译期CARGO_TARGET_TMPDIR；"
                             "target必须在/private/tmp与TMPDIR之外，不能将本就允许的临时目录误当拒写区域。",
}, ensure_ascii=False, indent=2) + "\n")
ids_path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
ids = json.loads(ids_path.read_text())
if row["id"] not in ids:
    ids.append(row["id"])
ids_path.write_text(json.dumps(ids, indent=2) + "\n")
print("write原叶三组完整绑定；阶段仍未验收。")
