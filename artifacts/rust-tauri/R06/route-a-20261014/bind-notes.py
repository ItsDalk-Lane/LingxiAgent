"""补齐会话笔记错误边界后绑定原始叶；不改变原断言或阶段判定。"""
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
leaf = next(x for x in inventory["items"] if x["id"] == "R00-T02-LA-628005EDFF37")
original = {"assertions": list(leaf["assertions"]), "then": leaf["then"]}
suite = "r06_original_memory_skill_proofs"
test = "original_context_notes_append_reopen_and_oversize_preserve_old_value"
location = anchors.test_location(suite, test)
assert any("no_session.outcome" in q["quote"] for q in location["assertions"])
assert any("overflow.outcome" in q["quote"] for q in location["assertions"])
assert any("error.code" in q["quote"] for q in location["assertions"])
for group in leaf["groups"]:
    # 空写入由成功拒绝文案改为明确 Failed；原成功/持久化/容量断言保留并加强。
    group["proofLocations"] = [location]
    group["tests"] = [{"package": "lingxi-service", "suite": suite, "test": test}]
    if group["index"] == 3:
        group.update(
            coverage="FULL", missingClauses=[],
            meaning="实际网关执行：无会话、空write/append、缺text均Failed且原笔记不变；"
                    "恰16KiB写入成功，追加越界Failed且字节不变；17KiB失败后仍读回原值。",
        )
assert [g["index"] for g in leaf["groups"]] == list(range(len(leaf["assertions"])))
assert all(g["coverage"] == "FULL" and not g["missingClauses"] for g in leaf["groups"])
assert {"assertions": leaf["assertions"], "then": leaf["then"]} == original
report_path = HERE / "notes-proof-review.json"
leaf.update(status="READY", gaps=[], reviewSource=report_path.relative_to(ROOT).as_posix())
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report_path.write_text(json.dumps({
    "status": "EXECUTED_TEST_BOUND_NOT_STAGE_ACCEPTANCE",
    "leaf": leaf["id"], "suitePassed": 19, "suiteFailed": 0,
    "intentionalReanchor": "空写入错误合同从成功文案收紧为Failed；原叶断言不变。"
                           "测试增加无会话、缺text、空append、精确容量及越界不覆盖。",
    "proof": location,
}, ensure_ascii=False, indent=2) + "\n")
ids_path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
ids = json.loads(ids_path.read_text())
if leaf["id"] not in ids:
    ids.append(leaf["id"])
ids_path.write_text(json.dumps(ids, indent=2) + "\n")
print("会话笔记原始叶全部四组已绑定；阶段仍未验收。")
