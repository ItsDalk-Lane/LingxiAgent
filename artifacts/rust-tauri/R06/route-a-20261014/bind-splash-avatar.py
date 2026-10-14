"""当前Rust保存后停服，真实Electron桥及启动组件逐原断言绑定。"""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
spec = importlib.util.spec_from_file_location("anchors", HERE.parent / "continuation-20261013" / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
leaf_id = "R00-T02-LA-A02D8C572F5C"
row = next(x for x in inventory["items"] if x["id"] == leaf_id)
original = {"assertions": list(row["assertions"]), "then": row["then"]}
suite = "r06_splash_avatar_live"
test = "current_rust_saved_offline_splash_real_ipc_and_image_fallback"
reference = {"package": "lingxi-service", "suite": suite, "test": test}
assert [g["index"] for g in row["groups"]] == [0, 1]
for group in row["groups"]:
    if reference not in group["tests"]:
        group["tests"].append(reference)
        group["proofLocations"].append(anchors.test_location(suite, test))
    group.update(coverage="FULL", missingClauses=[], meaning="当前候选Rust真实API保存PNG/JPEG/WebP并切当前助手，真实停服后生产IPC/preload/当前SplashApp原生窗口显示正确本地WebP。原16场景含真实图片解码、缺失/坏格式/非普通文件/祖先及文件链接/EACCES和返回路径后加载失败的默认回退；最终原头像字节保留。独立当前binary19场景补真实子frame/非名单本地页拒绝、WebP异步解码途中链接替换拒绝。原生fixture在tests/rust-native纳入源摘要，无白名单或门禁变更。不宣称完整引导、Tauri、其他平台或正式打包。")
assert {"assertions": row["assertions"], "then": row["then"]} == original
row.update(status="READY", gaps=[], reviewSource=(HERE / "splash-avatar-proof-review.json").relative_to(ROOT).as_posix())
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "splash-avatar-proof-review.json").write_text(json.dumps({"status":"INDEPENDENT_CURRENT_RUST_BINARY_NATIVE_PASS_NOT_STAGE_ACCEPTANCE","leaves":[leaf_id],"stageAccepted":False,"independentReview":"splash-avatar-independent-review.json","nativeFixture":"tests/rust-native/splash-avatar-native.cjs","scope":"当前macOS Electron桥和生产启动组件，真实Rust保存后停服；无完整desktop引导/Tauri/其他平台/正式门禁宣称。"}, ensure_ascii=False, indent=2) + "\n")
(HERE / "splash-avatar-leaf-ids.json").write_text(json.dumps([leaf_id], indent=2) + "\n")
path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
ids = json.loads(path.read_text())
if leaf_id not in ids:
    ids.append(leaf_id)
path.write_text(json.dumps(ids, indent=2) + "\n")
print("启动头像逐原断言已登记；阶段仍未验收。")
