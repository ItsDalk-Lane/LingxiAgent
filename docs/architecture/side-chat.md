# 侧边对话（Side Chat）

> 记录「选中聊天内容 → 右侧栏开一个独立会话继续追问」的现行执行链。
> 机制描述绑定当前源码；不代表某个安装包或平台已验收。

## 产品语义

1. 在聊天界面（或预览面板）选中一段文字，浮层出现两个动作：
   「引用到对话」与「在侧边聊天中对话」。
2. 点后者：右侧栏打开一个**真实的新会话**，携带刚才的选区作为引用，
   主对话不被切换、不被中断。
3. 侧栏内的会话与主聊天页布局一致：消息记录 + 输入区（模型 / 思考级别 /
   权限模式 / 附件 / 引用 / 草稿 / 斜杠 / 记忆开关）。
4. 关闭侧栏只是从侧栏移除；会话本体与其记录留在左侧会话列表里，可再次打开。

## 关键约定

**侧边会话是独立会话，不是主会话的视图。** 会话由服务端
`POST /api/sessions/new-detached` 创建——`createDetachedSession`
（`core/session-coordinator.ts`）在 finally 里恢复 `_session` /
`_currentSessionPath`，因此建会话不会抢走主对话的运行时焦点。侧边会话默认：

- 继承当前会话的**工作台**（`workspaceMountId` 优先，否则 `cwd`，二者互斥）；
- 继承当前**助手**（`agentId`）；
- **关闭长期记忆**（`memoryEnabled: false`）。旁支追问默认不写入长期记忆，
  避免与主会话并发写记忆互相串味；用户可在侧栏工具栏里自行打开。

**两个输入区各读各的会话。** 主聊天页与侧边面板会同时挂载 `InputArea`，
而 store 里只有一份全局「当前会话」字段。因此：

- `SessionScopeProvider`（`desktop/src/react/components/session-scope-context.tsx`）
  给子树提供「本输入区归属的会话」，`useScopedSessionPath()` 是唯一读取入口。
- 会话级状态一律按 sessionPath 分桶读取：
  `useScopedAttachedFiles` / `useScopedQuotedSelections` / `useScopedDocContext`
  （`components/input/composer-scope.ts`）、`useScopedPermissionMode`、
  `useScopedMemoryEnabled`、`useScopedSessionPath` + `thinkingLevelBySession`。
  全局字段保留为「主聊天页兼容镜像」。
- 写入口是 `input-slice` 的 `*ForSession` 动作；主会话目标同时维护 scoped 桶
  与全局镜像，两侧不会分叉。

**选区浮层按来源会话归属。** 全局只有一个 `quoteCandidate`；主聊天页与侧边
面板各挂一个 `SelectionQuoteActionSurface`，`quoteCandidateOwnedBySession()`
（`stores/selection-actions.ts`）决定谁渲染：chat 选区归其所在会话，
`sourceKind: 'preview'` 的选区归主聊天页。侧边选区因此不会被投递到主输入区。

**会话级配置的真相**

| 配置 | 权威位置 | 写入接口 |
| --- | --- | --- |
| 附件 / 引用 / 草稿 | `attachedFilesBySession` / `quotedSelectionsBySession` / `drafts`（按 sessionId） | `input-slice` `*ForSession` |
| 模型 | `sessionModelsByPath` | `POST /api/models/switch`（带 sessionPath） |
| 思考级别 | `thinkingLevelBySession` | `POST /api/session-thinking-level`（带 sessionPath） |
| 权限模式 | 服务端按会话保存 | `POST /api/session-permission-mode`（带 sessionPath） |
| 记忆开关 | 服务端 manifest.memoryPolicy + session-meta | `GET/PATCH /api/sessions/memory` |

**一次只开一个侧边会话。** 再次点「在侧边聊天中对话」会先关闭当前侧栏
（旧会话保留），再创建新的。`stores/side-chat-slice.ts` 只保存侧栏 UI 与
侧边会话身份；消息与流式状态仍按 sessionPath 存在 `chatSessions` /
`streamingSessions`，与主会话同源。

**布局**：`.preview-panel-slot` 让预览面板在侧栏打开时排到其左侧
（`order` 提升）。空间不足时先被挤出视口的是预览面板，保证主对话输入区与
右侧栏始终可见。

## 源码坐标

- 入口按钮：`desktop/src/react/components/selection/SelectionQuoteActionSurface.tsx`
- 状态与动作：`desktop/src/react/stores/side-chat-slice.ts`、`side-chat-actions.ts`
- 面板：`desktop/src/react/components/side-chat/SideChatPanel.tsx`
- 会话作用域：`desktop/src/react/components/session-scope-context.tsx`
- 挂载点：`desktop/src/react/components/app/AppPages.tsx`
- 服务端建会话：`server/routes/sessions.ts`（`/sessions/new-detached`）
- 服务端记忆开关：`server/routes/sessions.ts`（`/sessions/memory`）

## 验证

- 前端：`desktop/src/react/__tests__/stores/side-chat-actions.test.ts`、
  `components/SideChatPanel.test.tsx`、`components/SelectionQuoteActionSurface.test.tsx`
- 服务端：`tests/sessions-route.test.ts`
- 已知限制：未经真实 GUI 手工验收；未覆盖跨平台打包产物与真实供应商调用。
