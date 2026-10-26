# R08 收尾 · D01 交互两叶双内核 A/B 证据（interactive-dev）

叶子：

| 叶 ID | 任务键（drill observation） | 套件测试名 |
|---|---|---|
| R00-T02-LA-94CB746D04B7（ask_user，5 断言） | `leaf.tool.ask-user` | `leaf.tool.ask-user: 卡片可见/确认映射/超时推荐/拒答与中止无答案/无效与缺省显式错误（5 断言）` |
| R00-T02-LA-B47CAC08F418（POST task abort，2 断言） | `leaf.chat.task-abort` | `leaf.chat.task-abort: 终止本会话拥有的后台子代理任务 + 404/403/运行态/不扩权（2 断言）` |

套件文件（新增，非产品代码）：`desktop/src/react/__tests__/r08/r08-t07-interactive-ab.test.ts`
作为追加套件由 `scripts/rust-tauri/r08_t07_ab_drill.mjs --suite-files` 与主套件同跑；任务键/案例键与主套件无冲突。

## 复跑命令（exit 0）

```bash
export PATH="$HOME/.cargo/bin:$PATH"
LINGXI_R08_AB_TEST_FILTER='leaf.tool.ask-user|leaf.chat.task-abort' \
node scripts/rust-tauri/r08_t07_ab_drill.mjs \
  --bin rust/target/debug/lingxi-service \
  --work /private/tmp/lingxi-r08-verify/r08_t07_ab-interactive-dev-03 \
  --suite-files desktop/src/react/__tests__/r08/r08-t07-interactive-ab.test.ts
```

终态：`AB_RESULT tasks=2 parity=2 differs=0 recorded=0 failedA=0 failedB=0`，`T07 AB OK`，drill exit=0，两腿 vitest exit 均 0（Node v24.16.0 / cargo 1.98.1 / HEAD aa99d2ec1）。

## 断言 → case 映射

### ask_user（`leaf.tool.ask-user`，29 个 case，双腿全 true）

| 原断言 | 专属 case |
|---|---|
| A0 卡片问题与选项正确可见 | `a0.questions-and-options-visible`、`a0.values-and-labels-visible`、`a0.recommended-visible`、`a0.pending-question-exposed` |
| A1 提交值与 key/label 映射一致（确认返回按 key 映射、answered=true） | `a1.confirmed-answered-true`、`a1.keys-mapped`、`a1.values-mapped`、`a1.labels-mapped`、`a1.unknown-value-dropped`、`a1.optional-unanswered`、`a1.no-auto-selected`、`a1.pending-cleared-after-answer`、`a1.round-completes` |
| A2 超时只自动填推荐项（timedOut/autoSelected） | `a2.timedout-flag`、`a2.recommended-auto-selected`、`a2.non-recommended-not-filled`、`a2.round-completes`、`a2.pending-cleared-after-timeout` |
| A3 拒答和中止均无答案（dismissed/answers 空） | `a3.dismissed-no-answers`、`a3.dismissed-pending-cleared`、`a3.dismissed-round-completes`、`a3.aborted-no-answers`、`a3.abort-acknowledged` |
| A4 无效/无会话返回错误；aborted 与 dismissed 均不编造答案 | `a4.invalid-questions-explicit-error`、`a4.invalid-questions-no-card`、`a4.missing-session-explicit-error`、`a4.settled-or-unknown-question-explicit-error`、`a4.dismissed-does-not-fabricate-answer`、`a4.aborted-does-not-fabricate-answer` |

证据面：卡片在各自真实 UI 绑定面取证（旧栈 = WS `content_block{kind:ask_user,surface:input}` 经真实 ws-message-handler 写 store pending；Rust = `InteractiveToolsPanel` 轮询的 `GET /lingxi/v1/sessions/interactions`）；确认/拒答走真实 answer 路由；工具回包 details 逐字段断言（key→values→labels、timedOut/autoSelected、dismissed/answers:[]；旧栈 details 取自真实会话转录原文，Rust 取自 history 工具结果 details）。超时用 `timeout_seconds=30`（两内核均 clamp 最小 30s）真实等待。

### POST task abort（`leaf.chat.task-abort`，9 个 case，双腿全 true）

| 原断言 | 专属 case |
|---|---|
| A0 成功：终止当前会话拥有的后台子代理任务后返回 aborted 或 already_stopped；找不到 404、非本会话拥有 403 | `a0.owned-task-aborted-or-already-stopped`、`a0.owned-response-scoped`、`a0.unknown-task-404`、`a0.cross-session-403` |
| A1 拒绝/边界：body 会话上下文经身份解析；taskId 所有权与运行态决定结果；不错误扩大写入/披露范围 | `a1.terminal-state-decides-result`、`a1.missing-session-context-400`、`a1.unauthorized-rejected`、`a1.cross-session-no-leak`、`a1.cross-session-task-unaffected` |

证据面：真实后台子代理任务由 `R08-TOOL-PLAN` 回合调用 `subagent` 工具派出（子任务为 20 步 read 计划，替身 `--tool-delay-ms=350` 撑出 ≥8s 运行窗口），taskId 取自父会话真实工具回包（旧栈 `task-id:` 文本/details、Rust taskId JSON）；随后按各自真实路由 POST abort。旧栈真实挂载点 `POST /api/task/:taskId/abort`（`server/composition/open-root.ts` 把 chat restRoute 挂在 `/api`；原断言文案里的 `CHAT` 是路由模块名）；Rust `POST /lingxi/v1/task/{task_id}/abort`（`background_tasks::stop_route`）。

## 实现改动

**无产品代码改动**。两侧实现均已存在：Rust `interactive_tools.rs` / `interaction_store.rs`、`background_tasks.rs::stop_route`（`lib.rs:8550`）；旧栈 `lib/tools/ask-user-tool.ts` / `lib/confirm-store.ts`、`server/routes/chat.ts:2242`。本轮只新增测试套件与证据目录。`tsc --noEmit` 对本文件 0 错误（同批其他文件错误来自并行进行中的改动，非本套件）。

## A/B 差异（如实记录，未硬改产品；不进 cases）

1. 第二次 abort 已终结任务：旧栈 404 `{status:'rejected',reason:'not_found'}`（deferred 任务已移出 registry），Rust 200 `{status:'already_stopped'}`。原断言「找不到 404」与「already_stopped」均在条款内，`a1.terminal-state-decides-result` 接受两种显式终态（绝不再次返回 aborted）。
2. dismiss 动作词：旧栈 `POST /api/confirm/:id {action:'rejected',value}`；Rust `POST /.../interactions/answer {action:'dismissed',values}`。语义同为 dismissed/answers 空。
3. 卡片面：旧栈 WS content_block + store pending；Rust interactions 轮询面（InteractiveToolsPanel）。
4. 未授权状态码：旧栈 403 / Rust 401，`a1.unauthorized-rejected` 接受 401/403。
5. abort 响应体：旧栈 `{ok,status[,reason]}`；Rust 另带 `sessionId/sessionPath/taskId`；`a0.owned-response-scoped` 以白名单键 + 不含会话内容/答案/历史断言。

## 残差（未在 AB 演习覆盖，不记为已证）

- A4「ConfirmStore 不可用返回错误」：两内核均无公开开关可在活体服务驱动该分支。既有专属测试覆盖：`tests/ask-user-tool.test.ts:131`（`error.askUserUnavailable`）、`rust/crates/lingxi-service/tests/r06_interactive_completion.rs:358-370`（`set_available(false)` → 工具失败）。
- A4「无会话」（工具级）：活体 run 必有会话上下文，AB 以路由级缺会话/未知问题显式错误取证（两腿 404）；工具级分支由上述两套内核测试与 Rust `ask(ctx=missing run)` 负例覆盖。
- 旧栈中止 ask_user 后工具结果未落转录（`details=null`，与 Rust 相同）；A3 中止面证据 = 待答问题离场 + 无编造答案 + WS abort 回执，符合原断言「中止均无答案」的否定式表述。
