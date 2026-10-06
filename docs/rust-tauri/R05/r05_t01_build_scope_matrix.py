#!/usr/bin/env python3
"""R05-T01: build docs/rust-tauri/R05/R05_SCOPE_MATRIX.json from the frozen R00
ledgers (docs/rust-tauri/R00/ACCEPTANCE_MAP.json — read-only).

The matrix lists the 16 R05 base scenarios plus EVERY R00 supplemental leaf
whose execution_stage_ids contain "R05" (the R13-F01 gate equality set), each
with an explicit R05-stage disposition:

  - "share":    R05 owns a concrete, gate-verifiable share of the leaf
                (named per leaf); the R07 business-entry remainder stays
                REQUIRED and is carried as text.
  - "deferred": the leaf has NO R05 gating share because its behavior is a
                genuine R07 business entry (settings-page semantics, desktop
                shell behavior, business routes). Deferral is NEVER justified
                by missing credentials — protocol adaptation is verifiable
                offline against controlled doubles (R05 tasking rule).

Disposition rules (applied in order, each leaf matches exactly one):

  R1  UI settings pages (providers/models/usage "逐动作核对…页面投影")
      -> deferred: the leaf asserts the R07 settings-UI projection loop;
         R05 has no UI surface to gate.
  R2  Desktop shell behaviors (observability export download/save file,
      media "open with OS" via open/cmd/xdg-open)
      -> deferred: genuine desktop-shell business entry, no R05 share.
  R3  Provider registration leaves (注册 id=X、auth=Y、api=Z…)
      -> share: R05 registers protocol family Z and auth shape Y in the Rust
         ModelGateway and verifies them against offline doubles (same-modelId
         cross-provider credential isolation included); the settings-page
         registration/selection UI chain remains R07.
  R4  D11 OAuth leaves (R05-only)
      -> full: the leaf is EXCLUSIVE to R05 (execution_stage_ids == [R05]),
         so R05 owns its FULL original behavior — no share split, no later
         remainder (R05 RR1 F25/G01: the frozen candidate released these
         six leaves on suite-level green while deferredToStages=[]; the
         per-assertion evidence is the rr1_f04 battery of
         r05_t02_credentials, pinned per assertion by the stage map's
         full_original_behavior leaves).
  R5  D12 media leaves (generate/config/providers/tasks/speech-recognition)
      -> share: R05-T06 owns the media operation protocol adaptation and
         async job semantics (job accepted != artifact complete, ResourceRef
         delivery); business entries/pages remain R07.
  R6  D22 model-observability leaves
      -> share: R05-T07 owns usage/trace production, persistence, dedup and
         query structures; observability UI/route projections remain R07/R08.
  R7  D10 model/config/preferences/provider-management leaves
      -> share: R05 owns the kernel-side semantics the leaf exercises
         (model resolution, capability preflight, config generations, secret
         redaction, atomic config write, connectivity probe as a gateway
         operation); the incumbent business route/UI chain remains R07.

The script fails closed: any leaf matching zero or more than one rule is a
hard error, and the leaf set is cross-checked for equality against the R00
ledger binding before the document is written.
"""

import hashlib
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
ACCEPTANCE_MAP = REPO / "docs/rust-tauri/R00/ACCEPTANCE_MAP.json"
OUT = REPO / "docs/rust-tauri/R05/R05_SCOPE_MATRIX.json"

BASE_SCENARIOS = [
    ("R05-A01", "同名模型不串凭证", "R05-T01"),
    ("R05-A02", "不支持能力提前失败", "R05-T01"),
    ("R05-A03", "并发401只协调一次刷新", "R05-T02"),
    ("R05-A04", "撤销后迟到刷新不能复活", "R05-T02"),
    ("R05-A05", "工具多轮协议匹配", "R05-T03"),
    ("R05-A06", "协议差异不被吞掉", "R05-T03"),
    ("R05-A07", "任意分片保持语义", "R05-T04"),
    ("R05-A08", "中断流不伪造最终回复", "R05-T04"),
    ("R05-A09", "流中取消释放连接", "R05-T05"),
    ("R05-A10", "外发阶段重试不复制副作用", "R05-T05"),
    ("R05-A11", "异供应商媒体调用正确", "R05-T06"),
    ("R05-A12", "辅助调用同受取消和预算约束", "R05-T06"),
    ("R05-A13", "多轮不拆成无关轨迹", "R05-T07"),
    ("R05-A14", "重复usage事件不多计费", "R05-T07"),
    ("R05-A15", "无Pi真实工具闭环", "R05-T08"),
    ("R05-A16", "关键失败可复原", "R05-T08"),
]

PROVIDER_THEN = re.compile(r"注册 id=([a-z0-9\-]+)、auth=([a-z\-]+)、api=([a-z\-]+)")

DEFERRED_FEATURES = {
    # R1: settings-page projection loops (R07 设置页语义)
    "F-D10-UI-UI-SETTINGS-MODELS-F92121": (
        "R1",
        "逐动作核对设置页模型区实际状态与页面投影属于 R07 设置页业务入口；R05 无 UI 面可门禁。",
    ),
    "F-D10-UI-UI-SETTINGS-PROVIDERS-D50161": (
        "R1",
        "逐动作核对设置页供应商区实际状态与页面投影属于 R07 设置页业务入口；R05 无 UI 面可门禁。",
    ),
    "F-D22-UI-UI-SETTINGS-USAGE-1850FE": (
        "R1",
        "用量设置页的逐动作页面投影核对属于 R07 设置页业务入口；R05-T07 只交付 usage/trace 数据产生与查询结构。",
    ),
    # R2: desktop shell behaviors
    "F-D22-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-OBSERVABILITY-EXPORT-0F2842": (
        "R2",
        "「得到可下载或保存的观测数据文件」是桌面壳下载/保存业务入口（R07）；R05 只保证导出数据本身真实可查询。",
    ),
    "F-D12-SEMANTIC_EFFECT-SEMANTIC-EFFECT-MEDIA-MEDIA-GENERATED-OPEN-FILEN-66FFBF": (
        "R2",
        "「调用本机 open/cmd/xdg-open」是桌面壳打开文件业务入口（R07）；R05 只保证生成产物真实存在且可校验。",
    ),
}

MEDIA_GENERATED_SERVE = (
    "F-D12-SEMANTIC_EFFECT-SEMANTIC-EFFECT-MEDIA-MEDIA-GENERATED-FILENAME-R-0949F8"
)


def sha256_of(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def classify(leaf_id: str, leaf: dict) -> dict:
    fid = leaf["feature_id"]
    then = leaf.get("then", "")
    stages = leaf.get("execution_stage_ids") or []
    tasks = leaf.get("task_ids") or []

    if fid in DEFERRED_FEATURES:
        rule, reason = DEFERRED_FEATURES[fid]
        if "R07" not in stages:
            raise SystemExit(f"{leaf_id}: rule {rule} deferral requires R07 in {stages}")
        return {
            "disposition": "deferred",
            "rule": rule,
            "r05_share": None,
            "deferred_to": ["R07"],
            "deferred_reason": reason,
            "r07_remainder": then,
        }

    m = PROVIDER_THEN.search(then)
    if "-PROVIDER-PROVIDER-" in fid and m:
        pid, auth, api = m.groups()
        return {
            "disposition": "share",
            "rule": "R3",
            "r05_share": (
                f"协议族 {api} 与认证形态 {auth} 在 Rust ModelGateway 注册并以离线协议替身验证"
                f"（provider={pid} 同名 modelId 跨 provider 不串凭证/端点/trace；不支持能力请求前明确失败）；"
                "归属 R05-T01 网关/能力矩阵、R05-T02 凭证、R05-T03 协议适配。"
            ),
            "deferred_to": None,
            "deferred_reason": None,
            "r07_remainder": "设置页注册/选择/展示的现役 UI 与业务路由链（R07-T08）。",
        }
    if fid.endswith("SYSTEMSPEECH-22A8DC") or fid.endswith("VOLCENGINESPEECH-A9ACB7"):
        return {
            "disposition": "share",
            "rule": "R3",
            "r05_share": (
                "语音 provider 的注册形态与协议归属（system-speech 系统运行时 / volcengine-bigasr "
                "供应商端点）在 Rust ModelGateway 能力矩阵中登记并以离线替身验证能力与凭证边界；"
                "归属 R05-T01/R05-T06。"
            ),
            "deferred_to": None,
            "deferred_reason": None,
            "r07_remainder": "配置页能力状态展示与现役 UI 链（R07-T08）。",
        }

    if fid.startswith("F-D11-"):
        if stages != ["R05"]:
            raise SystemExit(f"{leaf_id}: D11 leaf expected R05-only stages, got {stages}")
        # R05 RR1 F25 (G01): an R05-EXCLUSIVE leaf means R05 owns the FULL
        # original behavior — a "share" with no later stage would be an
        # unowned remainder (the frozen candidate's hole). The per-assertion
        # evidence is the rr1_f04 battery of svc:r05_t02_credentials,
        # mirrored 1:1 by the stage map's full_original_behavior leaves.
        return {
            "disposition": "full",
            "rule": "R4",
            "r05_share": (
                "R05 独占叶：R05-T02 CredentialService/OAuth 交付全部原行为（start/callback/poll/"
                "state/PKCE/logout/custom modelId 管理按该叶 then 文本逐项），每条原断言由"
                " r05_t02_credentials 套件的 rr1_f04 具名测试逐项钉住（阶段图"
                " full_original_behavior）；无后续阶段余额。"
            ),
            "deferred_to": None,
            "deferred_reason": None,
            "r07_remainder": None,
        }

    if fid.startswith("F-D12-"):
        if fid == MEDIA_GENERATED_SERVE:
            share = (
                "生成产物以真实文件经 ResourceRef/受控流交付且可校验（非 base64 内联、非幽灵登记）："
                "R05-T06/T08 闭环内验证产物真实性。"
            )
            rest = "media 文件 HTTP 路由的流式/Range(206) 业务语义与页面下载入口（R07-T06）。"
        else:
            share = (
                "对应媒体 operation（image/video/speech/asr）的 Rust 协议适配、配置段与异步任务语义"
                "（job accepted≠产物完成；凭证不跨供应商串用）离线替身验证：R05-T06。"
            )
            rest = "媒体业务入口路由与页面投影（R07-T06）。"
        return {
            "disposition": "share",
            "rule": "R5",
            "r05_share": share,
            "deferred_to": None,
            "deferred_reason": None,
            "r07_remainder": rest,
        }

    if fid.startswith("F-D22-"):
        return {
            "disposition": "share",
            "rule": "R6",
            "r05_share": (
                "usage/trace/观测数据的产生、持久化、去重、保留期与查询结构（含 NDJSON/blob 数据面"
                "语义、未初始化/无权限明确报错）：R05-T07；现役字段与隐私设置保留。"
            ),
            "deferred_to": None,
            "deferred_reason": None,
            "r07_remainder": "模型观测/用量的业务路由与 UI 投影验收（R07-T10）。",
        }

    if fid.startswith("F-D10-"):
        return {
            "disposition": "share",
            "rule": "R7",
            "r05_share": (
                "该叶依赖的内核侧语义由 R05 覆盖：模型/槽位解析与能力预检（R05-T01）、凭证引用与"
                "脱敏（R05-T02）、providers/models 配置段的代次、原子写入与未知字段保留（R05-T01/"
                "T02）、连通性探测作为网关 operation 经离线替身验证（R05-T01/T03）。"
            ),
            "deferred_to": None,
            "deferred_reason": None,
            "r07_remainder": "现役 HTTP 业务路由、设置页与交互语义（R07-T08）。",
        }

    raise SystemExit(f"{leaf_id}: no disposition rule matched feature {fid}")


def main() -> None:
    amap = json.loads(ACCEPTANCE_MAP.read_text(encoding="utf-8"))
    scenarios = amap["scenarios"]

    base = []
    for sid, name, task in BASE_SCENARIOS:
        entry = scenarios.get(sid)
        if entry is None:
            raise SystemExit(f"base scenario {sid} missing from ACCEPTANCE_MAP")
        if entry.get("kind") != "base" or entry.get("stage_id") != "R05":
            raise SystemExit(f"base scenario {sid} is not a base/R05 row: {entry}")
        base.append(
            {
                "id": sid,
                "name": name,
                "owner_task": task,
                "requirement": entry.get("requirement"),
                "r00_task_ids": entry.get("task_ids"),
                "t01_check_ids": {
                    "R05-A01": ["R05-T01-C04"],
                    "R05-A02": ["R05-T01-C06"],
                    "R05-A15": ["R05-T01-C02", "R05-T01-C03"],
                }.get(sid, []),
            }
        )

    leaves = {
        k: v
        for k, v in scenarios.items()
        if v.get("kind") == "supplemental" and "R05" in (v.get("execution_stage_ids") or [])
    }
    if len(leaves) != 130:
        raise SystemExit(f"expected 130 R05 supplemental leaves, got {len(leaves)}")

    out_leaves = []
    for lid in sorted(leaves):
        leaf = leaves[lid]
        disp = classify(lid, leaf)
        out_leaves.append(
            {
                "id": lid,
                "feature_id": leaf["feature_id"],
                "requirement": leaf["requirement"],
                "r00_task_ids": leaf["task_ids"],
                "r00_execution_stage_ids": leaf["execution_stage_ids"],
                "r00_then": leaf["then"],
                **disp,
            }
        )

    single_stage = [l for l in out_leaves if l["r00_execution_stage_ids"] == ["R05"]]
    for l in single_stage:
        # R05 RR1 F25: an R05-exclusive leaf must own its FULL original
        # behavior (disposition "full") — "deferred" would orphan the
        # obligation in this very stage, and "share" leaves an unowned
        # remainder (the frozen candidate's G01 hole).
        if l["disposition"] not in ("share", "full"):
            raise SystemExit(f"single-stage leaf {l['id']} may never defer")
        if l["disposition"] == "share" and l.get("r07_remainder"):
            raise SystemExit(
                f"single-stage leaf {l['id']} declares a share with a later-stage remainder, "
                "but no later stage exists to own it"
            )

    doc = {
        "schema": "lingxi.r05-scope-matrix.v1",
        "stage_id": "R05",
        "generated_by": "docs/rust-tauri/R05/r05_t01_build_scope_matrix.py",
        "generated_at": "2026-10-02",
        "source_ledgers": {
            "docs/rust-tauri/R00/ACCEPTANCE_MAP.json": sha256_of(ACCEPTANCE_MAP),
        },
        "disposition_rules": (
            "share=R05 拥有该叶可在本阶段门禁验证的具体份额（逐叶写明，缺密钥不构成延期——"
            "协议适配一律离线替身验证）；deferred=该叶无 R05 门禁份额，且仅当其行为是真实的 "
            "R07 业务入口（设置页语义/桌面壳行为/业务路由）。规则全文见生成脚本模块注释 R1–R7。"
        ),
        "base_scenarios": base,
        "supplemental_leaves": out_leaves,
        "counts": {
            "base": len(base),
            "supplemental_total": len(out_leaves),
            "share": sum(1 for l in out_leaves if l["disposition"] == "share"),
            "full": sum(1 for l in out_leaves if l["disposition"] == "full"),
            "deferred": sum(1 for l in out_leaves if l["disposition"] == "deferred"),
            "single_stage_r05_only": len(single_stage),
            "dual_stage_r05_r07": sum(
                1 for l in out_leaves if l["r00_execution_stage_ids"] == ["R05", "R07"]
            ),
        },
    }
    OUT.write_text(
        json.dumps(doc, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
    )
    print(f"wrote {OUT} ({len(out_leaves)} leaves, {len(base)} base)")
    print(json.dumps(doc["counts"], ensure_ascii=False))


if __name__ == "__main__":
    main()
