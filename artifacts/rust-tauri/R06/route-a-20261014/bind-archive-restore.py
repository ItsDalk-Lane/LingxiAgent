"""归档JSONL和sidecar恢复具名实效证明完整后，绑定原始叶；不改原断言。"""
import importlib.util
import json
import re
from pathlib import Path
HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
spec = importlib.util.spec_from_file_location("anchors", HERE.parent / "continuation-20261013" / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
leaf_id = "R00-T02-LA-31CDA283A11A"
row = next(x for x in inventory["items"] if x["id"] == leaf_id)
original = {"then": row["then"], "assertions": list(row["assertions"])}
suite = "r06_archive_jsonl_restore"
source = ROOT / "rust/crates/lingxi-service/tests" / (suite + ".rs")
tests = re.findall(r"^async fn ([a-z_]+)\(\)\s*\{", source.read_text(), re.M)
assert len(tests) == 17, tests
for group in row["groups"]:
    for test in tests:
        reference = {"package":"lingxi-service","suite":suite,"test":test}
        if reference not in group["tests"]:
            group["tests"].append(reference)
            group["proofLocations"].append(anchors.test_location(suite,test))
    group.update(coverage="FULL",missingClauses=[],meaning="真实认证HTTP核路径、实际session/agent/owner/header与冲突；JSONL和sidecar双件实际回活跃并更新DB返回新路径，旧header-only可修复而其他实际消息不可覆盖。prepared日志在最终事务核完整身份/快照/digest，不同会话并发不误收尾。实际SQLite、源/owner变化、第二件rename和权限故障拒绝回滚；提交后cleanup故障明确已提交状态并真实重启收尾。真实SIGKILL同窗口后重启双件原字节恢复，不伪造DB-only文件。配套archive/delete/bulk/periodic清理保持实物与生命周期一致。")
assert [g["index"] for g in row["groups"]] == list(range(len(row["assertions"])))
assert {"then":row["then"],"assertions":row["assertions"]} == original
review = HERE / "archive-restore-proof-review.json"
row.update(status="READY",gaps=[],reviewSource=review.relative_to(ROOT).as_posix())
anchors.MAP.write_text(json.dumps(inventory,ensure_ascii=False,indent=2)+"\n")
review.write_text(json.dumps({"status":"EXECUTED_AND_INDEPENDENTLY_REVIEWED_NOT_STAGE_ACCEPTANCE","leaf":leaf_id,"tests":tests,"independentReview":"archive-restore-independent-review-r2.json","stageAccepted":False,"scope":"macOS真实中断与I/O、并发故障；不替代其他平台、断电、正式打包或R06全门禁。"},ensure_ascii=False,indent=2)+"\n")
ids_path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
ids = json.loads(ids_path.read_text())
if leaf_id not in ids: ids.append(leaf_id)
ids_path.write_text(json.dumps(ids,indent=2)+"\n")
(HERE/"archive-restore-leaf-ids.json").write_text(json.dumps([leaf_id],indent=2)+"\n")
print("归档恢复两组原断言完整绑定；阶段仍未验收。")
