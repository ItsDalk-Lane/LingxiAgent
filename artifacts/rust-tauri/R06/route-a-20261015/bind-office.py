"""以真实Rust Office转换、当前生产预览及拒绝边界登记两原叶。"""
import importlib.util,json
from pathlib import Path
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3]
spec=importlib.util.spec_from_file_location("anchors",HERE.parent/"continuation-20261013"/"review-proof-anchors.py");a=importlib.util.module_from_spec(spec);spec.loader.exec_module(a)
inventory=json.loads(a.MAP.read_text());rows={r["id"]:r for r in inventory["items"]}
ids=["R00-T02-LA-2F2B6E173C1B","R00-T02-LA-3617DF5B23D5"];suite="r06_office_completion"
ui="current_native_office_preview_and_visible_real_rust_errors";errors="office_http_refusals_are_visible_and_never_publish_or_modify_files";bounds="office_xml_external_entities_and_expansion_limits_are_explicit_refusals"
checks={ids[0]:[["current_docx_html_route_converts_actual_archive_and_preserves_file",ui,bounds],[errors,ui,bounds]],ids[1]:[["current_xlsx_html_route_uses_first_relationship_and_real_cells","real_excel_dates_keep_time_in_both_calendars_and_never_show_serials",ui,bounds],[errors,ui,bounds]]}
for leaf in ids:
 row=rows[leaf];before=(list(row["assertions"]),row["then"]);assert len(row["assertions"])==2
 for index,names in enumerate(checks[leaf]):
  group=next((g for g in row["groups"] if g["index"]==index),None)
  if group is None:group={"index":index,"tests":[],"proofLocations":[]};row["groups"].append(group)
  for name in names:
   ref={"package":"lingxi-service","suite":suite,"test":name}
   if ref not in group["tests"]:group["tests"].append(ref);group["proofLocations"].append(a.test_location(suite,name))
  meaning="真实授权文件ZIP/XML转换，实际当前Rust服务HTTP返回HTML；生产Electron main/preload→当前openFilePreview/PreviewPanel显示真实正文/表格。Word标题/表格/实体转义；Excel按首表关系而非文件名，保留共享富文本/稀疏列/公式缓存/错误与日期。日期按1900/1904与styles转明确ISO8601 UTC，独立ExcelJS原日期反例保留后修复，不把序号当日期；不声称排版字体或公式重算。" if index==0 else "真实缺凭证401、根外/链接403、缺失404、目录400、超限413、损坏/实体500；实际生产错误总线与Toast显示真实500/403且不打开假预览；所有原文件SHA不变，无越权内容或写出归档成员。"
  group.update(coverage="FULL",missingClauses=[],meaning=meaning+" 当前macOS实际执行；Windows/Linux/Tauri完整预览和全阶段仍不宣称通过。")
 row["groups"].sort(key=lambda g:g["index"]);assert (row["assertions"],row["then"])==before;row.update(status="READY",gaps=[],reviewSource=(HERE/"office-proof-review.json").relative_to(ROOT).as_posix())
a.MAP.write_text(json.dumps(inventory,ensure_ascii=False,indent=2)+"\n");(HERE/"office-leaf-ids.json").write_text(json.dumps(ids,indent=2)+"\n")
p=HERE.parent/"continuation-20261013"/"reviewed-original-ids.json";reviewed=json.loads(p.read_text())
for leaf in ids:
 if leaf not in reviewed:reviewed.append(leaf)
p.write_text(json.dumps(reviewed,indent=2)+"\n");print("Office两叶原断言登记，须实际本候选窗口再计入完成")
