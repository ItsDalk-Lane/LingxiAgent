"""R00-T03：用只读导入图生成职责、依赖账本并执行差集检查。

R2 修复：worker 清单改为从现役执行入口与终态写入点双向枚举后闭合分类（W1-W15），
依赖账本保留初始手工锚点并按图分层加载时点复核；不再由人工预选五个例子。
"""
from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
GRAPH = json.loads((HERE / "R00-T03_IMPORT_GRAPH.json").read_text())
PKG = json.loads((ROOT / "package.json").read_text())
LOCK = json.loads((ROOT / "package-lock.json").read_text())
HEAD = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
TASK_BASE_SHA = "4b4a1d98f6d2d0e03f9573aa75b7992efed03db6"

# 受控源码集合：与导入图生成器同口径（git 跟踪 + 目录 + 扩展名 + 排除项）。
TRACKED = subprocess.check_output(["git", "ls-files"], cwd=ROOT, text=True).splitlines()
SOURCE_DIRS = ("core/", "server/", "lib/", "hub/", "cli/", "desktop/", "plugins/", "scripts/", "shared/")
CONTROLLED = sorted(
    f for f in TRACKED
    if f.startswith(SOURCE_DIRS)
    and re.search(r"\.(?:[cm]?js|tsx?)$", f)
    and not re.search(r"(?:^|/)(?:__tests__|dist|dist-renderer|dist-splash|dist-theme|node_modules)(?:/|$)", f)
    and not re.search(r"\.(?:test|spec)\.[cm]?[jt]sx?$", f)
)


def capability(id, duty, symbols, sources, owner, retained, test, exit_evidence, stage):
    return dict(id=id, current_duty=duty, adapter_symbols=symbols.split(), source_refs=sources.split(),
                rust_owner=owner, permitted_peripheral=retained, protocol_test=test,
                retirement_evidence=exit_evidence, target_stage=stage, disposition="KERNEL_MIGRATION")


CAPS = [
    capability("PI-01", "正常、Bridge、临时代理、电话代理和隔离子会话的模型/工具循环；Pi 决定下一步和停止。", "createAgentSession runAgentLoop AgentContext AgentEvent AgentLoopConfig AgentMessage AgentTool StreamFn", "lib/pi-sdk/index.ts:14 lib/pi-sdk/index.ts:60 core/session-coordinator.ts:2230 core/session-coordinator.ts:8385 core/bridge-session-manager.ts:1305 hub/agent-executor.ts:290 hub/agent-executor.ts:556 lib/llm/cache-preserving-compaction-agent-run.ts:550", "Rust RunSupervisor 与 AgentLoop", "无；单次工具 worker 不能调用 prompt/runAgentLoop。", "受控模型流产生工具请求→授权执行→再请求→唯一终态；各入口及取消/重试/临时子会话协议对照。", "生产进程树和最终打包依赖图无 Pi AgentLoop；禁用旧 Node 服务仍能完成真实工具任务。", "R03/R05/R06/R07/R11"),
    capability("PI-02", "JSONL 会话树、分支/回退、消息读取和旧格式兼容；多个调用者直接打开 SessionManager。", "SessionManager parseSessionEntries buildSessionContext", "core/session-coordinator.ts:1831 core/session-coordinator.ts:4825 core/bridge-session-manager.ts:777 hub/agent-executor.ts:240 server/routes/sessions.ts:30 lib/tools/todo-compat.ts:22", "Rust SessionService 与规范化消息存储", "旧 JSONL 只读导入器可暂留，不作为新消息权威写者。", "旧 ID/分支/fork/坏行/分页/导出 round-trip 与跨入口同一历史投影。", "新任务只写 Rust 权威存储；旧 SDK SessionManager 不在生产写链，旧资料只读可导入。", "R02/R06/R08/R11"),
    capability("PI-03", "上下文估算、截断点、压缩触发、原生压缩与缓存保留压缩旁路。", "calculateContextTokens estimateTokens findCutPoint serializeConversation shouldCompact prepareCompaction buildNativeCompactionRequestShapes convertAgentMessagesToLlm", "core/session-compaction-runtime.ts:42 core/session-compactor.ts:5 lib/extensions/compaction-guard-ext.ts:237 lib/llm/cache-preserving-compaction-agent-run.ts:550", "Rust ContextService、Compactor 与 ModelGateway", "分词实现可封装库调用，但预算、保护段和历史写入由 Rust 决定。", "长会话预算、工具配对、压缩失败不覆原历史、native/cache 路径、取消及 trace 对照。", "Pi prepareCompaction/generateSummary/runAgentLoop 不在新上下文路径；压缩摘要由 Rust 归档。", "R05/R06/R11"),
    capability("PI-04", "资源加载、技能/扩展发现、系统提示词组装及 extension 生命周期。", "DefaultResourceLoader createExtensionRuntime LoadExtensionsResult", "core/engine.ts:3577 core/session-coordinator.ts:2012 core/session-coordinator.ts:2171 server/index.ts:493", "Rust ContextService、SkillRegistry 与 ExtensionPort", "文件解析可受控 worker；用户自装扩展若需要兼容必须收窄为单项调用。", "同配置下 prompt、技能/角色卡、扩展启停/热更新、hook 次序与错误传播对照。", "旧 ResourceLoader/ExtensionRuntime 不参与 Rust 任务装配；所有活跃 hook 有新 port 或明确弃用证据。", "R06/R07/R11"),
    capability("PI-05", "基础文件/命令/搜索工具工厂与 allowlist/customTools 转换、同名执行去重。", "createReadTool createWriteTool createEditTool createBashTool createLsTool createGrepTool createFindTool", "lib/pi-sdk/session-options.ts:165 lib/sandbox/index.ts:36 lib/sandbox/file-tool-presentation.ts:3 lib/pi-sdk/search-tools.ts:22", "Rust ToolRegistry、InvocationGateway、ResourceService 与 ProcessService", "具体搜索算法可库化；worker 只能处理已授权单次调用。", "read/write/edit/exec 与 grep/find/ls 参数、权限、路径、取消、结果 schema 和副作用收据对照。", "Rust 路径不加载 Pi 工厂；工具目录与执行统一走同一 Rust 网关。", "R04/R07/R11"),
    capability("PI-06", "模型目录、可用性、供应商注册/刷新、当前会话模型对象重绑。", "getPiModel getPiModels createModelRegistry registerModelProvider unregisterModelProvider refreshSessionModelFromRegistry", "core/model-manager.ts:12 core/model-sync.ts:9 core/model-known-enrichment.ts:1 lib/providers/opencode.ts:9 lib/pi-sdk/index.ts:473", "Rust ModelGateway 与 ConfigService", "供应商 HTTP 编解码适配可独立库；模型选择与配置代次由 Rust 持有。", "同名模型跨 provider 不混、注册/禁用/刷新/旧模型对象、Ollama 与配置保留。", "Rust 生产模型选择不调用 Pi ModelRegistry/compat 目录；停用旧 Node 不改变可用模型集合。", "R05/R07/R11"),
    capability("PI-07", "auth.json 凭证锁、OAuth 登录/刷新/撤销、环境凭证禁止回退和 provider 认证路由。", "AuthStorage FileAuthStorageBackend SdkAuthFacade loginOAuthProvider OAuthLoginCallbacks SdkOAuthProvider", "core/model-manager.ts:12 server/routes/auth.ts:18 lib/pi-sdk/auth-facade.ts:32 lib/pi-sdk/index.ts:353", "Rust CredentialService 与 ModelGateway", "OAuth 浏览器交互可在受控 UI/OS helper，密钥读取及刷新判断留 Rust。", "并发刷新、撤销后迟到请求、环境变量偷读、provider 隔离与 auth.json 兼容导入。", "生产凭证不通过 Pi AuthStorage/ModelRuntime 读取；旧文件只读迁移并校验轮换。", "R05/R08/R11"),
    capability("PI-08", "Pi provider 流/非流式调用、消息转换、直接摘要/快照旁路与媒体协议内容。", "completeSimple generateSummary", "lib/llm/session-snapshot-side-task-runner.ts:94 lib/diary/diary-writer.ts:376 lib/pi-sdk/index.ts:234 core/llm-client.ts:520", "Rust ModelGateway，所有模型目的均走同一凭证与用量边界", "图片/音频编码器可做单次 worker；不得自行路由密钥或执行多轮模型循环。", "实际启用协议族的流片段、工具调用、错误、特殊签名块、摘要和旁路用量对账。", "新内核所有模型目的均无 Pi complete/stream；本地 direct HTTP 旁路也收归同一网关。", "R05/R07/R11"),
    capability("PI-09", "streamFunction/afterToolCall/transformContext 包装：损坏工具片段防护、工具错误语义、轮中待办提醒。", "", "lib/pi-sdk/index.ts:82 lib/pi-sdk/stream-guard.ts:8 lib/pi-sdk/tool-outcome-adapter.ts:9 lib/pi-sdk/todo-context-reminder.ts:88", "Rust StreamNormalizer、ToolResult 与 ContextService", "无 Agent 决策型 worker；纯图像解码可作为有界单次处理。", "畸形 toolcall、工具 isError、输出截断/注入、待办节流和历史不污染对照。", "Pi agent 方法猴补丁退出；相应效果由 Rust 规范化事件和上下文产物证明。", "R04/R05/R06/R11"),
    capability("PI-10", "用量字段、模型调用 ingress/trace、provider 前后 hook 与直接摘要观测。", "getLastAssistantUsage", "server/routes/chat.ts:22 lib/pi-sdk/model-call-stream-observer.ts:1 lib/extensions/model-call-observer-ext.ts:30 lib/pi-sdk/index.ts:234", "Rust Trace/UsageService 与唯一 ModelCall 记录", "观测存储后端可替换，关联 ID/去重事实由 Rust 持有。", "普通/压缩/辅助/失败调用的 callId、attempt、usage 去重及不可观测字段标注。", "不依赖 Pi streamFunction/hooks 获取关键 trace；旧记录只读可查询。", "R05/R07/R11"),
    capability("PI-11", "steer/followUp、轮中输入提交、abort/消息落盘事件和 shutdown 扩展清理。", "", "core/session-coordinator.ts:5121 core/session-coordinator.ts:5299 core/session-coordinator.ts:5344 lib/pi-sdk/desktop-input-commit.ts:36 lib/pi-sdk/session-shutdown.ts:26", "Rust RunSupervisor、SessionService 与 EventService", "UI 输入可以是客户端事件，但队列身份和提交收据由 Rust 生成。", "插话/跟进顺序、取消竞争、重复提交、关窗后任务存活、shutdown 清理对照。", "新入口不调 Pi session.steer/sendCustomMessage/abort；同一 run 的终态只有 Rust 写入。", "R03/R06/R08/R11"),
    capability("PI-12", "SettingsManager 的 session 默认设置、thinking/config 兼容。", "SettingsManager", "core/engine.ts:45 core/session-defaults.ts:1 core/session-coordinator.ts:8388", "Rust ConfigService 与 SessionService", "旧设置只读迁移可保留，不作业务设置权威。", "旧模型/思考级别/工具模式默认值与用户覆盖、配置变更代次。", "新 session 设置不依赖 Pi SettingsManager。", "R05/R06/R08/R11"),
    capability("PI-13", "图片模型入参重采样和尺寸提示；Pi 内部通过 photon 原生/WASM。", "*", "core/model-image-preprocess.ts:203 lib/pi-sdk/index.ts:18 lib/pi-sdk/index.ts:334", "Rust ModelGateway 输入预算与错误策略", "单次图像转换可保留 photon/替代编码 worker，输入、输出与取消有界。", "base64/MIME/上限/转换失败及尺寸提示对照；模型请求不得超预算。", "Rust 模型入口不调用 Pi resizeImage；可保留独立图像执行器且不读凭证。", "R05/R07/R11"),
    capability("PI-14", "工具参数 TypeBox schema 与 StringEnum 的 Pi 适配导出。", "Type StringEnum", "lib/pi-sdk/index.ts:299 lib/pi-sdk/index.ts:309 lib/tools/browser-tool.ts:26 core/tool-catalog-bridge.ts:20", "Rust ToolRegistry/Protocol schema 生成与校验", "TypeScript 类型或前端 schema 消费可保留；执行时 Rust 同源校验。", "全量工具描述/执行 schema 一致、未知 dialect 明确失败、optional/default/union/enum 对照。", "Rust 执行器不依赖 Pi/typebox 决定参数有效性；生成协议与客户端一致。", "R01/R04/R11"),
]

SYMBOL_OWNER = {}
for cap in CAPS:
    for symbol in cap["adapter_symbols"]:
        if symbol in SYMBOL_OWNER:
            raise SystemExit(f"duplicate mapping: {symbol}")
        SYMBOL_OWNER[symbol] = cap["id"]

index_imports = [r for r in GRAPH["adapter_imports"] if r["resolved"] == "lib/pi-sdk/index.ts"
                 and r["file"] != "lib/pi-sdk/index.ts"]
observed_symbols = {s["imported"] for r in index_imports for s in r["symbols"]}
symbol_gap = sorted(observed_symbols - SYMBOL_OWNER.keys())
if symbol_gap:
    raise SystemExit(f"unmapped adapter symbols: {symbol_gap}")

vendor_owner_by_file = {
    "lib/pi-sdk/index.ts": ["PI-01", "PI-02", "PI-03", "PI-04", "PI-05", "PI-06", "PI-07", "PI-08", "PI-10", "PI-12", "PI-13", "PI-14"],
    "lib/pi-sdk/auth-facade.ts": ["PI-07"],
    "lib/pi-sdk/compaction-request-shape.ts": ["PI-03"],
    "lib/pi-sdk/search-tools.ts": ["PI-05"],
    "lib/pi-sdk/stream-guard.ts": ["PI-09"],
    "scripts/sync-known-models-from-pi.mjs": ["PI-06"],
}
vendor_gap = sorted({r["file"] for r in GRAPH["vendor_pi_imports"]} - vendor_owner_by_file.keys())
if vendor_gap:
    raise SystemExit(f"unmapped Pi vendor import files: {vendor_gap}")

hook_owner = {
    "session_start": "PI-04", "session_shutdown": "PI-11", "agent_start": "PI-01",
    "tool_call": "PI-05", "tool_result": "PI-09", "context": "PI-03",
    "session_before_compact": "PI-03", "before_provider_request": "PI-08",
    "after_provider_response": "PI-10",
}
hooks = []
for file in [*sorted((ROOT / "lib/extensions").glob("*.ts")), ROOT / "core/engine.ts"]:
    for line_number, line in enumerate(file.read_text().splitlines(), 1):
        for match in re.finditer(r'pi\.on\(["\']([^"\']+)["\']', line):
            event = match.group(1)
            hooks.append(dict(source_ref=f"{file.relative_to(ROOT)}:{line_number}", event=event,
                              capability_id=hook_owner.get(event)))
hook_gap = [h for h in hooks if not h["capability_id"]]
if hook_gap:
    raise SystemExit(f"unmapped extension hooks: {hook_gap}")

submodule_owner = {
    "lib/pi-sdk/auth-facade.ts": "PI-07",
    "lib/pi-sdk/compaction-request-shape.ts": "PI-03",
    "lib/pi-sdk/desktop-input-commit.ts": "PI-11",
    "lib/pi-sdk/model-call-stream-observer.ts": "PI-10",
    "lib/pi-sdk/search-presentation.ts": "PI-05",
    "lib/pi-sdk/search-tools.ts": "PI-05",
    "lib/pi-sdk/session-options.ts": "PI-05",
    "lib/pi-sdk/session-shutdown.ts": "PI-11",
    "lib/pi-sdk/stream-guard.ts": "PI-09",
    "lib/pi-sdk/todo-context-reminder.ts": "PI-09",
    "lib/pi-sdk/tool-outcome-adapter.ts": "PI-09",
}
submodule_gap = sorted({r["resolved"] for r in GRAPH["adapter_imports"] if r["resolved"] != "lib/pi-sdk/index.ts"}
                       - submodule_owner.keys())
if submodule_gap:
    raise SystemExit(f"unmapped Pi adapter submodules: {submodule_gap}")


def classify_opaque(item):
    file = item["file"]
    if file == "core/fresh-import.ts":
        return "生产插件目录按路径加载；不能以静态图证明用户自装插件无 Pi，R07/R11 需运行时登记并阻断第二内核"
    if file == "desktop/bootstrap.cjs":
        return "生产条件 require 主进程 bundle/source；另从 desktop/main.cjs 与服务启动链人工复核"
    if file == "server/bootstrap.ts":
        return "生产打包服务入口；另从 main-full/main-open 静态组合根复核"
    if file == "desktop/speech-permissions.cjs":
        return "桌面权限 helper 的可选候选路径；属于宿主动态加载，未发现 Pi 字面量"
    if file == "scripts/generate-persistence-schema-fingerprint.mjs":
        return "构建/校验脚本；其中 PI_SESSION_PACKAGE 为 Pi 会话版本校验，不是运行服务入口"
    if file.startswith("scripts/"):
        return "构建、基准或 smoke 脚本；不在产品服务入口，但需保留开发依赖区分"
    raise SystemExit(f"unclassified opaque dynamic import: {file}:{item['line']}")

opaque_classification = [dict(**item, classification=classify_opaque(item))
                         for item in GRAPH["opaque_dynamic_imports"]]

ENTRYPOINTS = [
    dict(id="E01", surface="完整 Node 服务", chain=["scripts/launch.js:36", "server/main-full.ts:17", "server/index.ts:120", "core/engine.ts:45"], capabilities=["PI-01", "PI-02", "PI-03", "PI-04", "PI-05", "PI-06", "PI-07", "PI-08", "PI-09", "PI-10", "PI-11", "PI-12", "PI-13", "PI-14"], activation="正常启动；部分功能按用户动作激活"),
    dict(id="E02", surface="开放组合服务", chain=["scripts/build-server-open.mjs:57", "server/main-open.ts:23", "server/index.ts:120", "core/engine.ts:45"], capabilities=["PI-01", "PI-02", "PI-03", "PI-04", "PI-05", "PI-06", "PI-07", "PI-08", "PI-09", "PI-10", "PI-11", "PI-12", "PI-13", "PI-14"], activation="构建 open 版本时"),
    dict(id="E03", surface="Electron 主进程及渲染端", chain=["desktop/bootstrap.cjs:195", "desktop/main.cjs:1904", "desktop/main.cjs:1959", "server/bootstrap.ts:62", "desktop/src/index.html:17", "desktop/src/react/app-init.ts:267", "server/routes/chat.ts:2894", "hub/index.ts:213", "core/session-coordinator.ts:5278"], capabilities=["PI-01", "PI-02", "PI-03", "PI-04", "PI-05", "PI-06", "PI-07", "PI-08", "PI-09", "PI-10", "PI-11", "PI-12", "PI-13", "PI-14"], activation="Electron 客户端启动 Node 服务；HTML/React 经 HTTP/WS 调用，非源码静态 import 到 Pi"),
    dict(id="E04", surface="CLI", chain=["cli/entry.ts:17", "cli/server-runner.ts:163", "server/main-full.ts:17", "cli/client.ts:66", "server/routes/chat.ts:2894"], capabilities=["PI-01", "PI-02", "PI-03", "PI-05", "PI-06", "PI-07", "PI-08", "PI-09", "PI-10", "PI-11"], activation="CLI serve 或连接本地服务后提交任务"),
    dict(id="E05", surface="Hub 临时/电话代理", chain=["hub/channel-router.ts:820", "hub/agent-executor.ts:290", "hub/agent-executor.ts:343", "hub/agent-executor.ts:556", "hub/agent-executor.ts:716"], capabilities=["PI-01", "PI-02", "PI-04", "PI-05", "PI-06", "PI-07", "PI-08", "PI-09", "PI-10", "PI-11"], activation="频道、电话或临时任务触发"),
    dict(id="E06", surface="Bridge 五平台", chain=["server/index.ts:860", "server/index.ts:1313", "lib/bridge/bridge-manager.ts:2093", "hub/index.ts:213", "core/bridge-session-manager.ts:1305", "core/bridge-session-manager.ts:1456"], capabilities=["PI-01", "PI-02", "PI-04", "PI-05", "PI-06", "PI-07", "PI-08", "PI-09", "PI-10", "PI-11"], activation="服务 ready 后 manager 可延迟加载；平台连接须启用并有凭证"),
    dict(id="E07", surface="Cron / 心跳 / 后台", chain=["server/index.ts:557", "hub/index.ts:367", "hub/scheduler.ts:163", "hub/scheduler.ts:308", "hub/scheduler.ts:457", "core/session-coordinator.ts:8385", "core/session-coordinator.ts:8657"], capabilities=["PI-01", "PI-02", "PI-03", "PI-04", "PI-05", "PI-06", "PI-07", "PI-08", "PI-09", "PI-10", "PI-11"], activation="定时/心跳开启且到触发时间"),
    dict(id="E08", surface="压缩 AgentRun 旁路", chain=["server/index.ts:496", "lib/extensions/compaction-guard-ext.ts:237", "lib/llm/cache-preserving-compaction-agent-run.ts:550"], capabilities=["PI-01", "PI-03", "PI-08", "PI-10"], activation="达到压缩阈值或显式压缩"),
]

# ── W1-W15：从现役执行入口与终态写入点双向枚举后的完整分类 ──────────────────
# verdict 含义：KERNEL_MIGRATION=该链的调度、模型决策、任务终态及父会话续跑由 Rust 唯一负责；
# PERIPHERAL_CANDIDATE=限权单次外围操作候选；ADJACENT_SESSION_STATE=会话相邻状态权威（随会话服务迁移）。
# 锚点结构化：ref 严格为 "路径" 或 "路径:行号[-行号][,行号…]"，note 为纯语义说明；
# note 中出现文件样 token 直接报错，防止真实锚点借语义说明绕过行号核验。
def sink(ref=None, note=None):
    entry = {}
    if ref is not None:
        entry["ref"] = ref
    if note is not None:
        entry["note"] = note
    if not entry:
        raise SystemExit("empty structured anchor")
    return entry


WORKERS = [
    dict(id="W1-1", candidate="hub/agent-executor.ts",
         role="Hub 临时/电话代理执行器",
         evidence=["hub/agent-executor.ts:290", "hub/agent-executor.ts:343", "hub/agent-executor.ts:556", "hub/agent-executor.ts:716"],
         terminal_state_sinks=[sink(note="activity 终态由调用方 scheduler 写（W1-3 sink）")],
         injection_or_call_source=[sink("hub/index.ts:334", "ephemeral 分支"), sink("hub/channel-router.ts:820"), sink("hub/dm-router.ts:187")],
         activation="频道、电话或临时任务触发",
         observed="创建 Pi session 并多轮 prompt，持有模型循环；电话会话结果交由 router/ticker 决定回复或跳过。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust RunSupervisor/AgentLoop",
         allowed_peripheral="仅可保留已授权单项平台收发/编解码",
         protocol_test="临时/电话会话取消、超时、重试与唯一终态对照",
         retirement_evidence="生产调用图无该执行器；Hub 轮次由 Rust 调度"),
    dict(id="W1-2", candidate="lib/llm/cache-preserving-compaction-agent-run.ts",
         role="压缩旁路 AgentRun",
         evidence=["lib/llm/cache-preserving-compaction-agent-run.ts:550", "lib/extensions/compaction-guard-ext.ts:237"],
         terminal_state_sinks=[sink(note="压缩摘要写回会话历史（经 PI-03 压缩链），不自持独立任务终态")],
         injection_or_call_source=[sink("lib/extensions/compaction-guard-ext.ts:237", "达到阈值触发")],
         activation="达到压缩阈值或显式压缩",
         observed="直接运行 Pi runAgentLoop，可能产生工具意图与模型调用。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust ContextService/ModelGateway",
         allowed_peripheral="纯摘要文本格式化可保留为确定性函数",
         protocol_test="压缩失败不覆原历史、取消及 trace 对照",
         retirement_evidence="Pi runAgentLoop 不在新上下文路径"),
    dict(id="W1-3", candidate="hub/scheduler.ts",
         role="Hub 后台任务调度器",
         evidence=["hub/scheduler.ts:440", "hub/scheduler.ts:457", "hub/scheduler.ts:473", "hub/scheduler.ts:514"],
         terminal_state_sinks=[sink("hub/scheduler.ts:514", "activity 终态写入")],
         injection_or_call_source=[sink("server/index.ts:557"), sink("hub/index.ts:367", "engine.executeIsolated 注入")],
         activation="cron/heartbeat 到触发时间",
         observed="调用隔离 Agent session 且写 activity status，不能整体留作工具 worker。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust Scheduler/RunSupervisor",
         allowed_peripheral="OS 定时唤醒可做无决策通知",
         protocol_test="任务终态唯一写者、取消/超时、重启恢复对照",
         retirement_evidence="旧 Node 调度器不再决策任何任务执行"),
    dict(id="W1-4", candidate="lib/bridge/bridge-manager.ts",
         role="Bridge 平台管理器",
         evidence=["lib/bridge/bridge-manager.ts:2093", "lib/bridge/bridge-manager.ts:2369", "server/index.ts:913"],
         terminal_state_sinks=[sink(note="进行中/完成结果状态维护（bridge turn 结果），终态落会话与活动链")],
         injection_or_call_source=[sink("server/index.ts:871", "延迟加载"), sink("core/bridge-session-manager.ts:1305")],
         activation="平台连接启用且有凭证",
         observed="触发 loop turn 并维护进行中/完成结果，不是纯 SDK 编解码。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust Bridge ingress/Scheduler/RunSupervisor",
         allowed_peripheral="平台 SDK 消息接收/发送可保留为单次外围操作",
         protocol_test="多平台轮次预算、取消、重投去重对照",
         retirement_evidence="Bridge 轮次决策全部在 Rust"),
    dict(id="W1-5", candidate="plugins/office/lib/read-document.ts",
         role="Office 文档单次解析",
         evidence=["plugins/office/lib/read-document.ts:1", "plugins/office/tools/read-document.ts:1"],
         terminal_state_sinks=[sink(note="无；一次解析返回结果，不写任务/会话终态")],
         injection_or_call_source=[sink(note="Office 内置插件工具注册")],
         activation="用户读取 Office 文件",
         observed="一次文档解析；不创建 Pi session 或写 run 终态。",
         verdict="PERIPHERAL_CANDIDATE", rust_owner="Rust ToolInvocationGateway 授权和结果归属",
         allowed_peripheral="受控解析 worker；限定文件、字节、超时和输出",
         protocol_test="文件授权、字节上限、超时与取消对照",
         retirement_evidence="外围正例：无 session、无终态写入、无多轮模型调用"),
    dict(id="W2", candidate="lib/tools/subagent-tool.ts",
         role="子代理新建/续接/中止",
         evidence=["lib/tools/subagent-tool.ts:466", "lib/tools/subagent-tool.ts:551", "lib/tools/subagent-tool.ts:802", "lib/tools/subagent-tool.ts:865"],
         terminal_state_sinks=[sink("lib/tools/subagent-tool.ts:382", "defer"), sink("lib/tools/subagent-tool.ts:576,619", "fail"), sink("lib/tools/subagent-tool.ts:584,883", "resolve"), sink("lib/tools/subagent-tool.ts:567,592,627", "hub 终态 upsert"), sink("lib/subagent-thread-store.ts:248", "finishRun"), sink("lib/subagent-run-store.ts:159,243-285", "runStore resolve/fail 持久 sink")],
         injection_or_call_source=[sink("core/agent.ts:936"), sink("core/engine.ts:2304"), sink("core/session-coordinator.ts:8048,8385,8657", "executeIsolated 实现")],
         activation="用户调用 subagent 工具即激活；主动委派实验开关只管自动建议",
         observed="后台派出/续接隔离 Agent，自行对 deferred store、run store、thread store 和 hub 写 resolved/failed/aborted 终态。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust RunSupervisor/TaskRegistry/DeferredResult/Thread",
         allowed_peripheral="无；单次检索/编码可另走工具网关",
         protocol_test="新建/续接/取消/重复回调/重启恢复、完成/失败/中止终态唯一写者对照",
         retirement_evidence="旧 store 不再是权威；Node 不再裁决子代理终态"),
    dict(id="W3", candidate="lib/tools/workflow-tool.ts",
         role="Workflow 节点执行/恢复",
         evidence=["lib/tools/workflow-tool.ts:283", "lib/tools/workflow-tool.ts:304", "lib/tools/workflow-tool.ts:320", "lib/tools/workflow-tool.ts:418"],
         terminal_state_sinks=[sink("lib/tools/workflow-tool.ts:336,344", "store.resolve/fail"), sink("lib/tools/workflow-tool.ts:418-429", "thread finishRun"), sink("lib/tools/workflow-tool.ts:338,345", "hub done/failed")],
         injection_or_call_source=[sink("core/agent.ts:940"), sink("lib/workflow/host-api.ts:278", "脚本节点调隔离 Agent"), sink("lib/workflow/sandbox.ts", "runWorkflowScript 执行宿主")],
         activation="per-agent tools.disabled 控制，源码注释默认关；启用后节点产生多次模型调用",
         observed="后台运行脚本节点的隔离 Agent，并自行写 done/failed 终态与 deferred/thread/hub。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust RunSupervisor/Workflow journal/TaskRegistry",
         allowed_peripheral="脚本沙箱可限权外围执行单步；不得决定节点终态",
         protocol_test="成功/失败/取消/恢复/重复节点与父 turn 交付对照",
         retirement_evidence="关闭开关不等于可漏账；workflow 终态由 Rust 唯一写入"),
    dict(id="W4", candidate="lib/loop/loop-controller.ts",
         role="循环任务控制器",
         evidence=["lib/loop/loop-controller.ts:125", "lib/loop/loop-controller.ts:145", "lib/loop/loop-controller.ts:179", "lib/loop/loop-controller.ts:280", "lib/loop/loop-controller.ts:353", "lib/loop/loop-controller.ts:366"],
         terminal_state_sinks=[sink("lib/loop/loop-controller.ts:185", "status completed"), sink("lib/loop/loop-controller.ts:125,280,353", "stopped"), sink("lib/loop/loop-controller.ts:366", "paused"), sink("lib/loop/loop-store.ts", "持久 loop-state")],
         injection_or_call_source=[sink("server/index.ts:488,910", "LoopStore 注入"), sink("core/engine.ts:1435", "LoopController 装配"), sink("lib/tools/loop-control-tool.ts:49", "工具入口"), sink("core/engine.ts:1441", "deliverCustomMessage triggerTurn")],
         activation="用户启动循环后按闹钟/恢复触发；桌面走 deliverCustomMessage(triggerTurn)，Bridge 走 executeLoopTurn",
         observed="写持久化 completed/stopped/paused 并触发后续 turn。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust LoopService/RunSupervisor",
         allowed_peripheral="无；闹钟唤醒可由 OS timer 通知，但决策与终态在 Rust",
         protocol_test="完成/暂停/恢复/会话换代/后台任务守恒对照",
         retirement_evidence="Node 不再写 loop-state"),
    dict(id="W5", candidate="server/routes/desk.ts",
         role="桌面封面生成路由",
         evidence=["server/routes/desk.ts:488", "server/routes/desk.ts:836", "server/routes/desk.ts:888"],
         terminal_state_sinks=[sink("server/routes/desk.ts:903", "done/error"), sink("server/routes/desk.ts:913", "error"), sink("lib/desk/activity-store.ts:65", "持久化")],
         injection_or_call_source=[sink("server/routes/desk.ts:888", "engine.executeIsolated，超时 abort")],
         activation="美化插件及工具启用、图片模型可用、用户发起生成",
         observed="HTTP 路由建活动并 executeIsolated，超时 abort，自写 done/error。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust ActivityService/RunSupervisor",
         allowed_peripheral="图片格式转换可外围",
         protocol_test="HTTP 成功/失败/超时/并发重复收尾及 ActivityStore 退出对照",
         retirement_evidence="活动终态由 Rust 唯一写"),
    dict(id="W6", candidate="lib/desk/cron-scheduler.ts",
         role="desk/studio cron 内层调度器",
         evidence=["lib/desk/cron-scheduler.ts:63", "lib/desk/cron-scheduler.ts:137", "lib/desk/cron-scheduler.ts:175"],
         terminal_state_sinks=[sink("lib/desk/cron-scheduler.ts:137", "logRun success"), sink("lib/desk/cron-scheduler.ts:175", "logRun error"), sink("lib/desk/cron-store.ts", "logRun/markRun 持久"), sink("core/studio-cron-service.ts:194", "studio 转发")],
         injection_or_call_source=[sink("lib/desk/automation-executors.ts", "agent_session executor 描述缺省"), sink("hub/scheduler.ts:457", "executeIsolated")],
         activation="到期且 job 启用；heartbeat 按配置；一次失败也推进游标",
         observed="独自判到期、超时、推进游标、logRun 成败，与 hub scheduler 同属任务决策，不能只迁 Agent 调用。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust CronService/Scheduler/RunSupervisor",
         allowed_peripheral="无；OS 定时唤醒仅通知",
         protocol_test="配置换代、超时、失败退避、重启、恰好一次游标对照",
         retirement_evidence="旧 CronStore 不写权威"),
    dict(id="W7", candidate="hub/channel-router.ts",
         role="频道轮次路由",
         evidence=["hub/channel-router.ts:370", "hub/channel-router.ts:508", "hub/channel-router.ts:790", "hub/channel-router.ts:820"],
         terminal_state_sinks=[sink("lib/channels/channel-ticker.ts:280", "轮询/重试"), sink("lib/channels/channel-ticker.ts:452", "已读书签推进"), sink("lib/channels/channel-store.ts", "updateBookmark 持久 sink")],
         injection_or_call_source=[sink("lib/channels/channel-ticker.ts:452", "注入 router"), sink("hub/channel-router.ts:820", "runAgentPhoneSession")],
         activation="频道总开关默认关；开启、有频道成员/新消息或主动提醒时",
         observed="轮询/重试、更新已读书签，调 runAgentPhoneSession 并决定 reply/pass。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust ChannelService/RunSupervisor",
         allowed_peripheral="平台消息读写可外围单次执行",
         protocol_test="多 agent、重试、取消、书签不提前推进对照",
         retirement_evidence="旧 ticker/router 不再决策"),
    dict(id="W8", candidate="hub/dm-router.ts",
         role="Agent 私信多轮路由",
         evidence=["hub/dm-router.ts:144", "hub/dm-router.ts:187", "hub/dm-router.ts:189", "hub/dm-router.ts:292"],
         terminal_state_sinks=[sink("hub/dm-router.ts:189-343", "MAX_ROUNDS 循环内写双方 DM 文件与 phone activity")],
         injection_or_call_source=[sink("hub/index.ts:1041", "注入回调"), sink("hub/agent-executor.ts:556,716", "runAgentPhoneSession")],
         activation="频道总开关默认关；启用后新私信触发",
         observed="去重/冷却后最多 MAX_ROUNDS 轮调用 runAgentPhoneSession，按 [NO_REPLY]/<done/> 停止并写双方 DM 与 phone activity。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust DmSessionService/RunSupervisor",
         allowed_peripheral="文件/平台收发可外围",
         protocol_test="无回复、done、轮数上限、重复回调/重启、双边消息一致性对照",
         retirement_evidence="Node 不再决定 DM 终止"),
    dict(id="W9", candidate="lib/deferred-result-coordinator.ts",
         role="延迟结果协调器",
         evidence=["lib/deferred-result-coordinator.ts:20", "lib/deferred-result-coordinator.ts:55", "lib/deferred-result-coordinator.ts:127"],
         terminal_state_sinks=[sink("lib/deferred-result-store.ts:139,159,204,243-285,470", "结果/中止写入")],
         injection_or_call_source=[sink("server/index.ts:480-486", "store 创建"), sink("core/engine.ts:994-1005", "coordinator 启动"), sink("lib/deferred-result-coordinator.ts:127-130", "deliverCustomMessage triggerTurn")],
         activation="任一后台任务返回、重启后补投或重试",
         observed="决定是否 deliverCustomMessage(...,{triggerTurn}) 续跑父会话，是父 turn 续跑决策者。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust DeferredResult/RunSupervisor",
         allowed_peripheral="外围只能回传结果",
         protocol_test="结果先到/会话失效/重复/崩溃恢复、交付屏蔽与幂等对照",
         retirement_evidence="旧 deferred JSON 不再权威"),
    dict(id="W10", candidate="server/task-bus-handlers.ts",
         role="通用任务状态入口与取消",
         evidence=["server/task-bus-handlers.ts:20", "server/task-bus-handlers.ts:44", "server/deferred-result-bus-handlers.ts:91", "server/deferred-result-bus-handlers.ts:110"],
         terminal_state_sinks=[sink("server/task-bus-handlers.ts:20-63", "register/update/complete/fail/abort"), sink("server/deferred-result-bus-handlers.ts:91-115", "deferred:resolve/fail/abort"), sink("lib/task-registry.ts", "TaskRegistry 持久 sink 与 attempt 栅栏")],
         injection_or_call_source=[sink("core/engine.ts:866-889", "TaskRegistry 持久"), sink("server/routes/chat.ts:485-509", "停止子代理"), sink("lib/tools/stop-task-tool.ts:93"), sink("core/session-turn-actions.ts:789-808", "中止/回档旧任务")],
         activation="插件/用户停任务、重试与回档",
         observed="对插件暴露任务登记/更新/完成/失败/中止与 deferred 结果写入口；插件不得直接裁决任务终态。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust TaskRegistry 公共写入口与主体/attempt 校验",
         allowed_peripheral="单次插件操作可外围；所有 bus 写者可枚举",
         protocol_test="旧 attempt、重复/跨会话/插件消息、取消竞态对照",
         retirement_evidence="任务终态唯一写者为 Rust"),
    dict(id="W11", candidate="lib/exec-command/background.ts",
         role="命令转后台",
         evidence=["lib/exec-command/tool.ts:120", "lib/exec-command/tool.ts:180", "lib/exec-command/background.ts:125", "lib/exec-command/background.ts:129"],
         terminal_state_sinks=[sink("lib/exec-command/background.ts:125", "TaskRegistry.complete"), sink("lib/exec-command/background.ts:129", "DeferredResultStore.resolve")],
         injection_or_call_source=[sink("lib/exec-command/tool.ts:180-260", "wait_mode=auto 转后台")],
         activation="长命令超出前台窗口后才转后台；PTY 退出触发",
         observed="进程退出后 TaskRegistry.complete + DeferredResultStore.resolve，决定父 turn 回送。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust TaskRegistry/DeferredResult",
         allowed_peripheral="命令/PTY 可限权外围",
         protocol_test="窗口内退出、超窗、非零退出、取消、迟到结果/旧 attempt 对照",
         retirement_evidence="任务登记、终态与父 turn 属 Rust"),
    dict(id="W12", candidate="lib/tools/rewind-tool.ts",
         role="回档后台化",
         evidence=["lib/tools/rewind-tool.ts:131", "lib/tools/rewind-tool.ts:160"],
         terminal_state_sinks=[sink("lib/tools/rewind-tool.ts:160-170", "execution resolve/fail + deferredStore.fail")],
         injection_or_call_source=[sink("core/agent.ts:863", "装配"), sink("core/agent.ts:1224", "纳入快照")],
         activation="用户确认回档且会话仍流式；否则走直接回档",
         observed="当前轮仍流式时延迟回档，写 registry/deferred 成败。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust SessionService 回档事务/TaskRegistry",
         allowed_peripheral="文件恢复可受控单次操作",
         protocol_test="确认、流结束后执行、失败回滚、被丢弃后台任务取消对照",
         retirement_evidence="会话回档事务与后台终态属 Rust"),
    dict(id="W13", candidate="core/media/poller.ts",
         role="媒体生成/轮询",
         evidence=["core/media/poller.ts:154", "core/media/poller.ts:303", "core/media/poller.ts:326", "core/media/poller.ts:343", "core/media/poller.ts:379"],
         terminal_state_sinks=[sink("core/media/poller.ts:154", "cancelled"), sink("core/media/poller.ts:303", "settleTask"), sink("core/media/poller.ts:343-347", "deferred:resolve/fail/abort"), sink("core/media/task-store.ts:391-393,471,505", "持久终态"), sink("core/media/image-task-runner.ts:458", "提交失败直接终态"), sink("core/media/submit-image.ts", "提交路径登记任务")],
         injection_or_call_source=[sink("core/engine.ts:583,3748", "UniversalMediaManager 装配"), sink("core/media/universal-media-manager.ts:577-595", "poller 启动")],
         activation="媒体任务提交、供应商异步查询或重启恢复",
         observed="写 done/failed/cancelled 并经 deferred bus 交付，含仅 UI 记录与父 turn 两种交付。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust MediaTaskService/DeferredResult",
         allowed_peripheral="供应商查询/下载可外围",
         protocol_test="重启 pending、同一 attempt、提交失败、取消、两种交付对照",
         retirement_evidence="持久 task 与交付决策属 Rust"),
    dict(id="W14", candidate="lib/memory/dream/runner.ts",
         role="记忆 Dream 多阶段作业",
         evidence=["lib/memory/dream/runner.ts:98", "lib/memory/dream/runner.ts:150", "lib/memory/dream/runner.ts:250", "lib/memory/dream/runner.ts:314"],
         terminal_state_sinks=[sink("lib/memory/dream/runner.ts:250-309", "多阶段执行"), sink("lib/memory/dream/state-store.ts:39,71-76,121-129", "succeeded/failed 落盘"), sink("lib/memory/dream/model-runner.ts", "阶段模型调用编排"), sink("lib/memory/dream/revision-store.ts", "修订持久与回滚")],
         injection_or_call_source=[sink("server/routes/memory-dream.ts:66-82", "手动触发"), sink("lib/memory/memory-ticker.ts:744-752", "自动触发")],
         activation="手动或自动条件满足",
         observed="多阶段模型调用并持久写 succeeded/failed；不调用 Pi AgentLoop，但独立持有模型作业与终态，不能因无 executeIsolated 而遗漏。",
         verdict="KERNEL_MIGRATION", rust_owner="Rust MemoryJobService/ModelGateway",
         allowed_peripheral="单次文本处理算法可外围",
         protocol_test="阶段失败、输入变更、重启、取消、修订回滚对照",
         retirement_evidence="模型作业编排与记忆写入权属 Rust"),
    dict(id="W15", candidate="lib/goal/goal-engine.ts",
         role="会话目标相邻状态",
         evidence=["lib/goal/goal-engine.ts:128", "lib/goal/goal-engine.ts:213", "lib/tools/goal-tool.ts:105"],
         terminal_state_sinks=[sink("lib/goal/goal-engine.ts:213-225", "finish completed/dropped")],
         injection_or_call_source=[sink("server/index.ts:972", "单实例挂 engine")],
         activation="用户/模型目标工具及聊天 turn",
         observed="不是独立后台 worker，但写会话 goal 完成/放弃，属会话预算与状态权威；普通 assistant run 终态是 RunSupervisor 基线，不以 worker 反例重复计数。",
         verdict="ADJACENT_SESSION_STATE", rust_owner="Rust SessionService/RunSupervisor（PI-11 交接）",
         allowed_peripheral="无",
         protocol_test="状态、用量、steer 切分与恰好一次收尾对照",
         retirement_evidence="goal 状态随会话服务迁移，Node 不再写权威"),
]

# 发现模式：源头=执行入口/注入转发，终点=终态写入 sink。子串匹配故意过近似
# （如 createAgentSessionAutomationExecutor 含 createAgentSession），由分类台账逐个解释。
# deferred: 覆盖 deferred:register/retry/resolve/fail/abort/query 总线写入口；
# LoopController/deliverLoopMessage 覆盖循环控制器与父 turn 触发；Dream 覆盖记忆作业；
# updateBookmark 覆盖频道已读书签游标。
SOURCE_PATTERNS = ["executeIsolated", "runAgentLoop", "createAgentSession", "runAgentPhoneSession",
                   "executeLoopTurn", "deliverCustomMessage", "runWorkflowScript", "deliverLoopMessage"]
SINK_PATTERNS = ["finishRun", "settleTask", "logRun", "markRun", "TaskRegistry", "DeferredResultStore",
                 "deferredStore", "deferred:", "LoopStore", "LoopController", "Dream", "updateBookmark"]
ALL_WORKER_PATTERNS = SOURCE_PATTERNS + SINK_PATTERNS

# 分类台账：发现集合中的每个文件必须恰好有一条分类；manual_adjacent 为不经模式
# 命中、由 sink 证据手工补入的相邻状态/持久 sink。
WORKER_FILE_CLASSIFICATION = {
    "core/session-coordinator.ts": ("KERNEL_INFRASTRUCTURE", "W2/W3", "executeIsolated 实现位：创建 Pi session 并 prompt；PI-01 内核本体"),
    "core/engine.ts": ("SUPPORTING_WIRING", "W2/W3/W4/W9/W10/W13", "注入枢纽：转发 executeIsolated、装配 LoopController/TaskRegistry/DeferredResult/Media"),
    "core/agent.ts": ("SUPPORTING_WIRING", "W2/W3/W10/W12", "创建 subagent/workflow 工具并注入执行器"),
    "core/agent-manager.ts": ("SUPPORTING_WIRING", "W2", "agent 装配处转发 executeIsolated"),
    "server/index.ts": ("SUPPORTING_WIRING", "W4/W9/W10/W15", "服务接线：LoopStore/DeferredResultStore/TaskRegistry/goalEngine 装配与 executeLoopTurn 桥"),
    "hub/index.ts": ("SUPPORTING_WIRING", "W7/W8", "ephemeral 分支直调 executeIsolated；注入 DM 回调"),
    "server/routes/chat.ts": ("SUPPORTING_WIRING", "W10/W15", "assistant run 终态与停止子代理任务的 HTTP 入口"),
    "core/session-turn-actions.ts": ("SUPPORTING_LIBRARY", "W10", "turn 动作：中止/回档旧任务与 deliverCustomMessage"),
    "hub/scheduler.ts": ("KERNEL_MIGRATION", "W1-3", "后台任务调度：隔离执行并写 activity 终态"),
    "hub/agent-executor.ts": ("KERNEL_MIGRATION", "W1-1", "临时/电话执行器创建 Pi session 并 prompt"),
    "hub/channel-router.ts": ("KERNEL_MIGRATION", "W7", "频道轮次决策：runAgentPhoneSession 后决定 reply/pass"),
    "hub/dm-router.ts": ("KERNEL_MIGRATION", "W8", "私信多轮决策：MAX_ROUNDS 内写双方 DM 与 phone activity"),
    "lib/llm/cache-preserving-compaction-agent-run.ts": ("KERNEL_MIGRATION", "W1-2", "压缩旁路直接 runAgentLoop"),
    "lib/bridge/bridge-manager.ts": ("KERNEL_MIGRATION", "W1-4", "Bridge loop turn 触发与结果状态维护"),
    "core/bridge-session-manager.ts": ("KERNEL_MIGRATION", "W1-4", "Bridge owner 会话创建（createAgentSession）"),
    "lib/tools/subagent-tool.ts": ("KERNEL_MIGRATION", "W2", "子代理后台执行并写 deferred/run/thread/hub 终态"),
    "lib/subagent-thread-store.ts": ("KERNEL_MIGRATION", "W2", "thread run 序列化与 finishRun 持久 sink"),
    "lib/tools/workflow-tool.ts": ("KERNEL_MIGRATION", "W3", "workflow 节点后台执行并写终态"),
    "lib/workflow/host-api.ts": ("KERNEL_MIGRATION", "W3", "脚本节点经 host-api 调用隔离 Agent"),
    "lib/workflow/sandbox.ts": ("KERNEL_MIGRATION", "W3", "runWorkflowScript 脚本执行宿主"),
    "lib/loop/loop-controller.ts": ("KERNEL_MIGRATION", "W4", "循环控制器写 completed/stopped/paused 并触发后续 turn"),
    "lib/loop/loop-store.ts": ("KERNEL_MIGRATION", "W4", "loop-state 持久 sink"),
    "server/routes/desk.ts": ("KERNEL_MIGRATION", "W5", "封面生成路由建活动、executeIsolated、自写 done/error"),
    "lib/desk/cron-scheduler.ts": ("KERNEL_MIGRATION", "W6", "desk/studio cron 内层调度：判到期/超时/游标/logRun"),
    "lib/desk/cron-store.ts": ("KERNEL_MIGRATION", "W6", "cron logRun/markRun 持久 sink"),
    "core/studio-cron-service.ts": ("KERNEL_MIGRATION", "W6", "studio cron 持久存储转发"),
    "lib/channels/channel-ticker.ts": ("KERNEL_MIGRATION", "W7", "频道轮询/重试与已读书签推进"),
    "lib/deferred-result-coordinator.ts": ("KERNEL_MIGRATION", "W9", "延迟结果协调：决定父 turn 续跑"),
    "lib/deferred-result-store.ts": ("KERNEL_MIGRATION", "W9", "deferred 结果/中止持久 sink"),
    "server/task-bus-handlers.ts": ("KERNEL_MIGRATION", "W10", "任务状态公共写入口（register/update/complete/fail/abort）"),
    "server/deferred-result-bus-handlers.ts": ("KERNEL_MIGRATION", "W10", "deferred:resolve/fail/abort 公共写入口"),
    "lib/task-registry.ts": ("KERNEL_MIGRATION", "W10", "TaskRegistry 持久 sink 与 attempt 栅栏"),
    "lib/tools/stop-task-tool.ts": ("KERNEL_MIGRATION", "W10", "停止任务入口"),
    "lib/exec-command/background.ts": ("KERNEL_MIGRATION", "W11", "PTY 退出后 TaskRegistry.complete + deferred resolve"),
    "lib/exec-command/tool.ts": ("KERNEL_MIGRATION", "W11", "wait_mode=auto 前台窗口与转后台决策"),
    "lib/tools/rewind-tool.ts": ("KERNEL_MIGRATION", "W12", "流式中延迟回档并写 registry/deferred 终态"),
    "core/media/poller.ts": ("KERNEL_MIGRATION", "W13", "媒体轮询写 done/failed/cancelled 并经 deferred 交付"),
    "core/media/task-store.ts": ("KERNEL_MIGRATION", "W13", "媒体任务持久终态 sink"),
    "core/media/image-task-runner.ts": ("KERNEL_MIGRATION", "W13", "提交失败直接写终态"),
    "core/media/universal-media-manager.ts": ("KERNEL_MIGRATION", "W13", "媒体管理器装配 poller 与恢复"),
    "core/media/submit-image.ts": ("KERNEL_MIGRATION", "W13", "图片提交路径登记任务"),
    "lib/memory/dream/runner.ts": ("KERNEL_MIGRATION", "W14", "Dream 多阶段模型作业并写 succeeded/failed"),
    "lib/memory/memory-ticker.ts": ("SUPPORTING_WIRING", "W14", "Dream 自动触发位（executeIsolated 仅注释提及）"),
    "lib/pi-sdk/index.ts": ("PI_ADAPTER", "PI-01", "Pi facade：runAgentLoop/createAgentSession 适配层本体"),
    "lib/pi-sdk/model-call-stream-observer.ts": ("PI_ADAPTER", "PI-10", "观测适配；runAgentLoop/createAgentSession 为注释/文档提及"),
    "lib/pi-sdk/session-options.ts": ("PI_ADAPTER", "PI-01", "createAgentSession 参数装配适配"),
    "lib/desk/agent-run-automation.ts": ("SUPPORTING_LIBRARY", "W6", "名称含 createAgentSession 但只构造 {kind:'agent_session'} 执行描述对象"),
    "lib/desk/automation-normalizer.ts": ("SUPPORTING_LIBRARY", "W6", "automation 执行描述规范化"),
    "lib/tools/automation-draft.ts": ("SUPPORTING_LIBRARY", "W6", "automation 草稿配置工具"),
    "lib/tools/automation-tool.ts": ("SUPPORTING_LIBRARY", "W6", "automation notify 配置；实际执行经 hub scheduler"),
    "lib/tools/subagent-tool-policy.ts": ("SUPPORTING_LIBRARY", "W2", "子代理工具策略过滤（executeIsolated 仅注释提及）"),
    "lib/workflow/node-folder-scope.ts": ("SUPPORTING_LIBRARY", "W3", "解析节点写作用域为 executeIsolated 的 folder 入参（注释提及）"),
    "lib/tasks/task-execution.ts": ("SUPPORTING_LIBRARY", "W10", "TaskExecution attempt 栅栏支持库"),
    "lib/tasks/task-identity.ts": ("SUPPORTING_LIBRARY", "W10", "taskId 身份语义支持库"),
    "lib/sandbox/index.ts": ("SUPPORTING_LIBRARY", "W10", "沙盒工具依赖接口暴露 getTaskRegistry"),
    "lib/extensions/deferred-result-ext.ts": ("SUPPORTING_LIBRARY", "W9", "扩展把 deferred 结果面暴露给 agent"),
    "lib/tools/check-deferred-tool.ts": ("SUPPORTING_LIBRARY", "W9", "只读查询 deferred 结果工具"),
    "shared/persistence/store-registry.ts": ("SUPPORTING_LIBRARY", "W10", "持久 store 登记基础设施"),
    "desktop/src/react/hooks/use-stream-buffer.ts": ("RENDERER_PROJECTION", "-", "渲染端流缓冲投影；finishRun 为 UI 流状态，不写服务端任务终态"),
    "desktop/src/react/services/stream-resume.ts": ("RENDERER_PROJECTION", "-", "渲染端断线续流投影"),
    "desktop/src/react/services/ws-message-handler.ts": ("RENDERER_PROJECTION", "-", "渲染端 WS 消息投影"),
    "server/history-read/hydrate.ts": ("READ_ONLY_PROJECTION", "W9", "历史读取对 deferredStore 只读投影"),
    "server/history-read/project-page.ts": ("READ_ONLY_PROJECTION", "W9", "历史分页只读投影"),
    "lib/sandbox/win32-runtime-cache.ts": ("COMMENT_OR_DOC_ONLY", "-", "executeIsolated 仅出现在注释"),
    "lib/llm/model-trace-scope.ts": ("COMMENT_OR_DOC_ONLY", "PI-10", "runAgentLoop 仅出现在注释（trace scope 文档）"),
    "core/model-manager.ts": ("COMMENT_OR_DOC_ONLY", "PI-01", "createAgentSession 仅出现在装配注释"),
    "scripts/patch-pi-sdk.cjs": ("BUILD_OR_TEST_SCRIPT", "-", "postinstall 只读校验脚本，字符串检查含 runAgentLoop/createAgentSession"),
    "scripts/scan-persistent-stores.mjs": ("BUILD_OR_TEST_SCRIPT", "-", "持久 store 扫描脚本"),
    "hub/event-bus-capabilities.ts": ("SUPPORTING_LIBRARY", "W10", "event bus 能力清单含 deferred:* 处理器声明"),
    "lib/channels/channel-store.ts": ("KERNEL_MIGRATION", "W7", "频道已读书签 updateBookmark 持久 sink"),
    "server/routes/channels.ts": ("SUPPORTING_WIRING", "W7", "频道书签/成员 HTTP 入口"),
    "server/deferred-result-interlude.ts": ("SUPPORTING_LIBRARY", "W9", "deferred 等待期间的用户提示投影"),
    "lib/memory/dream/state-store.ts": ("KERNEL_MIGRATION", "W14", "Dream succeeded/failed 持久 sink"),
    "lib/memory/dream/revision-store.ts": ("KERNEL_MIGRATION", "W14", "Dream 修订持久 sink（applyDreamSections/回滚）"),
    "lib/memory/dream/model-runner.ts": ("KERNEL_MIGRATION", "W14", "Dream 各阶段模型调用编排"),
    "lib/memory/dream/memory-units.ts": ("SUPPORTING_LIBRARY", "W14", "Dream 记忆单元文本处理"),
    "lib/memory/prompts/dream.ts": ("SUPPORTING_LIBRARY", "W14", "Dream 阶段提示词模板"),
    "server/routes/memory-dream.ts": ("SUPPORTING_WIRING", "W14", "Dream 手动触发 HTTP 入口"),
    "server/composition/open-root.ts": ("SUPPORTING_WIRING", "W14", "open 组合根注册 memory-dream 路由"),
    "shared/error-user-messages.ts": ("SUPPORTING_LIBRARY", "W14", "Dream 稳定错误码文案"),
    "lib/llm/model-call-integration.ts": ("SUPPORTING_LIBRARY", "PI-10", "Dream 命中来自 MC-07 Dreamina CLI 注释；外部进程 attempt 观测"),
    "lib/tools/loop-control-tool.ts": ("SUPPORTING_LIBRARY", "W4", "循环启停工具入口"),
    "plugins/jimeng-cli/adapters/dreamina.ts": ("BUNDLED_PLUGIN_ADAPTER", "W13", "Dream 命中来自 Dreamina 模型名（即梦 CLI 插件），与记忆 Dream 无关；供应商调用按 W13/R07 外围边界处理"),
    "plugins/jimeng-cli/index.ts": ("BUNDLED_PLUGIN_ADAPTER", "W13", "即梦 CLI 插件入口；Dream 命中同上"),
    "plugins/jimeng-cli/lib/dreamina-capabilities.ts": ("BUNDLED_PLUGIN_ADAPTER", "W13", "即梦模型能力描述；Dream 命中同上"),
    "desktop/src/react/components/input/CompactionAskDialog.tsx": ("RENDERER_PROJECTION", "-", "渲染端压缩确认对话框；deferred: 为样式/状态字符串"),
    "desktop/src/react/components/preview/cover-gallery-assets.ts": ("RENDERER_PROJECTION", "W5", "封面资产描述；Dream 命中来自封面主题/模型名"),
    "desktop/src/react/settings/tabs/AgentTab.tsx": ("RENDERER_PROJECTION", "W14", "设置页含 Dream 入口 UI"),
    "desktop/src/react/settings/tabs/agent/AgentMemory.tsx": ("RENDERER_PROJECTION", "W14", "记忆设置 UI"),
    "desktop/src/react/settings/tabs/agent/AgentMemoryDream.tsx": ("RENDERER_PROJECTION", "W14", "Dream 设置 UI"),
    "desktop/src/react/settings/tabs/agent/DreamRevisionBrowser.tsx": ("RENDERER_PROJECTION", "W14", "Dream 修订浏览 UI"),
    "desktop/src/react/settings/tabs/agent/agent-memory-dream-actions.ts": ("RENDERER_PROJECTION", "W14", "Dream 设置动作"),
    "desktop/src/react/settings/tabs/agent/dream-error-presenter.ts": ("RENDERER_PROJECTION", "W14", "Dream 错误展示"),
    "desktop/src/react/utils/history-builder.ts": ("RENDERER_PROJECTION", "W9", "历史渲染中的 deferred 任务块投影"),
    "desktop/src/react/utils/turn-projector.ts": ("RENDERER_PROJECTION", "W9", "turn 投影中的 deferred 状态"),
}
# 手工补入的相邻状态/持久 sink（未被模式命中，需 sink 证据锚点支撑）。
MANUAL_ADJACENT = {
    "lib/goal/goal-engine.ts": ("ADJACENT_SESSION_STATE", "W15", "写会话 goal completed/dropped；相邻会话状态权威"),
    "lib/tools/goal-tool.ts": ("ADJACENT_SESSION_STATE", "W15", "goal 工具入口调 engine.complete"),
    "lib/desk/activity-store.ts": ("KERNEL_MIGRATION", "W5", "封面活动持久 sink（W5 终态落盘）"),
    "lib/subagent-run-store.ts": ("KERNEL_MIGRATION", "W2", "subagent run 持久 sink（runStore.resolve/fail）"),
    "lib/desk/automation-executors.ts": ("SUPPORTING_LIBRARY", "W6", "cron job executor 描述缺省构造"),
}

discovered = {}
for file in CONTROLLED:
    text = (ROOT / file).read_text(errors="replace")
    matched = [p for p in ALL_WORKER_PATTERNS if p in text]
    if matched:
        discovered[file] = matched

classified = WORKER_FILE_CLASSIFICATION
unclassified = sorted(set(discovered) - set(classified))
if unclassified:
    raise SystemExit(f"worker discovery closure violated, unclassified files: {unclassified}")
stale = sorted(set(classified) - set(discovered))
if stale:
    raise SystemExit(f"worker classification entries not discovered by patterns (move to MANUAL_ADJACENT if adjacent sink): {stale}")
missing_files = sorted(f for f in [*classified, *MANUAL_ADJACENT] if not (ROOT / f).is_file())
if missing_files:
    raise SystemExit(f"classification references missing files: {missing_files}")

worker_discovery = dict(
    method="源头子串：execute 入口/注入转发；终点子串：终态 sink。子串匹配故意过近似，逐文件分类解释；闭合要求发现集合与分类台账（含手工相邻补入）双向差集为空。",
    source_patterns=SOURCE_PATTERNS, sink_patterns=SINK_PATTERNS,
    discovered_files=sorted(discovered),
    classification={f"{f}": dict(klass=k, worker_ref=w, reason=r) for f, (k, w, r) in sorted(classified.items())},
    manual_adjacent={f: dict(klass=k, worker_ref=w, reason=r) for f, (k, w, r) in sorted(MANUAL_ADJACENT.items())},
    closure=dict(unclassified=unclassified, stale=stale,
                 discovered_count=len(discovered), classified_count=len(classified), manual_adjacent_count=len(MANUAL_ADJACENT)),
)

# W 表自身的完整性约束：evidence 与结构化锚点 ref 逐条严格校验（格式、文件存在、行号在界内）；
# note 只允许纯语义说明，内含文件样 token 即报错；台账中所有 kernel 分类文件必须被某条 W 条目引用。
worker_candidate_files = {w["candidate"] for w in WORKERS}
kernel_ledger_files = {f for f, (k, _, _) in {**classified, **MANUAL_ADJACENT}.items()
                       if k in {"KERNEL_MIGRATION", "KERNEL_INFRASTRUCTURE", "ADJACENT_SESSION_STATE"}}

# 严格锚点格式：裸路径 或 路径:行号[-行号][,行号…]；行号 1 起始
ANCHOR_RE = re.compile(r"^[A-Za-z0-9_./@\\-]+(?::[1-9]\d*(?:-[1-9]\d*)?(?:,[1-9]\d*(?:-[1-9]\d*)*)*)?$")
# note 中不得出现文件样 token（相对路径或扩展名），防止真实锚点借语义说明绕过核验。
NOTE_FILE_RE = re.compile(r"(?:[A-Za-z0-9_./@\\-]+/)?[A-Za-z0-9_.@\\-]+\.(?:ts|tsx|js|cjs|mjs|json)\b")


def anchor_ref_lines_exist(anchor):
    """严格校验结构化锚点：返回 (违规说明 or None, 引用文件 or None)。"""
    if ":" in anchor:
        file, rest = anchor.rsplit(":", 1)
    else:
        file, rest = anchor, None
    source = ROOT / file
    if not source.is_file():
        return f"file missing: {anchor}", file
    if rest is None:
        return None, file
    total = len(source.read_text(errors="replace").splitlines())
    for part in rest.split(","):
        if "-" in part:
            a, b = part.split("-", 1)
            if not (a.isdigit() and b.isdigit() and 0 < int(a) <= int(b) <= total):
                return f"line range out of bounds: {anchor} (file has {total} lines)", file
        elif not (part.isdigit() and 0 < int(part) <= total):
            return f"line out of bounds: {anchor} (file has {total} lines)", file
    return None, file


worker_anchor_errors = []
worker_ref_files = set()
for w in WORKERS:
    worker_ref_files.add(w["candidate"])
    for entry in w["evidence"]:
        if not ANCHOR_RE.match(entry):
            worker_anchor_errors.append(f"{w['id']} evidence 非严格锚点格式: {entry!r}")
            continue
        error, file = anchor_ref_lines_exist(entry)
        if error:
            worker_anchor_errors.append(f"{w['id']} evidence {error}")
        else:
            worker_ref_files.add(file)
    for field in ("terminal_state_sinks", "injection_or_call_source"):
        for structured in w[field]:
            ref, note = structured.get("ref"), structured.get("note")
            if not ref and not note:
                worker_anchor_errors.append(f"{w['id']} {field} 空锚点")
                continue
            if ref is not None:
                if not ANCHOR_RE.match(ref):
                    worker_anchor_errors.append(f"{w['id']} {field} ref 非严格锚点格式: {ref!r}")
                    continue
                error, file = anchor_ref_lines_exist(ref)
                if error:
                    worker_anchor_errors.append(f"{w['id']} {field} {error}")
                else:
                    worker_ref_files.add(file)
            if note is not None and NOTE_FILE_RE.search(note):
                worker_anchor_errors.append(f"{w['id']} {field} note 含文件样 token，须改用 ref: {note!r}")
if worker_anchor_errors:
    raise SystemExit("worker anchor gate failed:\n  " + "\n  ".join(worker_anchor_errors))

uncovered_kernel = sorted(kernel_ledger_files - worker_ref_files)
if uncovered_kernel:
    raise SystemExit(f"kernel-classified files not referenced by any W entry: {uncovered_kernel}")


production_imports = [r for r in GRAPH["adapter_imports"] if r["file"] != "lib/pi-sdk/index.ts"]


def imported_capabilities(record):
    if record["resolved"] != "lib/pi-sdk/index.ts":
        return [submodule_owner[record["resolved"]]]
    if not record["symbols"] and record["file"] == "scripts/lib/history-read-fixture.mjs":
        return ["PI-02"]  # 测试夹具运行时解构 SessionManager；非生产入口。
    return sorted({SYMBOL_OWNER[s["imported"]] for s in record["symbols"]})


matrix = dict(schema="r00-t03-pi-replacement-v2", task="R00-T03", base_sha=TASK_BASE_SHA,
              observed_head=HEAD,
              evidence_level="源码静态分类及生产组合调用链人工复核；没有启动真实服务、模型或外部平台",
              graph_artifact="R00-T03_IMPORT_GRAPH.json", capabilities=CAPS,
              production_entrypoint_capabilities=ENTRYPOINTS,
              adapter_import_coverage=[dict(source_ref=f'{r["file"]}:{r["line"]}', symbols=[s["imported"] for s in r["symbols"]],
                                            capability_ids=imported_capabilities(r),
                                            scope="test_fixture" if r["file"] == "scripts/lib/history-read-fixture.mjs" else "production_source",
                                            note="子模块直接导入按 hook/adapter 账本覆盖" if r["resolved"] != "lib/pi-sdk/index.ts" else None)
                                       for r in production_imports],
              pi_vendor_import_coverage=[dict(source_ref=f'{r["file"]}:{r["line"]}', specifier=r["specifier"],
                                              capability_ids=vendor_owner_by_file[r["file"]],
                                              production_reachable=bool(r["reachable_from"]),
                                              static_runtime_reachable=bool(r["static_runtime_reachable_from"]))
                                         for r in GRAPH["vendor_pi_imports"]],
              facade_exports_without_named_import=[
                  dict(symbol=e["symbol"], source_ref=f'lib/pi-sdk/index.ts:{e["line"]}',
                       finding=("namespace call in core/model-image-preprocess.ts:203" if e["symbol"] in
                                {"resizeModelImageInput", "formatModelImageDimensionNote"} else
                                "internal facade call" if e["symbol"] == "createModelRuntime" else
                                "direct submodule consumer exists" if e["symbol"] == "emitSessionShutdown" else
                                "no named import found in scanned tracked source; dynamic plugin/reflection not disproved"))
                  for e in GRAPH["facade_public_exports"] if not e["static_consumers"]
              ],
              extension_hooks=hooks,
              opaque_dynamic_imports=opaque_classification,
              nonimport_dynamic_boundaries=[
                  dict(source_ref="lib/pi-sdk/session-options.ts:9", fact="import.meta.resolve 在运行时解析 Pi 包版本", capability_id="PI-05"),
                  dict(source_ref="server/bootstrap.ts:62", fact="运行时 URL 导入已打包服务入口；图中静态链从 main-full/main-open 另行追踪", capability_id="PI-01"),
                  dict(source_ref="desktop/bootstrap.cjs:195", fact="条件 require 主进程 bundle 或源码；图中静态链从 desktop/main.cjs 另行追踪", capability_id="PI-01"),
                  dict(source_ref="server/index.ts:871", fact="BridgeManager 延迟加载，平台 SDK 静态导入与实际连接分开判定", capability_id="PI-01"),
                  dict(source_ref="scripts/patch-pi-sdk.cjs:1", fact="postinstall 名为 patch，但当前仅验证包版本、导出标记与生产 import 边界，不修改 node_modules", capability_id="PI-05"),
                  dict(source_ref="lib/pi-sdk/index.ts:28", fact="直接使用 node_modules 的 AuthStorage 深路径", capability_id="PI-07"),
                  dict(source_ref="lib/pi-sdk/index.ts:54", fact="直接使用 node_modules 的 prepareCompaction 深路径", capability_id="PI-03"),
              ],
              worker_counterexamples=WORKERS,
              worker_discovery=worker_discovery,
              coverage_difference=dict(unmapped_index_symbols=symbol_gap, unmapped_vendor_files=vendor_gap,
                                       unmapped_adapter_submodules=submodule_gap,
                                       unmapped_hooks=hook_gap, index_symbols_seen=sorted(observed_symbols),
                                       static_adapter_imports=len(GRAPH["adapter_imports"]),
                                       static_vendor_imports=len(GRAPH["vendor_pi_imports"]),
                                       worker_discovery_closure=worker_discovery["closure"],
                                       caveat="静态闭包不证明条件分支实际运行；用户自装插件与反射式动态加载需后续真实运行取证"))


def dep(id, package, role, activation, refs, owner, peripheral, test, exit_evidence, native=False):
    lock_key = f"node_modules/{package}"
    return dict(id=id, package=package, manifest_range=PKG.get("dependencies", {}).get(package) or
                PKG.get("optionalDependencies", {}).get(package) or PKG.get("devDependencies", {}).get(package),
                lock_version=LOCK.get("packages", {}).get(lock_key, {}).get("version"),
                native_or_wasm=native, current_role=role, activation=activation,
                source_refs=refs.split(), rust_authority=owner, peripheral_retention=peripheral,
                protocol_test=test, retirement_evidence=exit_evidence)


DEPS = [
    dep("D01", "@earendil-works/pi-coding-agent", "会话/资源/工具/模型适配核心", "服务启动即静态加载；具体会话按输入激活", "lib/pi-sdk/index.ts:14 core/session-coordinator.ts:2230", "Rust Run/Session/Context/Tool", "旧数据只读导入", "全部 PI-xx 对照", "生产调用图无该包"),
    dep("D02", "@earendil-works/pi-agent-core", "直接 runAgentLoop；普通 session 也间接依赖", "压缩侧 lane 按需运行", "lib/pi-sdk/index.ts:60 lib/llm/cache-preserving-compaction-agent-run.ts:550", "Rust AgentLoop", "无", "压缩及多轮工具闭环", "生产无 AgentLoop import"),
    dep("D03", "@earendil-works/pi-ai", "provider compat、事件流、模型目录和 direct call", "Pi 门面静态加载；请求按模型触发", "lib/pi-sdk/index.ts:34 lib/pi-sdk/stream-guard.ts:2", "Rust ModelGateway", "无模型决策型 worker", "各实际协议流与特殊块", "生产无 Pi model runtime"),
    dep("D04", "better-sqlite3", "会话 manifest、记忆/知识/观测等 SQLite 写读", "服务启动及相应库操作", "core/session-manifest/store.ts:1 lib/memory/fact-store.ts:14 core/data-epoch-checkpoint-provider.ts:291", "Rust Storage/单写者", "只读旧库导入可使用受控工具；新权威库不得双写", "WAL 备份、版本/单写者/迁移", "新权威库只有 Rust 写者", True),
    dep("D05", "node-pty", "交互终端 PTY", "创建终端时动态加载", "lib/terminal/terminal-session-manager.ts:19 lib/terminal/node-pty-backend.ts:8", "Rust ProcessService/ToolGateway", "OS PTY helper 可保留，但会话/权限/取消权在 Rust", "真实 PTY、resize、子孙进程清理", "Rust 工具路径不经 Node PTY 管理器", True),
    dep("D06", "usearch", "可选原生 ANN 索引", "知识向量后端按需选用；有 portable fallback", "lib/knowledge/vector-search-backend-factory.ts:9 lib/knowledge/usearch-vector-backend.ts:28", "Rust KnowledgeService 的索引选择/元数据", "独立向量计算/索引 worker 可保留", "维度/检索排序/索引恢复/后端切换", "新知识权威不由旧 Node 服务写", True),
    dep("D07", "@node-rs/jieba", "中文分词/搜索", "搜索或索引路径调用", "lib/search/search-text.ts:34 lib/search/session-search-tokenizer.ts:73", "Rust Search/KnowledgeService", "纯分词 worker 可保留", "中文查询与历史索引等价", "入口不依赖旧 Node 索引权威", True),
    dep("D08", "@silvia-odwyer/photon-node", "Pi 图片重采样间接调用 WASM", "模型图片压缩触发", "core/model-image-preprocess.ts:203 lib/pi-sdk/index.ts:334", "Rust ModelGateway 入参预算", "单次图像转换 worker 可保留", "MIME/大小/失败与取消", "不通过 Pi resizeImage", True),
    dep("D09", "@firecrawl/anydoc", "文档提取，N-API addon", "read 文件工具触发动态加载", "lib/tools/file-tool.ts:216 lib/document-extract/anydoc-loader.ts:28", "Rust ToolGateway/ResourceService", "受限文档解析 worker", "文件授权、格式/页码/大小、错误", "worker 不读全量 home 或写任务终态", True),
    dep("D10", "mammoth", "docx 提取/HTML 转换", "Office/read 或文件路由按需加载", "lib/sandbox/read-enhanced.ts:110 server/routes/fs.ts:129", "Rust Resource/ToolGateway", "受控 Office worker", "DOCX 内容与资源边界", "无第二任务写者"),
    dep("D11", "exceljs", "xlsx 读取", "表格文件操作时动态加载", "lib/sandbox/read-enhanced.ts:82 server/routes/fs.ts:150", "Rust Resource/ToolGateway", "受控 Office worker", "单元格/公式/大文件限制", "无第二任务写者"),
    dep("D12", "unpdf", "PDF 内容提取", "知识来源/PDF 操作触发", "lib/knowledge/source-adapters.ts:154 plugins/office/lib/read-pdf.ts:1", "Rust Resource/ToolGateway", "受控 PDF worker", "分页/错误/大小与 OCR 边界", "无第二任务写者"),
    dep("D13", "jsdom", "文档/网页 HTML 解析", "知识来源或 Office 操作触发", "lib/knowledge/source-processors.ts:1", "Rust Resource/ToolGateway", "受控 HTML 解析 worker", "不可信 HTML 资源/脚本隔离", "无主界面 native 权限"),
    dep("D14", "@larksuiteoapi/node-sdk", "飞书平台消息收发", "BridgeManager 延迟加载时模块求值；连接须启用凭证", "lib/bridge/feishu-adapter.ts:8 server/index.ts:871", "Rust Bridge ingress/身份/Run", "平台 SDK 编解码/收发 worker", "重投去重、媒体、回执、凭证范围", "SDK worker 不建 Agent session"),
    dep("D15", "node-telegram-bot-api", "Telegram 消息收发", "同 Bridge manager 加载；实际连接按凭证", "lib/bridge/telegram-adapter.ts:8 server/routes/bridge.ts:825", "Rust Bridge ingress/身份/Run", "平台 SDK 收发 worker", "重复消息、附件、回执", "worker 不写任务终态"),
    dep("D16", "undici", "Bridge/其他 HTTP 客户端", "对应外发调用时", "lib/bridge/outbound-http.ts:27 lib/net/outbound-proxy.ts:9", "Rust 网络/工具授权与回执", "单次受控 HTTP helper 可保留", "超时/取消/代理/网络权限", "外发决策在 Rust"),
    dep("D17", "proxy-agent", "Node 代理连接", "配置网络代理后", "lib/net/outbound-proxy.ts:10", "Rust NetworkConfig/ModelGateway", "外围 SDK 自身代理连接可保留", "代理及凭证隔离", "新模型调用不经旧 Node"),
    dep("D18", "hono", "Node HTTP API 路由", "服务启动", "server/index.ts:14", "Rust Service/认证入口", "旧客户端过渡服务仅隔离运行", "HTTP 身份、错误、版本", "正式服务不依赖 Node 路由"),
    dep("D19", "@hono/node-server", "Node HTTP server", "服务启动", "server/index.ts:15", "Rust Service", "无长期内核宿主", "真实监听/停机/认证", "Rust 独立服务", False),
    dep("D20", "@hono/node-ws", "Node WebSocket 适配", "服务启动", "server/index.ts:16", "Rust Service/EventStream", "无长期内核宿主", "重连/订阅/cursor", "Rust WS 真正服务"),
    dep("D21", "ws", "服务和 CLI WebSocket", "服务监听或 CLI 连接", "server/index.ts:17 cli/client.ts:1", "Rust Service/CLI client", "React 浏览器 WS 客户端保留", "历史/实时/重连一致", "Node 仅开发测试时可见"),
    dep("D22", "chokidar", "桌面/技能文件监听", "监听开启后", "desktop/file-watch-adapter.cjs:2 core/skill-manager.ts:9", "Tauri DesktopHost 与 Rust SkillRegistry", "OS 文件通知 helper 可保留", "变更/撤销/关闭清理", "不由 Node 决定工具目录权威"),
    dep("D23", "qrcode", "微信登录二维码", "微信登录流程", "lib/bridge/wechat-login.ts:8 server/routes/access.ts:2", "Rust Bridge auth workflow", "二维码编码可单次 worker", "登录状态/过期/撤销", "凭证与身份在 Rust"),
    dep("D24", "yauzl", "ZIP 提取", "导入包时", "lib/extract-zip.ts:22", "Rust Import/ResourceService", "受控解压 worker", "路径穿越/尺寸/原子性", "worker 不改全局任务权威"),
    dep("D25", "electron", "桌面窗口、浏览器 WebContentsView、PDF printToPDF helper", "桌面/浏览器/PDF 路径", "desktop/main.cjs:2919 desktop/main.cjs:3786 desktop/src/office-pdf-helper.cjs:117", "Tauri DesktopHost + Rust BrowserPort/ToolGateway", "独立可见浏览器/PDF renderer 可保留，不能依赖 Electron 正式宿主", "真实可见浏览器与 PDF 视觉回归", "正式桌面与 PDF 均无 Electron 进程", True),
    dep("D26", "electron-updater", "旧桌面自动更新", "安装版检查/下载", "desktop/auto-updater.cjs:10", "Tauri updater/发布验证", "旧版本过渡更新器隔离保留", "签名/回滚/旧包升级", "Tauri 更新链经真实安装验证"),
]

# package.json 的其余直接生产依赖也逐个登记；多数属于保留的 React 界面。
UI_PACKAGES = {
    "@codemirror/lang-markdown", "@codemirror/language-data", "@codemirror/view",
    "@tanstack/react-virtual", "@tiptap/core", "@tiptap/extension-bold",
    "@tiptap/extension-placeholder", "@tiptap/pm", "@tiptap/react", "@tiptap/starter-kit",
    "@traptitech/markdown-it-katex", "ansi_up", "codemirror", "katex", "markdown-it",
    "markdown-it-task-lists", "mermaid", "motion", "react", "react-dom", "zustand",
}
EXTRA_DIRECT = set(PKG["dependencies"]) | set(PKG.get("optionalDependencies", {}))
mapped_direct = {d["package"] for d in DEPS}
for package in sorted(EXTRA_DIRECT - mapped_direct):
    hits = [r for r in GRAPH["external_imports"]
            if r["specifier"] == package or r["specifier"].startswith(package + "/")]
    refs = [f'{r["file"]}:{r["line"]}' for r in hits]
    if not refs:
        manifest_line = next(number for number, line in enumerate((ROOT / "package.json").read_text().splitlines(), 1)
                             if f'"{package}"' in line)
        refs = [f"package.json:{manifest_line}"]
    if package in UI_PACKAGES:
        role, owner, retained, test, retire = (
            "React/HTML 用户界面呈现、编辑或可视化依赖", "React 界面；Tauri 仅提供宿主能力",
            "保留前端依赖，不能据此推出生产 Agent 仍依赖 Node", "R08/R09 对应 UI 动作、历史与可访问性回归",
            "无需作为 Agent 内核清退；最终产物检查前端可用")
    elif package in {"diff", "js-yaml", "semver"}:
        role, owner, retained, test, retire = {
            "diff": ("服务端文件变化与 React 差异展示", "Rust ResourceService 负责服务端文件变化；React 负责展示", "纯展示计算可留 React", "服务端补丁文本与前端 diff 展示的输入/输出对照", "新文件变更权威不由 Node diff 表示决定"),
            "js-yaml": ("服务端代理/模型/技能/记忆配置解析及前端 Markdown 文档解析", "Rust ConfigService/SkillRegistry 负责服务配置；React 负责文档展示", "前端文档 YAML 解析可留 React", "代理、供应商、技能、记忆 YAML 合法/非法输入和前端文档解析对照", "新服务配置由 Rust 解析并拒绝无效值"),
            "semver": ("旧产物版本排序与桌面启动/更新兼容判断，另用于发布预检", "Tauri updater 与 Rust ArtifactService 负责产物排序；CLI 客户端遵循同一协议", "旧版本过渡期可保留只读排序 helper；发布预检属构建外围", "旧 manifest 无 releaseGeneration、混合 generation、非法版本和升级/回滚排序协议对照", "正式桌面启动和更新不依赖 Node release-order；旧产物排序仍一致"),
        }[package]
    elif package == "typebox":
        role, owner, retained, test, retire = (
            "Pi 工具 schema 构造，经 facade 的 Type 转发", "Rust Protocol/ToolRegistry 执行校验",
            "前端 TypeScript schema 描述可保留，但不作为执行判定源", "全部工具描述/执行 schema 同源和未知 dialect 拒绝",
            "Rust 工具执行不由 Node TypeBox 独自放行")
    else:
        raise SystemExit(f"unclassified direct dependency: {package}")
    DEPS.append(dep(f"D{len(DEPS)+1:02d}", package, role, "见 module_load_activation 与 operation_activation 分栏",
                    " ".join(refs), owner, retained, test, retire))
direct_gap = sorted(EXTRA_DIRECT - {d["package"] for d in DEPS})
if direct_gap:
    raise SystemExit(f"unmapped direct dependencies: {direct_gap}")

# 每项都从完整导入集合取证。静态 import、函数内 require、动态 import 与实际动作
# 分开记录；AST 可达性仅表示组合路径，不等同于进程启动时必然执行。
OPERATION_ACTIVATION = {
    "@earendil-works/pi-coding-agent": "创建会话、运行工具/模型轮次、读取资源或压缩时调用具体能力",
    "@earendil-works/pi-agent-core": "压缩旁路 runAgentLoop 或普通 Pi session 进入模型循环时",
    "@earendil-works/pi-ai": "模型目录、认证或实际模型调用时",
    "better-sqlite3": "会话、记忆、知识、文件历史或观测库打开与读写时",
    "node-pty": "创建交互终端、resize 或清理进程时",
    "usearch": "知识向量后端选择原生索引并检索/写入时",
    "@node-rs/jieba": "中文搜索或索引分词时",
    "@silvia-odwyer/photon-node": "Pi 图片重采样路径接收需转换的图片时",
    "@firecrawl/anydoc": "文件读取工具需要文档提取时",
    "mammoth": "知识 DOCX 来源解析、文件/Office 读取或桌面 DOCX 预览时",
    "exceljs": "知识 XLSX 来源解析、文件/Office 读取或桌面 XLSX 预览时",
    "unpdf": "知识或 Office PDF 提取时",
    "jsdom": "知识 HTML 来源或 WebReader 解析网页时",
    "@larksuiteoapi/node-sdk": "飞书 Bridge 启用且收发消息时",
    "node-telegram-bot-api": "Telegram Bridge 启用且收发消息时",
    "undici": "Bridge/网络代理发起 HTTP 请求时；发布镜像脚本单独运行时",
    "proxy-agent": "已配置代理的外发连接建立时",
    "hono": "服务组装路由、匹配请求与生成响应时",
    "@hono/node-server": "Node 服务开始监听及处理 HTTP 请求时",
    "@hono/node-ws": "Node 服务注册并处理 WebSocket 连接时",
    "ws": "服务/CLI/桌面 WebSocket 或钉钉、QQ Bridge 连接及消息收发时",
    "chokidar": "技能或桌面文件监听开启、收到变更或关闭时",
    "qrcode": "设备远程接入配对 SVG 或微信登录 data URL 生成时",
    "yauzl": "导入技能/角色卡 ZIP 或下载搜索二进制并解压时",
    "electron": "桌面窗口、IPC、可见浏览器或 PDF 输出等宿主动作时",
    "electron-updater": "安装版检查、下载、校验和安装更新时",
    "@codemirror/lang-markdown": "预览编辑器需要 Markdown 语言支持时",
    "@codemirror/language-data": "消息活动或预览编辑器选择语言时",
    "@codemirror/view": "预览/消息编辑器创建视图与执行编辑命令时",
    "@tanstack/react-virtual": "观测轨迹表渲染长列表时",
    "@tiptap/core": "输入框编辑器、徽章或草稿序列化时",
    "@tiptap/extension-bold": "输入框启用粗体扩展时",
    "@tiptap/extension-placeholder": "输入框启用占位提示时",
    "@tiptap/pm": "本仓无直接生产调用；Tiptap starter-kit 锁依赖可间接使用，需由前端打包/运行证据确认",
    "@tiptap/react": "输入框与徽章的 React 编辑器视图渲染时",
    "@tiptap/starter-kit": "输入框启用 Tiptap 基础扩展时",
    "@traptitech/markdown-it-katex": "React Markdown 或桌面截图 Markdown 公式渲染时",
    "ansi_up": "终端卡片展示 ANSI 文本时",
    "codemirror": "本仓无直接生产调用；当前可见编辑器源码使用 @codemirror/*，是否纳入最终产物需构建检查",
    "diff": "服务端文件变化补丁生成或 React 差异展示时",
    "js-yaml": "服务端配置/技能/角色卡/共享 YAML 读取（含 safe-fs 按需 createRequire 加载）或前端 Markdown 文档解析时",
    "katex": "React 公式渲染或桌面截图读取 KaTeX CSS 时",
    "markdown-it": "Bridge 消息格式化、React Markdown 展示或桌面截图渲染时",
    "markdown-it-task-lists": "React Markdown 或桌面截图任务列表渲染时",
    "mermaid": "前端 Mermaid 图表渲染时",
    "motion": "前端媒体与交互动效播放时",
    "react": "各前端入口渲染组件和处理状态时",
    "react-dom": "各前端入口挂载组件、Portal 或弹窗时",
    "semver": "桌面启动/更新产物排序、旧 manifest 兼容比较、CLI 产物判定或发布预检时",
    "typebox": "服务端注册工具 schema 与逐次执行参数校验时",
    "zustand": "前端设置与会话状态容器读写时",
}
if set(OPERATION_ACTIVATION) != {d["package"] for d in DEPS}:
    raise SystemExit("operation activation must cover every dependency exactly")

LOAD_OVERRIDES = {
    "better-sqlite3": "服务可先静态加载数据库包装模块；native addon 在各 store 的 require/动态导入位置（通常数据库打开或构造时）加载",
    "@silvia-odwyer/photon-node": "本仓无直接 import；已安装 Pi 包 photon.js:108 在图片重采样时动态 import 原生/WASM 包",
    "mammoth": "知识 source-processors.ts 为服务组合静态可达的顶层 import；桌面预览在 IPC 内 require，文件/Office 路径在调用时动态 import",
    "exceljs": "知识 source-processors.ts 为服务组合静态可达的顶层 import；桌面预览在 IPC 内 require，文件/Office 路径在调用时动态 import",
    "jsdom": "知识 source-adapters.ts/source-processors.ts 和 web-reader.ts 顶层静态 import，随相应服务模块求值加载",
    "@larksuiteoapi/node-sdk": "飞书 adapter 顶层 import；BridgeManager 在服务 ready 后延迟加载，平台连接再按启用/凭证建立",
    "node-telegram-bot-api": "Telegram adapter 顶层 import；BridgeManager 延迟加载，另有 Bridge 路由函数内动态 import；连接仍按启用/凭证建立",
    "@tiptap/pm": "本仓无直接生产 import；锁文件列作 @tiptap/starter-kit 依赖，不能从静态图推断实际求值",
    "codemirror": "本仓无直接生产 import；不能从 @codemirror/* 的使用推断 codemirror 旧包实际求值",
    "semver": "release-order.cjs 顶层 require semver；该模块被桌面 artifact-boot.cjs 与 ota-core.cjs 在启动/更新链加载，CLI 亦可达；发布预检独立 import",
    "electron-updater": "desktop/auto-updater.cjs 顶层 require，desktop/main.cjs 顶层载入该模块；检查/下载动作稍后触发",
    "chokidar": "core/skill-manager.ts 顶层静态 import，desktop/main.cjs/file-watch-adapter.cjs 顶层 require；启动监听是后续动作",
    "markdown-it": "Bridge 两处顶层静态 import；React Markdown 静态 import；桌面截图 helper 调用时 require",
    "@traptitech/markdown-it-katex": "React Markdown 模块静态 import；桌面截图 helper 调用时 try/require",
    "markdown-it-task-lists": "React Markdown 模块静态 import；桌面截图 helper 调用时 try/require",
    "katex": "React Markdown 模块静态 import；桌面截图 helper 仅在调用时读取 CSS 资源（require.resolve 定位，不执行模块）",
    "js-yaml": "服务端配置/技能/角色卡为顶层静态 import；shared/safe-fs.ts:37 为函数内 createRequire 按调用加载（R2 补漏边）",
}

EXTRA_CALL_CHAIN_REFS = {
    "@earendil-works/pi-coding-agent": ["core/session-coordinator.ts:2230"],
    "@earendil-works/pi-agent-core": ["lib/llm/cache-preserving-compaction-agent-run.ts:550"],
    "node-pty": ["lib/terminal/terminal-session-manager.ts:19"],
    "usearch": ["lib/knowledge/vector-search-backend-factory.ts:9"],
    "@silvia-odwyer/photon-node": ["core/model-image-preprocess.ts:203", "lib/pi-sdk/index.ts:334", "node_modules/@earendil-works/pi-coding-agent/dist/utils/photon.js:108"],
    "@firecrawl/anydoc": ["lib/tools/file-tool.ts:216"],
    "mammoth": ["lib/knowledge/knowledge-manager.ts:47"],
    "exceljs": ["lib/knowledge/knowledge-manager.ts:47"],
    "jsdom": ["lib/knowledge/knowledge-manager.ts:38", "lib/knowledge/knowledge-manager.ts:47"],
    "unpdf": ["lib/knowledge/knowledge-manager.ts:38"],
    "@larksuiteoapi/node-sdk": ["server/index.ts:871"],
    "qrcode": ["server/routes/access.ts:76", "lib/bridge/wechat-login.ts:41"],
    "better-sqlite3": ["lib/file-history/history-store.ts:49"],
    "electron": ["desktop/main.cjs:2919", "desktop/main.cjs:3786", "desktop/src/office-pdf-helper.cjs:117"],
    "typebox": ["core/engine.ts:4133", "core/engine.ts:4186", "core/engine.ts:4390"],
    "semver": ["desktop/src/shared/artifact-boot.cjs:59", "shared/artifact-core/ota-core.cjs:167", "shared/artifact-core/release-order.cjs:10", "cli/bundle.ts:23"],
    "electron-updater": ["desktop/main.cjs:19"],
    "katex": ["desktop/main.cjs:4748", "desktop/src/react/utils/markdown.ts:15"],
    "@tiptap/pm": ["desktop/src/react/components/input/input-editor-extensions.ts:1"],
    "codemirror": ["desktop/src/react/components/PreviewEditor.tsx:18"],
    "proxy-agent": ["lib/bridge/dingtalk-adapter.ts:15", "lib/bridge/qq-adapter.ts:17", "core/mcp/clients/http-client.ts:2"],
    "yauzl": ["lib/skills/skill-package-installer.ts:240", "lib/skills/skill-package-installer.ts:301", "lib/skills/skill-package-installer.ts:390", "lib/pi-sdk/search-tools.ts:233", "lib/character-cards/service.ts:627"],
    "@traptitech/markdown-it-katex": ["desktop/main.cjs:6083"],
    "markdown-it-task-lists": ["desktop/main.cjs:6083"],
    "ws": ["desktop/main.cjs:4547", "lib/bridge/dingtalk-adapter.ts:15", "lib/bridge/qq-adapter.ts:17"],
    "undici": ["server/index.ts:31"],
    "js-yaml": ["shared/safe-fs.ts:37"],
}

OWNER_OVERRIDES = {
    "mammoth": "Rust KnowledgeService/ResourceService/ToolGateway；Tauri DesktopHost 仅提交预览请求",
    "exceljs": "Rust KnowledgeService/ResourceService/ToolGateway；Tauri DesktopHost 仅提交预览请求",
    "jsdom": "Rust KnowledgeService/ResourceService/ToolGateway",
    "unpdf": "Rust KnowledgeService/ResourceService/ToolGateway 负责 PDF 来源与 Office 文件解析",
    "ws": "Rust Service/EventStream、BridgePort、BrowserPort 与 CLI 客户端；Tauri DesktopHost 负责桌面浏览器 WS 宿主连接",
    "chokidar": "Rust SkillRegistry 与 Tauri DesktopHost 文件通知；监听权归相应宿主",
    "qrcode": "Rust Access/DeviceRegistry 负责设备配对凭证及权限；Rust Bridge auth workflow 负责微信登录；编码可单次外围执行",
    "@traptitech/markdown-it-katex": "React 负责界面公式渲染；Tauri DesktopHost 负责截图渲染宿主",
    "markdown-it": "Rust BridgePort 负责服务端消息格式化；React 负责界面；Tauri DesktopHost 负责截图渲染宿主",
    "markdown-it-task-lists": "React 负责界面任务列表；Tauri DesktopHost 负责截图渲染宿主",
    "katex": "React 负责界面公式渲染；Tauri DesktopHost 负责截图 CSS 资源",
    "typebox": "Rust ToolRegistry/InvocationGateway 负责工具描述及运行时参数校验",
    "proxy-agent": "Rust NetworkConfig/ModelGateway/BridgePort/MCP 客户端负责代理配置和外发权限；外围 SDK 连接可留受控 helper",
    "yauzl": "Rust SkillRegistry/ImportService/ResourceService 负责技能包、角色卡和内置搜索工具 ZIP 入口",
}

ROLE_OVERRIDES = {
    "better-sqlite3": "会话、文件历史、记忆、知识、观测等 SQLite 数据库读写",
    "ws": "服务和 CLI WebSocket、钉钉/QQ Bridge 通道、桌面浏览器命令连接",
    "qrcode": "设备远程接入配对凭证 SVG 与微信登录 data URL 编码",
    "@tiptap/pm": "Tiptap 编辑器锁依赖；本仓无直接生产 import，运行消费尚待产物核实",
    "codemirror": "直接依赖但本仓无直接生产 import；当前编辑器直接使用 @codemirror/*",
    "@traptitech/markdown-it-katex": "React Markdown 与桌面截图的公式渲染",
    "markdown-it": "Bridge 消息格式化、React Markdown 与桌面截图渲染",
    "markdown-it-task-lists": "React Markdown 与桌面截图的任务列表渲染",
    "katex": "React 公式样式/渲染与桌面截图 CSS 资源",
    "typebox": "Pi 工具 schema 构造与服务端工具调用的运行时参数校验",
}

RETENTION_OVERRIDES = {
    "@tiptap/pm": "仅在前端产物确实需要时保留；不能把锁依赖当成已证实模块求值",
    "codemirror": "待前端产物核实；不能把 @codemirror/* 用途归给 codemirror 旧包",
    "markdown-it": "React 展示可保留；Bridge 格式化迁入 Rust BridgePort；截图由 Tauri 宿主处理",
    "@traptitech/markdown-it-katex": "React 公式展示可保留；截图渲染由 Tauri 宿主接管",
    "markdown-it-task-lists": "React 任务列表展示可保留；截图渲染由 Tauri 宿主接管",
    "qrcode": "单次二维码编码可保留；配对凭证和登录状态由 Rust 决定",
}

RETIREMENT_OVERRIDES = {
    "mammoth": "Rust 知识与文件入口持有解析结果；旧 Node 只可作为限权单次 DOCX 转换 worker，不能写知识权威",
    "exceljs": "Rust 知识与文件入口持有解析结果；旧 Node 只可作为限权单次 XLSX 转换 worker，不能写知识权威",
    "unpdf": "Rust KnowledgeService 持有 PDF 来源和页定位；旧解析仅作为授权单次 worker",
    "jsdom": "Rust 知识与 WebReader 入口持有资源授权和文本结果；旧 DOM 解析无全局权限",
    "@tiptap/pm": "最终前端包依赖图证明保留必要性；未包含则以产物无依赖证明可移除",
    "codemirror": "最终前端包依赖图证明是否含旧包；无直接消费时不得声称已运行",
    "@traptitech/markdown-it-katex": "正式桌面截图不再调用 Electron helper；React 展示经产物视觉对照保留",
    "markdown-it": "Bridge 格式化由 Rust BridgePort 持有；截图不依赖 Electron，React 展示可继续使用",
    "markdown-it-task-lists": "正式桌面截图不再调用 Electron helper；React 展示经产物视觉对照保留",
}

PROTOCOL_OVERRIDES = {
    "mammoth": "知识 DOCX 导入与文件/桌面预览同输入内容、格式、资源授权、取消和大小限制对照",
    "exceljs": "知识 XLSX 导入与文件/桌面预览同输入单元格、公式、大小限制和错误对照",
    "unpdf": "知识 PDF 来源与 Office 读取的分页、大小、错误及取消对照",
    "jsdom": "知识 HTML 来源与 WebReader 不可信 HTML 的脚本/资源隔离、文本定位和导入结果对照",
    "ws": "服务/CLI/钉钉/QQ Bridge 消息重连，以及浏览器命令请求、结果、断线重连协议对照",
    "proxy-agent": "HTTP/WS、Bridge 与 MCP 的代理协议、认证范围、取消和错误对照",
    "qrcode": "设备配对 URL/SVG 的凭证范围、有效期、撤销和微信登录 data URL/状态分别对照",
    "better-sqlite3": "会话、文件历史、记忆、知识和观测库的 WAL 备份、版本、恢复、迁移及单写者对照",
    "yauzl": "技能包、角色卡与搜索二进制 ZIP 的路径穿越/符号链接/大小、可执行文件定位、错误清理对照",
    "@traptitech/markdown-it-katex": "React Markdown 与桌面截图公式渲染的视觉/格式对照",
    "markdown-it-task-lists": "React Markdown 与桌面截图任务列表的视觉/格式对照",
    "markdown-it": "Bridge 飞书/Telegram 格式化、React 展示和桌面截图的消息格式/视觉对照",
    "@tiptap/pm": "检查前端产物的 Tiptap 依赖图和真实输入框操作；若未纳入产物，记录未使用证据",
    "codemirror": "检查前端产物是否含 codemirror 旧包及 PreviewEditor 行为；若未纳入，记录未使用证据",
    "semver": "旧 manifest 无 generation、混合 generation、非法版本、桌面启动/升级/回滚和 CLI bundle 排序协议对照",
    "typebox": "工具 schema 注册与实际调用同源校验，未知 dialect、optional/default/union/enum、非法参数拒绝对照",
}


def import_surface(file):
    if file.startswith("desktop/src/"):
        return "react_renderer" if "/react/" in file or file.endswith(("-main.tsx", "main.tsx", "assets.d.ts")) else "desktop_renderer"
    if file.startswith("desktop/"):
        return "electron_main_preload"
    if file.startswith("scripts/"):
        return "build_or_test_script"
    if file.startswith("plugins/"):
        return "bundled_plugin"
    if file.startswith("cli/"):
        return "cli"
    if file.startswith("shared/"):
        return "shared_service_desktop_cli"
    return "node_service"


for item in DEPS:
    package = item["package"]
    hits = [r for r in GRAPH["external_imports"]
            if r["specifier"] == package or r["specifier"].startswith(package + "/")]
    import_sites = [dict(source_ref=f'{r["file"]}:{r["line"]}', kind=r["kind"],
                         specifier=r["specifier"], surface=import_surface(r["file"]),
                         type_only=bool(r["symbols"]) and all(s["type_only"] for s in r["symbols"]),
                         load_timing=r["load_timing"],
                         via_create_require=r.get("via_create_require", False),
                         reachable_from=r["reachable_from"],
                         static_runtime_reachable_from=r["static_runtime_reachable_from"])
                    for r in hits]
    item["direct_import_sites"] = import_sites
    item["indirect_call_chain_refs"] = EXTRA_CALL_CHAIN_REFS.get(package, [])
    # R2 修复：图命中不再吞掉初始手工锚点；三类来源全量并集去重。
    item["source_refs"] = list(dict.fromkeys([
        *(site["source_ref"] for site in import_sites),
        *item["indirect_call_chain_refs"],
        *item["source_refs"],
    ]))
    item["consumer_surfaces"] = sorted({site["surface"] for site in import_sites
                                         if not site["type_only"]})
    if not item["consumer_surfaces"]:
        item["consumer_surfaces"] = ["transitive_dependency"]
    static_surfaces = sorted({site["surface"] for site in import_sites
                              if site["kind"] in {"import", "export_from"} and not site["type_only"]})
    require_surfaces = sorted({site["surface"] for site in import_sites if site["kind"] == "require"})
    dynamic_surfaces = sorted({site["surface"] for site in import_sites if site["kind"] == "dynamic_import"})
    load_parts = []
    if static_surfaces:
        load_parts.append(f"{','.join(static_surfaces)} 的导入模块求值时静态加载")
    if require_surfaces:
        load_parts.append(f"{','.join(require_surfaces)} 执行 require 所在位置时加载")
    if dynamic_surfaces:
        load_parts.append(f"{','.join(dynamic_surfaces)} 执行动态 import 时加载")
    if not load_parts:
        load_parts.append("本仓无直接运行时 import；由上游包传递加载")
    item["module_load_activation"] = LOAD_OVERRIDES.get(package, "；".join(load_parts))
    item["operation_activation"] = OPERATION_ACTIVATION[package]
    item["activation"] = f'模块：{item["module_load_activation"]}；功能：{item["operation_activation"]}'
    item["rust_authority"] = OWNER_OVERRIDES.get(package, item["rust_authority"])
    item["protocol_test"] = PROTOCOL_OVERRIDES.get(package, item["protocol_test"])
    item["current_role"] = ROLE_OVERRIDES.get(package, item["current_role"])
    item["peripheral_retention"] = RETENTION_OVERRIDES.get(package, item["peripheral_retention"])
    item["retirement_evidence"] = RETIREMENT_OVERRIDES.get(package, item["retirement_evidence"])
    item["runtime_consumption_status"] = (
        "DIRECT_RUNTIME_IMPORT" if any(not site["type_only"] for site in import_sites) else
        "EVIDENCED_INDIRECT" if package in {"@silvia-odwyer/photon-node", "@tiptap/pm"} else
        "NO_DIRECT_PRODUCTION_IMPORT_EVIDENCE"
    )
    # 声明与图的一致性：声称"本仓无直接 import"的行不得存在任何非类型直接位点；
    # 声称服务组合静态可达的行必须确实从服务根静态运行时可达。
    no_direct = package in {"@tiptap/pm", "codemirror", "@silvia-odwyer/photon-node"}
    if no_direct and any(not site["type_only"] for site in import_sites):
        raise SystemExit(f"load claim conflict: {package} claims no direct import but graph has runtime sites")
    if package in {"mammoth", "exceljs", "jsdom"}:
        static_ok = any("server/main-full.ts" in site["static_runtime_reachable_from"] for site in import_sites
                        if site["kind"] == "import" and not site["type_only"])
        if not static_ok:
            raise SystemExit(f"load claim conflict: {package} claims service-composition static reachability but graph disagrees")

runtime = dict(schema="r00-t03-runtime-dependencies-v2", task="R00-T03", base_sha=TASK_BASE_SHA,
               observed_head=HEAD,
               source="package.json + package-lock.json 锁版本 + 源码消费定位；非安装包实测", dependencies=DEPS,
               direct_dependency_coverage=dict(manifest_total=len(EXTRA_DIRECT),
                                               mapped_total=len(EXTRA_DIRECT & {d["package"] for d in DEPS}),
                                               unmapped=direct_gap,
                                               note="仅顶层生产/可选依赖全覆盖；锁文件传递依赖按相关运行链追踪，不声称全量都在生产加载"),
               worker_contract=dict(owner="Rust ToolInvocationGateway / RunSupervisor",
                                    request="只接已授权的单次 operation、调用 ID、主体/run/attempt/generation、规范化资源范围、deadline 和取消信号",
                                    credentials="由 Rust 按 operation 下发最小短期凭证；worker 不读全量 home 或 provider 全局密钥",
                                    result="有界结构化结果与真实外部收据；Rust 校验并决定任务状态，未知副作用不盲重试",
                                    forbidden=["Agent loop", "上下文构建权", "批准决定权", "任务/会话终态写入权", "跨会话消息写入权", "全局 provider 凭证读取权"]),
               process_roles=[
                   dict(role="旧 Node Agent 服务", evidence=["server/bootstrap.ts:62", "server/main-full.ts:17", "server/index.ts:120"], classification="KERNEL_MIGRATION", target="Rust 独立服务"),
                   dict(role="Electron 主进程", evidence=["desktop/main.cjs:1959"], classification="HOST_MIGRATION", target="Tauri DesktopHost"),
                   dict(role="受控平台/文档/图像 worker 候选", evidence=["lib/bridge/feishu-adapter.ts:8", "lib/document-extract/anydoc-loader.ts:28", "core/model-image-preprocess.ts:203"], classification="CONDITIONAL_PERIPHERAL", target="Rust 限权单次 RPC；R07/R09 决定实际列表"),
               ],
               not_current_runtime=[dict(package="koffi", evidence="仅 scripts/build-server-phases.mjs 清理逻辑；package.json/lock 无安装项、无生产 import", status="NOT_EVIDENCED_AS_RUNTIME")],
               limits=["尚未运行安装包或外部平台；静态导入证明可能求值，不证明实际建立连接。", "Node/npm 开发与打包依赖不得直接算成迁移失败；生产内核 Node 依赖另计。", "R00-T04 将详查身份/权限/存储；本账本只定职责边界，不提前实现。"])

for output, value in [("PI_REPLACEMENT_MATRIX.json", matrix), ("RUNTIME_DEPENDENCIES.json", runtime)]:
    (HERE / output).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(dict(matrix_capabilities=len(CAPS), adapter_symbols=len(observed_symbols),
                      vendor_imports=len(GRAPH["vendor_pi_imports"]), hooks=len(hooks),
                      workers=len(WORKERS), worker_discovery_files=len(discovered),
                      worker_classifications=len(classified), manual_adjacent=len(MANUAL_ADJACENT),
                      runtime_dependencies=len(DEPS), symbol_gap=symbol_gap,
                      vendor_gap=vendor_gap, hook_gap=hook_gap), ensure_ascii=False))
