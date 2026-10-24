# R08-A04「卡片展开不退化」交接（t02-a04）

任务书 R08 §4 R08-T02。前置：只有工具结果没有最终回复的会话；操作：展开汇总区再点击
任意工具卡；通过：正常展开，错误/思考/产物内容可查看。**结论：PASS**（证据目录见下）。

## 交付物

1. 服务套件 `rust/crates/lingxi-service/tests/r08_a04_no_final_ws.rs`（新增，880 行）
   + 接口 golden `rust/crates/lingxi-service/tests/fixtures/r08_a04_ws_golden.json`。
2. 桌面夹具 `desktop/src/react/__tests__/services/fixtures/r08-a04-history.json`
   （服务捕获 `a04-history-raw.json` 原样拷贝，拷贝本身即证据）。
3. 桌面组件测试 `desktop/src/react/__tests__/components/r08-a04-card-expansion.test.tsx`
   （3 个用例：夹具契约 / 真实投影 / 汇总区+逐卡点击交互）。

## 服务套件实现要点

- 模板沿用 A01（`r08_t01_a01_a02_ws.rs`）：确定性替身 Provider（`R08_SYNTHETIC_ONLY`
  apiKey，本机回环）、隔离 LINGXI_HOME（`lingxi-r08-a04-*`）、WS `lingxi.wire` 握手、
  golden 占位规则（ID_KEYS/TS_KEYS/argsDigest/工作区路径串→`<workspace-path>`）。
- Provider 三幕剧本：第 1 幕 SSE `reasoning_content=A04-THINKING-ALOUD` +
  `content=A04-READING-SEED` + tool_call 读 `a04-seed.txt`；第 2 幕 tool_call 读
  `a04-missing.txt`（真实失败）；第 3 幕 `delta:{}` + `finish:"stop"` + usage →
  `ProviderTurn::Empty`（openai_completions.rs:825-919 分类）→ `finish_no_final`
  （runs.rs:6860），原因优先级工具失败 > 进程 > 空 →
  `NoFinalCause::ToolPartialFailure`（`completed.no_final.tool_partial_failure`）。
- 断言面：
  - WS 直播帧：2 对 tool_start/tool_end，id 一一配对；失败 end `success:false`、
    `error` 原文 `Could not read file: a04-missing.txt. Error code: ENOENT.`；
    无任何 `final_message` 帧；无 `final_answer` 相位段帧；seq 全链连续 +1。
  - 真实副作用与反证：种子内容原样；缺失文件从未被创建。
  - 持久历史（`lingxi.canonical-history.v1`）：6 条目 = user + 2 assistant_segment
    （reasoning 相位 complete；叙述段 `unresolved` 相位，绝不伪造 final_answer）
    + 2 tool_call（成功含种子内容原文与 `source.path`；失败 `not_found` 原文；
    argsDigest 两卡互异）+ 1 run_terminal（completed、terminalReason、十进制
    eventSeq）；0 final_message；run 归属同源。
  - 跨通道同源：历史 toolCallId/runId 与直播帧逐一相等。
  - 供应商线：3 次 chat 请求携带 tool 消息数 0/1/2；失败结果同样回灌
    （model_history.rs:1586-1615 归档每个 ToolResult），第 3 次请求含
    `a04-missing.txt`。
  - 账本：key_events `tool_call_started/completed` 各 2；runs 表 `status=completed`、
    `terminal_reason=completed.no_final.tool_partial_failure`，与历史 terminal 同文。
  - 接口 golden（events/history/etag/files/providerChatRequests）字节级比对 +
    原始捕获（history/events/normalized golden 三份，capture/ 下）。

## 桌面组件测试实现要点（端口决定）

- 全真实链路：夹具 → `projectRustHistoryItems` → `ChatTranscript enableProcessFold`
  → `ProcessFoldBlock`/`ToolGroupBlock`/`ThinkingBlock`。不手搭投影、不 mock 任何
  产品组件（连 `message-turn-actions` 都未 mock，导入在 jsdom 下无副作用）。
- 用例 1（夹具契约）：6 条目种类序列、0 final_message、reasoning/unresolved 相位、
  双工具成败字段与 argsDigest、run_terminal 诚实终态、run 归属同源。
- 用例 2（投影）：turnProjection `status=completed`、
  `outcome=completed_without_user_output`、
  `missingFinalAnswerReason=only_process_blocks`（枚举以 turn-projector.ts /
  chat-types.ts:237 为准）、`answerBlockIds` 为空；思考块 sealed、叙述文本
  `surfaceRole=process`；两 tool_group（succeeded/failed，error 原文）；
  `turn_status missing_final_answer` 结果块在 `resultBlockIds`（答案面）。
- 用例 3（交互）：折叠态过程内容不可见、结果块立即可见 → 点击汇总区
  （`aria-expanded` false→true）→ 两张工具卡出现（失败卡行摘要即展示 ENOENT 原文）
  → 点击失败卡展开，错误可查；点击成功卡正常展开且无错误文本；点击思考卡展开，
  `A04-THINKING-ALOUD` 可查 → 再点汇总区收折，退场动画结束后卡片卸载（waitFor），
  结果块始终在位。

## 证据目录（artifacts/rust-tauri/R08/）

| 目录 | 结果 | 内容 |
|---|---|---|
| a04-suite-01 | RED（保留） | 隔离套件构建撞上工作区瞬时态（见偏差 1），exit 101 |
| a04-suite-02 | RED（保留） | 套件编译通过、测试体红：段帧 `kind` 断言与实测线格式不符（见偏差 2）|
| a04-suite-03 | GREEN | golden 写入轮；capture/ 下三份原始捕获（history 即桌面夹具来源）|
| a04-suite-04 | GREEN | golden 字节级比对轮（无 GOLDEN_WRITE），exit 0 |
| a04-ui-01 | RED（保留） | jsdom 下 `import.meta.url` 非 file://（见偏差 3）|
| a04-ui-02 | RED（保留） | toolCallId 位数断言 7→6；收折断言撞上退场动画（见偏差 4）|
| a04-ui-03 | GREEN | `r08-a04-card-expansion.test.tsx` 3/3 通过（151ms）|
| a04-typecheck-01 | GREEN | `npm run typecheck` 全绿（tsc x3），exit 0 |
| a04-ui-regression-01 | GREEN | 相邻套件回归：A01 投影 5/5 + ProcessFoldBlock 5/5，共 10/10 |

## 偏差记录（全部为断言对齐实测/环境，无任何产品代码改动）

1. **a04-suite-01 环境红**：隔离套件 `cargo test --locked` 编译 lingxi-service lib 报
   `PermissionKind::ArgumentAwareLsp/ArgumentAwareRunCode` 不存在，而同一棵树工作区
   `cargo check --tests` 为绿。根因：并行队友对
   `rust/crates/lingxi-kernel/src/toolcatalog.rs`（未提交 M）与
   `rust/crates/lingxi-service/src/coding_ast_lsp.rs`（未跟踪）的进行中编辑在套件
   编译瞬间处于缺变体中间态；非本人写入范围，未做任何源码改动。数分钟后树稳定，
   a04-suite-02 起编译通过。红证据保留。
2. **a04-suite-02 断言红**：我假设段 delta 帧携带 `kind`；实测线格式 `kind` 只在
   `assistant_segment_start` 帧，delta/end 帧只带 `semanticPhase`（以实际捕获帧为准）。
   已按真相修正：start 帧断言 kind+相位、delta 帧按 semanticPhase 匹配，测试内注释
   记录该线格式事实。
3. **a04-ui-01 断言红**：jsdom 环境 `import.meta.url` 非 file 方案（A01 投影测试为
   node 环境故可用 fileURLToPath）。改为 `process.cwd()` 相对路径（与
   ProcessFoldBlock.test.tsx 读源码同法）。
4. **a04-ui-02 断言红**：(a) toolCallId run 序号实际 6 位（`run_<16hex>_000001`），
   正则按实测改为 `\d{6}`；(b) 收折后面板不立即卸载——`@/ui` Collapse 走
   framer-motion 退场动画（AnimationPrimitives.tsx:96-113 `{open && ...}` +
   AnimatePresence exit），此前从模板拷贝的同步单次 RAF mock 让时间永不前进、
   卸载永不完成。移除 RAF mock 改用 jsdom 原生 RAF + `waitFor` 等待真实卸载，
   测试内注释记录该约束。

## 红线自查

- 无占位实现/跳过断言/仅更新状态；失败工具卡、思考卡、成功卡均为真实点击展开断言。
- 写入范围仅：服务 tests/（新文件+golden）、桌面 __tests__/（新夹具+新测试）、本文档。
  未触碰任何产品源码（rust-chat-history.ts、turn-projector.ts、组件、kernel、service）。
- 未触碰 `~/.lingxi`；全部运行时为合成根（/private/tmp/lingxi-r07-verify 等）。
