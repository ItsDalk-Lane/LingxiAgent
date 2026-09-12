# 清单上下文连续性融合方案（Codex × openclaude）— 实施记录

日期：2026-09-11。前置：任务清单改版五阶段已完成（同目录 PLAN.md / ACCEPTANCE.md）。
本文档记录"让模型在中断/长任务后仍按权威清单继续"的融合方案实施。

## 方案骨架：一个事实源，三个消费者

权威事实源 = 会话历史中最新清单快照（已持久化、分支感知、带版本号）。
三个消费者（界面实时/历史恢复、轮次注入、轮中注入）从同一份快照、
用同一个提取函数（`extractLatestTodoSnapshot` 系）取数。

## 实施的五个机制

### 机制 1：管道一致性修复（必修前置）
- 缺陷：`server/routes/chat.ts` 的 `tool_end` 广播白名单丢 `todoVersion`，
  实时更新全部被前端误判为 v1 旧格式（全完成即移除）——面板闪烁/消失的根因。
- 修复：白名单加入 `todoVersion`。
- 回归：`tests/todo-pipeline-consistency.test.ts`——同一份工具结果分别走
  实时事件（前端 `panelSnapshotFromToolDetails`）与历史提取
  （`extractLatestTodoSnapshot`），逐字段断言一致（含版本哈希）。

### 机制 2：轮次注入（新请求接受时）
- `server/routes/chat.ts` `buildTodoContextBlockForPrompt`：存在未完成清单时，
  把权威清单状态（含快照版本、逐项状态、受阻原因、in_progress 核实指引）
  拼进 `promptText`（不动 `displayMessage`，用户气泡不可见）。
- 与自动收纳互斥：已结束 → dismiss 通道；未完成 → 本注入。
- 数据源是持久化快照（强于 openclaude 的内存 appState，跨重启正确）。
- 幂等：同一状态生成同一文本；`TODO_CONTEXT_BLOCK_MARKER` 去重。
- 测试：`tests/todo-prompt-injection.test.ts`（5 例）。

### 机制 3：轮中节流提醒（长任务防停滞）
- `lib/pi-sdk/todo-context-reminder.ts`：经 pi SDK `transformContext` 钩子
  （每次模型调用前、convertToLlm 之前），距上次 todo_write ≥10 个工具调用
  且距上次注入 ≥10 个工具调用时，在上下文末尾追加一条 ephemeral
  `<system-reminder>` user 消息——不落盘、不污染历史。
- 仅当快照存在且有未完成项时注入。
- 安装点：`lib/pi-sdk/index.ts` `createAgentSession`（与既有安装器并列，
  链式包裹前一个 transformContext，WeakSet 防重复安装）。
- 测试：`tests/todo-context-reminder.test.ts`（11 例）。

### 机制 4a：中断标记（模型可见、UI 不展示）
- `core/interrupted-turn-marker.ts`：用户停止本轮（SDK abort settle 后，
  `_forceReleaseStreamingSession` 链式调用），向会话历史追加合成 user 消息：
  "上一轮被有意停止；工具可能只执行了一半；in_progress 项并未在运行；
  继续前先核实真实状态"（Codex `<turn_aborted>` 措辞路线）。
- 标记格式走既有系统消息惯例 `<hana-turn-interrupted>…</hana-turn-interrupted>`：
  模型在上下文中可见；UI 侧按 `<hana-background-result>` 同款规则过滤
  （`lib/turn-input-presentation.ts` HIDDEN_TURN_INPUT_RE 注册为隐藏轮输入；
  `desktop/.../history-builder.ts` 投影时跳过，含旧版明文前缀兼容）。
- 幂等：历史末尾已是标记（新旧两种格式）则不重复写；中间有新用户消息则
  允许再标。
- 测试：`tests/interrupted-turn-marker.test.ts`（7 例）+
  `history-builder.test.ts` 隐藏中断标记用例。

### 机制 4b：收尾验证提醒
- `lib/tools/todo.ts`：一次性把 3+ 项清单全部标 completed 且清单中无验证类
  条目（/verif|测试|test|check|验证|检查|核对|review|评审/i）时，在工具结果
  文本后追加提醒（openclaude verificationNudge 的轻量化——不依赖
  verification-agent 基础设施，保留"在最易跳过的精确时刻拦截"的思想）。
- 测试：`tests/todo-write-tool.test.ts` 新增 4 例。

### 机制 5：工具规则文本融合
- `todo_write` description 重写为三家纪律并集：时机纪律（开始前标
  in_progress、完成立即标不攒批）、防停滞（turn 结束前到达终态或如实保留）、
  中断恢复（对照注入的权威清单核实后再决定是否重写）、保留既有五态/
  整体替换/多 in_progress 合法/版本保护语义。

## 明确不做
- 不做运行时"任务与现实核对"（三家都没有可靠定义）；不引入 verification
  agent；不把面板改成对话流历史卡片（面板是差异化特性）。
- 中断标记对用户不可见（初版曾按 Claude Code 惯例可见，评审后改为既有
  隐藏轮输入惯例）。

## 验证
- typecheck（tsc ×3）：通过。
- 相关测试 78 例 + 协调器/路由 61 例：通过。
- build:renderer：通过。
- 全量测试：13951 通过 / 9 失败，全部落在已证明的基线失败类
  （open-boundary-lint ×2、persistence-schema-tripwire ×4 为确定性存量失败；
  seal/round2/round3 证据 ×3 为全量运行环境敏感、单独运行通过、基线同失败）。
  实施过程中曾引入一条 style-discipline 回归（旋转动画字面量时长），
  已按样式立法收进 custom property 定义行修复，`tests/style-discipline.test.ts`
  现已通过。
