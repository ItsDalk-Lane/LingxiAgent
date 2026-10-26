#!/usr/bin/env python3
"""生成 R08 阶段图与输入镜像；只镜像 R00 原账字段，缺逐断言行为绑定的叶保持
required_behavior_pending，不冒充完成（R07 生成器同构）。

用法：
  python3 scripts/rust-tauri/r08_stage_inputs.py            # 生成 stage map + inputs
  python3 scripts/rust-tauri/r08_stage_inputs.py --check --evidence <dir>  # 输入自检
  python3 scripts/rust-tauri/r08_stage_inputs.py --run-suite <suite> --evidence <dir>  # 生产者模式
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ACCEPTANCE_PATH = Path("docs/rust-tauri/R00/ACCEPTANCE_MAP.json")
FEATURE_PATH = Path("docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json")
MAP_PATH = Path("rust/crates/xtask/src/stage_maps/R08.json")
INPUT_PATH = Path("rust/crates/xtask/src/stage_maps/R08_INPUTS.json")

# 任务 → 现役真实验证面（每个键都必须在 SUITE_REGISTRY 内有真实 runner）。
TASK_SUITES = {
    "R08-T01": ["r08_t01_a01_a02_ws", "r08_t01_env_deps"],
    "R08-T02": ["r08_desktop_boundary_vitest"],
    "R08-T03": ["r08_t03_a05_drill", "r08_t03_a06_drill"],
    "R08-T04": ["r08_t04_sessionfiles_v2", "r08_registry_amend_proof", "r08_registry_consistency"],
    "R08-T05": ["r08_t05_seed_guard", "r08_t06_d1", "r08_t06_d2", "r08_t06_d3"],
    "R08-T06": ["r08_t06_d1", "r08_t06_d2", "r08_t06_d3"],
    "R08-T07": ["r08_t07_ab", "r08_t07_real_format"],
    "R08-T08": ["r08_a15_isolation", "r08_a16_versions"],
}

# A/B 线束已实测平价的份额图钉（case 名 → r08_t07_ab 生产者 cases.json 的键）。
SHARE_PINS = {
    "R00-T02-LA-38EEA99077E8": {
        "share": "正路径：同一数据两内核经 UI 绑定层返回同数会话列表（计数与置顶面平价）",
        "cases": [("ab::sessions.list.count", 1), ("ab::sessions.list.parity", 1)],
    },
    "R00-T02-LA-FDB0FE6528EE": {
        "share": "正路径：改名成功且重载持久化（双内核）",
        "cases": [("ab::sessions.rename.ok", 1), ("ab::sessions.rename.persisted", 1)],
    },
    "R00-T02-LA-45C5CE361CCB": {
        "share": "正路径：完成调用成功（双内核）；广播与重复写幂等未测",
        "cases": [("ab::sessions.todos.complete", 1)],
    },
    "R00-T02-LA-5CD35D9EED20": {
        "share": "正路径：取消调用成功（双内核）；不终止其他任务未测",
        "cases": [("ab::sessions.todos.cancel", 1)],
    },
    "R00-T02-LA-17FCFE45576B": {
        "share": "正路径：收纳调用成功（双内核）；仅已结束且未收纳未测",
        "cases": [("ab::sessions.todos.dismiss", 1)],
    },
    "R00-T02-LA-B0F05BC32D62": {
        "share": "正路径：返回删除数量形状（双内核）；实际按期限删除未测",
        "cases": [("ab::sessions.maintenance.cleanup", 1)],
    },
    "R00-T02-LA-21A99DB98735": {
        "share": "正路径：返回数量形状（双内核）；实际归档与跳过运行中未测",
        "cases": [("ab::sessions.maintenance.sweep", 1)],
    },
    "R00-T02-LA-25C4FF66FEE5": {
        "share": "正路径：返回同一 permissionMode（双内核 auto）；未设置时 ASK 未测",
        "cases": [("ab::permissions.default.parity", 1)],
    },
    "R00-T02-LA-B0F05BC32D62": {
        "share": "形状+期限边界（新归档保留双内核）；A 腿 cleanup(30) 对 fixture 超期归档计数 0 vs B 1——旧栈删除计数语义未对齐（t12-ab-drill-d3-07），如实留债",
        "cases": [("ab::sessions.maintenance.cleanup", 1)],
    },
    "R00-T02-LA-FF4F06011B48": {
        "share": "记忆开关读/写与负路径双内核全绿；提示词快照 A 腿 500——合成会话无冻结快照 sidecar（需真实模型回合产生，t19-ab-drill-memsnap-01 实证；B 腿 200）——产品侧合成前提，如实留债",
        "cases": [],
    },
    "R00-T02-LA-BC2FBD618278": {
        "share": "switch 投影正路径 A 腿被旧栈无模型 500 阻断（D-3 同根）；B 腿 200 但 permissionMode 为 DB 原值 null（新发现 D-7：旧栈归一化默认 mode vs Rust 透传 DB 值）——双内核均不可绑定，如实留债",
        "cases": [],
    },
}

# 套件注册表（runner 形态见 run_suite 的逐套件分支；argv 全字面量）。
SUITE_REGISTRY = {
    "r08_t01_a01_a02_ws", "r08_t01_env_deps", "r08_a04_no_final_ws",
    "r08_a04_ui_vitest", "r08_cli_data_vitest", "r08_c3_find_bridge_redaction",
    "r08_t04_sessionfiles_v2", "r08_t05_seed_guard",
    "r08_registry_amend_proof", "r08_t06_d1", "r08_t06_d2", "r08_t06_d3",
    "r08_t07_real_format", "r08_t07_ab",
    "r08_t03_a05_drill", "r08_t03_a06_drill",
    "r08_desktop_boundary_vitest", "r08_registry_consistency",
    "r08_a15_isolation", "r08_a16_versions", "r08_ui_pages",
}

# A13「相同界面验证内核替换」判定的已批准 recorded 登记（verdict=recorded
# 的任务须在此登记方计合格；未登记差异=0 → A13 FAIL）。登记依据：
# T07_UI_DIFFERENCE_REPORT.md 的归因 + R08_FINAL_REVIEW.md 的
# ACCEPT_HONEST_CLOSURE 复核；登记条目须附差异编号与根因。
# R08 收尾：双腿模型面齐备后 sessions.switch / sessions.create 改为真实对拍，
# 不再以 recorded 规避（D-2/D-3/D-4 的环境前提已由线束消除）。
RECORDED_APPROVALS = {
    "leaf.prompt-snapshot": {
        "diff": "FIXTURE-PROMPT-SNAPSHOT",
        "basis": "A 腿旧栈对合成会话（无真实模型回合）读提示词快照 500——冻结 sidecar 只在首个模型回合后产生（夹具前提缺口，非内核语义差；Rust 腿 200、双腿无效会话均显式拒）。叶 FF4F06011B48 保持 pending，待聊天线束（D01）产生真实回合后转为平价对拍。",
    },
    "leaf.slash.new": {
        "diff": "D-13",
        "basis": "旧栈 /new 对 desktop 会话回『[命令错误] rotate for desktop kind not supported in phase 1』（bridge-commands sessionOps.rotate 的 phase-1 限制，run-19/20 帧原文）——旧栈自身不支持该叶 A0 的 desktop 轮换；Rust 已实现（/new 轮换+归档+可恢复，B 腿三案例全真）。属旧栈缺口而非 Rust 差异，叶 485B2D0A8A02 保持 pending 待产品裁决（是否以 Rust 行为为准关闭）。",
    },
    "leaf.slash": {
        "diff": "D-14",
        "basis": "旧栈 /reset 对 desktop 会话清史不生效（run-21 实证：清前 6 行→清后 6 行、无『已重置会话』回执帧）——与 /new 同族 phase-1 限制；Rust 侧 /reset 清史+回执+他话不受扰全绿（/compact 双腿均正常）。属旧栈缺口而非 Rust 差异，叶 3FCB259FF69C 保持 pending 待产品裁决。",
    },
    "leaf.ws.terminal": {
        "diff": "D15-TRANSPORT-ONLY",
        "basis": "该任务只承担 D13 的「帧族传输面历史回归」（三帧不被当成未知帧断连 + 显式回复形状），保留 recorded 不影响叶证据：R08 收尾第六轮已实现 Rust 真实终端子系统（procsupervisor PTY 只读 transcript log + terminals.rs WS 三帧），D13 三叶现按 leaf.terminal.positive 双内核正路径案例绑定（snapshot/tail/close 各自正负 14 case）。",
    },
}
# 已达成「每条原断言均有专属案例」全覆盖的叶（full_original_behavior）：
# 断言 i → 专属案例名列表（跨断言不共用；全部经 r08_t07_ab cases.json 机器核对）。
FULL_BINDINGS = {
    "R00-T02-LA-FDB0FE6528EE": {  # 重命名会话：断言0=保存成功+空标题拒绝；断言1=明确错误
        "assertion_cases": [
            ["ab::sessions.rename.ok", "ab::sessions.rename.persisted", "leaf::rename.empty-title.rejected"],
            ["leaf::rename.invalid-session.explicit-error"],
        ],
    },
    "R00-T02-LA-38EEA99077E8": {  # 查看可见会话列表：断言0=投影字段+双内核同数；断言1=鉴权不足明确拒绝
        "assertion_cases": [
            ["ab::sessions.list.count", "ab::sessions.list.parity", "leaf::list.revision-field-present", "leaf::list.summary-or-badge-present"],
            ["leaf::list.unauthorized-rejected"],
        ],
    },
    "R00-T02-LA-DD47275AC5AE": {  # 查看 MCP 连接状态：断言0=连接器状态+延迟加载开关投影；断言1=鉴权不足明确拒绝
        "assertion_cases": [
            ["leaf::mcp.state.shape"],
            ["leaf::mcp.state.unauthorized-rejected"],
        ],
    },
    "R00-T02-LA-8D3CEB6133E1": {  # 设置新会话默认权限
        "assertion_cases": [
            ["leaf::permission.default-roundtrip"],
            ["leaf::permission.default-invalid-rejected"],
        ],
    },
    "R00-T02-LA-51905CECDD1B": {  # 查看会话记忆开关：断言0=读面返回 memoryEnabled；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::memory.read"],
            ["leaf::memory-snapshot.invalid-rejected"],
        ],
    },
    "R00-T02-LA-9616596B5720": {  # 设置会话记忆开关：断言0=布尔设置生效回读一致；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::memory.set"],
            ["leaf::memory-snapshot.invalid-rejected"],
        ],
    },
    "R00-T02-LA-98DDC94776F1": {  # 查看会话授权目录：断言0=读面返回；断言1=无效目录显式拒
        "assertion_cases": [
            ["leaf::auth-folders.read"],
            ["leaf::auth-folders.invalid-rejected"],
        ],
    },
    "R00-T02-LA-604C64F4828A": {  # 增加会话授权目录：断言0=验证真实目录后扩大范围；断言1=不存在目录显式拒
        "assertion_cases": [
            ["leaf::auth-folders.add"],
            ["leaf::auth-folders.invalid-rejected"],
        ],
    },
    "R00-T02-LA-1E0D477755CD": {  # 撤销会话授权目录：断言0=从授权范围移除指定目录；断言1=负路径显式拒
        "assertion_cases": [
            ["leaf::auth-folders.remove"],
            ["leaf::auth-folders.invalid-rejected"],
        ],
    },
    "R00-T02-LA-73B631D77531": {  # 替换会话授权目录：断言0=整体替换原授权范围；断言1=负路径显式拒
        "assertion_cases": [
            ["leaf::auth-folders.set"],
            ["leaf::auth-folders.invalid-rejected"],
        ],
    },
    "R00-T02-LA-AA9EB3FD034D": {  # 阅读会话消息页：断言0=消息页读面返回（history/content 投影）；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::read.history-page"],
            ["leaf::read.invalid-rejected"],
        ],
    },
    "R00-T02-LA-CDBFE6616EB9": {  # 查看会话历史概览：断言0=概览读面返回；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::read.overview"],
            ["leaf::read.invalid-rejected"],
        ],
    },
    "R00-T02-LA-F0319FC4F93D": {  # 查看单会话摘要：断言0=摘要读面返回；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::read.summary"],
            ["leaf::read.invalid-rejected"],
        ],
    },
    "R00-T02-LA-0D4F0EFC9DAE": {  # 设置会话置顶：断言0=置顶投影（pinned+stamp/order 落列表）；断言1=取消置顶清除投影
        "assertion_cases": [
            ["leaf::pin.state-projected"],
            ["leaf::pin.state-cleared"],
        ],
    },
    "R00-T02-LA-2D4EBE0FF3C2": {  # 归档工作台会话：断言0=移入可恢复归档；断言1=无效目标显式拒
        "assertion_cases": [
            ["leaf::archive-workspace.recoverable"],
            ["leaf::archive-workspace.invalid-rejected"],
        ],
    },
    "R00-T02-LA-51905CECDD1B": {  # 查看会话记忆开关：断言0=读面返回 memoryEnabled；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::memory.read"],
            ["leaf::memory-snapshot.invalid-rejected"],
        ],
    },
    "R00-T02-LA-9616596B5720": {  # 设置会话记忆开关：断言0=布尔设置生效回读一致；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::memory.set"],
            ["leaf::memory-snapshot.invalid-rejected"],
        ],
    },
    "R00-T02-LA-98DDC94776F1": {  # 查看会话授权目录：断言0=读面返回；断言1=无效目录显式拒
        "assertion_cases": [
            ["leaf::auth-folders.read"],
            ["leaf::auth-folders.invalid-rejected"],
        ],
    },
    "R00-T02-LA-604C64F4828A": {  # 增加会话授权目录：断言0=验证真实目录后扩大范围；断言1=不存在目录显式拒
        "assertion_cases": [
            ["leaf::auth-folders.add"],
            ["leaf::auth-folders.invalid-rejected"],
        ],
    },
    "R00-T02-LA-1E0D477755CD": {  # 撤销会话授权目录：断言0=从授权范围移除指定目录；断言1=负路径显式拒
        "assertion_cases": [
            ["leaf::auth-folders.remove"],
            ["leaf::auth-folders.invalid-rejected"],
        ],
    },
    "R00-T02-LA-73B631D77531": {  # 替换会话授权目录：断言0=整体替换原授权范围；断言1=负路径显式拒
        "assertion_cases": [
            ["leaf::auth-folders.set"],
            ["leaf::auth-folders.invalid-rejected"],
        ],
    },
    "R00-T02-LA-AA9EB3FD034D": {  # 阅读会话消息页：断言0=消息页读面返回（history/content 投影）；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::read.history-page"],
            ["leaf::read.invalid-rejected"],
        ],
    },
    "R00-T02-LA-CDBFE6616EB9": {  # 查看会话历史概览：断言0=概览读面返回；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::read.overview"],
            ["leaf::read.invalid-rejected"],
        ],
    },
    "R00-T02-LA-F0319FC4F93D": {  # 查看单会话摘要：断言0=摘要读面返回；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::read.summary"],
            ["leaf::read.invalid-rejected"],
        ],
    },
    "R00-T02-LA-0D4F0EFC9DAE": {  # 设置会话置顶：断言0=置顶语义+投影（pinned/stamp/order 落列表+清除）；断言1=无效目标显式拒
        "assertion_cases": [
            ["ab::sessions.pin.pin", "ab::sessions.pin.unpin", "leaf::pin.state-projected"],
            ["leaf::pin.invalid-session.explicit-error", "leaf::pin.state-cleared"],
        ],
    },
    "R00-T02-LA-2ED065BED168": {  # 重排置顶会话：断言0=整体重编号生效；断言1=重复 id 显式拒
        "assertion_cases": [
            ["leaf::pin-order.reorder-applies"],
            ["leaf::pin-order.duplicate-rejected"],
        ],
    },
    "R00-T02-LA-52226FEA5997": {  # 永久删除归档会话：断言0=归档文件与生命周期清除；断言1=删除后不可恢复（恢复接口显式拒）
        "assertion_cases": [
            ["leaf::archived-delete.file-and-lifecycle-cleared"],
            ["leaf::archived-delete.unrecoverable"],
        ],
    },
    "R00-T02-LA-A18187567AF9": {  # 跨会话搜索：断言0=内容命中；断言1=空查询空结果
        "assertion_cases": [
            ["leaf::search.content-hit"],
            ["leaf::search.empty-query-empty-result"],
        ],
    },
    "R00-T02-LA-43FFB75F252C": {  # 查看归档会话：断言0=归档列表含目标；断言1=无效目标显式拒绝
        "assertion_cases": [
            ["leaf::archive.positive-cycle"],
            ["leaf::archive.negative.rejected"],
        ],
    },
    "R00-T02-LA-31CDA283A11A": {  # 恢复归档会话：断言0=恢复回可见；断言1=无效恢复目标显式拒绝
        "assertion_cases": [
            ["leaf::archive.positive-cycle"],
            ["leaf::archive.negative.rejected"],
        ],
    },
    "R00-T02-LA-4F1F69CCA760": {  # 在指定会话内查找（C③/D-10 关闭）：断言0=解析定位+读历史+命中位置+Bridge 脱敏；断言1=无效会话显式拒
        "assertion_cases": [
            ["leaf::find.in-session-hit", "leaf::find.bridge-redaction"],
            ["leaf::find.in-session-invalid-rejected"],
        ],
    },
    # ── R08 收尾 D02 批次 1（cases 泛化任务 leaf.projects / leaf.conversation-map /
    #    leaf.checkpoints / leaf.workspace-rollback / leaf.sessions.create；案例键 =
    #    leaf::<task>.<case>，由 derive_ab_cases 自动派生，双腿同键全真记 1）──
    "R00-T02-LA-AFA7227BFB63": {  # 查看会话项目目录
        "assertion_cases": [["leaf::leaf.projects.catalog.read"], ["leaf::leaf.projects.negative.catalog.unauthorized"]],
    },
    "R00-T02-LA-60A58027CCCE": {  # 创建项目文件夹
        "assertion_cases": [["leaf::leaf.projects.folder.create"], ["leaf::leaf.projects.negative.folder.create-empty-name"]],
    },
    "R00-T02-LA-E167298FD87C": {  # 创建会话项目（文件夹内/根）
        "assertion_cases": [["leaf::leaf.projects.project.create"], ["leaf::leaf.projects.negative.project.create-empty-name"]],
    },
    "R00-T02-LA-D43836A3A579": {  # 修改会话项目
        "assertion_cases": [["leaf::leaf.projects.project.patch"], ["leaf::leaf.projects.negative.project.patch-missing"]],
    },
    "R00-T02-LA-059066969D44": {  # 修改项目文件夹
        "assertion_cases": [["leaf::leaf.projects.folder.patch"], ["leaf::leaf.projects.negative.folder.patch-missing"]],
    },
    "R00-T02-LA-D58B651BF099": {  # 重排会话项目
        "assertion_cases": [["leaf::leaf.projects.projects.reorder"], ["leaf::leaf.projects.negative.projects.reorder-missing-folder"]],
    },
    "R00-T02-LA-EEDFB7AB889C": {  # 重排项目文件夹
        "assertion_cases": [["leaf::leaf.projects.folders.reorder"], ["leaf::leaf.projects.negative.folders.reorder-unauthorized"]],
    },
    "R00-T02-LA-20E9C8F26CEA": {  # 设置会话项目归属（分配/null 清除）
        "assertion_cases": [["leaf::leaf.projects.assignment.set.and.clear"], ["leaf::leaf.projects.negative.assignment.unauthorized"]],
    },
    "R00-T02-LA-3ECCF7C95483": {  # 删除项目：项目消失、会话归属未分类、会话内容仍在
        "assertion_cases": [["leaf::leaf.projects.project.delete.semantics"], ["leaf::leaf.projects.negative.project.delete-missing"]],
    },
    "R00-T02-LA-EF5DD65C558E": {  # 删除文件夹：文件夹消失、项目移根、会话仍在原项目
        "assertion_cases": [["leaf::leaf.projects.folder.delete.semantics"], ["leaf::leaf.projects.negative.folder.delete-missing"]],
    },
    "R00-T02-LA-F28086B64522": {  # 会话地图：返回持久化布局或默认空布局
        "assertion_cases": [["leaf::leaf.conversation-map.layout.read.default-or-persisted"], ["leaf::leaf.conversation-map.negative.invalid-not-persisted"]],
    },
    "R00-T02-LA-F6A1A50E97E4": {  # 会话地图：读取会话消息并折叠问答卡片
        "assertion_cases": [["leaf::leaf.conversation-map.turns.read.folds-qa-cards"], ["leaf::leaf.conversation-map.turns.invalid-session-rejected"]],
    },
    "R00-T02-LA-5370DA3742CB": {  # 位置合并：仅指定卡片变化
        "assertion_cases": [["leaf::leaf.conversation-map.positions.merge.only-specified-changes"], ["leaf::leaf.conversation-map.negative.invalid-not-persisted"]],
    },
    "R00-T02-LA-5271217DB01B": {  # 位置替换：未指定坐标被删除
        "assertion_cases": [["leaf::leaf.conversation-map.positions.replace.unspecified-removed"], ["leaf::leaf.conversation-map.negative.invalid-not-persisted"]],
    },
    "R00-T02-LA-94B9BE3AA7A6": {  # 折叠列表整体更新
        "assertion_cases": [["leaf::leaf.conversation-map.collapsed.replace.whole-list"], ["leaf::leaf.conversation-map.negative.invalid-not-persisted"]],
    },
    "R00-T02-LA-3FAFA387AD68": {  # 查看检查点列表
        "assertion_cases": [["leaf::leaf.checkpoints.list.read"], ["leaf::leaf.checkpoints.list.unauthorized-rejected"]],
    },
    "R00-T02-LA-1AEDB2670EB9": {  # 移除指定检查点
        "assertion_cases": [["leaf::leaf.checkpoints.delete.removes-from-store"], ["leaf::leaf.checkpoints.delete.unauthorized-rejected"]],
    },
    "R00-T02-LA-3562B5D44C5E": {  # 恢复指定检查点：写回位置；不存在 404
        "assertion_cases": [["leaf::leaf.checkpoints.restore.writes-back-and-reports-location"], ["leaf::leaf.checkpoints.restore.missing-404"]],
    },
    "R00-T02-LA-BB0A168006D7": {  # 用户编辑创建检查点：只接受绝对路径与两种原因
        "assertion_cases": [["leaf::leaf.checkpoints.user-edit.create.listed"], ["leaf::leaf.checkpoints.user-edit.rejects-bad-reason-and-relative-path"]],
    },
    "R00-T02-LA-22150190C7B1": {  # 查看文件回退开关
        "assertion_cases": [["leaf::leaf.workspace-rollback.read.enabled-boolean", "leaf::leaf.workspace-rollback.no-file-rollback-side-effect"], ["leaf::leaf.workspace-rollback.read.unauthorized-rejected"]],
    },
    "R00-T02-LA-015A65726C05": {  # 设置文件回退开关
        "assertion_cases": [["leaf::leaf.workspace-rollback.put.roundtrip", "leaf::leaf.workspace-rollback.no-file-rollback-side-effect"], ["leaf::leaf.workspace-rollback.put.invalid-rejected-state-unchanged"]],
    },
    "R00-T02-LA-B9DA1A584C6E": {  # 创建并切换会话
        "assertion_cases": [["leaf::leaf.sessions.create.new.focus.created-with-metadata"], ["leaf::leaf.sessions.create.negative.invalid-agent-rejected"]],
    },
    "R00-T02-LA-9D9E72CB6051": {  # 创建独立会话：不切走焦点；可选谱系；深度过限拒绝
        "assertion_cases": [["leaf::leaf.sessions.create.new-detached.created", "leaf::leaf.sessions.create.new-detached.focus-unchanged", "leaf::leaf.sessions.create.new-detached.lineage-depth-limit-rejected"], ["leaf::leaf.sessions.create.negative.invalid-agent-rejected"]],
    },
    # ── D04 设置域（leaf.permission-mode）──
    "R00-T02-LA-5E61048F19B9": {  # 查看当前会话权限模式：mode/accessMode/defaultMode，不改变权限
        "assertion_cases": [["leaf::leaf.permission-mode.read.shape-mode-accessMode-defaultMode", "leaf::leaf.permission-mode.read.no-side-effect"], ["leaf::leaf.permission-mode.read.unauthorized-rejected"]],
    },
    "R00-T02-LA-0B669B30C854": {  # 切换指定作用域的会话权限模式：当前会话/指定会话/全局
        "assertion_cases": [["leaf::leaf.permission-mode.set.current-scope.applies", "leaf::leaf.permission-mode.set.session-scope.applies", "leaf::leaf.permission-mode.set.global-scope.applies"], ["leaf::leaf.permission-mode.set.unauthorized-rejected", "leaf::leaf.permission-mode.set.unauthorized-state-unchanged"]],
    },
    "R00-T02-LA-D01766AF4475": {  # 设置会话权限模式：body.mode → 返回 mode/accessMode
        "assertion_cases": [["leaf::leaf.permission-mode.set.session-scope.applies"], ["leaf::leaf.permission-mode.set.unauthorized-rejected"]],
    },
    # ── D01 聊天/WS（leaf.chat.transport / leaf.chat.round / leaf.chat.abort；真实
    #    websocket.ts 驱动双内核 + 帧捕获）──
    "R00-T02-LA-010BA6693880": {  # 聊天 WebSocket 传输：握手登记 + 事件转发
        "assertion_cases": [["leaf::leaf.chat.transport.ws.connected", "leaf::leaf.chat.transport.events.forwarded"], ["leaf::leaf.chat.transport.ws.unauthorized-ticket-rejected", "leaf::leaf.chat.transport.unauthorized-foreign-session-rejected"]],
    },
    # FF4F06011B48（提示词快照）暂不绑定：A 腿旧栈 readSessionPromptContextByPath
    # 对本夹具会话仍 500（诊断中，snapBody 已入观测）；双腿绿后再入 FULL_BINDINGS。
    "R00-T02-LA-7CBF6F4A6760": {  # WS 中止：abort_result accepted / 排队提交停止 / 找不到与非本体拒
        "assertion_cases": [["leaf::leaf.chat.abort.abort.accepted-stream-stopped"], ["leaf::leaf.chat.abort.abort.invalid-explicit-rejected"]],
    },
    # ── 业主裁决绑定（CLOSEOUT_ADJUDICATIONS.md 2026-10-26）：ADJ-4/ADJ-5 以
    #    Rust 真实实现为验收面；案例判真口径见 ADJUDICATED_RUST_AUTHORITATIVE。──
    "R00-T02-LA-485B2D0A8A02": {  # /new：轮换为新会话，旧会话归档（ADJ-4，Rust-authoritative）
        "assertion_cases": [
            ["leaf::leaf.slash.new.slash.new.rotates-and-archives", "leaf::leaf.slash.new.slash.new.old-recoverable", "leaf::leaf.slash.new.slash.new.explicit-reply"],
            ["leaf::leaf.slash.new.slash.new.invalid-session-rejected"],
        ],
    },
    "R00-T02-LA-3FCB259FF69C": {  # /reset：删除当前会话历史并提示已清除（ADJ-5，Rust-authoritative）
        "assertion_cases": [
            ["leaf::leaf.slash.slash.reset.history-cleared", "leaf::leaf.slash.slash.reset.explicit-reply", "leaf::leaf.slash.slash.reset.other-sessions-intact"],
            ["leaf::leaf.slash.slash.reset.invalid-session-rejected"],
        ],
    },
    # ADJ-1（P-ASK-DEFAULT）：原文"未设置时为 ASK"与两内核实际（auto）不一致，
    # 业主裁定改说明书——按实际语义绑定：读面返回当前模式（双内核一致），
    # 非法值显式拒。语义订正权威出处 CLOSEOUT_ADJUDICATIONS.md。
    "R00-T02-LA-25C4FF66FEE5": {
        "assertion_cases": [
            ["ab::permissions.default.parity"],
            ["leaf::permission.default-invalid-rejected"],
        ],
    },
    # ── D01 批次（leaf.prefs.quick-chat / leaf.drafts / leaf.ws.compact）──
    "R00-T02-LA-B5CB2993A5C0": {  # 读取快捷聊天偏好：返回设置不改状态
        "assertion_cases": [["leaf::leaf.prefs.quick-chat.read.shape", "leaf::leaf.prefs.quick-chat.put.roundtrip"], ["leaf::leaf.prefs.quick-chat.read.unauthorized-rejected"]],
    },
    "R00-T02-LA-D92CD567D6D6": {  # 更新快捷聊天偏好：保存并返回新状态；非法 body 拒
        "assertion_cases": [["leaf::leaf.prefs.quick-chat.put.roundtrip"], ["leaf::leaf.prefs.quick-chat.put.invalid-rejected-state-unchanged"]],
    },
    "R00-T02-LA-1B712E189DDF": {  # 读取输入框草稿：返回指定 surface 全部 home/会话草稿
        "assertion_cases": [["leaf::leaf.drafts.read.shape"], ["leaf::leaf.drafts.invalid-surface-rejected"]],
    },
    "R00-T02-LA-F829AEA5931C": {  # 保存主页草稿：home 槽 text/doc/updatedAt
        "assertion_cases": [["leaf::leaf.drafts.home.save-readback"], ["leaf::leaf.drafts.put-missing-scope-rejected"]],
    },
    "R00-T02-LA-D901852E1B1A": {  # 保存会话草稿：按 sessionId（path 先解析）
        "assertion_cases": [["leaf::leaf.drafts.session.save-readback"], ["leaf::leaf.drafts.put-missing-scope-rejected"]],
    },
    "R00-T02-LA-081FACA79F2C": {  # 清空草稿：text 空删除；仅限选定 surface/scope
        "assertion_cases": [["leaf::leaf.drafts.clear.empties-scoped-only"], ["leaf::leaf.drafts.invalid-surface-rejected"]],
    },
    "R00-T02-LA-421EEA1AC8D5": {  # WS compact：先回 accepted 再回 succeeded/noop/failed
        "assertion_cases": [
            ["leaf::leaf.ws.compact.compact.accepted-frame", "leaf::leaf.ws.compact.compact.result-succeeded-or-noop"],
            ["leaf::leaf.ws.compact.compact.invalid-session-rejected"],
        ],
    },
    # ── D03 MCP（leaf.mcp.crud：确定性 stdio 替身 r08_mcp_standin.cjs 双内核真实启动）──
    "R00-T02-LA-FAE7503D0D0F": {  # 开关 MCP 全局连接：enabled 布尔 + 合并状态返回
        "assertion_cases": [["leaf::leaf.mcp.crud.enabled.on-returns-merged-state"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    "R00-T02-LA-BB1BB3A9C5F4": {  # 设置 MCP 工具延迟加载：只更新提供的字段返回新状态
        "assertion_cases": [["leaf::leaf.mcp.crud.defer.patch-only-provided-fields"], ["leaf::leaf.mcp.crud.state.unauthorized-rejected"]],
    },
    "R00-T02-LA-0199B843D759": {  # 添加 MCP 连接器：保存并异步启动，返回公开配置与状态
        "assertion_cases": [["leaf::leaf.mcp.crud.add.saved-in-state", "leaf::leaf.mcp.crud.add.autostart-running-with-tools"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    "R00-T02-LA-21B3F4DC9140": {  # 刷新连接器工具：重新读取目录返回状态
        "assertion_cases": [["leaf::leaf.mcp.crud.refresh-tools.returns-status"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    "R00-T02-LA-18EFB2D9D5FD": {  # 启动连接器：新运行状态或错误
        "assertion_cases": [["leaf::leaf.mcp.crud.stop-then-start.new-run-state"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    "R00-T02-LA-E9C7A48CADC4": {  # 停止连接器：新运行状态或错误
        "assertion_cases": [["leaf::leaf.mcp.crud.stop-then-start.new-run-state"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    "R00-T02-LA-F4DA2AFCB72B": {  # 修改 MCP 连接器：配置更新返回
        "assertion_cases": [["leaf::leaf.mcp.crud.update.config-updated"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    "R00-T02-LA-48B0C7A7453E": {  # 批量导入：先校验全部再写入，坏行拒绝整批
        "assertion_cases": [["leaf::leaf.mcp.crud.bulk.valid-rows-imported", "leaf::leaf.mcp.crud.bulk.bad-row-rejects-whole-batch"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    "R00-T02-LA-E1FBE1A59BC6": {  # 查看 MCP 应用列表：应用与连接器状态；未初始化 503
        "assertion_cases": [["leaf::leaf.mcp.crud.apps.list-or-503"], ["leaf::leaf.mcp.crud.state.unauthorized-rejected"]],
    },
    "R00-T02-LA-240E200EF440": {  # 设置指定 Agent 的连接器：更新并返回配置
        "assertion_cases": [["leaf::leaf.mcp.crud.agent-connector.config-returned", "leaf::leaf.mcp.crud.agent-connector.reflected-in-state"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    "R00-T02-LA-67256417FB2B": {  # 批准当前会话一次性 MCP 工具能力：仅本会话放行不持久化
        "assertion_cases": [["leaf::leaf.mcp.crud.session-permission.granted-scoped"], ["leaf::leaf.mcp.crud.state.unauthorized-rejected"]],
    },
    "R00-T02-LA-2D194C1684BC": {  # 删除 MCP 连接器：配置与运行状态移除
        "assertion_cases": [["leaf::leaf.mcp.crud.remove.config-and-runtime-removed"], ["leaf::leaf.mcp.crud.negative.invalid-connector-rejected"]],
    },
    # ── D02 会话管理（leaf.sessions.admin）──
    "R00-T02-LA-960668141E07": {  # fork：带谱系新会话 + 深度过限/无效拒
        "assertion_cases": [["leaf::leaf.sessions.admin.fork.new-session-with-lineage", "leaf::leaf.sessions.admin.fork.depth-limit-rejected"], ["leaf::leaf.sessions.admin.fork.invalid-session-rejected"]],
    },
    "R00-T02-LA-3F95164A488C": {  # 连同子会话归档
        "assertion_cases": [["leaf::leaf.sessions.admin.archive-cascade.parent-and-children-archived"], ["leaf::leaf.sessions.admin.fork.invalid-session-rejected"]],
    },
    "R00-T02-LA-5235D8EDD673": {  # 解除子会话谱系后归档
        "assertion_cases": [["leaf::leaf.sessions.admin.archive-detach.child-stays-parent-archived"], ["leaf::leaf.sessions.admin.fork.invalid-session-rejected"]],
    },
    "R00-T02-LA-477DA3D558B8": {  # 预览会话轮次回退：只读
        "assertion_cases": [["leaf::leaf.sessions.admin.rollback-preview.readonly-shape"], ["leaf::leaf.sessions.admin.rollback-preview.invalid-rejected"]],
    },
    "R00-T02-LA-21A99DB98735": {  # 清扫孤儿工作台会话
        "assertion_cases": [["leaf::leaf.sessions.admin.sweep-orphaned.missing-dir-archived"], ["leaf::leaf.sessions.admin.fork.invalid-session-rejected"]],
    },
    "R00-T02-LA-6658E9B80314": {  # 永久删除工作台会话
        "assertion_cases": [["leaf::leaf.sessions.admin.workspace-disposal.permanent-delete"], ["leaf::leaf.sessions.admin.fork.invalid-session-rejected"]],
    },
    "R00-T02-LA-D62A81C2AEB2": {  # 刷新能力并压缩会话
        "assertion_cases": [["leaf::leaf.sessions.admin.fresh-compact.returns-result"], ["leaf::leaf.sessions.admin.fork.invalid-session-rejected"]],
    },
    "R00-T02-LA-4CEBEA945B7A": {  # 重试指定会话轮次（未开回退偏好时拒）
        "assertion_cases": [["leaf::leaf.sessions.admin.turns-retry.submitted-and-replied"], ["leaf::leaf.sessions.admin.turns-retry.file-rollback-disabled-rejected", "leaf::leaf.sessions.admin.turns-retry.invalid-session-rejected"]],
    },
    # ── D20 CLI data 族（r08_cli_data_vitest：产品测试逐用例合同）──
    "R00-T02-LA-00E57BB5F32B": {  # CLI data checkpoints：读有效检查点打印 epoch/日期/store 数/体积
        "producer": "r08_cli_data_vitest", "evidence_file": "cli-cases.json",
        "assertion_cases": [["cli::checkpoints.list-complete"], ["cli::checkpoints.skips-staging-quarantine"]],
    },
    "R00-T02-LA-1388F8EE0EBC": {  # CLI data restore：精确确认后恢复+收据/隔离目录；非 TTY 无 token/不符/不存在退出 1
        "producer": "r08_cli_data_vitest", "evidence_file": "cli-cases.json",
        "assertion_cases": [["cli::restore.executes-with-exact-token"], ["cli::restore.non-tty-no-token-refused", "cli::restore.wrong-token-refused", "cli::restore.tty-mismatch-aborts", "cli::restore.unknown-transition-errored"]],
    },
    "R00-T02-LA-0BE8B22DA5D5": {  # CLI data diagnose：读 epoch/升级日志/维护状态/检查点打印；损坏报 corrupt 不修复
        "producer": "r08_cli_data_vitest", "evidence_file": "cli-cases.json",
        "assertion_cases": [["cli::diagnose.clean-home", "cli::diagnose.in-progress-journal", "cli::diagnose.available-checkpoint", "cli::diagnose.zero-write"], ["cli::diagnose.corrupt-reported-not-repaired"]],
    },
    "R00-T02-LA-BC2FBD618278": {  # 查看/读规划权限模式：planMode/permissionMode/accessMode
        "assertion_cases": [["leaf::leaf.plan-mode.read.shape", "leaf::leaf.plan-mode.read.no-side-effect"], ["leaf::leaf.plan-mode.read.unauthorized-rejected"]],
    },
    "R00-T02-LA-A336F79E964D": {  # 切换规划模式：无 mode 由 !!enabled 调 setPlanMode
        "assertion_cases": [
            ["leaf::leaf.plan-mode.toggle.on-readback", "leaf::leaf.plan-mode.toggle.off-readback", "leaf::leaf.plan-mode.mode-precedence"],
            ["leaf::leaf.plan-mode.invalid-rejected-state-unchanged"],
        ],
    },
    # ── R08 收尾批次 5：工具回合线束（替身 R08-TOOL-PLAN，双内核同一按需目录协议）──
    "R00-T02-LA-45C5CE361CCB": {  # 完成当前计划余项：有未完成项追加完成记录并广播；已完成返回当前面板不重复写 / 版本失配·无效目标显式错误
        "assertion_cases": [
            ["leaf::leaf.tool.todos.complete.marks-unfinished-completed", "leaf::leaf.tool.todos.complete.broadcasts-panel", "leaf::leaf.tool.todos.complete.already-finished-returns-panel-no-rewrite"],
            ["leaf::leaf.tool.todos.complete.version-mismatch-409", "leaf::leaf.tool.todos.complete.invalid-session-rejected"],
        ],
    },
    "R00-T02-LA-5CD35D9EED20": {  # 取消当前计划余项：只取消未完成项并广播，不终止其他后台任务 / 版本失配·无效目标显式错误
        "assertion_cases": [
            ["leaf::leaf.tool.todos.cancel.marks-unfinished-cancelled-keeps-completed", "leaf::leaf.tool.todos.cancel.broadcasts-panel", "leaf::leaf.tool.todos.cancel.no-stream-abort-side-effect", "leaf::leaf.tool.todos.cancel.already-finished-returns-panel-no-rewrite"],
            ["leaf::leaf.tool.todos.cancel.version-mismatch-409", "leaf::leaf.tool.todos.cancel.invalid-session-rejected"],
        ],
    },
    "R00-T02-LA-17FCFE45576B": {  # 收纳已结束计划：仅已结束且未收纳追加收纳记录，历史保留 / 版本失配·无效目标显式错误
        "assertion_cases": [
            ["leaf::leaf.tool.todos.dismiss.unfinished-is-noop-list-survives", "leaf::leaf.tool.todos.dismiss.finished-list-dismissed", "leaf::leaf.tool.todos.dismiss.history-retained"],
            ["leaf::leaf.tool.todos.dismiss.version-mismatch-409", "leaf::leaf.tool.todos.dismiss.invalid-session-rejected"],
        ],
    },
    "R00-T02-LA-B34F2B298BFA": {  # todo_write 工具（3 断言）：界面（线上面板事件 + 真实 store）与会话记录一致 / 错误输入不覆盖 / 状态或 blocked 理由无效时报错不覆盖旧清单
        "assertion_cases": [
            ["leaf::leaf.tool.todos.todo-write.round-final", "leaf::leaf.tool.todos.todo-write.wire-panel-matches-list", "leaf::leaf.tool.todos.todo-write.ui-store-panel-matches-list", "leaf::leaf.tool.todos.todo-write.history-records-list"],
            ["leaf::leaf.tool.todos.todo-write.invalid-does-not-overwrite"],
            ["leaf::leaf.tool.todos.todo-write.invalid-rejected-explicitly", "leaf::leaf.tool.todos.todo-write.invalid-status-rejected-keeps-list"],
        ],
    },
    "R00-T02-LA-6207F7E8C697": {  # checkpoint 工具（4 断言）：创建后能列出 / 删除后不可见 / 无会话不写快照（模型路径恒有会话——以 snapshot:false 的「不写文件快照」为可达面）/ 删除不存在不伪报
        "assertion_cases": [
            ["leaf::leaf.tool.checkpoint.create.then-list-shows-name"],
            ["leaf::leaf.tool.checkpoint.drop.then-list-hides-name"],
            ["leaf::leaf.tool.checkpoint.create.snapshot-disabled-writes-no-file-snapshot"],
            ["leaf::leaf.tool.checkpoint.drop.missing-reported-not-found"],
        ],
    },
    "R00-T02-LA-E1625F70C7CC": {  # rewind 工具（4 断言）：拒绝不回退 / 回退后分支与文件符合锚点（retry 重置点语义）/ 外部改动保留 / 会话忙时不立刻回退（scheduled 挂到流结束）
        "assertion_cases": [
            ["leaf::leaf.tool.rewind.rewind.pending-confirmation-surfaced", "leaf::leaf.tool.rewind.rewind.rejected-does-not-rewind", "leaf::leaf.tool.rewind.rewind.rejected-reports-not-rewound"],
            ["leaf::leaf.tool.rewind.rewind.confirmed-resets-branch-to-anchor", "leaf::leaf.tool.rewind.rewind.confirmed-files-untouched-without-restore"],
            ["leaf::leaf.tool.rewind.rewind.files-untouched-without-restore"],
            ["leaf::leaf.tool.rewind.rewind.confirmed-in-turn-deferred-until-turn-end"],
        ],
    },
    "R00-T02-LA-5DCB0352EB08": {  # session 工具（4 断言）：跨会话范围受控 / 发送只达指定目标（草稿卡不直写）/ 部分成功保留新会话 ID（create 草稿句柄）/ 目标不可用·模型无效分开报告、拒自发
        "assertion_cases": [
            ["leaf::leaf.tool.session.list.shows-own-other-session", "leaf::leaf.tool.session.read.own-session-transcript"],
            ["leaf::leaf.tool.session.send.creates-draft-card-for-target", "leaf::leaf.tool.session.send.target-history-not-written-before-confirm"],
            ["leaf::leaf.tool.session.create.returns-draft-handle-for-agent"],
            ["leaf::leaf.tool.session.read.unknown-session-rejected", "leaf::leaf.tool.session.send.unknown-target-rejected", "leaf::leaf.tool.session.send.to-self-refused", "leaf::leaf.tool.session.create.unknown-agent-rejected"],
        ],
    },
    "R00-T02-LA-EC033184BA37": {  # confirm 路由 confirmed：标记+广播 confirmation_resolved+待批动作才继续 / 无效 action 400·已处理 404·越权
        "assertion_cases": [
            ["leaf::leaf.tool.rewind.confirm.confirmed-broadcasts-resolved", "leaf::leaf.tool.rewind.confirm.confirmed-continues-pending-action"],
            ["leaf::leaf.tool.rewind.confirm.invalid-action-400", "leaf::leaf.tool.rewind.confirm.confirmed-already-resolved-404", "leaf::leaf.tool.rewind.confirm.unauthorized-rejected"],
        ],
    },
    "R00-T02-LA-EF43ADCE19A1": {  # confirm 路由 rejected：标记+广播+待批动作不执行 / 已处理 404·拒绝后再提交不得继续原动作
        "assertion_cases": [
            ["leaf::leaf.tool.rewind.confirm.rejected-200", "leaf::leaf.tool.rewind.confirm.rejected-broadcasts-resolved", "leaf::leaf.tool.rewind.rewind.rejected-does-not-rewind"],
            ["leaf::leaf.tool.rewind.confirm.already-resolved-404"],
        ],
    },
    "R00-T02-LA-75C0AE981505": {  # /confirm 斜杠：标 confirmed、继续原待批动作并广播 / ID 缺失·已处理不重复执行
        "assertion_cases": [
            ["leaf::leaf.tool.rewind.slash.confirm.resolves-and-replies", "leaf::leaf.tool.rewind.confirm.confirmed-continues-pending-action", "leaf::leaf.tool.rewind.confirm.confirmed-broadcasts-resolved"],
            ["leaf::leaf.tool.rewind.slash.confirm.missing-id-usage", "leaf::leaf.tool.rewind.slash.confirm.already-processed-no-reexecute"],
        ],
    },
    "R00-T02-LA-CD1524CC7DC3": {  # /reject 斜杠：标 rejected、回复已取消、原待批动作不执行 / ID 缺失·已处理不再次决策
        "assertion_cases": [
            ["leaf::leaf.tool.rewind.slash.reject.resolves-and-replies", "leaf::leaf.tool.rewind.slash.reject.pending-action-not-executed"],
            ["leaf::leaf.tool.rewind.slash.reject.missing-id-usage", "leaf::leaf.tool.rewind.slash.reject.already-processed-no-redecide"],
        ],
    },
    "R00-T02-LA-38667593C3E1": {  # current_status 工具（4 断言）：键目录与 get 一致 / 模型·会话值与运行时一致 / 未知值标未知 / 未知 key 或依赖不可用返回明确错误
        "assertion_cases": [
            ["leaf::leaf.tool.current-status.list.returns-key-catalog", "leaf::leaf.tool.current-status.get.every-listed-key-resolves"],
            ["leaf::leaf.tool.current-status.get.model-matches-runtime", "leaf::leaf.tool.current-status.get.agent-matches-runtime", "leaf::leaf.tool.current-status.get.time-has-iso-and-timezone-fields"],
            ["leaf::leaf.tool.current-status.get.bridge-context-explicit-not-bridge"],
            ["leaf::leaf.tool.current-status.get.unknown-key-explicit-error", "leaf::leaf.tool.current-status.get.missing-key-explicit-error"],
        ],
    },
    "R00-T02-LA-7C959E75B8C3": {  # session_folders 工具（3 断言）：批准后仅当前会话变化 / 拒绝后范围不变 / 撤权后范围缩回且不扩大其他会话范围
        "assertion_cases": [
            ["leaf::leaf.tool.session-folders.list.returns-scope-shape", "leaf::leaf.tool.session-folders.add.approved-changes-current-session-only"],
            ["leaf::leaf.tool.session-folders.add.ask-mode-surfaces-confirmation", "leaf::leaf.tool.session-folders.add.rejected-scope-unchanged"],
            ["leaf::leaf.tool.session-folders.remove.restores-scope", "leaf::leaf.tool.session-folders.reject.other-session-unchanged"],
        ],
    },
    # ── R08 收尾批次 6：WS 入站帧族（leaf.ws.stream，批次 6 双腿实跑）──
    "R00-T02-LA-D483E4CD0935": {  # prompt：流式阶段/工具/最终回复；空内容与非法图片类型化拒、不写消息
        "assertion_cases": [
            ["leaf::leaf.ws.stream.prompt.run-streams-status-tool-final"],
            ["leaf::leaf.ws.stream.prompt.empty-not-submitted", "leaf::leaf.ws.stream.prompt.unsupported-image-typed-rejection", "leaf::leaf.ws.stream.prompt.too-many-images-typed-rejection", "leaf::leaf.ws.stream.prompt.rejected-inputs-write-no-message"],
        ],
    },
    "R00-T02-LA-C1FB9C92E813": {  # steer：活动流 steered 回执且到达模型；空闲回落普通 prompt；无权拒
        "assertion_cases": [
            ["leaf::leaf.ws.stream.steer.active-stream-acked", "leaf::leaf.ws.stream.steer.text-reaches-model"],
            ["leaf::leaf.ws.stream.steer.idle-falls-back-to-prompt", "leaf::leaf.ws.stream.steer.unknown-session-rejected"],
        ],
    },
    "R00-T02-LA-92620BF2264F": {  # interject：流中回执+新回复；媒体超限拒且不写空消息；无效会话拒
        "assertion_cases": [
            ["leaf::leaf.ws.stream.interject.during-stream-receipt-and-new-reply"],
            ["leaf::leaf.ws.stream.interject.media-over-limit-rejected-no-empty-message", "leaf::leaf.ws.stream.interject.invalid-session-rejected"],
        ],
    },
    "R00-T02-LA-81664285F927": {  # resume_stream：补发/reset/truncated；无流空事件+运行态；越权不泄露
        "assertion_cases": [
            ["leaf::leaf.ws.stream.resume.known-stream-replays-or-marks-reset"],
            ["leaf::leaf.ws.stream.resume.no-active-stream-empty-events-with-runtime-flag", "leaf::leaf.ws.stream.resume.unknown-session-rejected-no-frames-leaked"],
        ],
    },
    "R00-T02-LA-2FAA749DA790": {  # WS slash：同一 dispatcher 的 slash_result；未知命令错误；Agent 不匹配拒
        "assertion_cases": [
            ["leaf::leaf.ws.stream.slash.ws.dispatched-as-slash-result"],
            ["leaf::leaf.ws.stream.slash.ws.unknown-command-error", "leaf::leaf.ws.stream.slash.ws.agent-identity-mismatch-rejected"],
        ],
    },
    "R00-T02-LA-0FE8BED686E2": {  # /compact：先应答正在压缩再回结果；无效会话显式错误
        "assertion_cases": [
            ["leaf::leaf.ws.stream.slash.compact.ack-before-result", "leaf::leaf.ws.stream.slash.compact.result-tokens-or-completion"],
            ["leaf::leaf.ws.stream.slash.compact.invalid-session-explicit-error"],
        ],
    },
    "R00-T02-LA-5A6E0D526903": {  # /fresh-compact：应答+结果；失败显式+原始历史保留
        "assertion_cases": [
            ["leaf::leaf.ws.stream.slash.fresh-compact.ack-then-result-tokens-or-reason"],
            ["leaf::leaf.ws.stream.slash.fresh-compact.invalid-session-explicit-error", "leaf::leaf.ws.stream.slash.fresh-compact.original-history-retained"],
        ],
    },
    "R00-T02-LA-4EA88B5073E4": {  # /stop：中止当前流且无终结回答；空闲回已停止；其他会话不受扰
        "assertion_cases": [
            ["leaf::leaf.ws.stream.slash.stop.aborts-active-stream"],
            ["leaf::leaf.ws.stream.slash.stop.idle-reports-already-stopped", "leaf::leaf.ws.stream.slash.stop.other-session-unaffected"],
        ],
    },
    # ── R08 收尾批次 7：D02 HTTP 语义 ──
    "R00-T02-LA-B0F05BC32D62": {  # cleanup：窗口内保留/窗口外永久删除并清生命周期与标题/活动不动；越权不删
        "assertion_cases": [
            ["leaf::leaf.cleanup.expiry.cleanup.within-window-keeps-fresh-archives", "leaf::leaf.cleanup.expiry.cleanup.beyond-window-deletes-and-reports-count", "leaf::leaf.cleanup.expiry.cleanup.deleted-unrecoverable-and-title-gone", "leaf::leaf.cleanup.expiry.cleanup.active-sessions-untouched"],
            ["leaf::leaf.cleanup.expiry.cleanup.unauthorized-rejected-no-side-effect"],
        ],
    },
    "R00-T02-LA-4DD3F71FA2CD": {  # 重放最近用户消息：按版本+替换文字、运行中 409；无效目标显式错
        "assertion_cases": [
            ["leaf::leaf.replay-latest.replay.latest-replayed-new-run", "leaf::leaf.replay-latest.replay.replacement-text-replayed", "leaf::leaf.replay-latest.replay.while-running-409"],
            ["leaf::leaf.replay-latest.replay.invalid-session-rejected"],
        ],
    },
    "R00-T02-LA-FBDEEDAC0B11": {  # switch 焦点：改当前会话+浏览器恢复回执+来回不挂；无效目标显式错
        "assertion_cases": [
            ["leaf::leaf.switch.focus.switch.returns-projection", "leaf::leaf.switch.focus.switch.browser-restore-attempt-surfaced", "leaf::leaf.switch.focus.switch.store-current-session-changes", "leaf::leaf.switch.focus.switch.back-and-forth-no-hang"],
            ["leaf::leaf.switch.focus.switch.invalid-session-rejected"],
        ],
    },
    "R00-T02-LA-FF4F06011B48": {  # 真回合后的提示词快照：冻结系统提示词+工具名；无效会话显式拒
        "assertion_cases": [
            ["leaf::leaf.prompt-snapshot.real.snapshot.read-after-real-round", "leaf::leaf.prompt-snapshot.real.snapshot.tool-names-include-resident-read"],
            ["leaf::leaf.prompt-snapshot.real.snapshot.invalid-session-rejected"],
        ],
    },
    "R00-T02-LA-3818ADEAFA7A": {  # D-16 翻案：续接已删除 Agent（真实服务端铸 id + 活 Agent 400 负路径）
        "assertion_cases": [
            ["leaf::leaf.sessions.admin.continue-deleted-agent.new-identity-and-path"],
            ["leaf::leaf.sessions.admin.continue-deleted-agent.live-agent-rejected"],
        ],
    },
    # ── R08 收尾批次 8：D03 OAuth（环回授权服务器替身，真实发现/动态注册/PKCE）──
    "R00-T02-LA-2DA782C5C7B9": {  # OAuth start：浏览器资料；未知连接器显式拒
        "assertion_cases": [
            ["leaf::leaf.mcp.oauth.oauth.start-returns-authorization-material"],
            ["leaf::leaf.mcp.oauth.oauth.start-unknown-connector-rejected"],
        ],
    },
    "R00-T02-LA-E7F852F9BF60": {  # OAuth cancel：结束等待且已存配置不变；无等待诚实；无权拒
        "assertion_cases": [
            ["leaf::leaf.mcp.oauth.oauth.cancel-ends-pending-roundtrip", "leaf::leaf.mcp.oauth.oauth.cancel-keeps-saved-credentials", "leaf::leaf.mcp.oauth.oauth.cancel-without-pending-is-honest"],
            ["leaf::leaf.mcp.oauth.oauth.cancel-unauthorized-rejected"],
        ],
    },
    "R00-T02-LA-C70E819F8DA4": {  # OAuth logout：清凭证回公开状态；未知连接器/无权显式拒
        "assertion_cases": [
            ["leaf::leaf.mcp.oauth.oauth.logout-returns-public-state", "leaf::leaf.mcp.oauth.oauth.logout-clears-public-credential-state"],
            ["leaf::leaf.mcp.oauth.oauth.logout-unknown-connector-rejected", "leaf::leaf.mcp.oauth.oauth.logout-unauthorized-rejected"],
        ],
    },

    # ── R08 收尾批次 8 续：MCP 资源/应用（ui:// 资源真实读取 + launchInput 交接）──
    "R00-T02-LA-04A6A2BD1547": {  # 按 uri 返回连接器资源内容；缺 uri/未知 uri 拒
        "assertion_cases": [
            ["leaf::leaf.mcp.face.resources.uri-returns-content", "leaf::leaf.mcp.face.resources.missing-uri-400", "leaf::leaf.mcp.face.resources.unknown-uri-rejected"],
            ["leaf::leaf.mcp.face.resources.unknown-connector-rejected"],
        ],
    },
    "R00-T02-LA-7FF8D4E48BC9": {  # 启动连接器应用：launchInput 原样交接；未知工具/无权拒
        "assertion_cases": [
            ["leaf::leaf.mcp.face.apps.launch-hands-over-launch-input"],
            ["leaf::leaf.mcp.face.apps.unknown-tool-rejected", "leaf::leaf.mcp.face.apps.unauthorized-rejected"],
        ],
    },
    # ── R08 收尾批次 7 续：历史延迟内容（旧栈 deferred locator / Rust exec-output 资源引用）──
    "R00-T02-LA-ED089133C106": {  # 只在授权会话历史内按 contentId 取大内容；他会话/未知 404
        "assertion_cases": [
            ["leaf::leaf.lazy-content.lazy.large-result-referenced-in-history", "leaf::leaf.lazy-content.lazy.content-fetch-returns-full-body"],
            ["leaf::leaf.lazy-content.lazy.other-session-locator-404", "leaf::leaf.lazy-content.lazy.unknown-content-404"],
        ],
    },

    # ── R08 收尾第六轮：D13 终端子系统真实正路径（leaf.terminal.positive，追加套件双腿实跑）──
    "R00-T02-LA-CE69063550AA": {  # snapshot：送当前终端快照；身份不符拒绝且不送他人终端
        "assertion_cases": [
            ["leaf::leaf.terminal.positive.snapshot.current-terminal-listed", "leaf::leaf.terminal.positive.snapshot.lists-both-session-terminals"],
            ["leaf::leaf.terminal.positive.snapshot.identity-mismatch-rejected", "leaf::leaf.terminal.positive.snapshot.other-session-no-leak"],
        ],
    },
    "R00-T02-LA-4C35E6AEC6F7": {  # tail：按 terminalId/sinceSeq 返回后续输出；无权/不匹配拒绝
        "assertion_cases": [
            ["leaf::leaf.terminal.positive.tail.initial-window", "leaf::leaf.terminal.positive.tail.new-output-after-since"],
            ["leaf::leaf.terminal.positive.tail.identity-mismatch-rejected", "leaf::leaf.terminal.positive.tail.foreign-terminal-rejected"],
        ],
    },
    "R00-T02-LA-CFD9F02BC6AA": {  # close：终止并返回 killed|already_stopped（真实终止）；rejected 不谎报
        "assertion_cases": [
            ["leaf::leaf.terminal.positive.close.reports-killed-or-already-stopped", "leaf::leaf.terminal.positive.close.process-really-stopped"],
            ["leaf::leaf.terminal.positive.close.unknown-id-rejected", "leaf::leaf.terminal.positive.close.foreign-terminal-rejected", "leaf::leaf.terminal.positive.close.identity-mismatch-rejected"],
        ],
    },

    # ── R08 客户端 UI 页份额（r08_ui_pages 生产者：jsdom 真实组件投影 + 既有专属单测 +
    #    真 Electron live；每断言 ≥1 专属 case，叶内不复用。证据：ui-cases.json）──
    "R00-T02-LA-81B217A7E581": {  # 聊天页 6 动作
        "producer": "r08_ui_pages", "evidence_file": "ui-cases.json",
        "assertion_cases": [
            ["ui::chat.a0.welcome-input-exclusive-projection", "ui::chat.a0.mobile-disconnected-no-thinking-get", "ui::chat.a0.regional-failure-boundary"],
            ["ui::chat.a1.optimistic-user-message-client-id", "ui::chat.a1.slash-sent-via-ws", "ui::chat.a1.slash-result-inline-notice", "ui::chat.a1.blank-input-blocked", "ui::chat.a1.ws-unavailable-explicit-result", "ui::chat.a1.ws-error-shown-in-page"],
            ["ui::chat.a2.streaming-send-queues-then-interjects", "ui::chat.a2.stop-binds-current-stream", "ui::chat.a2.idle-vs-running-control", "ui::chat.a2.resume-keeps-streaming-state", "ui::chat.a2.resume-status-drives-control"],
            ["ui::chat.a3.side-chat-transcript-composer", "ui::chat.a3.side-chat-closed-empty", "ui::chat.a3.attachments-preview", "ui::chat.a3.runtime-capsule-pill"],
            ["ui::chat.a4.existing-session-model-switch", "ui::chat.a4.new-session-default-model-set", "ui::chat.a4.same-model-zero-request", "ui::chat.a4.switch-failure-explicit-toast"],
            ["ui::chat.a5.thinking-session-save", "ui::chat.a5.thinking-pending-default-only", "ui::chat.a5.thinking-save-failure-console-only"],
        ],
    },
    "R00-T02-LA-8E2D44B48905": {  # 会话地图页 7 动作（Electron live 真服务）
        "producer": "r08_ui_pages", "evidence_file": "ui-cases.json",
        "assertion_cases": [
            ["ui::map.a0.loads-real-layout-and-turn-cards", "ui::map.a0.other-service-empty-map-zero-mutation", "ui::map.a0.read-failure-explicit-warning"],
            ["ui::map.a1.workspace-selector-no-cwd-zero-files-read", "ui::map.a1.card-selects-real-session-same-session-zero-fetch", "ui::map.a1.same-revision-cache-zero-history-changed-revision-refresh", "ui::map.a1.archive-refusal-retains-session-selected-card"],
            ["ui::map.a2.zoom-locate-only-change-real-camera", "ui::map.a2.invalid-coordinates-zero-put"],
            ["ui::map.a3.pointer-drag-persists-card-position", "ui::map.a3.drag-write-failure-keeps-local-disk-unchanged"],
            ["ui::map.a4.collapse-reload-and-external-reveal-replaces-list", "ui::map.a4.collapse-write-failure-keeps-local-old-disk"],
            ["ui::map.a5.reset-replaces-all-positions-on-disk", "ui::map.a5.reset-write-failure-keeps-local-old-disk"],
            ["ui::map.a6.empty-whitespace-zero-send-continue-no-fork", "ui::map.a6.fork-failure-missing-anchor-preserves-draft-then-branch", "ui::map.a6.running-busy-queues-input-zero-extra-prompt", "ui::map.a6.paused-unknown-delivery-preserves-draft"],
        ],
    },
    "R00-T02-LA-B4BB2438E855": {  # MCP 设置页 7 动作
        "producer": "r08_ui_pages", "evidence_file": "ui-cases.json",
        "assertion_cases": [
            ["ui::mcp-settings.a0.container-cold-start-mcp-page", "ui::mcp-settings.a0.cached-old-data-background-refresh", "ui::mcp-settings.a0.provider-summary-nonblocking"],
            ["ui::mcp-settings.a1.connector-list-status-tools", "ui::mcp-settings.a1.read-failure-keeps-old-state"],
            ["ui::mcp-settings.a2.global-toggle-save-reread", "ui::mcp-settings.a2.global-toggle-failure-restores", "ui::mcp-settings.a2.defer-threshold-save", "ui::mcp-settings.a2.no-agent-global-state-only"],
            ["ui::mcp-settings.a3.add-connector-with-start-intent", "ui::mcp-settings.a3.bulk-import-per-row-partial", "ui::mcp-settings.a3.bulk-partial-http200-mixed"],
            ["ui::mcp-settings.a4.remove-with-confirm", "ui::mcp-settings.a4.start-stop-refresh-actions-keep-credentials"],
            ["ui::mcp-settings.a5.agent-connector-tools-pinned", "ui::mcp-settings.a5.no-agent-refuses-detail", "ui::mcp-settings.a5.policy-save-failure-no-fake-grant"],
            ["ui::mcp-settings.a6.oauth-login-open-external-poll-refresh", "ui::mcp-settings.a6.oauth-cancel-post-only", "ui::mcp-settings.a6.oauth-logout-reread", "ui::mcp-settings.a6.oauth-start-failure-no-external"],
        ],
    },
    "R00-T02-LA-DA5AD5C40098": {  # 安全设置页 4 动作
        "producer": "r08_ui_pages", "evidence_file": "ui-cases.json",
        "assertion_cases": [
            ["ui::security.a0.container-cold-start-security-page", "ui::security.a0.init-error-ready-with-log", "ui::security.a0.fifteen-second-timeout-ready"],
            ["ui::security.a1.sandbox-switches-save-and-reread", "ui::security.a1.uninitialized-loading-not-off", "ui::security.a1.save-failure-restores-and-snapshot-failure", "ui::security.a1.no-connection-zero-network-request"],
            ["ui::security.a2.retention-and-backup-list", "ui::security.a2.empty-and-read-failure-explicit", "ui::security.a2.no-agent-load-settings-zero-request"],
            ["ui::security.a3.manual-proxy-save-and-notify", "ui::security.a3.system-mode-and-save-failure-keeps-draft", "ui::security.a3.experience-disabled-zero-request", "ui::security.a3.notify-failure-no-success"],
        ],
    },
    "R00-T02-LA-A2C385A33493": {  # 快捷聊天窗口 2 动作（Electron live 双真窗口）
        "producer": "r08_ui_pages", "evidence_file": "ui-cases.json",
        "assertion_cases": [
            ["ui::quick-chat.a0.visible-compact-bounds", "ui::quick-chat.a0.resize-increases-window", "ui::quick-chat.a0.switch-acks-focus-main-then-hide", "ui::quick-chat.a0.same-path-requires-real-service-focus"],
            ["ui::quick-chat.a1.missing-session-refuses-keeps-floating", "ui::quick-chat.a1.destroyed-main-explicit-failure", "ui::quick-chat.a1.changed-owner-refuses-without-hide", "ui::quick-chat.a1.late-ack-cannot-hide-after-timeout"],
        ],
    },

    # ── R08 收尾第六轮：D01 交互两叶（追加套件 leaf.tool.ask-user / leaf.chat.task-abort）──
    "R00-T02-LA-94CB746D04B7": {  # ask_user 5 断言：卡片可见/确认映射/超时推荐项/拒答中止无答案/错误面
        "assertion_cases": [
            ["leaf::leaf.tool.ask-user.a0.questions-and-options-visible", "leaf::leaf.tool.ask-user.a0.values-and-labels-visible", "leaf::leaf.tool.ask-user.a0.recommended-visible", "leaf::leaf.tool.ask-user.a0.pending-question-exposed"],
            ["leaf::leaf.tool.ask-user.a1.confirmed-answered-true", "leaf::leaf.tool.ask-user.a1.keys-mapped", "leaf::leaf.tool.ask-user.a1.values-mapped", "leaf::leaf.tool.ask-user.a1.labels-mapped", "leaf::leaf.tool.ask-user.a1.unknown-value-dropped", "leaf::leaf.tool.ask-user.a1.optional-unanswered", "leaf::leaf.tool.ask-user.a1.no-auto-selected", "leaf::leaf.tool.ask-user.a1.pending-cleared-after-answer", "leaf::leaf.tool.ask-user.a1.round-completes"],
            ["leaf::leaf.tool.ask-user.a2.timedout-flag", "leaf::leaf.tool.ask-user.a2.recommended-auto-selected", "leaf::leaf.tool.ask-user.a2.non-recommended-not-filled", "leaf::leaf.tool.ask-user.a2.round-completes", "leaf::leaf.tool.ask-user.a2.pending-cleared-after-timeout"],
            ["leaf::leaf.tool.ask-user.a3.dismissed-no-answers", "leaf::leaf.tool.ask-user.a3.dismissed-pending-cleared", "leaf::leaf.tool.ask-user.a3.dismissed-round-completes", "leaf::leaf.tool.ask-user.a3.aborted-no-answers", "leaf::leaf.tool.ask-user.a3.abort-acknowledged"],
            # A4：无效问题/缺会话/已处理 显式错误 + 拒答/中止不编造答案（活体 AB）；
            # 「ConfirmStore 不可用」无活体公开开关，由两内核既有专属测试覆盖
            # （tests/ask-user-tool.test.ts、rust .../r06_interactive_completion.rs），
            # 该子句在 CLOSEOUT_PROGRESS 如实披露。
            ["leaf::leaf.tool.ask-user.a4.invalid-questions-explicit-error", "leaf::leaf.tool.ask-user.a4.invalid-questions-no-card", "leaf::leaf.tool.ask-user.a4.missing-session-explicit-error", "leaf::leaf.tool.ask-user.a4.settled-or-unknown-question-explicit-error", "leaf::leaf.tool.ask-user.a4.dismissed-does-not-fabricate-answer", "leaf::leaf.tool.ask-user.a4.aborted-does-not-fabricate-answer"],
        ],
    },
    "R00-T02-LA-B47CAC08F418": {  # task abort POST 2 断言：aborted/already_stopped+404+403；身份/所有权/不扩权
        "assertion_cases": [
            ["leaf::leaf.chat.task-abort.a0.owned-task-aborted-or-already-stopped", "leaf::leaf.chat.task-abort.a0.owned-response-scoped", "leaf::leaf.chat.task-abort.a0.unknown-task-404", "leaf::leaf.chat.task-abort.a0.cross-session-403"],
            ["leaf::leaf.chat.task-abort.a1.terminal-state-decides-result", "leaf::leaf.chat.task-abort.a1.missing-session-context-400", "leaf::leaf.chat.task-abort.a1.unauthorized-rejected", "leaf::leaf.chat.task-abort.a1.cross-session-no-leak", "leaf::leaf.chat.task-abort.a1.cross-session-task-unaffected"],
        ],
    },

}

# A/B 演习合同只钉「r08_t07_ab 生产」的案例；改挂其他 producer 的叶
# （如 D20 CLI 族 → r08_cli_data_vitest/cli-cases.json）不入该文件。
CONTRACT_PINNED_CASES = {
    case
    for binding in FULL_BINDINGS.values()
    if binding.get("producer", "r08_t07_ab") == "r08_t07_ab"
    for group in binding["assertion_cases"]
    for case in group
}

CARGO_TEST_SUITES = {"r08_t01_a01_a02_ws", "r08_t01_env_deps", "r08_a04_no_final_ws",
                     "r08_t04_sessionfiles_v2", "r08_t05_seed_guard",
                     "r08_c3_find_bridge_redaction"}

# 由 r08_t07_ab 聚合的旁证 cargo 案例：案例名 → (套件, cargo 测试名)。该套件
# 由同场景（A13）在 r08_t07_ab 之前引用，其 output.log 的 `test <name> ... ok`
# 行即案例成立；缺行/缺套件 = 0（响亮）。
CARGO_SIDE_CASES = {
    "leaf::find.bridge-redaction": (
        "r08_c3_find_bridge_redaction",
        "c3_find_redacts_bridge_time_tags_only_for_bridge_bound_sessions",
    ),
}

# 业主裁决（docs/rust-tauri/R08/CLOSEOUT_ADJUDICATIONS.md，2026-10-26）：
# 叶 → (裁决号, [(任务, 案例名), ...])。这些叶的验收面依法裁定为 Rust 真实
# 实现（旧栈 phase-1 缺口/旧行为），案例判真以 Rust 腿为准；A 腿（旧栈）观测
# 仍入 cases.json（actual 如实），由 derive 按 rust-authoritative 口径计值。
ADJUDICATED_RUST_AUTHORITATIVE = {
    "R00-T02-LA-485B2D0A8A02": ("ADJ-4", [
        ("leaf.slash.new", "slash.new.rotates-and-archives"),
        ("leaf.slash.new", "slash.new.old-recoverable"),
        ("leaf.slash.new", "slash.new.explicit-reply"),
        ("leaf.slash.new", "slash.new.invalid-session-rejected"),
    ]),
    "R00-T02-LA-3FCB259FF69C": ("ADJ-5", [
        ("leaf.slash", "slash.reset.explicit-reply"),
        ("leaf.slash", "slash.reset.history-cleared"),
        ("leaf.slash", "slash.reset.other-sessions-intact"),
        ("leaf.slash", "slash.reset.invalid-session-rejected"),
    ]),
}

DRILL_PATH = Path("scripts/rust-tauri/r08_t07_ab_drill.mjs")

# R08 收尾第六轮：A13 主套件 + 域扩展套件（终端 / 交互工具）合并进同一 A/B 演习
# 观测（drill --suite-files）；扩展套件的 cases 由 derive_ab_cases 同口径派生
# leaf::<task>.<case> 图钉。开发可只跑子集，正式门必须全部执行。
MAIN_AB_SUITE = "desktop/src/react/__tests__/r08/r08-t07-kernel-ab.test.ts"
AB_EXTRA_SUITES = [
    "desktop/src/react/__tests__/r08/r08-t07-terminal-ab.test.ts",
    "desktop/src/react/__tests__/r08/r08-t07-interactive-ab.test.ts",
]
EXPECTED_EXTRA_TASKS = ["leaf.terminal.positive", "leaf.tool.ask-user", "leaf.chat.task-abort"]


def ab_compare_rules() -> list[str]:
    """从 A/B 演习脚本解析 RULES 任务键（唯一真理源；防双份漂移）。"""
    text = (ROOT / DRILL_PATH).read_text(encoding="utf-8")
    body = re.search(r"const RULES = \{(.*?)\n\};", text, re.S)
    if not body:
        raise SystemExit("r08_t07_ab_drill.mjs 中未找到 RULES 对象（解析失败）")
    keys = re.findall(r"^\s*'([a-z0-9_.-]+)':\s*\(", body.group(1), re.M)
    if not keys:
        raise SystemExit("RULES 解析结果为空")
    return keys


AB_COMPARE_RULES = ab_compare_rules()


def load(path: Path):
    return json.loads((ROOT / path).read_text(encoding="utf-8"))


def sha(value) -> str:
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True).encode()).hexdigest()


def command_for_suite(suite: str) -> dict:
    return {
        "argv": ["python3", "-B", "scripts/rust-tauri/r08_stage_inputs.py",
                 "--run-suite", suite, "--evidence", "{EVIDENCE}/SUITES/" + suite],
        "timeoutSecs": 3600,
        "evidencePaths": ["{EVIDENCE}/SUITES/" + suite + "/result.json",
                          "{EVIDENCE}/SUITES/" + suite + "/output.log"],
    }


def derive_ab_cases(out_dir: Path):
    """从 A/B 观测派生叶份额图钉：case=1 当且仅当双内核字段真值成立。"""
    try:
        a = json.loads((out_dir / "ab-node.json").read_text(encoding="utf-8"))["observations"]
        b = json.loads((out_dir / "ab-rust.json").read_text(encoding="utf-8"))["observations"]
    except Exception as error:  # noqa: BLE001 - 派生失败=全 0 图钉（响亮）
        print(f"ab cases derive failed: {error}")
        return {}

    def truthy(obs, task, field):
        return bool(obs.get(task, {}).get("observed", {}).get(field))

    def both(task, field):
        return 1 if (truthy(a, task, field) and truthy(b, task, field)) else 0

    a_list = a.get("sessions.list", {}).get("observed", {})
    b_list = b.get("sessions.list", {}).get("observed", {})
    pin, pin_b = (a.get("sessions.pin", {}).get("observed", {}),
                  b.get("sessions.pin", {}).get("observed", {}))
    todos, todos_b = (a.get("sessions.todos", {}).get("observed", {}),
                      b.get("sessions.todos", {}).get("observed", {}))
    memsnap = a.get("leaf.memory-snapshot", {}).get("observed", {})
    memsnap_b = b.get("leaf.memory-snapshot", {}).get("observed", {})
    authf = a.get("leaf.auth-folders", {}).get("observed", {})
    authf_b = b.get("leaf.auth-folders", {}).get("observed", {})
    rport = a.get("leaf.read-ports", {}).get("observed", {})
    rport_b = b.get("leaf.read-ports", {}).get("observed", {})
    pprj = a.get("leaf.pin.projection", {}).get("observed", {})
    pprj_b = b.get("leaf.pin.projection", {}).get("observed", {})
    awsp = a.get("leaf.archive.workspace", {}).get("observed", {})
    awsp_b = b.get("leaf.archive.workspace", {}).get("observed", {})
    fnd = a.get("leaf.find.in-session", {}).get("observed", {})
    fnd_b = b.get("leaf.find.in-session", {}).get("observed", {})
    pino = a.get("leaf.pin-order", {}).get("observed", {})
    pino_b = b.get("leaf.pin-order", {}).get("observed", {})
    adel = a.get("leaf.archived-delete", {}).get("observed", {})
    adel_b = b.get("leaf.archived-delete", {}).get("observed", {})
    srch = a.get("leaf.search", {}).get("observed", {})
    srch_b = b.get("leaf.search", {}).get("observed", {})
    archv = a.get("sessions.archive", {}).get("observed", {})
    archv_b = b.get("sessions.archive", {}).get("observed", {})
    maint, maint_b = (a.get("sessions.maintenance", {}).get("observed", {}),
                      b.get("sessions.maintenance", {}).get("observed", {}))
    perm = a.get("permissions.default", {}).get("observed", {})
    perm = {"mode": perm.get("mode")} if isinstance(perm, dict) else {}
    perm_b = b.get("permissions.default", {}).get("observed", {})
    perm_b = {"mode": perm_b.get("mode")} if isinstance(perm_b, dict) else {}
    cases = {
        "ab::sessions.list.count": 1 if (a_list.get("count", 0) > 0 and b_list.get("count", 0) > 0) else 0,
        "ab::sessions.list.parity": 1 if a_list.get("count") == b_list.get("count") else 0,
        "ab::sessions.rename.ok": both("sessions.rename", "ok"),
        "ab::sessions.rename.persisted": both("sessions.rename", "persisted"),
        "ab::sessions.pin.pin": 1 if (pin.get("pinA") and pin.get("pinB")
                                       and pin_b.get("pinA") and pin_b.get("pinB")) else 0,
        "ab::sessions.pin.unpin": 1 if (pin.get("unpinA") and pin.get("unpinB")
                                         and pin_b.get("unpinA") and pin_b.get("unpinB")) else 0,
        "ab::sessions.todos.complete": 1 if (todos.get("complete") and todos_b.get("complete")) else 0,
        "ab::sessions.todos.cancel": 1 if (todos.get("cancel") and todos_b.get("cancel")) else 0,
        "ab::sessions.todos.dismiss": 1 if (todos.get("dismiss") and todos_b.get("dismiss")) else 0,
        "ab::sessions.maintenance.cleanup": 1 if (maint.get("cleanupDeletedIsNumber")
                                                   and maint_b.get("cleanupDeletedIsNumber")) else 0,
        "ab::sessions.maintenance.sweep": 1 if (maint.get("sweptIsNumber")
                                                 and maint_b.get("sweptIsNumber")) else 0,
        "ab::permissions.default.parity": 1 if (perm.get("mode") == perm_b.get("mode")
                                                 and perm.get("mode") is not None) else 0,
        "leaf::permissions.mode-nonnull": 1 if (perm.get("mode") is not None and perm_b.get("mode") is not None) else 0,
    }
    neg = a.get("leaf.rename.negative", {}).get("observed", {})
    neg_b = b.get("leaf.rename.negative", {}).get("observed", {})
    pin_neg = a.get("leaf.pin.negative", {}).get("observed", {})
    pin_neg_b = b.get("leaf.pin.negative", {}).get("observed", {})
    todos_neg = a.get("leaf.todos.negative", {}).get("observed", {})
    todos_neg_b = b.get("leaf.todos.negative", {}).get("observed", {})
    clean = a.get("leaf.cleanup.boundary", {}).get("observed", {})
    clean_b = b.get("leaf.cleanup.boundary", {}).get("observed", {})
    fields = a.get("leaf.list.fields", {}).get("observed", {})
    fields_b = b.get("leaf.list.fields", {}).get("observed", {})
    pidem = a.get("leaf.permissions.read-idempotent", {}).get("observed", {})
    pidem_b = b.get("leaf.permissions.read-idempotent", {}).get("observed", {})
    aneg = a.get("leaf.archive.negative", {}).get("observed", {})
    aneg_b = b.get("leaf.archive.negative", {}).get("observed", {})
    psm = a.get("leaf.permission.session-mode", {}).get("observed", {})
    psm_b = b.get("leaf.permission.session-mode", {}).get("observed", {})
    pds = a.get("leaf.permission.default-set", {}).get("observed", {})
    pds_b = b.get("leaf.permission.default-set", {}).get("observed", {})
    mcp = a.get("leaf.mcp.state", {}).get("observed", {})
    mcp_b = b.get("leaf.mcp.state", {}).get("observed", {})
    mcpu = a.get("leaf.mcp.state.unauthorized", {}).get("observed", {})
    mcpu_b = b.get("leaf.mcp.state.unauthorized", {}).get("observed", {})
    cases.update({
        "leaf::rename.empty-title.rejected": 1 if (neg.get("emptyRejected") and neg_b.get("emptyRejected")) else 0,
        "leaf::rename.invalid-session.explicit-error": 1 if (neg.get("invalidRejected") and neg_b.get("invalidRejected")) else 0,
        "leaf::pin.invalid-session.explicit-error": 1 if (pin_neg.get("rejected") and pin_neg_b.get("rejected")) else 0,
        "leaf::todos.invalid-session.explicit-error": 1 if (todos_neg.get("rejected") and todos_neg_b.get("rejected")) else 0,
        "leaf::cleanup.fresh-archive-kept": 1 if (clean.get("freshKept") and clean_b.get("freshKept")) else 0,
        "leaf::cleanup.archived-delete-with-effect": 1 if (clean.get("purgedOk") and clean_b.get("purgedOk") and clean.get("expiringGone") and clean_b.get("expiringGone")) else 0,
        "leaf::list.revision-field-present": 1 if (fields.get("hasRevision") and fields_b.get("hasRevision")) else 0,
        "leaf::list.summary-or-badge-present": 1 if (fields.get("hasSummaryOrBadge") and fields_b.get("hasSummaryOrBadge")) else 0,
        "leaf::list.unauthorized-rejected": 1 if (a.get("error-path.unauthorized", {}).get("observed", {}).get("rejected")
                                                  and b.get("error-path.unauthorized", {}).get("observed", {}).get("rejected")) else 0,
        "leaf::permissions.read-idempotent": 1 if (pidem.get("identical") and pidem_b.get("identical") and pidem.get("mode") == pidem_b.get("mode")) else 0,
        "leaf::mcp.state.shape": 1 if (mcp.get("ok") and mcp_b.get("ok") and mcp.get("enabledIsBool")
                                        and mcp_b.get("enabledIsBool") and mcp.get("connectorsIsArray")
                                        and mcp_b.get("connectorsIsArray")) else 0,
        "leaf::mcp.state.unauthorized-rejected": 1 if (mcpu.get("rejected") and mcpu_b.get("rejected")) else 0,
        "leaf::permission.session-mode-present": 1 if (psm.get("modePresent") and psm_b.get("modePresent")) else 0,
        "leaf::permission.session-mode-idempotent": 1 if (psm.get("idempotent") and psm_b.get("idempotent")) else 0,
        "leaf::permission.default-roundtrip": 1 if (pds.get("roundTrip") and pds_b.get("roundTrip")) else 0,
        "leaf::permission.default-invalid-rejected": 1 if (pds.get("badRejected") and pds_b.get("badRejected")) else 0,
        "leaf::archive.negative.rejected": 1 if (aneg.get("archiveRejected") and aneg_b.get("archiveRejected")
                                                 and aneg.get("restoreRejected") and aneg_b.get("restoreRejected")) else 0,
        "leaf::memory.read": 1 if (memsnap.get("memoryRead") and memsnap_b.get("memoryRead")) else 0,
        "leaf::memory.set": 1 if (memsnap.get("memorySet") and memsnap_b.get("memorySet")) else 0,
        "leaf::prompt-snapshot.read": 1 if (memsnap.get("snapshotOk") and memsnap_b.get("snapshotOk")) else 0,
        "leaf::memory-snapshot.invalid-rejected": 1 if (memsnap.get("invalidRejected") and memsnap_b.get("invalidRejected")) else 0,
        "leaf::auth-folders.read": 1 if (authf.get("readOk") and authf_b.get("readOk")) else 0,
        "leaf::auth-folders.add": 1 if (authf.get("addOk") and authf_b.get("addOk")) else 0,
        "leaf::auth-folders.remove": 1 if (authf.get("removeOk") and authf_b.get("removeOk")) else 0,
        "leaf::auth-folders.set": 1 if (authf.get("setOk") and authf_b.get("setOk")) else 0,
        "leaf::auth-folders.invalid-rejected": 1 if (authf.get("invalidRejected") and authf_b.get("invalidRejected")) else 0,
        "leaf::read.history-page": 1 if (rport.get("historyOk") and rport_b.get("historyOk")) else 0,
        "leaf::read.overview": 1 if (rport.get("overviewOk") and rport_b.get("overviewOk")) else 0,
        "leaf::read.summary": 1 if (rport.get("summaryOk") and rport_b.get("summaryOk")) else 0,
        "leaf::read.invalid-rejected": 1 if (rport.get("invalidRejected") and rport_b.get("invalidRejected")) else 0,
        "leaf::pin.state-projected": 1 if (pprj.get("pinnedProjected") and pprj_b.get("pinnedProjected")) else 0,
        "leaf::pin.state-cleared": 1 if (pprj.get("clearedProjected") and pprj_b.get("clearedProjected")) else 0,
        "leaf::archive-workspace.recoverable": 1 if (awsp.get("archivedToRecoverable") and awsp_b.get("archivedToRecoverable")) else 0,
        "leaf::archive-workspace.invalid-rejected": 1 if (awsp.get("invalidRejected") and awsp_b.get("invalidRejected")) else 0,
        "leaf::find.in-session-hit": 1 if (fnd.get("hitAtLeastOne") and fnd_b.get("hitAtLeastOne")) else 0,
        "leaf::find.in-session-invalid-rejected": 1 if (fnd.get("invalidRejected") and fnd_b.get("invalidRejected")) else 0,
        "leaf::pin-order.reorder-applies": 1 if (pino.get("reorderOk") and pino_b.get("reorderOk")) else 0,
        "leaf::pin-order.duplicate-rejected": 1 if (pino.get("dupRejected") and pino_b.get("dupRejected")) else 0,
        "leaf::archived-delete.file-and-lifecycle-cleared": 1 if (adel.get("deleted") and adel_b.get("deleted")
                                                                       and adel.get("goneFromArchive") and adel_b.get("goneFromArchive")) else 0,
        "leaf::archived-delete.unrecoverable": 1 if (adel.get("restoreAfterDeleteRefused") and adel_b.get("restoreAfterDeleteRefused")) else 0,
        "leaf::search.content-hit": 1 if (srch.get("hitAtLeastOne") and srch_b.get("hitAtLeastOne")) else 0,
        "leaf::search.empty-query-empty-result": 1 if (srch.get("emptyIsEmpty") and srch_b.get("emptyIsEmpty")) else 0,
        "leaf::archive.positive-cycle": 1 if (archv.get("inArchive") and archv_b.get("inArchive")
                                                and archv.get("goneFromVisible") and archv_b.get("goneFromVisible")
                                                and archv.get("restored") and archv_b.get("restored")
                                                and archv.get("backVisible") and archv_b.get("backVisible")) else 0,
        "leaf::cleanup.expired-deleted": 1 if (maint.get("cleanupDeletedAtLeastOne") and maint_b.get("cleanupDeletedAtLeastOne")) else 0,
    })
    # R08 收尾泛化：观测含 cases:{name:bool} 的任务自动派生 leaf::<task>.<name>
    # （双腿同键均 true 才记 1；任一腿缺键/假/缺观测 = 0，响亮）。与演习比较器
    # casesParity 同一口径；新叶只需套件产出 cases + FULL_BINDINGS 登记。
    for task, obs_a in a.items():
        cases_a = (obs_a or {}).get("observed", {}).get("cases") if isinstance(obs_a, dict) else None
        if not isinstance(cases_a, dict):
            continue
        cases_b = (b.get(task) or {}).get("observed", {}).get("cases")
        cases_b = cases_b if isinstance(cases_b, dict) else {}
        for name in sorted(set(cases_a) | set(cases_b)):
            cases[f"leaf::{task}.{name}"] = 1 if (cases_a.get(name) is True and cases_b.get(name) is True) else 0
    return cases


def real_home_snapshot() -> dict:
    """只读采集真实 ~/.lingxi 摘要（A④ 前后比对共用同一形状）。"""
    home = Path.home() / ".lingxi"
    snap = {"schema": "lingxi.r08.real-home-snapshot.v1",
            "atUnixMs": int(time.time() * 1000), "path": str(home)}
    if not home.exists():
        snap["exists"] = False
        return snap
    snap["exists"] = True
    entries = {}
    for child in sorted(home.iterdir()):
        stat = child.stat()
        entries[child.name] = ("dir" if child.is_dir()
                               else f"file:{stat.st_size}:{stat.st_mtime_ns // 1_000_000}")
    snap["topLevel"] = entries
    stamp = home / "data-epoch.json"
    if stamp.exists():
        try:
            snap["stampSha256"] = hashlib.sha256(
                stamp.read_bytes()).hexdigest()
        except OSError as error:
            snap["stampError"] = str(error)
    return snap


def run_suite(suite: str, evidence: Path) -> int:
    """生产者模式：白名单校验 → 逐套件字面 argv（列表参数、shell=False）。"""
    if not re.fullmatch(r"[a-z0-9_]+", suite) or suite not in SUITE_REGISTRY:
        raise SystemExit(f"未登记或非法套件名: {suite}")
    evidence.mkdir(parents=True, exist_ok=True)
    scratch = Path(os.environ.get("LINGXI_R08_SUITE_RUNTIME", "/private/tmp/lingxi-r08-verify")) \
        / f"{suite}-{os.getpid()}-{int(time.time() * 1000)}"
    env = dict(os.environ)
    env.pop("ELECTRON_RUN_AS_NODE", None)
    started = time.time()
    if suite in CARGO_TEST_SUITES:
        env.update({"CARGO_TERM_COLOR": "never", "LINGXI_PRODUCT_DIR": str(ROOT / "lib"),
                    "TMPDIR": str(scratch), "LINGXI_HOME": str(scratch / "home")})
        (scratch / "home").mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(["cargo", "test", "--manifest-path", "rust/Cargo.toml",
                               "--locked", "-p", "lingxi-service", "--test", suite],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_registry_amend_proof":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t06_registry_amend.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--scratch", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t06_d1":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t06_d1_drill.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--work", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t06_d2":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t06_d2_drill.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--scratch", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t06_d3":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t06_d3_drill.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--work", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t07_real_format":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t07_real_format_probe.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--work", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t07_ab":
        suite_files = ",".join([MAIN_AB_SUITE, *AB_EXTRA_SUITES])
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t07_ab_drill.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--work", scratch.as_posix(),
                               "--suite-files", suite_files],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t03_a05_drill":
        scratch.mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t03_a05_drill.mjs", "--out",
                               scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t03_a06_drill":
        scratch.mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t03_a06_drill.mjs", "--out",
                               scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_ui_pages":
        # 客户端 UI 页份额：生产者脚本跑选定 vitest 文件并写 ui-cases.json
        # （lingxi.leaf-case-results.v1）；任何映射缺失/失败/跳过 → 非 0。
        # A15 V4：子进程 argv 不得含真实 home/仓库工作树路径（证据目录在仓库内），
        # 故生产者输出落合成 scratch，跑完再把产物拷入证据目录。
        ui_scratch = scratch / "ui-pages-out"
        ui_scratch.mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_ui_pages.mjs", "--out", ui_scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
        evidence.mkdir(parents=True, exist_ok=True)
        for name in ("ui-cases.json",):
            source = ui_scratch / name
            if source.is_file():
                shutil.copy2(source, evidence / name)
        for sub in ("raw", "live"):
            source_dir = ui_scratch / sub
            if source_dir.is_dir():
                shutil.copytree(source_dir, evidence / sub, dirs_exist_ok=True)
    elif suite == "r08_a15_isolation":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t08_isolation_check.mjs", "--out",
                               evidence.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_a16_versions":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t08_version_manifest.mjs", "--out",
                               evidence.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_desktop_boundary_vitest":
        # A②：真实服务往返测试的等值门槛变量必须注入——缺变量时该关键
        # 测试会 skipIf 静默跳过而套件仍退出 0（verify-r08-final-15 实证）。
        env["LINGXI_R07_SERVICE_BIN"] = str(ROOT / "rust/target/debug/lingxi-service")
        proc = subprocess.run(["npx", "vitest", "run",
                               "desktop/src/react/__tests__/host/r08-a03-web-desktop-host.test.tsx",
                               "desktop/src/react/__tests__/services/r08-a01-ui-projection.test.ts",
                               "desktop/src/react/__tests__/services/r08-p1-data-epoch-hello.test.ts",
                               "desktop/src/react/__tests__/services/r08-session-legacy-compat.test.ts",
                               # R08 收尾 C①/C② 真实 Rust 子进程验收（置顶投影形状 / 续接+处置）。
                               "desktop/src/react/__tests__/services/r08-pin-projection-compat.test.ts",
                               "desktop/src/react/__tests__/services/r08-continue-disposal-compat.test.ts",
                               "desktop/src/react/__tests__/hooks/use-hana-fetch.test.ts"],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_a04_ui_vitest":
        proc = subprocess.run(["npx", "vitest", "run",
                               "desktop/src/react/__tests__/components/r08-a04-card-expansion.test.tsx",
                               "--reporter=verbose"],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_cli_data_vitest":
        # D20：CLI data 族（diagnose/checkpoints/restore）产品测试逐用例合同。
        proc = subprocess.run(["npx", "vitest", "run", "tests/cli-data.test.ts",
                               "--reporter=verbose"],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_registry_consistency":
        proc = subprocess.run(["python3", "scripts/rust-tauri/r02_registry_consistency.py"],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    else:
        raise SystemExit(f"套件 {suite} 在注册表内但无分支（登记缺口）")
    log = proc.stdout + proc.stderr
    (evidence / "output.log").write_text(log, encoding="utf-8")
    guard_failures = []
    # A② vitest 关键 skip 防护：验收套件里的 skipped 测试一律失败——关键
    # 检查要么真实执行要么响亮失败，不允许静默跳过后以 0 退出冒充通过。
    if suite in {"r08_desktop_boundary_vitest", "r08_a04_ui_vitest", "r08_cli_data_vitest"}:
        skipped = re.findall(r"\| (\d+) skipped", log)
        if any(int(n) > 0 for n in skipped) or re.search(r"^\s*↓ ", log, re.M):
            guard_failures.append("vitest suite contains skipped tests")
    if suite == "r08_a04_ui_vitest":
        # A04 组件腿逐测试合同：verbose 报告的每个测试必须 ✓ 出现且非跳过；
        # 写入 ui-cases.json（lingxi.leaf-case-results.v1）供场景合同核对。
        A04_UI_TESTS = {
            "a04::fixture-is-honest-capture": "夹具即服务捕获：无最终回复、失败工具与诚实终态逐字段可查",
            "a04::projection-real-blocks": "真实投影：过程块成卡、缺最终回复结果块上答案面",
            "a04::expand-and-click-all-viewable": "展开汇总区再逐卡点击：错误/思考/产物全部可查，收折不退化",
        }
        # vitest verbose 行形如 ` ✓ <file> > <describe> > <test> 29ms`（无括号）；
        # 取去掉尾部时长后最后一个 " > " 之后的测试名。
        ran = set()
        for line in log.splitlines():
            stripped = line.strip()
            if not stripped.startswith("✓"):
                continue
            stripped = re.sub(r"\s+\d+(?:\.\d+)?m?s$", "", stripped)
            if " > " in stripped:
                ran.add(stripped.rsplit(" > ", 1)[1].strip())
        rows = []
        for case, test_name in sorted(A04_UI_TESTS.items()):
            hit = test_name in ran
            rows.append({"case": case, "expect": 1, "actual": 1 if hit else 0, "ok": hit})
            if not hit:
                guard_failures.append(f"a04 ui test not green: {case}")
        (evidence / "ui-cases.json").write_text(
            json.dumps({"schema": "lingxi.leaf-case-results.v1", "cases": rows},
                       ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    if suite == "r08_ui_pages":
        cases_path = evidence / "ui-cases.json"
        if not cases_path.is_file():
            guard_failures.append("ui pages producer produced no ui-cases.json")
        else:
            try:
                ui_rows = json.loads(cases_path.read_text(encoding="utf-8")).get("cases", [])
            except Exception as error:  # noqa: BLE001
                ui_rows = []
                guard_failures.append(f"ui-cases.json unreadable: {error}")
            for row in ui_rows:
                if row.get("ok") is not True:
                    guard_failures.append(f"ui case not green: {row.get('case')}")
            if not ui_rows:
                guard_failures.append("ui-cases.json has no cases")
    if suite == "r08_cli_data_vitest":
        # D20：CLI data 族逐测试合同（tests/cli-data.test.ts，产品测试即验收
        # 证据——每个案例钉一个用例名，verbose 报告必须 ✓ 且非跳过）。
        CLI_DATA_TESTS = {
            "cli::checkpoints.list-complete": "lists complete checkpoints with transitionId, epoch range, createdAt, store count, and size",
            "cli::checkpoints.skips-staging-quarantine": "skips .tmp-* and .invalid-* directories and reports them as a count instead of listing them",
            "cli::restore.non-tty-no-token-refused": "refuses on a non-TTY without --confirm-token, without calling restore",
            "cli::restore.wrong-token-refused": "refuses on a non-TTY when --confirm-token does not exactly match, without calling restore",
            "cli::restore.tty-mismatch-aborts": "aborts on a TTY when the retyped confirmation does not match, without calling restore",
            "cli::restore.unknown-transition-errored": "errors clearly when the transitionId has no checkpoint on disk",
            "cli::restore.executes-with-exact-token": "restores end-to-end with the exact confirm token on a non-TTY, using the real provider and restore transaction",
            "cli::diagnose.clean-home": "reports a clean, unstamped home",
            "cli::diagnose.in-progress-journal": "reports an in-progress transition journal with its continuation suggestion",
            "cli::diagnose.available-checkpoint": "reports an available checkpoint",
            "cli::diagnose.zero-write": "never writes to the home directory across all three states",
            "cli::diagnose.corrupt-reported-not-repaired": "reports a corrupt stamp explicitly and leaves the home untouched (no repair, no fresh store)",
        }
        ran = set()
        for line in log.splitlines():
            stripped = line.strip()
            if not stripped.startswith("✓"):
                continue
            stripped = re.sub(r"\s+\d+(?:\.\d+)?m?s$", "", stripped)
            if " > " in stripped:
                ran.add(stripped.rsplit(" > ", 1)[1].strip())
        cli_rows = []
        for case, test_name in sorted(CLI_DATA_TESTS.items()):
            hit = test_name in ran
            cli_rows.append({"case": case, "expect": 1, "actual": 1 if hit else 0, "ok": hit})
            if not hit:
                guard_failures.append(f"cli data test not green: {case}")
        (evidence / "cli-cases.json").write_text(
            json.dumps({"schema": "lingxi.leaf-case-results.v1", "cases": cli_rows},
                       ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    if suite == "r08_t07_ab":
        # 门合同格式 lingxi.leaf-case-results.v1：每案例 {case,expect,actual,ok}
        # （expect 钉 1=双内核断言成立；actual=实测；ok=expect==actual）。
        derived = derive_ab_cases(scratch / "out")
        # 业主裁决口径（CLOSEOUT_ADJUDICATIONS.md）：登记为 Rust-authoritative 的
        # 案例以 Rust 腿（ab-rust.json）真值为准；旧栈腿观测如实保留在
        # cases.json 的观测源文件中，此处按裁决口径计值。
        try:
            rust_only = json.loads((scratch / "out" / "ab-rust.json").read_text(encoding="utf-8"))["observations"]
        except Exception as error:  # noqa: BLE001 - 派生失败按缺失计 0（响亮）
            print(f"rust-leg observations unreadable: {error}")
            rust_only = {}
        for _leaf_id, (_adj, pairs) in ADJUDICATED_RUST_AUTHORITATIVE.items():
            for task, name in pairs:
                obs = (rust_only.get(task) or {}).get("observed") or {}
                value = ((obs.get("cases") or {}).get(name) is True)
                derived[f"leaf::{task}.{name}"] = 1 if value else 0
        # 扩展套件任务必须真实产出观测（缺=套件没跑/被 -t 过滤/崩溃）：响亮失败，
        # 不允许叶合同因缺观测悄悄变 0 而被当成「未通过」蒙混过去。
        try:
            node_obs = json.loads((scratch / "out" / "ab-node.json").read_text(encoding="utf-8"))["observations"]
            rust_obs = json.loads((scratch / "out" / "ab-rust.json").read_text(encoding="utf-8"))["observations"]
        except Exception as error:  # noqa: BLE001
            node_obs, rust_obs = {}, {}
            guard_failures.append(f"A/B 合并观测不可读: {error}")
        for extra_task in EXPECTED_EXTRA_TASKS:
            for leg_name, leg_obs in (("node", node_obs), ("rust", rust_obs)):
                if extra_task not in (leg_obs or {}):
                    guard_failures.append(f"A/B 扩展任务缺观测: {leg_name}:{extra_task}")
        # 旁证 cargo 案例聚合（同场景先行套件的日志；fail-closed）。
        for case, (side_suite, test_name) in CARGO_SIDE_CASES.items():
            side_log = evidence.parent / side_suite / "output.log"
            ok = False
            try:
                ok = any(line.strip() == f"test {test_name} ... ok"
                         for line in side_log.read_text(encoding="utf-8").splitlines())
            except OSError:
                ok = False
            derived[case] = 1 if ok else 0
        # 合同证据文件只收录阶段图钉住的案例（门规：整份记录必须全部成立）。
        # 未钉案例（如 cleanup 期限边界——D-6 真差异）不入此文件，其差异在
        # T07_UI_DIFFERENCE_REPORT.md / FEATURE_PARITY_RESULTS.json 如实披露。
        pinned = CONTRACT_PINNED_CASES
        rows = [{"case": case, "expect": 1, "actual": derived.get(case, 0), "ok": derived.get(case, 0) == 1}
                for case in sorted(pinned)]
        (evidence / "cases.json").write_text(
            json.dumps({"schema": "lingxi.leaf-case-results.v1", "cases": rows},
                       ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        # A③：A/B 演习业务判定进场景合同——ab-parity-cases.json 逐任务记录
        # parity=1；recorded 须在 RECORDED_APPROVALS 登记方计 1（未批准差异
        # 或缺观测=0 → A13/A14 FAIL）。任务集=演习 RULES 全集，缺一即 0。
        parity_rows = []
        compare_path = scratch / "out" / "ab-compare.json"
        verdicts = {}
        try:
            compare_doc = json.loads(compare_path.read_text(encoding="utf-8"))
            verdicts = {task: row.get("verdict")
                        for task, row in compare_doc.get("tasks", {}).items()}
        except Exception as error:  # noqa: BLE001 - 解析失败全任务记 0（响亮）
            guard_failures.append(f"ab-compare.json unreadable: {error}")
        for task in sorted(AB_COMPARE_RULES):
            verdict = verdicts.get(task)
            if verdict == "parity":
                ok = True
            elif verdict == "recorded" and task in RECORDED_APPROVALS:
                ok = True
            else:
                ok = False
                if verdict is None:
                    guard_failures.append(f"ab task missing observation: {task}")
                elif verdict not in ("recorded",):
                    guard_failures.append(f"ab task differs/unapproved: {task}={verdict}")
                else:
                    guard_failures.append(f"ab task recorded without approval: {task}")
            parity_rows.append({"case": f"ab::{task}", "expect": 1,
                                "actual": 1 if ok else 0, "ok": ok})
        (evidence / "ab-parity-cases.json").write_text(
            json.dumps({"schema": "lingxi.leaf-case-results.v1", "cases": parity_rows,
                        "recordedApprovals": RECORDED_APPROVALS},
                       ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    receipt = {
        "schema": "lingxi.r08.suite-receipt.v1",
        "suite": suite,
        "exitCode": proc.returncode,
        "guardFailures": guard_failures,
        "argv": list(proc.args),
        "durationSecs": round(time.time() - started, 3),
        "logSha256": hashlib.sha256(log.encode()).hexdigest(),
    }
    (evidence / "result.json").write_text(
        json.dumps(receipt, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    final_code = proc.returncode if not guard_failures else (proc.returncode or 1)
    print(f"suite {suite}: exitCode={proc.returncode} guardFailures={len(guard_failures)}")
    for failure in guard_failures:
        print(f"GUARD_FAIL {suite}: {failure}")
    return final_code


def main() -> int:
    if "--run-suite" in sys.argv:
        suite = sys.argv[sys.argv.index("--run-suite") + 1]
        evidence = Path(sys.argv[sys.argv.index("--evidence") + 1])
        return run_suite(suite, evidence)

    acceptance = load(ACCEPTANCE_PATH)["scenarios"]
    features = {row["id"]: row for row in load(FEATURE_PATH)["supplemental_scenarios"]}

    base = {k: v for k, v in acceptance.items()
            if v.get("kind") == "base" and v.get("stage_id") == "R08"}
    if sorted(base) != [f"R08-A{i:02}" for i in range(1, 17)]:
        raise SystemExit("R08 基础场景集合与 R00 原账不一致")

    leaves = {k: v for k, v in acceptance.items()
              if v.get("kind") == "supplemental" and "R08" in (v.get("execution_stage_ids") or [])}
    fsa_r08 = {k: v for k, v in features.items() if "R08" in (v.get("execution_stage_ids") or [])}
    if set(leaves) != set(fsa_r08) or len(leaves) != 126:
        raise SystemExit(f"R08 补充叶集合在两本原账间不一致或数量不为 126（{len(leaves)}/{len(fsa_r08)}）")

    for task, suites in TASK_SUITES.items():
        for suite in suites:
            if suite not in SUITE_REGISTRY:
                raise SystemExit(f"{task} 登记的套件不在注册表: {suite}")

    commands = {
        "r08_inputs_check": {
            "argv": ["python3", "-B", "scripts/rust-tauri/r08_stage_inputs.py",
                     "--check", "--evidence", "{EVIDENCE}/R08_INPUTS"],
            "timeoutSecs": 120,
            "evidencePaths": ["{EVIDENCE}/R08_INPUTS/inputs-check.json"],
        },
    }
    scenarios = []
    for aid in sorted(base):
        spec = base[aid]["spec"]
        task_ids = base[aid]["task_ids"]
        refs = []
        if aid == "R08-A01":
            refs.append("r08_inputs_check")
        for task in task_ids:
            for suite in TASK_SUITES.get(task, []):
                commands.setdefault(suite, command_for_suite(suite))
                refs.append(suite)
        # 场景→套件精确化：A05 的验收证据就是 a05 演习（读/写腿 + PPG-1 呈现），
        # A06 的证据就是 a06 演习（双客户端 remote 零接触）。按任务粗绑定会把
        # a06 的环境红拖垮 A05——本阶段各自专属（gate-05 实证）。
        if aid == 'R08-A05':
            refs = ['r08_t03_a05_drill']
        if aid == 'R08-A06':
            refs = ['r08_t03_a06_drill']
        # A04 精确绑定（收尾任务A①）：A04 的证据 = Rust 端到端套件（真实
        # WS/替身供应商）+ 桌面卡片展开组件验收；两者此前只注册/不执行。
        if aid == 'R08-A04':
            for a04_suite in ("r08_a04_no_final_ws", "r08_a04_ui_vitest"):
                commands.setdefault(a04_suite, command_for_suite(a04_suite))
            refs = ['r08_a04_no_final_ws', 'r08_a04_ui_vitest']
        scenario = {
            "id": aid,
            "requirement": "REQUIRED",
            "commandRefs": refs,
            "specMirror": {
                "name": spec["name"], "given": spec["given"], "when": spec["when"],
                "then": spec["then"], "evidence": spec["evidence"],
                "environment": spec["environment"], "taskIds": task_ids,
                "featureIds": base[aid]["feature_ids"],
                "ledgerStatus": base[aid]["ledger_status"],
            },
            "note": "命令全绿只证明套件通过；逐断言合同缺位时该场景不得判正式完成。",
        }
        # A04 测试锚点：Rust 端到端关键测试必须本轮真实出现在套件日志且 ok
        # （cargo 日志格式 `test <name> ... ok`）。组件腿由 ui-cases.json 合同覆盖。
        if aid == 'R08-A04':
            scenario["suiteTestAnchors"] = [
                {"command": "r08_a04_no_final_ws",
                 "test": "a04_tool_failure_then_empty_final_records_honest_terminal"},
            ]
            scenario["assertionContract"] = {
                "producerCommand": "r08_a04_ui_vitest",
                "evidencePath": "{EVIDENCE}/SUITES/r08_a04_ui_vitest/ui-cases.json",
                "cases": [
                    {"case": "a04::fixture-is-honest-capture", "expect": 1},
                    {"case": "a04::projection-real-blocks", "expect": 1},
                    {"case": "a04::expand-and-click-all-viewable", "expect": 1},
                ],
            }
        # A13/A14 逐断言合同（收尾任务A③）：A/B 演习的业务差异与缺前提
        # 进入场景判定——parity 或已登记 recorded 记 1，未批准差异/缺观测
        # 记 0，场景 FAIL。合同证据 ab-parity-cases.json 由 r08_t07_ab
        # 生产者本轮写入（35 任务逐条）。
        if aid == 'R08-A13':
            # C③ 旁证 cargo 套件先行（r08_t07_ab 聚合其日志为叶案例）；
            # D20 CLI data 族产品测试套件同场景先行（叶证据独立于 A/B 双内核面，
            # 挂此场景仅为获得门内执行——语义是原 CLI 责任的忠实验收）。
            commands.setdefault("r08_c3_find_bridge_redaction", command_for_suite("r08_c3_find_bridge_redaction"))
            commands.setdefault("r08_cli_data_vitest", command_for_suite("r08_cli_data_vitest"))
            # 客户端 UI 页份额证据（聊天/地图/设置/快捷窗口）同场景执行。
            commands.setdefault("r08_ui_pages", command_for_suite("r08_ui_pages"))
            refs = ["r08_c3_find_bridge_redaction", "r08_cli_data_vitest", "r08_ui_pages", *refs]
            scenario["commandRefs"] = refs
            scenario["suiteTestAnchors"] = [
                {"command": "r08_c3_find_bridge_redaction",
                 "test": "c3_find_redacts_bridge_time_tags_only_for_bridge_bound_sessions"},
            ]
            scenario["assertionContract"] = {
                "producerCommand": "r08_t07_ab",
                "evidencePath": "{EVIDENCE}/SUITES/r08_t07_ab/ab-parity-cases.json",
                "cases": [{"case": f"ab::{task}", "expect": 1}
                          for task in sorted(AB_COMPARE_RULES)],
            }
        if aid == 'R08-A14':
            # 「失败不靠隐藏功能消失」：错误路径必须双侧显式拒绝（负路径任务集）。
            negative_tasks = [t for t in AB_COMPARE_RULES
                              if "unauthorized" in t or "not-found" in t or "negative" in t
                              or "rejected" in t]
            scenario["assertionContract"] = {
                "producerCommand": "r08_t07_ab",
                "evidencePath": "{EVIDENCE}/SUITES/r08_t07_ab/ab-parity-cases.json",
                "cases": [{"case": f"ab::{task}", "expect": 1}
                          for task in sorted(negative_tasks)],
            }
        scenarios.append(scenario)

    leaf_rows = []
    for lid in sorted(leaves):
        src = leaves[lid]
        fsa = fsa_r08[lid]
        row = {
            "id": lid,
            "featureId": src["feature_id"],
            "requirement": src["requirement"],
            "basisKind": "required_behavior_pending",
            "r00Kind": src["kind"],
            "r00TaskIds": src["task_ids"],
            "r00ExecutionStageIds": src["execution_stage_ids"],
            "r00LedgerStatus": src["ledger_status"],
            "r00ResultIds": src["result_ids"],
            "r00TestIds": src["test_ids"],
            "r00Then": src["then"],
            "r00Assertions": fsa["assertions"],
            "r00Due": fsa["due"],
            "evidenceCommandRefs": [],
            "evidencePaths": [],
            "evidenceRequired": "保持原条款与原断言；每条原断言须绑定本阶段真实专属正反行为证据后方可判完成。",
        }
        full = FULL_BINDINGS.get(lid)
        if full is not None and len(full["assertion_cases"]) == len(fsa["assertions"]):
            row["basisKind"] = "full_original_behavior"
            producer = full.get("producer", "r08_t07_ab")
            evidence_file = full.get("evidence_file", "cases.json")
            row["evidenceCommandRefs"] = [producer]
            row["evidencePaths"] = [f"{{EVIDENCE}}/SUITES/{producer}/{evidence_file}"]
            row["evidenceRequired"] = "逐断言专属案例全覆盖（正路径平价 + 负路径双内核显式拒绝），机器核对 cases.json。"
            adj_note = {
                "R00-T02-LA-485B2D0A8A02": "【ADJ-4（2026-10-26 业主裁决）】旧栈 desktop /new phase-1 不支持（帧原文在案）；验收面以 Rust 真实实现为准（轮换+归档+可恢复+回执），案例判真取 Rust 腿。见 CLOSEOUT_ADJUDICATIONS.md。",
                "R00-T02-LA-3FCB259FF69C": "【ADJ-5（2026-10-26 业主裁决）】旧栈 desktop /reset 清史不生效（三实测轮在案）；验收面以 Rust 真实实现为准（清史+回执+他话不受扰），案例判真取 Rust 腿。见 CLOSEOUT_ADJUDICATIONS.md。",
                "R00-T02-LA-25C4FF66FEE5": "【ADJ-1（2026-10-26 业主裁决：改说明书）】原文『未设置时为 ASK』与两内核实际行为（auto，旧栈路由兜底 ASK 实测不触发）不一致；语义订正为『返回当前 permissionMode（未设置→auto）』，权威出处 CLOSEOUT_ADJUDICATIONS.md；R00 冻结文件不动。",
            }.get(lid)
            if adj_note:
                row["evidenceRequired"] = adj_note
            flat = [c for group in full["assertion_cases"] for c in group]
            row["assertionContract"] = {
                "producerCommand": producer,
                "evidencePath": f"{{EVIDENCE}}/SUITES/{producer}/{evidence_file}",
                "cases": [{"case": c, "expect": 1} for c in flat],
            }
            row["originalAssertionCases"] = full["assertion_cases"]
        else:
            # 门规（R05 RR1 F25）：R08 是末位阶段，份额拆分无 later stage 承载 →
            # 保持 required_behavior_pending；已实测的正路径平价份额记入注记
            # （非门禁）；升级 full_original_behavior 须逐断言专属案例（在 FULL_BINDINGS）。
            pin = SHARE_PINS.get(lid)
            if pin is not None:
                row["evidenceRequired"] = (
                    "保持原条款与原断言；每条原断言须绑定本阶段真实专属正反行为证据后方可判完成。"
                    f"【已实测份额（非门禁）】{pin['share']}；图钉见 r08_t07_ab cases.json："
                    + ", ".join(case for case, _ in pin["cases"]))
        leaf_rows.append(row)

    inputs = {
        "schema": "lingxi.r08.stage-inputs.v1",
        "stage": "R08",
        "baseScenarios": [{"id": s["id"], "specMirror": s["specMirror"]} for s in scenarios],
        "requiredLeaves": [{"id": l["id"], "taskIds": l["r00TaskIds"],
                            "executionStageIds": l["r00ExecutionStageIds"]} for l in leaf_rows],
        "currentStatus": "IMPLEMENTATION_REGISTERED_ACCEPTANCE_PARTIAL",
        "coverageHonesty": "9/126 叶绑定本阶段份额图钉（A/B 正路径平价）；117 叶 required_behavior_pending；"
                           "A05 呈 PPG-1/PPG-2 双 BLOCKED（T03 裁决）；D-4 差异未决（T07 差异报告）。",
    }
    graph = {
        "schemaVersion": 1,
        "stage": "R08",
        "resultVersion": "lingxi.xtask.verify-stage.v1",
        "defaultTimeoutSecs": 3600,
        "commands": commands,
        "scenarios": scenarios,
        "supplementalLeafScenarios": leaf_rows,
        "inputDigestSha256": sha(inputs),
    }
    # A② 结构完整性：必需命令不得只注册不引用（门只执行被引用命令——
    # 未引用命令等于从未验收）；每个场景必须至少引用一条命令。
    referenced = {ref for s in scenarios for ref in s["commandRefs"]}
    referenced |= {ref for l in leaf_rows for ref in l["evidenceCommandRefs"]}
    unreferenced = sorted(set(commands) - referenced)
    if unreferenced:
        raise SystemExit(f"命令只注册未引用（不会被门执行）: {unreferenced}")
    missing_commands = sorted(referenced - set(commands))
    if missing_commands:
        raise SystemExit(f"场景/叶引用了未注册命令: {missing_commands}")
    empty_scenarios = [s["id"] for s in scenarios if not s["commandRefs"]]
    if empty_scenarios:
        raise SystemExit(f"场景未引用任何命令: {empty_scenarios}")

    out_map = "--check" not in sys.argv
    out_dir = (ROOT / MAP_PATH).parent
    if "--out" in sys.argv:
        out_dir = Path(sys.argv[sys.argv.index("--out") + 1])
    if out_map:
        (out_dir / INPUT_PATH.name).write_text(json.dumps(inputs, ensure_ascii=False, indent=2) + "\n")
        (out_dir / MAP_PATH.name).write_text(json.dumps(graph, ensure_ascii=False, indent=2) + "\n")
        pending = sum(1 for l in leaf_rows if l["basisKind"] == "required_behavior_pending")
        print(f"R08 map written: {len(commands)} commands, {len(scenarios)} scenarios, "
              f"{len(leaf_rows)} leaves ({len(leaf_rows) - pending} share-bound, {pending} pending)")
    else:
        digest = sha(inputs)
        current = json.loads((ROOT / MAP_PATH).read_text(encoding="utf-8"))
        ok = current.get("inputDigestSha256") == digest
        # A② 深化自检：除输入摘要外，整张阶段图必须与「从冻结原账重新生成」
        # 的结果逐字节一致——手改地图（放宽绑定/删合同）在此直接失败。
        regenerated = json.dumps(graph, ensure_ascii=False, indent=2) + "\n"
        committed = (ROOT / MAP_PATH).read_text(encoding="utf-8")
        graph_match = regenerated == committed
        ok = ok and graph_match
        if "--evidence" in sys.argv:
            receipt_dir = Path(sys.argv[sys.argv.index("--evidence") + 1])
            receipt_dir.mkdir(parents=True, exist_ok=True)
            (receipt_dir / "inputs-check.json").write_text(json.dumps({
                "schema": "lingxi.r08.inputs-check.v1",
                "digestMatch": ok,
                "expectedDigestSha256": digest,
                "mapDigestSha256": current.get("inputDigestSha256"),
                "graphByteMatch": graph_match,
            }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
            # A④ 受控前置快照（只读）：本轮门运行开始时的真实 ~/.lingxi 摘要，
            # 供 a15 隔离检查做事后比对。不伪造历史基线——此前轮次无快照即如实
            # 从本轮开始。读取失败按 fail-closed 处理（隔离不可证=不通过）。
            before = real_home_snapshot()
            (receipt_dir / "real-home-before.json").write_text(
                json.dumps(before, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print("inputs digest " + ("match" if ok else "MISMATCH")
              + ("" if graph_match else "; graph bytes MISMATCH"))
        return 0 if ok else 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
