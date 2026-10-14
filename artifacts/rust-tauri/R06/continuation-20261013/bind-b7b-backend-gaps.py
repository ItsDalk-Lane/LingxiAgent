"""Bind R06 closeout B7b backend-gap leaves (memory/skills/compaction/preferences
cluster) to the 2026-10-13 B7b suites.

6 leaves flip GAP->READY (every original assertion covered FULL):
B097BDDC1E10 (PUT /agents/{id}/skills broadcast + empty-array disable),
B0D4073E6AEB (bundle delete broadcast), BDEDEF118670 (external paths broadcast),
EFEEEFFCD60A (bundle create broadcast + per-arm boundaries),
23172C8397F2 (workspace skill policy immediate re-filter + mixed-patch readback),
24BC2E961E3E (splitByScope preferences: cross-agent readback, dual-leaf requests,
mixed-patch partial success).
1 leaf (0B483D7F6A43) stays GAP: group 1 (mixed-patch readback) flips FULL;
group 0 keeps its missing clause — the automated ticker host (R07) is not
ported, so pause/resume of a live ticker stays unproven (disabledSince /
reenableAt / memory-master-changed / health-face evidence is appended).
Slash/WS compaction, dream ticker, watch-file, summary-store and experiments
leaves are intentionally untouched (semantic hosts absent — see report).
Registration is not stage acceptance.
"""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())

S = "lingxi-service"
SKILLS_EV = "r06_b7b_skills_events"
BROADCAST = "skills_faces_broadcast_and_bundle_boundaries"
PREFS = "r06_b7b_config_scope_prefs"
SPLIT = "global_agent_leaf_split_cross_agent_readback_and_immediate_refilter"
MIXED = "mixed_patch_partial_success_and_memory_toggle_timestamps"
T06_SKILLS = ("r06_t06_skills", "skills_http_faces_whitelist_shadow_readonly_and_bundles")
T06_CONTEXT = ("r06_t06_skill_context", "enabled_skill_catalog_reaches_the_production_system_prompt")
BUNDLES = (
    "r06_original_skills_bundle_proofs",
    "original_bundles_agent_projection_update_order_delete_and_rejections",
)
EXTERNAL = (
    "r06_original_skills_bundle_proofs",
    "original_external_paths_persist_readback_and_rejections_without_mutation",
)


def tests(*pairs):
    return [{"package": S, "suite": suite, "test": test} for suite, test in pairs]


def locations(*pairs):
    return [anchors.test_location(suite, test) for suite, test in pairs]


def full_group(index, test_pairs, meaning):
    return {
        "index": index,
        "tests": tests(*test_pairs),
        "proofLocations": locations(*test_pairs),
        "coverage": "FULL",
        "missingClauses": [],
        "meaning": meaning,
    }


# id -> {"groups": [rebuilt groups in order], "ready": bool}
# For kept-existing groups we copy the original dict verbatim.
BATCH = {
    "R00-T02-LA-B097BDDC1E10": {
        "groups": [
            (0, [(T06_SKILLS), (T06_CONTEXT), (SKILLS_EV, BROADCAST)],
                "真实名单PUT过滤ghost、配置保留无关字段，模型侧目录达生产系统提示；B7b 补上写盘后 skills-changed(agentId=该助手) 经真实 app-events 面广播。"),
            (1, [(T06_SKILLS), (SKILLS_EV, BROADCAST)],
                "可见集交集过滤已有证明；B7b 补上空数组经同一PUT接口停用所有可见普通技能（运行时可用列表投影全部 enabled=false、config 名单清空）并广播，恢复子集仍走同一接口。"),
        ],
        "ready": True,
    },
    "R00-T02-LA-B0D4073E6AEB": {
        "groups": [
            (0, [BUNDLES, (SKILLS_EV, BROADCAST)],
                "真实DELETE删除组合定义且成员技能文件保留；B7b 补上删除成功后 skills-changed(agentId=null) 广播（经 app-events 面），未知 id 404 不广播。"),
            None,  # G1 保留原绑定（未知组合404、存储保持）
        ],
        "ready": True,
    },
    "R00-T02-LA-BDEDEF118670": {
        "groups": [
            (0, [EXTERNAL, (SKILLS_EV, BROADCAST)],
                "实际prefs清单与GET匹配、目录包含readonly外部技能；B7b 补上保存与空数组移除均广播 skills-changed(agentId=null)。"),
            None,  # G1 保留原绑定（非绝对/skillsDir/非数组400，空数组移除）
        ],
        "ready": True,
    },
    "R00-T02-LA-EFEEEFFCD60A": {
        "groups": [
            (0, [BUNDLES, (SKILLS_EV, BROADCAST)],
                "真实创建组合持久、GET按A/B投影、name/skillNames可重读；B7b 补上创建成功广播 skills-changed(agentId=null) 及缺省 name/skillNames 按原实现归一（Skill Bundle/[]）。"),
            (1, [(T06_SKILLS), (SKILLS_EV, BROADCAST)],
                "未知成员400已有；B7b 补上缺query.agentId 400 与未知成员400逐臂检查：组合存储字节不变、零广播；缺省 body 字段按原 TS 归一而非猜造拒绝。"),
        ],
        "ready": True,
    },
    "R00-T02-LA-23172C8397F2": {
        "groups": [
            (0, [(PREFS, SPLIT)],
                "经真实PUT /agents/{id}/config 写工作区技能策略：discover_project_skills=false 后同一现役服务的运行时技能列表立即重筛（standard 工作区技能停用）、skills-changed(agentId) 广播、config-flags 读回一致；重新启用与 compatible 开启同样立即生效。"),
            (1, [(PREFS, MIXED)],
                "mixed patch 后段（内联凭证缺 provider）400 报错时：前段全局 setter 已写入 preferences.json 被读回（部分成功），助手 config.yaml 不变，不宣称原子回滚。"),
        ],
        "ready": True,
    },
    "R00-T02-LA-24BC2E961E3E": {
        "groups": [
            (0, [(PREFS, SPLIT)],
                "纯全局 patch（keep_awake）只经 setter 写 preferences.json，agentPartial 为空直接返回且不写助手 config.yaml；跨助手经 GET config-flags 最小读面读到同一全局偏好（A/B 读回相等），keep-awake-changed 广播。"),
            (1, [(PREFS, SPLIT)],
                "editor 排版（全局叶）与 chat 布局（助手叶）两次真实请求各归其叶：前者只写 preferences.json 并广播 editor-typography-changed，后者只写助手 config.yaml 且偏好字节不变、B 助手配置不串用；不把两次请求说成单次 mixed patch。"),
            (2, [(PREFS, MIXED)],
                "真 mixed patch（keep_awake+chat）同请求覆盖全局与助手两叶；后段失败（内联凭证缺 provider 400）时全局 setter 可能已经写入，读回两处记录部分成功，不宣称原子回滚。"),
        ],
        "ready": True,
    },
}

review = []
report = HERE / "b7b-backend-gaps-proof-review.json"
ready_ids = []
partial_ids = []

for identifier, spec_row in BATCH.items():
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP", identifier
    original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}
    rebuilt = []
    for entry in spec_row["groups"]:
        if entry is None:
            # 保留原组绑定（原组已 FULL）。
            kept = next(g for g in original["groups"] if g["index"] == len(rebuilt))
            rebuilt.append(kept)
            continue
        index, pairs, meaning = entry
        rebuilt.append(full_group(index, pairs, meaning))
    assert [g["index"] for g in rebuilt] == list(range(len(row["assertions"]))), identifier
    row["groups"] = rebuilt
    assert all(group["coverage"] == "FULL" for group in rebuilt), identifier
    row.update(
        status="READY",
        gaps=[],
        reviewSource=report.relative_to(anchors.ROOT).as_posix(),
    )
    ready_ids.append(identifier)
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"], identifier
    review.append({"id": identifier, "original": original, "became": row["status"]})

# ── 0B483D7F6A43：G1 翻 FULL；G0 保持 PARTIAL（自动 ticker 宿主未移植）。──
identifier = "R00-T02-LA-0B483D7F6A43"
row = next(row for row in inventory["items"] if row["id"] == identifier)
assert row["status"] == "GAP", identifier
original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}
g0 = next(g for g in row["groups"] if g["index"] == 0)
g0["tests"].append({"package": S, "suite": PREFS, "test": MIXED})
g0["proofLocations"].append(anchors.test_location(PREFS, MIXED))
g0["meaning"] = (
    "布尔flag往返已有适配器证据；B7b 经真实PUT补上 enabled=false 写 disabledSince、"
    "重新启用写 reenableAt、memory-master-changed 广播与 health 面 ticker 停用/缺席的诚实报告；"
    "自动 ticker 宿主（R07）未移植，暂停/恢复运行中的 ticker 仍不可证，保持 PARTIAL。"
)
g1 = full_group(
    1,
    [(PREFS, MIXED)],
    "mixed patch 后段（内联凭证缺 provider）400 报错时：前段全局 setter 已写入 "
    "preferences.json 被读回（部分成功，不宣称自动回滚），助手 config.yaml 保持不变。",
)
row["groups"] = [g0, g1]
partial_ids.append(identifier)
assert row["then"] == original["then"] and row["assertions"] == original["assertions"], identifier
review.append({"id": identifier, "original": original, "became": row["status"]})

anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "GAP_BLOCKS_B7B_BACKEND_BOUND_NOT_STAGE_ACCEPTANCE",
    "ready": ready_ids,
    "partial": partial_ids,
    "review": review,
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(ready_ids).intersection(selected)
selected.extend(ready_ids)
ids.write_text(json.dumps(selected, indent=2) + "\n")
(HERE / "registered-ready-b7b-backend-ids.json").write_text(
    json.dumps(ready_ids, indent=2) + "\n")
ready_total = sum(1 for row in inventory["items"] if row["status"] == "READY")
print(f"{len(ready_ids)} READY registered this batch ({ready_total} total); "
      f"{len(partial_ids)} partial updated; full R06/R07 not accepted.")
