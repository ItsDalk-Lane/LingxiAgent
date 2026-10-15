"""按原义务登记真实文件发现入口；不替代阶段及其他平台验收。"""
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
ids = ["R00-T02-LA-B97D0D4296FA", "R00-T02-LA-EC77C2B04726", "R00-T02-LA-F88B0A969094"]
suite = "r06_native_discovery_current_source"
test = "macos_native_discovery_current_binary_real_run_and_boundaries"
reference = {"package": "lingxi-service", "suite": suite, "test": test}
for row in inventory["items"]:
    if row["id"] not in ids:
        continue
    original = (list(row["assertions"]), row["then"])
    assert len(row["groups"]) == len(row["assertions"]) == 3
    for group in row["groups"]:
        if reference not in group["tests"]:
            group["tests"].append(reference)
            group["proofLocations"].append(anchors.test_location(suite, test))
        group.update(
            coverage="FULL", missingClauses=[],
            meaning="真实当前Rust服务和模型注册发现/调用ls、find、grep；原40加独立6与深目录取消4共50场景，实际持久历史、精确路径与Unicode位置、空命中、无效正则、授权/链接交换、400层堆栈、取消有界收尾与无文件变更。原合法反斜线和深目录崩溃反例保留并修后通过。Windows扩展盘符与UNC匹配独立纯逻辑修后通过，实际Windows/Linux运行仍NOT_RUN；不宣称其他平台或正式阶段通过。",
        )
    assert (row["assertions"], row["then"]) == original
    row.update(status="READY", gaps=[], reviewSource=(HERE / "native-discovery-proof-review.json").relative_to(ROOT).as_posix())
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "native-discovery-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
(HERE / "native-discovery-proof-review.json").write_text(json.dumps({
    "status": "INDEPENDENT_NATIVE_MAC_PASS_NOT_STAGE_ACCEPTANCE",
    "leaves": ids, "stageAccepted": False,
    "independentReviews": ["native-discovery-r3-independent-review.md", "native-discovery-r4-independent-review.md"],
    "platforms": {"macOS": "ACTUAL_CURRENT_SOURCE_PASS", "Windows": "IMPLEMENTED_TYPECHECKED_RUNTIME_NOT_RUN", "Linux": "IMPLEMENTED_RUNTIME_NOT_RUN"},
    "scope": "逐叶原成功/拒绝/无写入结果；其他平台实际运行和最终完整门禁仍开放。",
}, ensure_ascii=False, indent=2) + "\n")
path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(path.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
path.write_text(json.dumps(reviewed, indent=2) + "\n")
print("文件发现三叶按原断言登记，等待实际绑定窗口；阶段仍未验收。")
