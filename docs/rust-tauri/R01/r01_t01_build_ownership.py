#!/usr/bin/env python3
"""R01-T01 ownership-table generator.

Regenerates docs/rust-tauri/R01/OWNERSHIP_TARGET.json deterministically
from the frozen R00 inventories:

  - docs/rust-tauri/R00/FEATURE_INVENTORY.json (736 production leaf F-IDs)
  - docs/rust-tauri/R00/STORES.json            (69 registered stores)

Design rules (taskbook R01 §4 R01-T01 / 02_目标架构与强制契约):

  * Every F-ID and every store gets EXACTLY ONE target owner. Multi-surface
    R00 labels such as "Rust Run/Session＋React" are resolved to the single
    business-authority component; the other surfaces (React UI, workers,
    hosts) are consumers/executors, never co-owners.
  * Peripheral workers never own run terminal state, approval decisions,
    context assembly or model loops (enforced by r01_t01_check_ownership.py
    via critical_facts + owner kinds, not by convention).
  * The mapping tables below are explicit and closed: an unmapped F-ID
    target_owner string or store id aborts generation (no silent default).

Usage:
  python3 -B docs/rust-tauri/R01/r01_t01_build_ownership.py            # write
  python3 -B docs/rust-tauri/R01/r01_t01_build_ownership.py --check    # drift check, exit 1 on diff

This script only reads R00 artifacts and writes its one output file.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
FEATURE_INVENTORY = REPO_ROOT / "docs/rust-tauri/R00/FEATURE_INVENTORY.json"
STORES = REPO_ROOT / "docs/rust-tauri/R00/STORES.json"
OUTPUT = REPO_ROOT / "docs/rust-tauri/R01/OWNERSHIP_TARGET.json"

SCHEMA_VERSION = 1

# ---------------------------------------------------------------------------
# Owner registry: controlled vocabulary. `kind` drives the machine checks:
#   core        - lingxi-protocol / lingxi-kernel (no desktop deps allowed)
#   adapters    - lingxi-adapters (implements kernel ports)
#   service     - lingxi-service (composition root / auth / transport)
#   cli         - lingxi-cli
#   xtask       - build/test orchestration
#   host        - Tauri desktop host + OS helpers (never business authority)
#   ui          - React presentation (consumer only, never an owner of facts)
#   worker      - peripheral workers (restricted; never critical-fact owner)
#   build       - build/sign/update tooling
# ---------------------------------------------------------------------------
OWNER_REGISTRY = [
    # --- lingxi-kernel components (domain authority) ---
    {"owner_id": "kernel.run-supervisor", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "Run/Task 状态机、终态、attempt/generation 栅栏、子代理监督、协作运行边界",
     "establish_stage": "R01-T01(crate)/R03(实现)"},
    {"owner_id": "kernel.session", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "会话身份/分支/规范化消息领域规则与统一投影语义",
     "establish_stage": "R01-T01(crate)/R06(实现)"},
    {"owner_id": "kernel.context", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "上下文组装/预算/压缩、人格与 MOOD 语义、goal/上下文笔记",
     "establish_stage": "R01-T01(crate)/R06(实现)"},
    {"owner_id": "kernel.tool-gateway", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "工具目录注册、PreparedInvocation、调用生命周期与内置插件登记",
     "establish_stage": "R01-T01(crate)/R04(实现)"},
    {"owner_id": "kernel.policy", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "权限模式、批准决定、资源范围硬规则、安全审计语义",
     "establish_stage": "R01-T01(crate)/R04(实现)"},
    {"owner_id": "kernel.scheduler", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "调度决策、触发收据去重、cron/loop/心跳语义",
     "establish_stage": "R01-T01(crate)/R03+R07(实现)"},
    {"owner_id": "kernel.memory", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "记忆领域规则（事实/总结/信条/梦境）",
     "establish_stage": "R01-T01(crate)/R06(实现)"},
    {"owner_id": "kernel.knowledge", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "知识库领域规则（导入/向量/检索/引用）；解析执行委托受控 worker",
     "establish_stage": "R01-T01(crate)/R06(实现)"},
    {"owner_id": "kernel.resource", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "真实文件/SessionFile/Resource 领域规则与授权",
     "establish_stage": "R01-T01(crate)/R04+R06(实现)"},
    {"owner_id": "kernel.model-gateway", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "模型/凭证路由决策、modelCall 唯一记录、用量与因果 trace",
     "establish_stage": "R01-T01(crate)/R05(实现)"},
    {"owner_id": "kernel.skill", "module": "lingxi-kernel", "kind": "core",
     "responsibility": "技能/角色卡配置领域规则（安装审核/启停/同步）；解析委托受控 worker",
     "establish_stage": "R01-T01(crate)/R06+R07(实现)"},
    # --- lingxi-adapters (implements kernel ports; never imported by kernel) ---
    {"owner_id": "adapters.storage", "module": "lingxi-adapters", "kind": "adapters",
     "responsibility": "运行/消息等持久化引擎实现（SQLite 等），由 service 注入",
     "establish_stage": "R02"},
    {"owner_id": "adapters.models", "module": "lingxi-adapters", "kind": "adapters",
     "responsibility": "provider 协议适配（流/非流/工具块），凭证经 service 解析",
     "establish_stage": "R05"},
    {"owner_id": "adapters.tools", "module": "lingxi-adapters", "kind": "adapters",
     "responsibility": "文件/命令/PTY 工具执行与托管运行时缓存",
     "establish_stage": "R04"},
    {"owner_id": "adapters.browser", "module": "lingxi-adapters", "kind": "adapters",
     "responsibility": "BrowserPort 实现，驱动独立浏览器宿主；宿主本身不拥有业务事实",
     "establish_stage": "R01-T04(spike)/R07"},
    {"owner_id": "adapters.integrations", "module": "lingxi-adapters", "kind": "adapters",
     "responsibility": "Bridge/MCP/插件集成适配；平台 SDK 编解码委托受控 worker",
     "establish_stage": "R07"},
    # --- lingxi-service (composition root) ---
    {"owner_id": "service.composition", "module": "lingxi-service", "kind": "service",
     "responsibility": "组合根、配置、进程生命周期、数据 epoch 与启动门禁、运行诊断",
     "establish_stage": "R02"},
    {"owner_id": "service.auth", "module": "lingxi-service", "kind": "service",
     "responsibility": "认证层与授权服务：主体、设备、web 会话、授予与票据密钥材料",
     "establish_stage": "R02"},
    {"owner_id": "service.credentials", "module": "lingxi-service", "kind": "service",
     "responsibility": "CredentialService：provider 凭证服务端解析/刷新/撤销（密钥不出服务端）",
     "establish_stage": "R05"},
    {"owner_id": "service.transport", "module": "lingxi-service", "kind": "service",
     "responsibility": "HTTP/WS 传输、事件分发、CLI/Web/Mobile/LAN 入口接入",
     "establish_stage": "R02/R07/R08"},
    # --- cli / xtask ---
    {"owner_id": "cli.commands", "module": "lingxi-cli", "kind": "cli",
     "responsibility": "CLI 命令语义，连接同一 service，不产生第二内核",
     "establish_stage": "R07"},
    {"owner_id": "xtask.automation", "module": "xtask", "kind": "xtask",
     "responsibility": "schema 生成、边界检查、测试编排与产物检查",
     "establish_stage": "R02"},
    # --- host / ui ---
    {"owner_id": "tauri-host.desktop-host", "module": "tauri-host", "kind": "host",
     "responsibility": "窗口/托盘/通知/快捷键/壳自态与本地服务进程托管；不拥有 Agent 业务执行权",
     "establish_stage": "R09"},
    {"owner_id": "tauri-host.os-helper", "module": "tauri-host", "kind": "host",
     "responsibility": "OS 权限 helper（屏幕/麦克风/辅助功能等）执行；授权决定恒在 kernel.policy",
     "establish_stage": "R09"},
    {"owner_id": "react-ui", "module": "react-ui", "kind": "ui",
     "responsibility": "React 展示层：消费协议与投影，永远不拥有业务事实",
     "establish_stage": "现役保留/R08 接入"},
    # --- peripheral workers (restricted) ---
    {"owner_id": "worker.doc-parse", "module": "workers", "kind": "worker",
     "responsibility": "文档解析外围执行（office/PDF 等），单次任务、受控回调",
     "establish_stage": "R01-T05(spike)/R07"},
    {"owner_id": "worker.browser-engine", "module": "workers", "kind": "worker",
     "responsibility": "浏览器引擎宿主进程执行体；可见交互/登录/隔离语义由 adapters.browser 负责",
     "establish_stage": "R01-T04(spike)/R07"},
    {"owner_id": "worker.media-encode", "module": "workers", "kind": "worker",
     "responsibility": "媒体编解码外围执行",
     "establish_stage": "R07"},
    {"owner_id": "worker.bridge-sdk", "module": "workers", "kind": "worker",
     "responsibility": "Bridge 平台 SDK 编解码外围执行；身份/重投语义在 adapters.integrations",
     "establish_stage": "R07"},
    # --- build/release ---
    {"owner_id": "build-release.tooling", "module": "build-release", "kind": "build",
     "responsibility": "构建/签名/更新链与签名产物；Rust 启动门禁配合",
     "establish_stage": "R10/R11"},
]

# ---------------------------------------------------------------------------
# F-ID mapping: R00 per-leaf `target_owner` label -> single owner component.
# Closed table: an unknown label aborts generation.
# ---------------------------------------------------------------------------
FEATURE_OWNER_BY_R00_LABEL = {
    "Rust Run/Session＋React": ("kernel.run-supervisor",
                                "Run 生命周期与输入语义的权威在内核；React 为消费方"),
    "Rust Session/Storage": ("kernel.session",
                             "会话领域权威在内核；存储引擎由 adapters.storage 实现注入"),
    "Rust Registry/Gateway": ("kernel.tool-gateway",
                              "工具目录与调用网关权威在内核"),
    "Rust Policy＋OS helper": ("kernel.policy",
                               "权限/批准决定权威在内核；OS helper 仅为执行体"),
    "Rust Supervisor": ("kernel.run-supervisor",
                        "子代理派发/监督权威在 RunSupervisor"),
    "Rust Context＋规范化投影": ("kernel.context",
                                 "上下文组装与 MOOD/人格语义权威在内核"),
    "Rust Memory": ("kernel.memory", "记忆领域权威在内核"),
    "Rust Knowledge＋受控解析器": ("kernel.knowledge",
                                   "知识库领域权威在内核；解析器为受控 worker 执行体"),
    "Rust Resource": ("kernel.resource", "文件资源领域权威在内核"),
    "Rust Model/Config": ("kernel.model-gateway",
                          "模型配置与路由决策权威在内核"),
    "Rust Credential": ("service.credentials",
                        "凭证服务端解析/刷新/撤销归 CredentialService"),
    "Rust ModelGateway＋编码worker": ("kernel.model-gateway",
                                      "模型网关决策权威在内核；编码 worker 为执行体"),
    "Rust Process/Tool": ("adapters.tools",
                          "进程/PTY 执行为内核端口实现；授权与生命周期仍归内核"),
    "Rust BrowserPort＋独立宿主": ("adapters.browser",
                                   "BrowserPort 实现拥有浏览器能力契约；独立宿主为执行体"),
    "Tauri/OS helper＋Rust授权": ("tauri-host.os-helper",
                                  "系统能力执行在 OS helper；授权决定恒在 kernel.policy"),
    "Rust登记/网关＋受控worker": ("kernel.tool-gateway",
                                  "内置插件登记/网关权威在内核；worker 为执行体"),
    "Rust入口/运行＋SDK worker": ("adapters.integrations",
                                  "Bridge 接入适配归 integrations；SDK worker 为执行体"),
    "Rust Scheduler/Supervisor": ("kernel.scheduler",
                                  "调度决策与触发收据权威在内核"),
    "Rust各服务复用共同运行边界": ("kernel.run-supervisor",
                                   "协作/工作台复用共同 Run 边界，权威归 RunSupervisor"),
    "Rust service＋客户端": ("service.transport",
                             "非桌面入口经统一 service 传输层；CLI 语义归 lingxi-cli"),
    "Rust Skill/Config＋解析worker": ("kernel.skill",
                                      "技能/角色卡配置权威在内核；解析 worker 为执行体"),
    "Rust Trace/Usage": ("kernel.model-gateway",
                         "唯一 modelCall 记录与用量 trace 权威在内核"),
    "Tauri DesktopHost＋原React": ("tauri-host.desktop-host",
                                   "桌面体验能力归 Tauri 宿主；不拥有 Agent 业务执行权"),
    "构建/更新＋Rust启动门禁": ("build-release.tooling",
                                "构建/签名/更新链归构建发布工具"),
    "Tauri DesktopHost＋Rust Resource＋React": ("kernel.resource",
                                                "文件预览/刷新的资源事实权威在内核；宿主与 UI 为消费方"),
}

# ---------------------------------------------------------------------------
# Store mapping: all 69 R00 store ids -> (owner, target writer process).
# Closed table: an unknown store id aborts generation; a mapped id absent
# from STORES.json also aborts (stale table detection).
# ---------------------------------------------------------------------------
RUST_SERVICE = "rust-service (lingxi-service 组合根进程，唯一业务数据写者)"
TAURI_HOST = "tauri-host (仅写自身壳自态)"
BUILD_TOOLING = "build-release tooling (签名/列车应用，契约固定序列)"

STORE_OWNER = {
    # data epoch / lifecycle (现 server 进程写；epoch 机制整体迁入组合根)
    "data-epoch-stamp": ("service.composition", RUST_SERVICE,
                         "epoch 高水位印章由组合根事务闸唯一写"),
    "data-epoch-transition-journal": ("service.composition", RUST_SERVICE,
                                      "迁移 journal 由组合根 epoch 协调器写"),
    "data-epoch-checkpoints": ("service.composition", RUST_SERVICE,
                               "epoch 检查点由组合根写；CLI 维护面经同一协调器"),
    "data-epoch-restore-quarantine": ("service.composition", RUST_SERVICE,
                                      "恢复隔离区由组合根维护面管理"),
    "server-runtime-info": ("service.composition", RUST_SERVICE,
                            "server-info 内容唯一写者为 service；宿主仅删除性清理"),
    "server-network-config": ("service.composition", RUST_SERVICE, "网络配置由组合根写"),
    "operational-checkpoints": ("service.composition", RUST_SERVICE, "运行检查点归组合根"),
    "runtime-diagnostics": ("service.composition", RUST_SERVICE, "运行诊断归组合根"),
    "user-preferences": ("service.composition", RUST_SERVICE,
                         "偏好唯一写者保持服务端（现 PUT /api/config 语义）"),
    # identity / auth
    "server-node-identity": ("service.auth", RUST_SERVICE, "节点身份归认证层"),
    "user-studio-registries": ("service.auth", RUST_SERVICE, "本地身份注册表归认证层"),
    "local-user-auth": ("service.auth", RUST_SERVICE, "本地账户认证归认证层"),
    "device-access-registries": ("service.auth", RUST_SERVICE, "设备注册表归认证层"),
    "web-session-registry": ("service.auth", RUST_SERVICE, "web 会话注册表归认证层"),
    "security-key-material": ("service.auth", RUST_SERVICE,
                              "票据签名密钥材料不出服务端认证层"),
    # policy
    "security-grants": ("kernel.policy", RUST_SERVICE, "授予登记归权限内核"),
    "execution-leases": ("kernel.policy", RUST_SERVICE, "执行租约归权限内核"),
    "security-audit-log": ("kernel.policy", RUST_SERVICE, "安全审计日志归权限内核"),
    "ephemeral-scanner-scratch": ("kernel.policy", RUST_SERVICE,
                                  "外部扫描暂存归权限内核安全链"),
    # model / credential / usage
    "provider-state": ("kernel.model-gateway", RUST_SERVICE,
                       "provider/model 配置权威归模型网关；密钥材料经 CredentialService 解析"),
    "usage-ledger": ("kernel.model-gateway", RUST_SERVICE, "用量账本归唯一 modelCall 记录"),
    "model-observability-db": ("kernel.model-gateway", RUST_SERVICE, "观测库归模型网关 trace"),
    "model-observability-blobs": ("kernel.model-gateway", RUST_SERVICE, "观测 blob 归模型网关 trace"),
    # session
    "session-jsonl": ("kernel.session", RUST_SERVICE,
                      "规范化消息权威存储（目标：Rust 新运行/消息库，旧 JSONL 只读导入）"),
    "session-manifest-sqlite": ("kernel.session", RUST_SERVICE, "会话清单归会话域"),
    "session-sidecars": ("kernel.session", RUST_SERVICE, "会话侧车归会话域"),
    "session-drafts-and-projects": ("kernel.session", RUST_SERVICE, "草稿/项目归会话域"),
    "session-checkpoints": ("kernel.session", RUST_SERVICE, "checkpoint/rewind 归会话域"),
    "conversation-map-layout": ("kernel.session", RUST_SERVICE, "会话图投影数据归会话域"),
    # context / memory / persona
    "agent-profile": ("kernel.context", RUST_SERVICE, "人格/Agent 配置归上下文域"),
    "session-goal": ("kernel.context", RUST_SERVICE, "goal 归上下文组装域"),
    "session-context-notes": ("kernel.context", RUST_SERVICE, "上下文笔记归上下文域"),
    "agent-facts-sqlite": ("kernel.memory", RUST_SERVICE, "事实库归记忆域"),
    "agent-memory": ("kernel.memory", RUST_SERVICE, "记忆编译产物归记忆域"),
    # resource
    "file-history-sqlite": ("kernel.resource", RUST_SERVICE, "文件历史归资源域"),
    "session-files": ("kernel.resource", RUST_SERVICE, "SessionFile/Resource 归资源域"),
    "workspace-snapshots": ("kernel.resource", RUST_SERVICE, "工作区影子快照归资源域"),
    "legacy-upload-cache": ("kernel.resource", RUST_SERVICE, "上传暂存归资源域"),
    "desk-cover-upload-staging": ("kernel.resource", RUST_SERVICE, "封面暂存归资源域"),
    "studio-mount-registry": ("kernel.resource", RUST_SERVICE, "书桌挂载登记归资源域"),
    # collaboration boundary (D19 复用共同运行边界)
    "channels": ("kernel.run-supervisor", RUST_SERVICE, "频道归共同运行边界"),
    "desk-activity": ("kernel.run-supervisor", RUST_SERVICE, "书桌活动归共同运行边界"),
    "agent-phone": ("kernel.run-supervisor", RUST_SERVICE, "电话会话投影归共同运行边界"),
    "agent-authored-records": ("kernel.run-supervisor", RUST_SERVICE,
                               "代理产出记录归共同运行边界"),
    "workflow-state": ("kernel.run-supervisor", RUST_SERVICE, "现役工作流 journal 归运行边界"),
    "subagent-state": ("kernel.run-supervisor", RUST_SERVICE, "子代理状态归 RunSupervisor"),
    # scheduler
    "cron-automation": ("kernel.scheduler", RUST_SERVICE, "cron 任务与收据归调度内核"),
    "loop-state": ("kernel.scheduler", RUST_SERVICE, "loop 闹钟状态归调度内核"),
    # tool gateway / execution
    "plugin-task-registry": ("kernel.tool-gateway", RUST_SERVICE, "插件任务登记归工具网关"),
    "deferred-result-state": ("kernel.tool-gateway", RUST_SERVICE, "延迟结果状态归工具网关"),
    "terminal-session-state": ("adapters.tools", RUST_SERVICE, "PTY 会话状态归工具执行适配"),
    "managed-runtime-caches": ("adapters.tools", RUST_SERVICE, "托管工具二进制缓存归工具适配"),
    "office-render-jobs": ("kernel.tool-gateway", RUST_SERVICE,
                           "office 渲染作业队列归工具网关；渲染执行委托受控 worker"),
    # knowledge / skill / integration
    "knowledge-database": ("kernel.knowledge", RUST_SERVICE, "知识库归知识域"),
    "knowledge-source-snapshots": ("kernel.knowledge", RUST_SERVICE, "知识来源快照归知识域"),
    "knowledge-parse-artifacts": ("kernel.knowledge", RUST_SERVICE,
                                  "解析产物归知识域（解析执行委托受控 worker）"),
    "knowledge-processing-artifacts": ("kernel.knowledge", RUST_SERVICE, "处理产物归知识域"),
    "knowledge-indexes": ("kernel.knowledge", RUST_SERVICE, "向量索引归知识域"),
    "skill-state": ("kernel.skill", RUST_SERVICE, "技能状态归技能配置域"),
    "skill-translation-cache": ("kernel.skill", RUST_SERVICE, "技能翻译缓存归技能域"),
    "character-card-staging": ("kernel.skill", RUST_SERVICE, "角色卡导入暂存归技能域"),
    "mcp-config": ("adapters.integrations", RUST_SERVICE, "MCP 配置归集成适配"),
    "plugin-runtime-data": ("adapters.integrations", RUST_SERVICE, "插件运行数据归集成适配"),
    # tauri host shell state (host 只写自身壳自态)
    "desktop-diagnostics": ("tauri-host.desktop-host", TAURI_HOST, "壳诊断归桌面宿主"),
    "desktop-gpu-startup-state": ("tauri-host.desktop-host", TAURI_HOST, "GPU 启动态归桌面宿主"),
    "desktop-win32-install-acl-heal-state": ("tauri-host.desktop-host", TAURI_HOST,
                                             "win32 安装 ACL 修复态归桌面宿主"),
    "desktop-window-version-state": ("tauri-host.desktop-host", TAURI_HOST, "窗口版本态归桌面宿主"),
    "desktop-update-channel": ("tauri-host.desktop-host", TAURI_HOST, "更新通道选择归桌面宿主"),
    # build / release
    "signed-artifacts": ("build-release.tooling", BUILD_TOOLING,
                         "签名产物/OTA 列车指针归构建发布工具；宿主按固定序列应用"),
}

# ---------------------------------------------------------------------------
# Critical facts (taskbook 02 §3): exactly one owner each, never a worker.
# ---------------------------------------------------------------------------
CRITICAL_FACTS = [
    {"fact_id": "authenticated_principal",
     "description": "当前认证主体及权限域",
     "owners": ["service.auth"], "contract_ref": "02 §3"},
    {"fact_id": "session_identity_branch_messages",
     "description": "会话身份/分支/规范化消息",
     "owners": ["kernel.session"], "contract_ref": "02 §3"},
    {"fact_id": "run_terminal_state",
     "description": "一次用户任务及终态（Run 终态唯一负责人）",
     "owners": ["kernel.run-supervisor"], "contract_ref": "02 §3/§4"},
    {"fact_id": "attempt_generation_fence",
     "description": "一次重试的 attempt 计数与 generation 栅栏",
     "owners": ["kernel.run-supervisor"], "contract_ref": "02 §3"},
    {"fact_id": "tool_availability",
     "description": "工具是否可用（当前 generation）",
     "owners": ["kernel.tool-gateway"], "contract_ref": "02 §3"},
    {"fact_id": "params_approval_resource_scope",
     "description": "参数、批准与资源范围",
     "owners": ["kernel.policy"], "contract_ref": "02 §3/§5"},
    {"fact_id": "model_credential_selection",
     "description": "模型/凭证选择（凭证服务端解析）",
     "owners": ["kernel.model-gateway"], "contract_ref": "02 §3/§6"},
    {"fact_id": "real_files_and_authorization",
     "description": "真实文件与授权",
     "owners": ["kernel.resource"], "contract_ref": "02 §3"},
    {"fact_id": "history_realtime_projection_semantics",
     "description": "历史/实时显示语义（统一投影唯一入口）",
     "owners": ["kernel.session"], "contract_ref": "02 §3/§7"},
    {"fact_id": "usage_causal_trace",
     "description": "用量和因果 trace（唯一 modelCall 记录）",
     "owners": ["kernel.model-gateway"], "contract_ref": "02 §3"},
    {"fact_id": "scheduler_trigger_dedup",
     "description": "调度触发去重（触发收据）",
     "owners": ["kernel.scheduler"], "contract_ref": "02 §3"},
]

WORKER_RESTRICTIONS = {
    "forbidden_responsibilities": [
        "run_terminal_state",
        "approval_decision",
        "context_assembly",
        "model_loop",
        "cross_session_message_write",
        "global_provider_credential_read",
    ],
    "rules": [
        "worker 类 owner 不得成为任何 critical_facts 的 owner",
        "worker 类 owner 不得成为任何 store 的写 owner（含 rebuildable_cache 权威化场景）",
        "worker 类 owner 不得成为任何 F-ID 的 target owner；外围执行通过受控 Rust 回调并受同一预算与授权",
        "worker 需要模型操作时经 kernel.model-gateway 回调，不得自行读取全局 provider 凭证",
    ],
}


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def build() -> dict:
    inventory = json.loads(FEATURE_INVENTORY.read_text(encoding="utf-8"))
    stores = json.loads(STORES.read_text(encoding="utf-8"))
    features = inventory["features"]
    store_entries = stores["stores"]

    registry_ids = [o["owner_id"] for o in OWNER_REGISTRY]
    assert len(registry_ids) == len(set(registry_ids)), "duplicate owner_id in registry"
    registry = {o["owner_id"]: o for o in OWNER_REGISTRY}

    # --- features ---
    feature_rows = []
    seen_fids = set()
    for f in sorted(features, key=lambda x: x["feature_id"]):
        fid = f["feature_id"]
        if fid in seen_fids:
            raise SystemExit(f"duplicate feature_id in R00 inventory: {fid}")
        seen_fids.add(fid)
        label = f["target_owner"]
        if label not in FEATURE_OWNER_BY_R00_LABEL:
            raise SystemExit(f"unmapped R00 target_owner label {label!r} (feature {fid})")
        owner, basis = FEATURE_OWNER_BY_R00_LABEL[label]
        if owner not in registry:
            raise SystemExit(f"unknown owner {owner} for feature {fid}")
        if registry[owner]["kind"] in ("worker", "ui"):
            raise SystemExit(
                f"feature {fid} mapped to {registry[owner]['kind']}-kind owner {owner}; "
                "workers/UI never own features")
        feature_rows.append({
            "feature_id": fid,
            "parent_domain": f["parent_domain"],
            "title": f["title"],
            "target_owner": owner,
            "r00_target_owner_label": label,
            "stage_ids": f["stage_ids"],
            "basis": basis,
        })
    if len(feature_rows) != inventory["feature_count"]:
        raise SystemExit("feature count mismatch vs inventory header")

    # --- stores ---
    store_rows = []
    store_ids = {s["id"] for s in store_entries}
    mapped_ids = set(STORE_OWNER)
    missing = store_ids - mapped_ids
    stale = mapped_ids - store_ids
    if missing:
        raise SystemExit(f"stores missing target owner mapping: {sorted(missing)}")
    if stale:
        raise SystemExit(f"stale store mapping not in STORES.json: {sorted(stale)}")
    for s in sorted(store_entries, key=lambda x: x["id"]):
        owner, writer, basis = STORE_OWNER[s["id"]]
        if owner not in registry:
            raise SystemExit(f"unknown owner {owner} for store {s['id']}")
        if registry[owner]["kind"] == "worker":
            raise SystemExit(f"store {s['id']} mapped to worker-kind owner {owner}")
        store_rows.append({
            "store_id": s["id"],
            "classification": s["classification"],
            "current_owner_module": s["owner_module"],
            "current_writers": s["processes"]["writers"],
            "target_owner": owner,
            "target_writer_process": writer,
            "basis": basis,
        })

    # --- critical facts sanity ---
    for fact in CRITICAL_FACTS:
        owners = fact["owners"]
        if len(owners) != 1:
            raise SystemExit(f"critical fact {fact['fact_id']} must have exactly one owner")
        owner = owners[0]
        if owner not in registry:
            raise SystemExit(f"critical fact {fact['fact_id']} references unknown owner {owner}")
        if registry[owner]["kind"] in ("worker", "ui", "host"):
            raise SystemExit(
                f"critical fact {fact['fact_id']} owned by {registry[owner]['kind']}-kind "
                f"owner {owner}; only core/service/adapters may own critical facts")

    return {
        "schema_version": SCHEMA_VERSION,
        "task_id": "R01-T01",
        "stage_id": "R01",
        "generated_by": "docs/rust-tauri/R01/r01_t01_build_ownership.py",
        "deterministic": "no timestamps/randomness; identical inputs produce identical bytes",
        "source_inputs": {
            "feature_inventory": {
                "path": "docs/rust-tauri/R00/FEATURE_INVENTORY.json",
                "sha256": sha256_file(FEATURE_INVENTORY),
                "feature_count": inventory["feature_count"],
                "inventory_tested_sha": inventory.get("tested_sha"),
            },
            "stores": {
                "path": "docs/rust-tauri/R00/STORES.json",
                "sha256": sha256_file(STORES),
                "store_count": len(store_entries),
                "registry_source": stores.get("registry_source"),
            },
        },
        "owner_registry": OWNER_REGISTRY,
        "critical_facts": CRITICAL_FACTS,
        "worker_restrictions": WORKER_RESTRICTIONS,
        "feature_ownership": feature_rows,
        "store_ownership": store_rows,
        "counts": {
            "features": len(feature_rows),
            "stores": len(store_rows),
            "owners": len(OWNER_REGISTRY),
            "critical_facts": len(CRITICAL_FACTS),
        },
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="do not write; exit 1 if the on-disk file differs")
    args = ap.parse_args()

    doc = build()
    text = json.dumps(doc, ensure_ascii=False, indent=1, sort_keys=False) + "\n"

    if args.check:
        if not OUTPUT.exists():
            print(f"MISSING {OUTPUT}")
            return 1
        current = OUTPUT.read_text(encoding="utf-8")
        if current != text:
            print(f"DRIFT: {OUTPUT} differs from regenerated content; "
                  "re-run r01_t01_build_ownership.py")
            return 1
        print(f"OWNERSHIP_TARGET_UP_TO_DATE features={doc['counts']['features']} "
              f"stores={doc['counts']['stores']}")
        return 0

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(text, encoding="utf-8")
    print(f"WROTE {OUTPUT} features={doc['counts']['features']} "
          f"stores={doc['counts']['stores']} owners={doc['counts']['owners']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
