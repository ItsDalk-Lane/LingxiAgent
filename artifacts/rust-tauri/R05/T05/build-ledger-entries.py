#!/usr/bin/env python3
"""R05-T05 acceptance-ledger entries generator (REVIEW-T05 R01 fix round).

Run AFTER the fix-r1 evidence logs exist and the tree is final. Appends the
C01..C13 (+C11B split) entries to docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json
and rewrites the R05-T05 task entry in PROGRESS_LEDGER.json. Digests are
computed from the live tree (working-tree manifest + test runner binaries
+ lockfile). R01 changes: F-01 (C12 private-CA rationale corrected), F-02
(oauth no_proxy + regression test), F-03 (idle-stream seam test in C01),
F-04 (C11 SSRF/attachment legs split out as C11B NOT_RUN), F-05 (wording).
"""
import json, hashlib, subprocess, pathlib, sys

ROOT = pathlib.Path("/Users/study_superior/Desktop/Code/LingxiAgent")
T05 = ROOT / "artifacts/rust-tauri/R05/T05"
FIX = T05 / "fix-r1"
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
(T05 / "working-tree-manifest.txt").write_text(manifest)
tree_digest = "sha256:" + sha256(T05 / "working-tree-manifest.txt")

lock_hash = "sha256:" + sha256(ROOT / "rust/Cargo.lock")

# ── runner digests: the r05_t05 test binaries actually executed (plus the
# r05_t02 oauth binary carrying the F-02 regression test) ────────────────────
runners = {}
deps = ROOT / "rust/target/debug/deps"
# Newest executable per suite stem (cargo keeps stale hash-named binaries).
# r05_t05_timeouts exists for TWO crates (lingxi-adapters + lingxi-service);
# the other stems are single-crate.
EXPECTED = {"r05_t05_compat": 1, "r05_t05_timeouts": 2, "r05_t02_oauth_flows": 1}
for stem, keep in EXPECTED.items():
    candidates = [
        p for p in deps.glob(f"{stem}-*")
        if p.is_file() and p.suffix == ""  # executables only (skip .d / .rcgu.o)
    ]
    candidates.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    for binary in candidates[:keep]:
        runners[binary.name] = "sha256:" + sha256(binary)
if len(runners) != sum(EXPECTED.values()):
    sys.exit(f"runner binary set incomplete: {sorted(runners)}")

window = (FIX / "evidence-window.txt").read_text().splitlines()
started_at, finished_at = window[0], window[1]

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
        "外部模型HTTP服务(real loopback raw-TCP stub at the far end of the wire with scripted fault injection; service-level runs use a scripted TurnProviderPort double that only produces external responses); NOT_REAL_API — no real provider key, account or traffic"
    ],
    "externalAuthorizationRef": None,
    "reviewerExecutionRef": None,
    "blockingReason": None,
    "supplementalObligationIds": [],
    "ignoredRequiredTests": [],
}

ADAPTERS_TIMEOUTS = "cargo test --manifest-path rust/Cargo.toml -p lingxi-adapters --test r05_t05_timeouts"
SERVICE_TIMEOUTS = "cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test r05_t05_timeouts"
COMPAT_SUITE = "cargo test --manifest-path rust/Cargo.toml -p lingxi-adapters --test r05_t05_compat"
OAUTH_FLOWS = "cargo test --manifest-path rust/Cargo.toml -p lingxi-adapters --test r05_t02_oauth_flows"
LIFECYCLE = "cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test run_lifecycle"
CANCEL_TREE = "cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test cancellation_tree --test cancel_terminal_race --test cancel_link_inheritance"
T04_SERVICE = "cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test r05_t04_streaming"

# REVIEW-T05 R01 fix round: every cited suite re-executed on the FINAL
# (post-F-01..F-05) tree; logs under fix-r1/.
EV_A = "artifacts/rust-tauri/R05/T05/fix-r1/test-r05-t05-timeouts-adapters.log"
EV_S = "artifacts/rust-tauri/R05/T05/fix-r1/test-r05-t05-timeouts-service.log"
EV_C = "artifacts/rust-tauri/R05/T05/fix-r1/test-r05-t05-compat-adapters.log"
EV_OAUTH = "artifacts/rust-tauri/R05/T05/fix-r1/test-r05-t02-oauth-flows.log"
EV_REG = "artifacts/rust-tauri/R05/T05/fix-r1/test-regression-t01-t04-sampling.log"
EV_RS = "artifacts/rust-tauri/R05/T05/fix-r1/test-regression-service-waits.log"
REVIEW = "artifacts/rust-tauri/R05/REVIEW-T05/R01_REVIEW.md"
REV_TLS = "artifacts/rust-tauri/R05/REVIEW-T05/tls-probe.log"
REV_CONNECT = "artifacts/rust-tauri/R05/REVIEW-T05/connect-probe.log"
REV_IDLE = "artifacts/rust-tauri/R05/REVIEW-T05/idle-probe.log"
REV_EXTRACA = "artifacts/rust-tauri/R05/REVIEW-T05/node-extra-ca-probe.log"
REV_FETCHPROXY = "artifacts/rust-tauri/R05/REVIEW-T05/node-fetch-proxy-probe.log"

def entry(case_id, formal, scope, dims, status, command, names, expected, observed, evidence, blocking=None):
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
    order = ["caseId","formalScenarioIds","supplementalObligationIds","scope","dimensions","status","command","testNames","executedCount","ignoredRequiredTests","exitCode","expected","observed","sourceSha","workingTreeDigest","runnerDigest","binarySha256","lockfileHashes","schemaHash","fixtureHash","redactedConfigHash","platform","toolchain","startedAt","finishedAt","evidence","mockBoundary","externalAuthorizationRef","reviewerExecutionRef","blockingReason"]
    e["exitCode"] = 0 if status == "PASS" else None
    e["blockingReason"] = blocking
    return {k: e[k] for k in order}

CHAT_DIM = {"protocol": "all five chat families (openai-completions exercised on the wire; the segment discipline lives in the shared dispatch layer every family consumes)", "operation": "chat", "auth": "api_key (stub)"}

entries = [
entry("R05-T05-C01", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS",
 f"{ADAPTERS_TIMEOUTS} && {SERVICE_TIMEOUTS}",
 ["pre_send_budget_exhaustion_sends_nothing_and_is_not_retryable",
  "first_byte_window_hit_is_terminal_no_blind_resend",
  "connect_refusal_is_retryable_upstream",
  "connect_refusal_is_the_expected_underlying_kind",
  "deadline_hit_inside_first_byte_wait_is_non_retryable_budget",
  "deadline_hit_mid_stream_abandons_the_read_without_rewind",
  "a_retry_that_cannot_fit_the_call_budget_is_vetoed_not_slept_into",
  "an_idle_stream_is_cut_at_the_injected_bound_and_settles_honestly"],
 "connect/TLS/首片/空闲流/总耗时分别可控挂起的受控替身；各期限生效；总预算覆盖排队+凭证刷新+退避；心跳不无限续总期限。用预登记值。",
 "分段位的执行证据（REVIEW-T05 R01 fix-r1 树复跑全绿）：raw-TCP 替身 hold-open 命中首片窗（NON-retryable，A10）；deadline 落在首片窗内由新鲜时钟读判为 budget_exceeded；预算耗尽在发送前响拒且 stub 观测 0 请求；流中段 deadline 命中放弃读取、已发 partial delta 不倒带；连接期失败（拒绝/挂起，可证明未接受）可重试。服务面：总预算 1.5s+退避 10s 的重试被 deadline 否决——按原失败结算（attempts=1、provider 恰 1 次调用、无 10s 盲睡）。空闲流腿（F-03 固化为常驻秒级测试）：经 ServiceDeps.stream_idle_timeout 注入 200ms 界限，「一个 delta 后永久挂起」的 double 在注入界限被切断，100ms 退避后重试，3 次尝试（预登记预算）后诚实失败 failed.provider_error，每尝试的 partial delta 持久不倒带；生产 60s 值的计时签名由审查者 idle-probe 补证（181.531s = 3×60.01s 跳断 + 0.5s/1.0s 退避）。断言覆盖如实登记（F-05）：total_budget 300s/backoff 500/8000/attempts=3 有测试断言钉死；connect 10s/first_byte 30s 常量无直接测试断言（审查者 connect-probe 打印核对 + R05_BASELINE §8 交叉佐证）。",
 [EV_A, EV_S, REV_IDLE, REV_CONNECT, "rust/crates/lingxi-adapters/tests/r05_t05_timeouts.rs", "rust/crates/lingxi-service/tests/r05_t05_timeouts.rs"]),
entry("R05-T05-C02", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{SERVICE_TIMEOUTS} && {CANCEL_TREE}",
 ["the_backoff_sleep_holds_no_model_call_permit",
  "r03_a05_cancel_exits_each_wait_state_and_returns_quotas",
  "subagent_timeout_settles_a_parked_quota_wait"],
 "模型配额小并发上限+多会话排队+取消：无无限等待者/permit 泄漏；无关会话与 health 可响应。",
 "全局模型配额压到 1：run A 失败进入 2.5s 退避后，run B（另一会话）在退避窗口内被准入并完成——permit 随失败调用的 I/O 结束即释放，退避睡眠不占配额（attempt 时间戳归属按 run id 前缀对账）。既有腿：配额等待中的取消按原生命周期退出并归还配额（cancellation_tree r03_a05）、停泊的配额等待被子代理超时结算（cancel_link_inheritance）。配额等待本身有界（wait_queue_capacity=64/wait_timeout 预登记，quota 校验拒绝零值）。",
 [EV_S, EV_RS]),
entry("R05-T05-C03", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{ADAPTERS_TIMEOUTS} && {SERVICE_TIMEOUTS}",
 ["http_429_delay_seconds_hint_lands_in_details",
  "http_429_http_date_hint_lands_in_details",
  "http_429_garbage_hint_is_ignored_never_trusted",
  "http_5xx_is_bounded_retryable_without_a_fabricated_hint",
  "retry_after_parsing_pins_the_rfc_9110_forms",
  "retry_after_hint_overrides_the_computed_backoff",
  "computed_backoff_applies_without_a_hint",
  "computed_backoff_doubles_from_the_base_to_the_cap",
  "the_default_attempt_budget_is_the_preregistered_three"],
 "429/Retry-After/5xx 有界重试：尊重合法等待且受次数/总预算约束；取消立即停止；无递归无限退避。",
 "429 → budget_exceeded 可重试 + details.retryAfterMs（delay-seconds 精确 2000ms；HTTP-date 换算区间钉住；过去日期饱和 0；空/soon/-5/12x/溢出等垃圾 hint 一律忽略不捏造）。5xx 可重试且无 hint 细节。driver：hint 覆盖计算退避（30s 计算值被 400ms hint 覆盖，gap≥400ms 且远小于 30s）；无 hint 走 base×2^(n-1) 封顶（500/1000/2000/4000/8000/8000 纯函数钉）；预登记默认 max_attempts=3；预算否决与取消立即结算见 C01/C08 条目。",
 [EV_A, EV_S]),
entry("R05-T05-C04", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{LIFECYCLE} && {SERVICE_TIMEOUTS}",
 ["retryable_provider_failure_reopens_attempt_on_the_same_run",
  "retry_budget_exhaustion_fails_loudly_with_attempt_count_honest",
  "retry_after_hint_overrides_the_computed_backoff"],
 "重试身份对账：Run/modelCall/transportAttempt 边界明确；实际可能计费请求不被去重成零成本。",
 "重试 = 同一 run 上的新 attempt（#a1→#a2，run 行恰一行、run_attempts 各自成行、key_events 按 attempt 归属 3+4 不交叉）；尝试预算耗尽响亮失败且 attempt_count 诚实（=2）；driver 重试永不复用旧 attempt 身份。每次重试都是一次真实外发（适配器无内部网络 retry——一次调用=一个网络 attempt，与现役 llm-client.ts:838 一致），计费面不存在去重。",
 [EV_S, EV_RS]),
entry("R05-T05-C05", ["R05-A10"], "OFFLINE_LOGIC", CHAT_DIM, "PASS",
 f"{ADAPTERS_TIMEOUTS} && {LIFECYCLE}",
 ["accepted_then_dropped_is_terminal_and_never_resent",
  "mid_stream_transport_break_is_terminal_and_never_resent",
  "first_byte_window_hit_is_terminal_no_blind_resend",
  "non_retryable_failure_settles_without_extra_attempts"],
 "受控服务接受非幂等请求后断连：无幂等键/核验→明确停止；不生成第二次外发。外部计数器证明。（A10）",
 "重试分类按 A10 收紧（对 T03/T04 较宽分类的修正，INTERFACE_EVOLUTION §25 裁决 2）：发送后失败/首片超时/流中段断读/408 全部不可重试（请求可能已被服务端持有，无幂等键即绝不盲重发；与现役 model-operation-client.ts:448 只重试 429/5xx 对齐）。替身脚本「读完整个请求后无应答关闭」→ 不可重试 upstream_unavailable + stub 命中恰 1 次（外部计数器证明无第二次外发）；流中段「部分 delta 后无终结标记关闭」同样恰一次外发且 partial 不倒带。服务面：不可重试失败原地结算、attempts=1（无第二次外发的组合证明）。唯一可重试传输类 = 连接期失败（连接拒绝 + 连接期挂起/TLS 握手 stall——两者都可证明服务端未接受任何字节；措辞按 REVIEW-T05 R01 connect-probe 修正：402ms 跳断、is_connect=true、归类可重试，符合 A10 方向）。",
 [EV_A, EV_RS, REV_CONNECT, "rust/crates/lingxi-adapters/src/models/dispatch.rs"]),
entry("R05-T05-C06", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{ADAPTERS_TIMEOUTS} && {T04_SERVICE} && {LIFECYCLE}",
 ["mid_stream_transport_break_is_terminal_and_never_resent",
  "deadline_hit_mid_stream_abandons_the_read_without_rewind",
  "c15_a08_http200_stream_failures_keep_the_run_honest",
  "retryable_provider_failure_reopens_attempt_on_the_same_run"],
 "第一请求部分文本后失败，获准新尝试成功：新旧尝试明确区分/替换；不无标记拼接。",
 "部分文本在流中段失败后作为已发 delta 保持持久（适配器 sink 观测到逐字前缀，失败不倒带）；运行诚实失败、无伪造 final（T04-A08 服务面钉住）。「获准新尝试」= 新 attempt/新 run 的显式身份（attempt 对账见 C04）——续跑请求携带已确认交换、失败调用自身不推 assistant turn，无拼接发生；final 只可能来自一个干净闭合的调用。",
 [EV_A, EV_RS]),
entry("R05-T05-C07", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS", SERVICE_TIMEOUTS,
 ["a_retry_after_a_confirmed_tool_effect_never_re_executes_it"],
 "写操作已确认后下一模型请求失败→重试不重做已确认工具；继续请求含原结果。",
 "turn1 工具请求→计数工具 double 执行成功（副作用计数=1，attempt #a1）→turn2 模型调用可重试失败→重试（attempt #a2）：工具计数恒 1（绝不重做已确认副作用）；#a2 的输入 prior 携带 assistant 工具请求轮 + 已确认 ToolResult（success，原文内容块）；运行 completed.with_final、attempts=2。与 T03-C12（重试保留已确认交换）互补——本条钉重试语义本身。",
 [EV_S, "rust/crates/lingxi-service/tests/r05_t05_timeouts.rs"]),
entry("R05-T05-C08", ["R05-A09"], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 f"{SERVICE_TIMEOUTS} && {CANCEL_TREE} && {T04_SERVICE}",
 ["cancellation_during_backoff_settles_now_not_after_the_delay",
  "r03_a05_cancel_exits_each_wait_state_and_returns_quotas",
  "cancel_accepted_while_final_events_persist_beats_the_completed_terminal",
  "cancel_racing_the_finalize_transaction_is_too_late_not_accepted",
  "c16_a09_midstream_cancel_releases_the_connection_and_settles_once"],
 "取消覆盖每类等待：配额/刷新/连接/流/参数/审批/工具/worker callback 逐一暂停点取消；各等待按原生命周期退出；迟到结果不入正文。",
 "退避等待臂（T05 新增）：60s 退避中取消 → 立即按四阶段结算 cancelled.requested（<5s，绝不睡满余量）、attempts=1、provider 不再被调。既有腿覆盖其余等待类：配额等待取消退出并归还（cancellation_tree r03_a05）；流中取消 drop 掉 socket 读、stub 观测断连、恰一次结算（T04 c16_a09）；终态提交边界的取消竞态双腿（接受先于 claim 者胜/太晚不接受）。连接期等待的取消由同一切换结构覆盖（select 臂挂在 root token 上，acquire/connect 共享同一取消树）。",
 [EV_S, EV_RS]),
entry("R05-T05-C09", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 CANCEL_TREE,
 ["cancel_accepted_while_final_events_persist_beats_the_completed_terminal",
  "cancel_accepted_before_the_terminal_claim_beats_every_terminal_shape",
  "cancel_racing_the_finalize_transaction_is_too_late_not_accepted",
  "concurrent_duplicate_fires_keep_the_first_reason_and_a_single_fired"],
 "自然结束与取消唯一结算：屏障同时到达多次固定顺序；终态/计费不冲突不双提交不误伤无关任务。",
 "取消与终态的唯一裁决在 adjudicated_finalize 单点（claim 先于 commit，无复查间隙）：终态事件持久化期间被接受的取消胜过 completed；一旦 claim 落定，取消不再被接受；重复/并发取消保留首个原因。唯一终态由 cancel_terminal_race 全套钉住（既有证据，T05 复跑确认无回归）。",
 [EV_RS]),
entry("R05-T05-C10", [], "OFFLINE_SERVICE", CHAT_DIM, "PASS",
 "cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test background_disconnect_recovery --test event_subscription",
 ["r03_a12_background_run_survives_disconnect_reconnect_replays_and_is_queryable",
  "a09_snapshot_and_subscription_have_no_gap_at_every_commit_boundary",
  "a09_parallel_subscribers_see_identical_views"],
 "客户端重连不重发：任务外发中断连再按 cursor 重连；只续读事件，外部请求与工具调用不增加。",
 "后台执行面断连恢复与事件订阅续读套件在 T05 最终树上复跑全绿（cursor 续读只回放事件流；run/driver 状态机不因订阅断连重启任何外发——外发次数由 C05 的 stub 计数语义保证，订阅面不持有任何外发触发路径）。",
 [EV_RS]),
entry("R05-T05-C11", [], "OFFLINE_LOGIC", CHAT_DIM, "PASS",
 ADAPTERS_TIMEOUTS,
 ["dispatch::tests::error_status_classification_matches_the_incumbent",
  "pre_send_budget_exhaustion_sends_nothing_and_is_not_retryable"],
 "URL 与资源边界（已执行腿，F-04 拆腿后）：显式授权本地端点保留；重定向按策略响拒、不跟随、不回显；凭证按族映射只发往配置端点、无跨族泄漏。",
 "已执行腿（REVIEW-T05 R01 fix-r1 树复跑绿）：无重定向 client（Policy::none）+ 3xx 响拒不跟随（Location 永不回显——错误消息净化单测钉住）+ 凭证材料按族映射只发往配置端点（T03 golden 的 absent_headers 钉住跨族泄漏）。拆腿纪律（F-04）：未授权内网地址（SSRF）策略腿与附件 URL 取数授权腿不折叠进本 PASS，单独登记为 R05-T05-C11B（NOT_RUN，附 blockingReason 与源码依据）。",
 [EV_A, EV_REG]),
entry("R05-T05-C11B", [], "OFFLINE_LOGIC", CHAT_DIM, "NOT_RUN",
 "n/a（无对应实现，登记遗留）",
 [],
 "URL 与资源边界（F-04 拆出的策略腿）：未授权内网地址（SSRF）按策略阻止越权取数与凭证转发；附件 URL 取数授权；DNS 策略一致性。",
 "现役 TS 模型路径同样无 SSRF 策略层（全仓 grep 证实：仅 core/model-operation-resolver.ts 的 isLocalBaseUrl 本地端点判定，非取数策略层）；附件 URL 取数属 T06 媒体范围。两腿在本阶段无对应实现，按 §9 不得标 NOT_APPLICABLE（非协议变体缺失），如实 NOT_RUN 并归后续网络加固阶段（REVIEW-T05 R01 确认遗留本身合理）。",
 ["docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md", REVIEW],
 "deferred-to-network-hardening: 现役模型路径无 SSRF 取数策略层（grep 证据）；附件 URL 取数为 T06 范围"),
entry("R05-T05-C12", [], "OFFLINE_LOGIC", CHAT_DIM, "NOT_RUN",
 "n/a（范围裁决 + F-01 更正登记）",
 [],
 "代理与 TLS 不降级：系统/手动/直连/NO_PROXY 配置面；有效/无效证书与测试私有 CA；拒绝无效证书；私有 CA 显式授权；不全局关闭 TLS 验证。",
 "拆腿登记（REVIEW-T05 R01）：(1) 无效证书拒绝腿 PASS——审查者 tls-probe 以生产 dispatch::build_client() 实证自签证书被拒（macOS 信任评估错误码 -67843），同端口纯 HTTP 对照 200，全仓无 danger_accept_invalid_certs 调用点；固化为常驻测试登记为廉价后续项。(2) 代理配置面腿 NOT_APPLICABLE 成立（附来源）：现役模型路径零代理处理代码；审查者 node-fetch-proxy-probe 实测内建 fetch 不受理 HTTP(S)_PROXY（直连 200、计数代理 0 命中）；lib/net/outbound-proxy.ts 的 dispatcher 只影响 MCP/bridge 面——直连裁决（§25 裁决 5 的 no_proxy()）与现役对齐，F-02 已把同一纪律补到 OAuth client（token/refresh 携带 client_secret/refresh_token；计数代理回归测试 r05_t02_oauth_flows::the_credential_bearing_client_never_consults_the_ambient_proxy 钉住，pre-fix 失败已实证）。(3) 私有 CA 腿为真实能力差距（F-01 更正，原「无现役对应物」理由事实错误）：现役内建 fetch 尊重 NODE_EXTRA_CA_CERTS（审查者 node-extra-ca-probe 实测私有 CA 签名端点 200 OK），另有 Windows 系统 CA 合并通道（desktop/src/shared/windows-system-ca.cjs，main.cjs:95 接线）；Rust 侧 rustls-platform-verifier 读平台根（Windows 系统 CA 腿由此覆盖），但无 NODE_EXTRA_CA_CERTS 等价物（全平台操作者附加 CA 文件通道）——显式登记为后续网络加固阶段任务。TLS 验证从未全局关闭。",
 ["docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md", REVIEW, REV_TLS, REV_EXTRACA, REV_FETCHPROXY, EV_OAUTH, "desktop/src/shared/windows-system-ca.cjs"],
 "deferred-by-scope-decision（F-01 更正后）：代理配置面现役无对应处理代码（NOT_APPLICABLE，附审查者来源证据）；私有 CA 腿 = 真实能力差距（现役有 NODE_EXTRA_CA_CERTS 通道，Rust 侧无等价物），归后续网络加固阶段；TLS 验证保持默认开启（无降级点）"),
entry("R05-T05-C13", [], "OFFLINE_LOGIC",
 {"protocol": "12 个 provider-compat 模块 × 4 信封族", "operation": "chat", "auth": "n/a (pure payload transform)"},
 "PASS",
 COMPAT_SUITE,
 ["every_golden_aligns_with_the_incumbent_ts_run",
  "models::compat::tests::dispatch_is_first_match_in_incumbent_order",
  "models::compat::tests::volcengine_disable_guard_passes_through_without_controls",
  "models::compat::tests::openrouter_adaptive_disable_is_a_loud_refusal",
  "models::compat::tests::anthropic_marks_system_and_two_recent_user_messages",
  "models::compat::tests::anthropic_max_effort_raises_the_implicit_one_third_cap",
  "models::compat::tests::codex_strips_budget_and_temperature_fields",
  "models::compat::tests::ollama_bridges_response_format_and_num_ctx"],
 "provider 专属 compat 补丁移植：12 个注册表模块逐个对照 matches()/patch 语义；golden 对齐现役 TS 输出；移植不完的逐条登记不适用理由附源码依据。",
 "12 模块全移植（deepseekResponses/deepseek/kimi/mimo/qwen/zhipu/volcengine/agnes/openrouter/anthropic/codexResponses/ollama），first-match-wins 顺序与现役 PROVIDER_MODULES 一致；49 个 golden fixture 由生成器直跑现役 TS 模块（Node 24 type-stripping）产出；Rust 套件断言推导链/派发/payload/拒绝文案结构相等（serde_json Value，key 序不敏感；拒绝文案字符串逐字相等），fixture 文件逐字节一致由生成器幂等重跑证明（R01 审查者复跑 49 fixture shasum diff 为空）。TS throw → 不可重试 InvalidMessage（拒绝文案逐字）。不移植登记（附源码依据，PROTOCOL_WIRE_MATRIX.json shared.provider_compat_port）：longcat（本管道恒等——所有触发条件 Rust 侧不可达）、openai-input-audio（T06 媒体范围）、openai-video-url（不在注册表）、中心层通用补丁（对 Rust 渲染输出恒等）；google 族不接 compat（现役无针对 google 信封的模块，登记偏差）。",
 [EV_C, "docs/rust-tauri/R05/r05_t05_generate_compat_goldens.mjs", "rust/crates/lingxi-adapters/src/models/compat.rs", "rust/crates/lingxi-adapters/tests/golden-compat/"]),
]

ledger = json.loads(LEDGER.read_text())
existing = {e["caseId"] for e in ledger["acceptances"]}
dupes = [e["caseId"] for e in entries if e["caseId"] in existing]
if dupes:
    sys.exit(f"duplicate caseIds already in ledger: {dupes}")
ledger["acceptances"].extend(entries)
ledger["note"] = ("T01+T02+T03 slices (each C01-C12) + T04 slice (C01-C16) + T05 slice "
                  "(C01-C13 + C11B leg split; 66 entries). Every PASS entry is backed by "
                  "real executed evidence under artifacts/rust-tauri/R05/ (T05 slice "
                  "re-executed on the REVIEW-T05 R01 fix tree, evidence in T05/fix-r1/); "
                  "the NOT_RUN entries (C11B SSRF/attachment policy legs; C12 proxy "
                  "config surface + private-CA capability gap) carry explicit "
                  "blockingReasons with source citations.")
LEDGER.write_text(json.dumps(ledger, indent=1, ensure_ascii=False) + "\n")
print(f"ledger: appended {len(entries)} entries (total {len(ledger['acceptances'])})")

# ── PROGRESS_LEDGER task entry ────────────────────────────────────────────────
progress = json.loads(PROGRESS.read_text())
for task in progress["tasks"]:
    if task.get("task") == "R05-T05":
        task.update({
            "name": "网络策略、超时与安全重试",
            "status": "READY_FOR_INDEPENDENT_REVIEW",
            "executor": "EXECUTOR-R05-T05",
            "cases_total": 14,
            "cases_pass": 12,
            "cases_not_run": 2,
            "acceptance_ledger": "docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json",
            "evidence_dir": "artifacts/rust-tauri/R05/T05/ (R01 fix round: artifacts/rust-tauri/R05/T05/fix-r1/)",
            "interface_evolution": "docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md §25-§28",
            "notes": "REVIEW-T05 R01 findings closed: F-01 C12 private-CA rationale corrected (incumbent HAS NODE_EXTRA_CA_CERTS + Windows system-CA merge; Rust lacks an equivalent — real capability gap registered to the network-hardening phase); F-02 oauth.rs OAuthHttp gained .no_proxy() with a counting-proxy regression test (discrimination proven pre/post); F-03 idle-stream leg pinned fast via ServiceDeps.stream_idle_timeout seam (injected 200ms; production 60s signature by reviewer idle-probe); F-04 C11 SSRF/attachment legs split out as C11B NOT_RUN; F-05 wording/count corrections. 12 PASS with executed evidence; NOT_RUN = C11B + C12 (both with blockingReasons). Compat port: 12 modules, 49 TS-generated goldens, structural equality + fixture byte-idempotence.",
        })
        break
else:
    sys.exit("R05-T05 task entry not found")
PROGRESS.write_text(json.dumps(progress, indent=1, ensure_ascii=False) + "\n")
print("progress ledger: R05-T05 entry updated")
