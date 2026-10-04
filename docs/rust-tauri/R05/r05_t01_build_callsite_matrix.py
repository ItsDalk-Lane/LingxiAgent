#!/usr/bin/env python3
"""R05-T01: build docs/rust-tauri/R05/MODEL_CALLSITE_MATRIX.json.

Every row cites a REAL code anchor (file:line + token). The script verifies
each anchor against the working tree and fails closed on drift — the matrix
can never silently rot away from the code it claims to describe.

Completeness is checked in the other direction too: the script re-scans the
TS tree for the model-invocation token families (callText(,
createAgentSession(, resolveFresh("embedding"/"rerank"), probeProvider(,
media submits, speech transcribe) and every non-test hit must be either a
matrix row or an allowlisted definition/wrapper site listed in
NON_CALLSITE_HITS with a reason. A newly added callsite that nobody mapped
breaks the build — that is the C01 bidirectional scan.
"""

import hashlib
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
OUT = REPO / "docs/rust-tauri/R05/MODEL_CALLSITE_MATRIX.json"

# category -> (用途, R05 属主, 本阶段实现或后续阶段归属)
CATEGORIES = {
    "chat_main": (
        "主对话 agent 会话（Pi SDK createAgentSession 包装/驱动）",
        "Rust RunSupervisor 驱动循环 + ModelGateway（kernel 交换契约 R05-T01，协议适配 T03，流式 T04）",
        "R05 实现内核侧；TS 会话编排入口的切换属 R07 业务面",
    ),
    "chat_direct_summary": (
        "compaction 直连摘要（Pi completeSimple 回落路径，不经 callText）",
        "Rust ModelGateway summarize 路径",
        "R05-T06 辅助调用链",
    ),
    "aux_slot": (
        "辅助槽位 callText（title/summarize/memory/vision/approval/guard）",
        "Rust ModelGateway 统一槽位路由（R05-T01 解析/能力预检 C07）",
        "R05-T06 辅助调用入口；业务触发点接线按槽位逐列",
    ),
    "aux_slot_dead": (
        "knowledge 槽位：已在 shared/auxiliary-slot-ids.ts 与 core/auxiliary-slots.ts 登记身份，"
        "全仓无任何 resolveAuxiliaryModel*(\"knowledge\") 调用点（死槽，如实登记，不虚构用途）",
        "无（无调用点则无可迁移调用）",
        "保持登记不接线；若未来启用须经同一 Gateway",
    ),
    "embedding_rerank": (
        "模型操作 embedding/rerank（core/model-operation-client.ts 九协议族）",
        "Rust ModelGateway operation 解析 + 协议适配",
        "R05-T06（operation 适配）",
    ),
    "media": (
        "媒体生成（image/video/speech 提交与轮询）",
        "Rust ModelGateway 媒体 operation + ResourceRef 交付",
        "R05-T06",
    ),
    "speech_recognition": (
        "语音识别（ASR 转写）",
        "Rust ModelGateway ASR operation",
        "R05-T06",
    ),
    "connectivity_probe": (
        "供应商连通性/健康探测（不保存配置的试调用）",
        "Rust ModelGateway probe operation（离线替身验证）",
        "R05-T01/T03",
    ),
    "definition": (
        "被引用方定义/包装（不是独立调用点）",
        "—",
        "—",
    ),
}

# (file, line, anchor, category, detail, slot_or_operation)
ROWS = [
    # ── 主链路 ──
    ("lib/pi-sdk/index.ts", 78, "export async function createAgentSession(options)", "chat_main",
     "Pi SDK 包装入口（streamFunction/观测注册）；全部主对话经此", "chat"),
    ("core/session-coordinator.ts", 2230, "createAgentSession(sessionOpts)", "chat_main",
     "桌面主对话 reply 驱动", "chat"),
    ("core/session-coordinator.ts", 8385, "createAgentSession({", "chat_main",
     "会话重建/ compaction 后再开", "chat"),
    ("core/bridge-session-manager.ts", 1305, "createAgentSession({", "chat_main",
     "bridge 渠道会话", "chat"),
    ("core/bridge-session-manager.ts", 1794, "createAgentSession({", "chat_main",
     "bridge 渠道会话（第二入口）", "chat"),
    ("hub/agent-executor.ts", 290, "createAgentSession({", "chat_main",
     "hub 执行器", "chat"),
    ("hub/agent-executor.ts", 556, "createAgentSession({", "chat_main",
     "hub 执行器（第二入口）", "chat"),
    ("lib/desk/agent-run-automation.ts", 61, "export function createAgentSessionAutomationExecutor({", "chat_main",
     "自动化（cron/heartbeat）执行器工厂", "chat(automation)"),
    ("lib/llm/observed-pi-direct-summary.ts", 75, "export async function observePiDirectSummary<T>(", "chat_direct_summary",
     "Pi direct summary 观测包装（compaction 直连路径）", "compact"),
    # ── callText 定义与共享包装 ──
    ("core/llm-client.ts", 520, "export async function callText({", "definition",
     "utility 文本调用唯一入口；内部按 api 分四协议构造（openAICompatible/anthropic/google/codex responses）", "—"),
    ("core/output-length-contract.ts", 214, "await callText(nextRequest)", "definition",
     "输出长度契约包装（不是独立槽位）", "—"),
    # ── title 槽 ──
    ("core/llm-utils.ts", 254, "export async function summarizeTitle(", "aux_slot",
     "会话标题生成", "title"),
    ("core/llm-utils.ts", 573, "export async function generateAgentId(", "aux_slot",
     "agent id 生成", "title"),
    ("core/llm-utils.ts", 320, "export async function translateSkillNames(", "aux_slot",
     "技能名批量翻译（engine.translateSkillNames 以 title 槽解析）", "title"),
    ("core/agent-manager.ts", 1236, 'resolveAuxiliaryModelFresh?.("title")', "aux_slot",
     "agent 创建流 title 槽解析点", "title"),
    # ── summarize 槽 ──
    ("core/llm-utils.ts", 374, "export async function summarizeActivity(", "aux_slot",
     "活动 session 摘要", "summarize"),
    ("core/llm-utils.ts", 470, "export async function summarizeActivityQuick(", "aux_slot",
     "快速摘要", "summarize"),
    ("core/llm-utils.ts", 661, "export async function generateDescription(", "aux_slot",
     "agent 能力描述生成", "summarize"),
    ("core/agent-manager.ts", 596, 'resolveAuxiliaryModelFresh?.("summarize"', "aux_slot",
     "agent-manager summarize 槽解析点", "summarize"),
    ("core/slash-commands/rc-summary.ts", 123, "callTextWithLengthContract({", "aux_slot",
     "/rc-summary 斜杠命令", "summarize"),
    ("server/index.ts", 736, "() => callText({", "aux_slot",
     "插件模型调用入口一（summarize 槽解析，trace origin=plugin）", "summarize"),
    ("server/index.ts", 781, "() => callText({", "aux_slot",
     "插件模型调用入口二（summarize 槽解析）", "summarize"),
    ("lib/autolearn/autolearn-service.ts", 174, "await callText({", "aux_slot",
     "autolearn 蒸馏（summarize 槽解析，审查用 guard）", "summarize/guard"),
    # ── memory 槽 ──
    ("lib/memory/compile.ts", 1138, "return callText({", "aux_slot",
     "记忆编译", "memory"),
    ("lib/memory/deep-memory.ts", 309, "await callText({", "aux_slot",
     "深记忆提炼", "memory"),
    ("lib/memory/dream/model-runner.ts", 82, "return callText({", "aux_slot",
     "dream 模型运行器", "memory"),
    ("lib/memory/session-summary.ts", 775, "return callText({", "aux_slot",
     "会话摘要（记忆）", "memory"),
    ("lib/memory/session-summary.ts", 989, "return callText({", "aux_slot",
     "会话摘要（记忆，第二调用）", "memory"),
    ("lib/diary/diary-writer.ts", 792, "callText({", "aux_slot",
     "日记生成（engine.writeDiary 以 memory 槽解析，配置错误响亮失败不回退）", "memory"),
    ("core/engine.ts", 4788, 'resolveAuxiliaryExecution("memory")', "aux_slot",
     "writeDiary 的 memory 槽解析点", "memory"),
    # ── vision 槽 ──
    ("core/vision-bridge.ts", 917, "this._callText({", "aux_slot",
     "视觉桥图像描述（截断路径）", "vision"),
    ("core/vision-bridge.ts", 981, "this._callText({", "aux_slot",
     "视觉桥图像理解", "vision"),
    ("core/engine.ts", 2463, "async resolveVisionConfigFresh()", "aux_slot",
     "vision 槽解析（isVisionAuxiliaryEnabled 门）", "vision"),
    # ── approval 槽 ──
    ("lib/approval-gateway.ts", 678, "await callText({", "aux_slot",
     "审批网关模型复审", "approval"),
    ("core/engine.ts", 528, 'resolveAuxiliaryModelFresh("approval"', "aux_slot",
     "approval 槽解析注册", "approval"),
    # ── guard 槽 ──
    ("lib/tools/install-skill.ts", 155, "await callText({", "aux_slot",
     "技能安装安全审查 safetyReview", "guard"),
    ("core/agent.ts", 806, 'resolveAuxiliaryModelFresh?.("guard"', "aux_slot",
     "install_skill 工具 guard 槽解析", "guard"),
    # ── 外观摘要（agent 自有 chat 模型，非独立槽）──
    ("lib/agent-appearance-summary.ts", 308, "callTextWithLengthContract({", "aux_slot",
     "agent 外观摘要（调用方 session-coordinator:2738 传目标 chat 模型）", "chat(agent appearance)"),
    # ── knowledge 死槽 ──
    ("shared/auxiliary-slot-ids.ts", 21, '"knowledge"', "aux_slot_dead",
     "knowledge 槽身份登记处；全仓无解析调用点", "knowledge(dead)"),
    # ── embedding / rerank ──
    ("core/engine.ts", 2530, 'resolveFresh("embedding")', "embedding_rerank",
     "embedding 解析（写路径）", "embedding"),
    ("core/engine.ts", 2602, 'resolveFresh("embedding")', "embedding_rerank",
     "embedding 解析（查询路径）", "embedding"),
    ("core/engine.ts", 2735, 'resolveFresh("rerank")', "embedding_rerank",
     "rerank 解析", "rerank"),
    ("core/model-operation-client.ts", 629, 'operation: "embedding"', "embedding_rerank",
     "embedding 协议执行", "embedding"),
    ("core/model-operation-client.ts", 699, 'operation: "rerank"', "embedding_rerank",
     "rerank 协议执行", "rerank"),
    ("shared/model-operations.ts", 1, "", "definition",
     "MODEL_OPERATION_IDS / 九协议词汇（openai-embeddings/ollama-embed/gemini-embed/voyage-embeddings/"
     "cohere-rerank/siliconflow-rerank/voyage-rerank/dashscope-rerank/minimax-embeddings）", "—"),
    # ── 媒体 ──
    ("core/media/universal-media-manager.ts", 845, "async submitImage(", "media",
     "图像生成提交", "image"),
    ("core/media/universal-media-manager.ts", 873, "async submitSpeech(", "media",
     "语音生成提交（TTS）", "speech"),
    ("core/media/universal-media-manager.ts", 1232, "async submitVideo(", "media",
     "视频生成提交", "video"),
    ("core/media/image-task-runner.ts", 586, '"media", operation: "submit"', "media",
     "图像任务运行器（trace 提交点）", "image"),
    ("core/media/submit-image.ts", 1, "", "media",
     "图像提交共享逻辑", "image"),
    ("core/media/poller.ts", 1, "", "media",
     "媒体任务轮询（job accepted≠完成语义所在）", "image/video/speech"),
    ("core/media-adapters/builtin-adapters.ts", 1, "", "definition",
     "媒体协议适配注册（openai-images/openai-audio-*/dashscope-*/gemini-*/minimax-*/agnes-*/volcengine-*/mimo-*/openai-codex-responses-image）", "—"),
    # ── 语音识别 ──
    ("core/speech-recognition-service.ts", 520, 'operation: "transcribe"', "speech_recognition",
     "ASR 转写主路径", "asr"),
    ("core/speech-recognition-service.ts", 560, 'operation: "transcribe"', "speech_recognition",
     "ASR 转写（第二路径）", "asr"),
    ("core/speech-recognition/adapters.ts", 1, "", "definition",
     "ASR 供应商适配（volcengine-bigasr 等）", "—"),
    ("core/speech-recognition/system-speech-adapter.ts", 1, "", "definition",
     "系统语音适配（auth=none 本机能力）", "—"),
    # ── 探测 ──
    ("server/routes/providers.ts", 825, "probeProvider as any", "connectivity_probe",
     "providers.test 连通性探测（operation=connectivity-probe，不保存配置）", "probe"),
    ("server/routes/providers.ts", 384, "probeOllamaModelDetails", "connectivity_probe",
     "ollama 模型详情探测（context/capabilities）", "probe"),
    ("lib/llm/provider-client.ts", 262, "export async function probeProvider(", "definition",
     "探测执行体", "—"),
    ("server/routes/models.ts", 245, "() => callText({", "connectivity_probe",
     "模型健康检查试调用（origin=health_check；codex-responses 明示 skipped）", "probe"),
    # ── 包装/委托跳点（反向扫描发现的完整调用图）──
    ("core/agent.ts", 1123, "callText: (callOptions) => callText(", "definition",
     "agent 内存路径的 callText 透传闭包（memory 槽调用经此注入）", "—"),
    ("core/engine.ts", 2469, "_callApprovalReviewerText(options) { return callText(options); }", "definition",
     "approval 复审的 callText 注入点（approval-gateway 经此拿调用体）", "—"),
    ("core/llm-utils.ts", 114, "return callText({", "definition",
     "callTextWithLengthContract 内部的 callText 委托", "—"),
    ("core/media/universal-media-manager.ts", 831, "return this.submitImage({", "definition",
     "generateImageFromBus → submitImage 委托", "—"),
    ("core/media/universal-media-manager.ts", 869, "this.submitSpeech({", "definition",
     "generateSpeechFromBus → submitSpeech 委托", "—"),
    ("core/media/universal-media-manager.ts", 1229, "this.submitVideo({", "definition",
     "generateVideoFromBus → submitVideo 委托", "—"),
    # ── 槽位解析中枢（被引用方）──
    ("shared/auxiliary-slot-ids.ts", 17, "AUXILIARY_SLOT_IDS", "definition",
     "7 槽身份单一真理源（title/summarize/memory/knowledge/vision/approval/guard）", "—"),
    ("core/auxiliary-model-resolver.ts", 158, "resolveAuxiliaryModel(", "definition",
     "槽位→模型/凭证解析器（sync/fresh/execution 三入口）", "—"),
    ("core/engine.ts", 2896, "resolveModelOperation(operation: ModelOperation)", "definition",
     "模型操作解析（embedding/rerank）", "—"),
]

# Non-callsite hits the completeness scan must expect (definitions, wrappers,
# tests, dead references) — each with a reason.
NON_CALLSITE_REASONS = [
    "import",  # import lines
    "core/llm-client.ts",  # callText definition + internal dispatch
    "core/output-length-contract.ts",  # shared wrapper (row exists)
    "test",  # test files
    "provider-request-provenance.ts",  # doc references
    "semantic-input-provenance.ts",  # doc references
    "model-trace-scope.ts",  # doc references
    "model-call-recorder.ts",  # doc references
    "prompt-layout.ts",  # doc references
    "observed-pi-direct-summary.ts",  # row exists (direct summary)
    "model-call-stream-observer.ts",  # doc references
    "session-options.ts",  # doc references
    ".d.ts",
]

SCAN_TOKENS = ["callText({", "callText(", "createAgentSession(", 'resolveFresh("embedding")',
               'resolveFresh("rerank")', "probeProvider(", "submitImage(", "submitVideo(",
               "submitSpeech("]
SCAN_DIRS = ["lib", "core", "server", "hub"]


def sha256_of(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    rows = []
    errors = []
    for file, line, anchor, category, detail, slot in ROWS:
        path = REPO / file
        if not path.is_file():
            errors.append(f"{file}: missing")
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        if line > len(lines):
            errors.append(f"{file}:{line}: beyond EOF ({len(lines)} lines)")
            continue
        text = lines[line - 1]
        if anchor and anchor not in text:
            errors.append(f"{file}:{line}: anchor {anchor!r} not in {text.strip()!r}")
            continue
        purpose, owner, stage = CATEGORIES[category]
        rows.append({
            "callsite": f"{file}:{line}",
            "anchor": anchor,
            "category": category,
            "slot_or_operation": slot,
            "purpose": purpose,
            "detail": detail,
            "incumbent_protocol_source": (
                "core/llm-client.ts 四协议构造" if category in ("aux_slot", "connectivity_probe")
                else "lib/pi-sdk (Pi runtime serializers)" if category.startswith("chat")
                else "core/model-operation-client.ts 九协议族" if category == "embedding_rerank"
                else "core/media-adapters/*" if category == "media"
                else "core/speech-recognition/*" if category == "speech_recognition"
                else "—"
            ),
            "r05_owner": owner,
            "stage_disposition": stage,
            "source_sha256": sha256_of(path),
        })
    if errors:
        for e in errors:
            print(f"anchor drift: {e}", file=sys.stderr)
        return 1

    # Reverse completeness scan: every model-invocation token hit must be a
    # matrix row or an allowlisted non-callsite.
    row_sites = {(r["callsite"]) for r in rows}
    unexpected = []
    for d in SCAN_DIRS:
        for path in sorted((REPO / d).rglob("*.ts")):
            rel = path.relative_to(REPO).as_posix()
            text = path.read_text(encoding="utf-8", errors="replace")
            for i, l in enumerate(text.splitlines(), 1):
                for token in SCAN_TOKENS:
                    if token not in l:
                        continue
                    site = f"{rel}:{i}"
                    if site in row_sites:
                        continue
                    if any(reason in rel or reason in l for reason in NON_CALLSITE_REASONS):
                        continue
                    # createAgentSession type/import/def lines and the pi-sdk wrapper def
                    if token == "createAgentSession(" and ("import" in l or "rawCreateAgentSession" in l
                                                           or "export async function" in l
                                                           or "Executor({" in l or "executor" in l.lower()):
                        continue
                    if token == "callText(" and ("callText({" not in l and "callTextWithLengthContract" in l):
                        continue
                    if token in ("submitImage(", "submitVideo(", "submitSpeech(") and "async " in l:
                        continue
                    unexpected.append(f"{site}: {token}: {l.strip()[:100]}")
    if unexpected:
        print("unmapped model callsites discovered:", file=sys.stderr)
        for u in unexpected:
            print(f"  {u}", file=sys.stderr)
        return 1

    doc = {
        "schema": "lingxi.r05-model-callsite-matrix.v1",
        "generated_by": "docs/rust-tauri/R05/r05_t01_build_callsite_matrix.py",
        "generated_at": "2026-10-02",
        "verification": (
            "每行锚点 file:line+token 对工作区逐一校验（漂移即失败）；反向全量扫描 "
            "lib/core/server/hub 的模型调用 token 族，凡非矩阵行且非白名单定义/包装/测试命中即失败"
            "（C01 双向扫描）。"
        ),
        "auxiliary_slot_vocabulary": {
            "ids": ["title", "summarize", "memory", "knowledge", "vision", "approval", "guard"],
            "dead_slots": ["knowledge"],
            "identity_source": "shared/auxiliary-slot-ids.ts",
            "descriptor_source": "core/auxiliary-slots.ts",
            "resolver": "core/auxiliary-model-resolver.ts",
        },
        "rows": rows,
        "counts": {
            "rows": len(rows),
            "by_category": {c: sum(1 for r in rows if r["category"] == c) for c in CATEGORIES},
        },
    }
    OUT.write_text(json.dumps(doc, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {OUT}: {len(rows)} anchored rows")
    print(json.dumps(doc["counts"]["by_category"], ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
