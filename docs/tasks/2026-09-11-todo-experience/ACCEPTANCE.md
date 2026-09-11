# 任务清单改版验收记录

日期：2026-09-11
执行依据：[PLAN.md](PLAN.md)（阶段 1–5）
工作区基线：`74cd07fb`（feat/tool-activity-presentation）；本任务改动未提交、未推送、未打包安装。

## 一、交付内容

### 数据契约（v2）
- 状态扩展为 `pending / in_progress / blocked / cancelled / completed`；`blocked` 必须携带
  `blockedReason`；`cancelled` 是终态，不被完成操作改写。
- `details.todoVersion = 2` 标记新格式；无版本旧记录保持旧语义（全完成即移除，不复活）。
- 清单版本号 = 规范化条目内容的 FNV-1a 哈希（`computeTodoListVersion`，前后端镜像逐字一致）；
  用户收尾操作携带版本，服务端失配返回 `409 todo_version_mismatch`。
- 完成状态与"是否收纳"分开记录：收尾摘要在历史中保留，收纳（用户主动或新请求被正式接受）
  追加独立 `lingxi.todo_state` 记录（`dismissed: true`）。

### 服务端
- `POST /sessions/todos/complete`：未完成项改为已完成（已取消项保持取消），返回完整面板负载；
  输出中 409 拒绝；版本失配 409 拒绝。
- 新增 `POST /sessions/todos/cancel`：待开始/进行中/受阻项改为已取消；同样受输出中与版本门禁约束。
- 新增 `POST /sessions/todos/dismiss`：收纳已结束摘要；只改变当前展示，历史保留。
- 新请求被 `hub.send` 正式接受后自动收纳旧的已结束摘要
  （`dismissFinishedTodosOnPromptAccepted`，拒绝/异常分支不触发）。
- hydrate / `session_branch_reset` / `todo_update` 广播均携带面板快照字段
  （todos / version / finished / allCompleted / dismissed / removed）。

### 客户端
- 新组件 `desktop/src/react/components/chat/TodoPanel.tsx`：输入框上方清单进度条
  （标题计数、首个进行中步骤、并行摘要"另有 n 项进行中"、展开五态列表、受阻原因、
  收尾操作区、停止提示、更新失败提示）；展开偏好按会话保存在本次运行期间。
- 移除右上角运行信息胶囊与右侧工作区中的重复清单（SessionTodoCard 已删除）。
- 消息流中清单工具行显示"任务 · 已完成 2/6 · 正在检查兼容性"短摘要。
- 实时 `tool_end` 失败或数据损坏：保留最后有效清单并标记更新失败（修复阶段 1 复现的
  D1/D2 缺陷——失败结果被转换为空清单）；明确空清单仍是明确清空。
- 历史恢复、会话切换、分支重置全部接入面板快照；live-version 防旧快照覆盖机制保留。

### 工具规则（模型侧）
- 每次提交完整清单；去除首尾空白；空白条目、重复条目明确拒绝（isError，不产出快照）；
  blocked 必须给出原因；多个 in_progress 合法（不再告警）；
  成功结果携带 `todoVersion = 2`。

## 二、验收清单对照

| 编号 | 结果 | 证据 |
| --- | --- | --- |
| A01 输入框上方进度、胶囊无第二份清单 | 通过（自动测试） | `TodoPanel.test.tsx` A01；`RuntimeInfoCapsule.test.tsx` |
| A02 点击/键盘展开收起、滚动不超过输入区 | 通过（自动测试）；滚动上限 `min(240px, 30vh)` 待界面验收复核 | `TodoPanel.test.tsx` A02；`TodoPanel.module.css` |
| A03 更新不重置展开状态 | 通过（自动测试） | `TodoPanel.test.tsx` A03 |
| A04 并行进行中摘要 | 通过（自动测试） | `TodoPanel.test.tsx` A04 |
| A05 受阻展示原因、不计完成 | 通过（自动测试） | `TodoPanel.test.tsx` A05；`todo-compat.test.ts` v2 受阻 |
| A06 停止后不打勾、提示真实含义 | 通过（自动测试：非流式提示"本轮已停止"） | `TodoPanel.test.tsx` A06/A06b |
| A07 继续工作时继承未完成清单 | 通过（自动测试） | `todo-auto-dismiss.test.ts` A07 |
| A08 全部完成收尾摘要；重开仍可见；接受新请求后收纳 | 通过（自动测试） | `TodoPanel.test.tsx` A08；`todo-auto-dismiss.test.ts` A08 |
| A09 发送失败不提前收纳 | 通过（调用点位置保证：仅在 `hub.send` 成功分支触发） | `server/routes/chat.ts` prompt 分支 |
| A10 完成+取消分别统计 | 通过（自动测试） | `TodoPanel.test.tsx` A10；`sessions-route.test.ts` cancel/complete |
| A11 确认完成/取消剩余记录正确 | 通过（自动测试） | `sessions-route.test.ts`；`session-actions.test.ts` |
| A12 输出期间完成/取消被拒绝 | 通过（自动测试：前端禁用 + 服务端 409） | `TodoPanel.test.tsx` A12；`sessions-route.test.ts` |
| A13 更新失败/缺失/重复条目保留原清单并提示 | 通过（自动测试） | `ws-todo-panel-lifecycle.test.ts` D1/D2/D2b；`todo-write-tool.test.ts` |
| A14 明确清空与失败可区分 | 通过（自动测试） | `ws-todo-panel-lifecycle.test.ts` K2；`todo-compat.test.ts` v2 空清单 |
| A15 会话隔离 | 通过（自动测试） | `ws-todo-panel-lifecycle.test.ts` K3 + 面板隔离用例 |
| A16 历史恢复一致 | 通过（自动测试：hydrate/branch_reset 面板快照） | `history-*` 测试套件；`session-actions.test.ts` |
| A17 旧版本操作被识别 | 通过（自动测试：409 + 本地不清单） | `sessions-route.test.ts` 版本失配；`session-actions.test.ts` |
| A18 旧记录不复活、不强制迁移 | 通过（自动测试） | `todo-compat.test.ts` 旧手动完成样本；`ws-todo-panel-lifecycle.test.ts` K4 |
| A19 不中断后台任务 | 通过（设计约束：取消只写清单记录，附界面说明） | `TodoPanel.test.tsx` A11 说明文案 |
| A20 主题与缩放 | 未执行（需界面验收） | — |
| A21 消息行统一入口与短摘要 | 通过（自动测试） | `ToolGroupBlock.test.tsx` 清单工具行 |

## 三、验证执行

- `npm run typecheck`：通过（tsc ×3）。
- `npm run build:renderer`：通过。
- `npm test`（全量 13937 条）：13919 通过、7 跳过、11 失败。11 个失败全部在
  本任务基线（未做任何改动的工作区）复现，属环境/审计类，与本改动无关：
  - `open-boundary-lint`（2）、`persistence-schema-tripwire`（4）、`style-discipline`（1）：
    基线环境即失败；
  - `post-verification-audit-seal`（1）：当前已提交版本与 `.sync-audit` 已验证坐标不一致，
    源变更非审计白名单；封印推进需要提交授权，不在本任务范围；
  - `round2/round3-delivery-evidence`（3）：历史轮次 manifest 不含当前未提交源码，
    需对应授权流程处理。
  - 验证过程中曾出现 `git stash pop` 部分恢复与 `cli-closure-census` 在异常树状态下的
    假失败；恢复完整工作区后全部相关测试通过，未遗留冲突标记或内容差异。

## 四、未覆盖与边界

- 真实界面验收（A20、移动端布局、缩放、窗口宽度）未执行；需要开发态应用人工核对。
- 真实模型行为（工具说明改动后的遵守情况）未验证；仅固定了工具校验与文案。
- 独立快捷聊天窗口与其他通信平台入口未改动；共享数据契约新增了面板字段，
  旧消费方读取 `todos` 字段的行为不变（向后兼容）。
- 未提交、未推送、未打包安装；发布审计封印不在本任务授权范围。
