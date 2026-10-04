#!/usr/bin/env python3
"""R05-T04 acceptance-ledger entries generator.

Run AFTER the T04 evidence logs exist and the tree is final. Appends the
16 C-entries to docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json and rewrites
the R05-T04 task entry in PROGRESS_LEDGER.json. Digests are computed from
the live tree (working-tree manifest + test runner binaries + lockfile).
"""
import json, hashlib, subprocess, pathlib, sys, datetime

ROOT = pathlib.Path("/Users/study_superior/Desktop/Code/LingxiAgent")
T04 = ROOT / "artifacts/rust-tauri/R05/T04"
LEDGER = ROOT / "docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json"
PROGRESS = ROOT / "docs/rust-tauri/R05/PROGRESS_LEDGER.json"

def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

# ── working-tree manifest: every modified/untracked file under rust/ ─────────
out = subprocess.run(
    ["git", "status", "--porcelain", "--", "rust/"],
    cwd=ROOT, capture_output=True, text=True, check=True,
).stdout
files = []
for line in out.splitlines():
    rel = line[3:]
    if " -> " in rel:
        rel = rel.split(" -> ", 1)[1]
    p = ROOT / rel
    if p.is_file():
        files.append(rel)
files = sorted(set(files))
manifest = "".join(f"{sha256(ROOT / f)}  {f}\n" for f in files)
(T04 / "working-tree-manifest.txt").write_text(manifest)
tree_digest = "sha256:" + sha256(T04 / "working-tree-manifest.txt")

lock_hash = "sha256:" + sha256(ROOT / "rust/Cargo.lock")

# ── runner digests: the two r05_t04 test binaries actually executed ──────────
runners = {}
deps = ROOT / "rust/target/debug/deps"
for binary in sorted(deps.glob("r05_t04_streaming-*")):
    if binary.suffix in (".d", ".dylib") or binary.name.endswith(".d"):
        continue
    if binary.is_file():
        runners[binary.name] = "sha256:" + sha256(binary)
if not runners:
    sys.exit("no r05_t04 runner binaries found — build the tests first")

started = (T04 / "evidence-window.txt").read_text().splitlines()
started_at, finished_at = started[0], started[1]

BASE = {
    "sourceSha": "c549ff654508ab951e2cf39cf9d309fc9c6b8656",
    "workingTreeDigest": tree_digest,
    "runnerDigest": runners,
    "binarySha256": None,
    "lockfileHashes": {"rust/Cargo.lock": lock_hash},
    "schemaHash": None,
    "fixtureHash": None,
    "redactedConfigHash": None,
    "platform": "macOS 27.0.1 (26A434) arm64 (aarch64-apple-darwin)",
    "toolchain": "rustc 1.98.1 (48a229cea 2026-09-01) via ~/.cargo/bin rustup proxy",
    "startedAt": started_at,
    "finishedAt": finished_at,
    "mockBoundary": [
        "外部模型HTTP服务(real loopback TCP stub at the far end of the wire; adapter-level cases have no network at all — decoder/accumulator are pure functions of the byte stream); NOT_REAL_API — no real provider key, account or traffic"
    ],
    "externalAuthorizationRef": None,
    "reviewerExecutionRef": None,
    "blockingReason": None,
    "supplementalObligationIds": [],
    "ignoredRequiredTests": [],
}

ADAPTERS_CMD = "cargo test --manifest-path rust/Cargo.toml -p lingxi-adapters --test r05_t04_streaming"
ADAPTERS_LIB_CMD = "cargo test --manifest-path rust/Cargo.toml -p lingxi-adapters --lib streaming"
SERVICE_CMD = "cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test r05_t04_streaming"
EVIDENCE_ADAPTERS = "artifacts/rust-tauri/R05/T04/test-r05-t04-adapters.log"
EVIDENCE_SERVICE = "artifacts/rust-tauri/R05/T04/test-r05-t04-service.log"

def entry(case_id, formal, scope, dims, status, command, names, expected, observed, evidence):
    e = dict(BASE)
    e.update({
        "caseId": case_id,
        "formalScenarioIds": formal,
        "scope": scope,
        "dimensions": dims,
        "status": status,
        "command": command,
        "testNames": names,
        "executedCount": len(names),
        "expected": expected,
        "observed": observed,
        "evidence": evidence,
    })
    # field order per appendix E
    order = ["caseId","formalScenarioIds","supplementalObligationIds","scope","dimensions","status","command","testNames","executedCount","ignoredRequiredTests","exitCode","expected","observed","sourceSha","workingTreeDigest","runnerDigest","binarySha256","lockfileHashes","schemaHash","fixtureHash","redactedConfigHash","platform","toolchain","startedAt","finishedAt","evidence","mockBoundary","externalAuthorizationRef","reviewerExecutionRef","blockingReason"]
    e["exitCode"] = 0
    return {k: e[k] for k in order}

CHAT_DIM = {"protocol": "openai-completions + anthropic-messages (the two fragmenting families under test; google/response families share the same decoder and closed-batch parsers)", "operation": "chat", "auth": "api_key (stub)"}

entries = [
entry("R05-T04-C01", ["R05-A07"], "OFFLINE_LOGIC", CHAT_DIM, "PASS", ADAPTERS_CMD,
 ["c01_anthropic_full_response_is_fragmentation_invariant",
  "c01_openai_full_response_is_fragmentation_invariant",
  "c01_split_bom_and_crlf_are_fragmentation_invariant"],
 "相同协议响应逐字节及随机切分后语义一致，工具参数完整且不重复执行（A07）。",
 "Anthropic 全要素 fixture（中文+emoji+signature_delta+两个 tool_use）与 openai 双语种 reasoning/text 交错+双工具交错 fixture：一次性解码 ≡ 逐字节 ≡ 每个二切点 ≡ 固定seed xorshift 随机 200 轮——delta 序列、闭合批次（turn+usage）逐字节相等；工具片段从不作为 live delta 出现（完成前不存在任何可执行物）。BOM 跨三片+CRLF 行尾+注释心跳腿同样等价。",
 [EVIDENCE_ADAPTERS, "rust/crates/lingxi-adapters/tests/r05_t04_streaming.rs"]),
entry("R05-T04-C02", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS", ADAPTERS_CMD,
 ["c02_invalid_utf8_is_deterministic_at_every_fragmentation"],
 "非法 UTF-8（含跨片半字符）按冻结策略确定性报错；不重复/静默丢字节、不泄漏后续缓冲。",
 "非法字节序列（含跨片拆开的 UTF-8 半字符）在每种分片下产生同一个 InvalidMessage（响亮、不可重试、无替换字符）；报错前已交付的事件恒为完好前缀，erring feed 内已完成但未交付的事件随错误一并丢弃（实测行为，测试注释钉住），后续缓冲绝不泄漏。",
 [EVIDENCE_ADAPTERS, "rust/crates/lingxi-adapters/tests/r05_t04_streaming.rs"]),
entry("R05-T04-C03", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS", ADAPTERS_LIB_CMD,
 ["streaming::tests::simple_frames_dispatch_on_blank_lines",
  "streaming::tests::multi_line_data_joins_and_chunks_may_split_anywhere",
  "streaming::tests::comments_and_id_and_retry_fields_are_ignored",
  "streaming::tests::a_leading_bom_is_stripped_once",
  "c01_split_bom_and_crlf_are_fragmentation_invariant"],
 "CRLF、注释/心跳、多 data 行、多帧一包、单帧多包按 WHATWG SSE 协议正确分帧；心跳不当文本、传输包不当消息边界。",
 "SseDecoder 增量单测：空行分帧、多 data 行按协议拼接、注释/id/retry 字段忽略、首个 BOM 只剥一次；任意字节切点不改变分帧结果（C01 harness 复跑）。CRLF 与裸 CR 行尾均正确（行尾 \\r 留缓冲等待配对，绝不半截解析）。",
 [EVIDENCE_ADAPTERS, "rust/crates/lingxi-adapters/src/models/streaming.rs"]),
entry("R05-T04-C04", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS", ADAPTERS_CMD,
 ["c04_bounds_are_loud_at_the_preregistered_limits",
  "streaming::tests::the_buffer_bound_is_loud",
  "streaming::tests::the_per_frame_bound_is_loud_across_lines_and_chunks"],
 "超大 frame/参数/嵌套 JSON/无终止片段到配置上限即明确拒绝、取消读取并释放资源；无无界内存/CPU 或静默截断。",
 "预登记值被断言钉死（sse_single_frame_max_bytes=1MiB、tool_arguments_max_bytes=1MiB；整流未投递缓冲 8MiB——比预登记 16MiB 上限更紧，§8 允许收紧）：8MiB+1 无换行行→整流界限响亮 InvalidMessage；1MiB+1 单帧→单帧界限响亮；openai/anthropic 两族工具参数片段越过 1MiB 在喂入时拒绝（零派发、不截断参数）；300 层嵌套 JSON 参数在闭合时被 serde 递归守卫响亮拒绝。所有界限错误不可重试。",
 [EVIDENCE_ADAPTERS, "rust/crates/lingxi-adapters/src/models/streaming.rs"]),
entry("R05-T04-C05", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{ADAPTERS_CMD} && {SERVICE_CMD}",
 ["c05_openai_half_json_arguments_close_loud_with_zero_dispatch",
  "c05_anthropic_half_json_arguments_close_loud_with_zero_dispatch",
  "c05_half_json_arguments_dispatch_zero_tools"],
 "半截 JSON 工具参数：零工具派发；不补括号猜参数，不把空对象占位当真实参数。",
 "两族 accumulator：arguments 停在字符串中间即 finish() → 响亮 InvalidMessage，零派发。服务面（真实 boot + loopback stub）：半截参数批次运行失败（failed.provider_error），tool_call_started=0，工作区无文件物化，无伪造 final；model_call_started/completed 各 1（调用事实诚实闭合，失败是运行的终态）。",
 [EVIDENCE_ADAPTERS, EVIDENCE_SERVICE, "rust/crates/lingxi-service/tests/r05_t04_streaming.rs"]),
entry("R05-T04-C06", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{ADAPTERS_CMD} && {SERVICE_CMD}",
 ["c06_parseable_arguments_admit_nothing_before_the_protocol_terminal",
  "c06_parseable_arguments_do_not_dispatch_before_the_terminal"],
 "参数已能解析但协议未宣布完成：不提前执行；只有协议完整、安全终结且 schema 通过才准入。",
 "accumulator 层：无 [DONE]/message_stop 时 finish() 响亮；闭合后恰一次准入。服务面 gated 屏障：完整可解析参数+finish_reason 已到达但 [DONE] 被屏障扣住——屏障内 50 次探测 tool_call_started 恒 0，运行保持 running；释放后恰一次派发，第二请求携带真实文件内容（read note.txt → 'c06 真实内容'）。",
 [EVIDENCE_ADAPTERS, EVIDENCE_SERVICE, "rust/crates/lingxi-service/tests/r05_t04_streaming.rs"]),
entry("R05-T04-C07", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS", ADAPTERS_CMD,
 ["c07_wrong_shaped_arguments_refuse_loudly",
  "c07_duplicate_keys_and_overprecision_numbers_follow_one_canonical_rule"],
 "参数为 null/数组/错误类型/重复键/超精度数：按冻结 schema/canonical 规则一致拒绝或保真；不偷偷转型导致批准摘要失真。",
 "非对象参数（null/数组/数字/布尔）一律响亮拒绝；重复键 last-wins 且 digest 覆盖 canonical 值；浮点与超安全整数一律响亮拒绝（ArgumentsNotSafeInteger，与 TS incumbent 逐点 parity——冻结规则而非舍入）。wire 值→有效值→digest 三者一致。",
 [EVIDENCE_ADAPTERS]),
entry("R05-T04-C08", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS", ADAPTERS_CMD,
 ["c08_anthropic_interleaved_tool_blocks_accumulate_independently",
  "c01_openai_full_response_is_fragmentation_invariant"],
 "两个工具 index 交替发送：各自独立累积、ID 关联正确；不能只维护一个全局 arguments 缓冲。",
 "anthropic 两个 tool_use 块逐片段交错（index 0/1 交替），各自累积出完整且正确的参数与 id 关联；openai 侧双工具交错在 C01 fixture 内等价验证（call_a/call_b 片段交替，闭合批次两个请求互不串块）。",
 [EVIDENCE_ADAPTERS]),
entry("R05-T04-C09", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{ADAPTERS_CMD} && {SERVICE_CMD}",
 ["c09_openai_length_stop_with_one_complete_tool_dispatches_nothing",
  "c09_anthropic_max_tokens_stop_with_one_complete_tool_dispatches_nothing",
  "c09_length_truncated_batch_executes_nothing_and_the_retry_is_clean"],
 "某工具已完整而另一个被 max_tokens 截断：明确批次准入策略——整轮合法闭合前零副作用，不执行半批后说全未执行。",
 "截断批次（一个完整工具调用+length/max_tokens 终因）= 可重试 BudgetExceeded：完整工具也不派发（整批全有或全无），partial 内容留 delta 事件，usage 保留。服务面：tool_call_started=0、无文件物化；重试是同一运行的新尝试（model_call_started/completed 各 2），失败调用的真实 usage（11/33）不丢，第二次尝试正常完成。",
 [EVIDENCE_ADAPTERS, EVIDENCE_SERVICE]),
entry("R05-T04-C10", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS", ADAPTERS_CMD,
 ["c10_openai_duplicates_conflicts_and_adjacent_identical_text",
  "c10_anthropic_duplicates_conflicts_and_event_field_agreement"],
 "重复完成事件、同 ID 不同参数、相邻相同文字 delta：同一次完整调用不重复派发；冲突拒绝；普通重复文本不能仅按内容去重。",
 "post-terminal 事件（[DONE] 后的帧、message_stop 后的事件）响亮；冲突 usage/id/signature 响亮；SSE event 字段与帧 type 不一致响亮；相同重发（幂等帧）容忍；相邻相同文本 delta 永不去重（内容不是身份）。",
 [EVIDENCE_ADAPTERS]),
entry("R05-T04-C11", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{ADAPTERS_CMD} && {SERVICE_CMD}",
 ["c11_stop_reason_table_is_complete_across_the_fragmenting_families",
  "c11_a_content_refusal_fails_in_place_without_retry",
  "c09_length_truncated_batch_executes_nothing_and_the_retry_is_clean"],
 "正常停止/工具/长度截断/内容拒绝/协议错误/EOF 语义完整：只有可信正常终结才可形成 final；部分/拒绝保留原因，不将 socket 关闭当成功。",
 "停止原因表（三族闭合批次解析器）：stop/end_turn/STOP→Final；tool_calls/tool_use→ToolRequests；length/max_tokens/model_context_window_exceeded/MAX_TOKENS→可重试 BudgetExceeded；content_filter/refusal/SAFETY 等→不可重试 Forbidden；EOF 无终结标记→响亮截断错误。服务面：content_filter 拒绝原地失败不重试（stub hits=1），partial 内容保留、无伪造 final；截断重试干净（见 C09 条目）。",
 [EVIDENCE_ADAPTERS, EVIDENCE_SERVICE]),
entry("R05-T04-C12", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS", SERVICE_CMD,
 ["c12_first_delta_reaches_the_subscriber_before_the_external_stream_ends"],
 "协议服务先发首片后停在屏障：真实订阅客户端在最终片前看到首片；模型网络未结束时 UI 事件已到；不得缓冲全量后假流式。",
 "GatedSse 屏障 stub：首帧刷出后结构性 hold（屏障未释放前终帧不可能发送）。真实订阅者（事件中心订阅→durable commit→post-commit 发布链）在屏障持有期间收到首片 delta；此刻 stub.hits=1 且流仍开。durable 顺序：started < deltas < segment_end < completed（A09 排序半）。释放后运行完成，final 内容为屏障前后片段的真实拼接。",
 [EVIDENCE_SERVICE, "rust/crates/lingxi-service/tests/r05_t04_streaming.rs"]),
entry("R05-T04-C13", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS", SERVICE_CMD,
 ["c13_live_normalization_and_history_projection_share_the_scanner"],
 "独立 MOOD/思考块不混正文；正常代码示例中的字面标签不被无差别正则删除；实时接收与历史投影同源规范化。",
 "anthropic 流（三个 text_delta，跨片切开 <think> 开标签）：围栏代码块内的字面 <think>不是标签</think> 保持正文；独立 <think>内部推理</think> 结构化为 reasoning 段（model_call_delta 与 assistant_segment_delta 双词汇一致）；<mood>开心</mood> 从事件流剥离（D5，所有事件 delta 均不含）但 canonical 消息原文保真；history 投影 split_reserved_tag_segments（与实时链同一 scanner）产出同一 think 块结构。",
 [EVIDENCE_SERVICE, "rust/crates/lingxi-service/src/streaming_norm.rs"]),
entry("R05-T04-C14", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS", ADAPTERS_CMD,
 ["c14_opaque_blocks_roundtrip_and_signatures_never_leak_into_deltas"],
 "未知非关键扩展块按规则保留/忽略；必需未知块响亮失败；签名不露正文。",
 "anthropic redacted_thinking opaque 块字节级保真往返（ContentBlock::Opaque）；对 opaque 块的 delta 响亮拒绝（适配器不知其语义，绝不猜）；thinking signature 从不进入 text/reasoning live delta。",
 [EVIDENCE_ADAPTERS]),
entry("R05-T04-C15", ["R05-A08"], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{ADAPTERS_CMD} && {SERVICE_CMD}",
 ["c15_a08_http200_stream_failures_keep_the_run_honest",
  "c15_anthropic_in_stream_error_event_retries_cleanly",
  "c15_decoder_level_stream_failures_are_loud"],
 "HTTP200 后发生错误事件/断流/重复终止/只 usage 无正文：错误不被 200 盖掉；保留 partial、usage 未知性，取消底层连接。中断流不伪造最终回复（A08）。",
 "服务面五腿（openai 平面四腿+anthropic 错误事件腿）：干净 EOF 无 [DONE]→失败且 partial delta 保留、无伪造 final（A08）；帧中 EOF→失败且完好帧 delta 保留；只 usage 无正文→completed.no_final.empty_reply 且 provider 报告的真实 usage（7/0）保留（不假造答案也不假造零）；[DONE] 后又有帧→失败；anthropic 流内 overloaded_error→可重试且重试干净（hits=2）。解码半：帧中 EOF、非 JSON 帧、sink 关闭（Cancelled 不可重试）均响亮。",
 [EVIDENCE_ADAPTERS, EVIDENCE_SERVICE]),
entry("R05-T04-C16", ["R05-A09"], "OFFLINE_SERVICE", CHAT_DIM, "PASS", SERVICE_CMD,
 ["c16_a09_midstream_cancel_releases_the_connection_and_settles_once",
  "cancel_terminal_race::cancel_racing_the_finalize_transaction_is_too_late_not_accepted",
  "cancel_terminal_race::cancel_accepted_before_the_terminal_claim_beats_every_terminal_shape"],
 "最后片/终态提交前后取消：唯一终态，迟到 delta/final 受 fence；不跨 Run 写入、不复活任务。流中取消释放连接（A09）。",
 "后台驱动面+gated stub：流持有中取消→runs 行恰一次 cancelled.requested；stub 观测到客户端断连（disconnects=1——取消 drop 了 provider socket 读，连接即回收）；model_call_started=1 且 completed=0（诚实不闭合）；无 final message；结算后事件计数在 50ms 窗口内稳定（迟到 delta 受 fence）。终态提交边界的取消竞态由 cancel_terminal_race 既有双腿钉住（too-late 不接受；接受先于 claim 者胜过一切终态形状）。",
 [EVIDENCE_SERVICE, "artifacts/rust-tauri/R05/T04/test-regression-service.log"]),
]

ledger = json.loads(LEDGER.read_text())
existing = {a["caseId"] for a in ledger["acceptances"]}
dups = [e["caseId"] for e in entries if e["caseId"] in existing]
if dups:
    sys.exit(f"duplicate caseIds already in ledger: {dups}")
ledger["acceptances"].extend(entries)
ledger["note"] = ("T01+T02+T03 slices (each C01-C12) + T04 slice (C01-C16; 52 entries). "
                  "Every entry is backed by real executed evidence under artifacts/rust-tauri/R05/.")
LEDGER.write_text(json.dumps(ledger, ensure_ascii=False, indent=1) + "\n")
print(f"appended {len(entries)} entries; total {len(ledger['acceptances'])}")

# ── progress ledger ──────────────────────────────────────────────────────────
progress = json.loads(PROGRESS.read_text())
for t in progress["tasks"]:
    if t.get("task") == "R05-T04":
        t.clear()
        t.update({
            "task": "R05-T04",
            "name": "流式解码、规范化与部分结果（生产流式切换 + 事件桥 + 16 检查点）",
            "status": "READY_FOR_INDEPENDENT_REVIEW",
            "executor": "EXECUTOR-R05-T04",
            "cases_total": 16,
            "cases_pass": 16,
            "cases_not_run": 0,
            "acceptance_ledger": "docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json",
            "deliverables": [
                "rust/crates/lingxi-kernel/src/ports.rs (next_turn + ModelTurnDelta + TurnDeltaSink)",
                "rust/crates/lingxi-adapters/src/models/streaming.rs (incremental bounded SseDecoder)",
                "rust/crates/lingxi-adapters/src/models/dispatch.rs (drive_sse_stream incremental read)",
                "rust/crates/lingxi-adapters/src/models/{openai_completions,anthropic_messages,google_generative_ai,openai_responses,openai_codex_responses}.rs (streaming production path + accumulators)",
                "rust/crates/lingxi-service/src/runs.rs (streaming bridge: bounded delta channel, D6 started-before-delta, idle timeout)",
                "rust/crates/lingxi-service/src/streaming_norm.rs (same-source normalization chain)",
                "rust/crates/lingxi-adapters/tests/r05_t04_streaming.rs (18 tests)",
                "rust/crates/lingxi-service/tests/r05_t04_streaming.rs (9 tests)",
                "docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md (§20-24)",
                "docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json",
                "docs/rust-tauri/R05/PROGRESS_LEDGER.json",
            ],
            "evidence_root": "artifacts/rust-tauri/R05/T04/",
            "notes": [
                "消费者同步（断言语义不变）：cancellation_tree stream_read durable 序纳入诚实的 model_call_started（D6）；late_result_fence mc0002 断言排除 started；r05_t01_model_plane/r05_t01_binary_wiring/r05_t02_credentials 的 stub 改答 SSE（生产已切流式，stream:false→true）。",
                "唯一已知失败：r00_management_leaves LAN 段在本机防火墙/未签名二进制下 stall（任务书允许的唯一 ALF 项；无 R05 断言，未改其测试）。",
                "live provider 验证保持 BLOCKED_NOT_AUTHORIZED（未获真实外网授权）。",
            ],
        })
        break
else:
    sys.exit("R05-T04 task entry not found")
PROGRESS.write_text(json.dumps(progress, ensure_ascii=False, indent=1) + "\n")
print("progress ledger updated")
