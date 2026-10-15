"""实际当前服务和Tauri窗口共同证明资源三原叶，不改变原断言。"""
import importlib.util,json
from pathlib import Path
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3]
spec=importlib.util.spec_from_file_location("anchors",HERE.parent/"continuation-20261013"/"review-proof-anchors.py");a=importlib.util.module_from_spec(spec);spec.loader.exec_module(a)
inventory=json.loads(a.MAP.read_text());rows={r["id"]:r for r in inventory["items"]}
ids=["R00-T02-LA-238267978DDA","R00-T02-LA-4173A08A51CC","R00-T02-LA-AF18F5CEA14F"];suite="r06_native_resource_completion"
checks={leaf:[["current_native_resource_three_leaves_real_windows_and_os_trash"] for _ in rows[leaf]["assertions"]] for leaf in ids}
for leaf in ids:
 row=rows[leaf];before=(list(row["assertions"]),row["then"]);assert len(row["assertions"])==2
 for index,names in enumerate(checks[leaf]):
  group=next((g for g in row["groups"] if g["index"]==index),None)
  if group is None:group={"index":index,"tests":[],"proofLocations":[]};row["groups"].append(group)
  for name in names:
   ref={"package":"lingxi-service","suite":suite,"test":name}
   if ref not in group["tests"]:group["tests"].append(ref);group["proofLocations"].append(a.test_location(suite,name))
  meanings={
   ids[0]:"真实Tauri兼容watch_file只接受绝对路径并发送file-changed；当前renderer不调用旧监听启停且没有旧按钮，当前主/独立viewer真实订阅当前Rust ResourceIO resource.changed并重读。",
   ids[1]:"当前原生WebView实际显示纯文本、Markdown、真实Word/首张Excel表格；sandbox iframe、二进制/大文件信息卡、损坏文件错误；真实独立只读窗口重读磁盘并拒绝读别的文件/写入，主窗未保存编辑保留。实际PNG保留供视觉复核。",
   ids[2]:"当前Tauri生产trash_item调用真实系统废纸篓；macOS NsFileManager避免Finder自动化权限等待，无永久删除回退。界面只在真实成功后移除列表；相对/缺失路径false。本轮唯一合成文件通过真实Finder定位系统Trash并复制载荷，原字节逐字核对；不以源消失代替OS证明。"
  }
  group.update(coverage="FULL",missingClauses=[],meaning=meanings[leaf]+" 当前macOS实跑；独立复核仅静态，不冒称Windows/Linux运行或R06全阶段验收。")
 row["groups"].sort(key=lambda g:g["index"]);assert (row["assertions"],row["then"])==before;row.update(status="READY",gaps=[],reviewSource=(HERE/"native-resource-proof-review.json").relative_to(ROOT).as_posix())
a.MAP.write_text(json.dumps(inventory,ensure_ascii=False,indent=2)+"\n");(HERE/"native-resource-leaf-ids.json").write_text(json.dumps(ids,indent=2)+"\n")
p=HERE.parent/"continuation-20261013"/"reviewed-original-ids.json";reviewed=json.loads(p.read_text())
for leaf in ids:
 if leaf not in reviewed:reviewed.append(leaf)
p.write_text(json.dumps(reviewed,indent=2)+"\n");print("资源三叶原断言登记，须实际本候选窗口再计入完成")
