# T09 CLI 实现交接（2026-10-18）

## 停止状态

用户决定转交其他 AI 工具。本轮已停止产品编辑、测试及构建；仅补此交接。没有提交或推送。

当前分支 `codex/rust-tauri-migration`，HEAD `0c86675d49442fe0e38259bd3097423fb340eecc`。工作区含此前大量未提交改动，HEAD 不是本轮已验证代码坐标，接手者不得重置或覆盖无关文件。

T08h 本地份额已经独立接受，接受记录 SHA256 为 `256c95bdb3951dbddf919a3455ce1915e1ca025da79a2a1cc923d8fb3be9a0e8`。T09 刚开始：下面代码均为**未验收初稿**，不能据此宣布 CLI 或 T09 完成。

## 本轮 CLI 修改

| 文件 | 已写入的初步内容 | SHA256 |
| --- | --- | --- |
| `cli/args.ts` | 默认 runtime 从 node 改为 rust；帮助文字同步。显式 node 分支保留。 | `4663d761b1eac8653d2bce5fc3fa41e46854f97d071bd607a44c01bb2685506f` |
| `cli/entry.ts` | Rust status/chat/continue 从不可用占位接到新客户端；保留现有维护命令、Rust serve runner 和显式 Node 路径。 | `62fdd5d382c2b066def12b29642336ea27c3a145bbf71b737959c0115e924f0c` |
| `cli/rust-service.ts` | 复用安全实例发现/凭证读取；补上下文、新建会话、历史、审批 HTTP 与真实 WS ticket/hello；请求失败不回退旧内核。 | `96b7834e5c47d13e793a59ae4c1f458ddc1c783bbe1de472af361a6581f8d9bf` |
| `cli/rust-chat.ts`（新增） | 会话选择、状态显示、stdin 排队、稳定 clientMessageId、流式输出、审批命令、取消、恢复及 EOF 清理的初稿。 | `05aae7b4549ac93f772fda5a56b919a49d29eafab2751683b892f9c0206787ef` |

`tests/cli*` 尚未修改。本轮没有修改 Rust、renderer 或公共协议文件。

## 已执行命令与准确结果

仅执行过以下语法检查，均退出 0：

```text
node --check cli/rust-chat.ts
node --check cli/rust-service.ts
node --check cli/entry.ts
```

上述工具返回已观察到；没有单独保存这三条的原始日志，不补造日志。语法检查不等于 TypeScript 类型检查，也不证明实际服务或聊天能够运行。

**尚未执行**：CLI 单测、类型检查、真实 Rust 服务/WS、聊天/审批/取消/恢复/EOF 检查、进程回收检查、任何 Cargo、构建或 T09 正式门禁。已有开发执行器 `artifacts/rust-tauri/R07/T09/evidence-tools/run_development.py`，本轮没有运行。原有失败记录未改写。

## 已明确发现、尚未修复的断点

以下是停止前静态复核发现，均不是已运行失败；接手者应先修复并建立有意义的验证，不能直接发布本初稿。

1. `rust-chat.ts` 的 `settle()` 在 `settling=true` 时调用 `pump()`，之后才在 finally 清理标志，没有再推动队列。第一轮结束后第二行 stdin 可能一直不发送。
2. 重连时没有清除旧连接的 `resumePending`。若断线发生在恢复响应之前，新连接可能不再发恢复请求。hello 有 5 秒限制，但恢复响应自身没有截止时间，EOF 也可能挂住。
3. `stream_resume` 在没有重放事件且没有 reset 时未采用响应的 streamId。已有任务仍运行时可能无法取得取消所需的准确标识。
4. 恢复帧的 nextSeq、events/内嵌事件序号、reset 等校验仍不完整；ServerHello protocolMin/protocolMax 也应严格检查整数，不能让 undefined 比较绕过校验。
5. `settleTimer`、异步历史/命令/审批返回的会话及连接代次保护需要完善。不能让旧请求的迟到结果覆盖切换后的会话；重连次数目前不在稳定连接后重置，需确定合理有界规则。
6. 继续其他助手的会话时，初始标题及 `/status` 仍调用无 sessionId 的全局 context，可能显示错误助手/模型。实际发送取已验证会话 agentId，但显示也应使用相同目标上下文。
7. `renderHistory` 只初步处理 user_message/final_message。须核对真实规范记录、重置后工具及助手片段、live/final 一致性；resource_ref 可能重复显示，不能以忽略未知记录冒充完整恢复。
8. Prompt 接受事实目前依赖 session_user_message/clientMessageId 或历史 legacyFields，须用真实服务确认其顺序与错误路径。接受不明时保持不重发，不能为了完成偷偷重复投递。
9. EOF、SIGINT/Esc、SIGTERM、待审批退出、readline/WS/timer 回收都没有实测。当前 EOF 待审批退出 2、保留真实 pending 的意图应有实证，不能自动批准。终态后 30 秒清理等待只是初稿，需核对现役行为及适用检查。
10. WS ClientHello 的 clientVersion 暂写为 `0.1`，应接实际版本。新 context 暂用本地接口，根端公共 DTO 生成后应替换，不能自己另造一套协议。
11. 没有私有 Rust instance 时仍按既有发现规则明确失败，尚未实现默认聊天自动启动 Rust。若要求自动启动，应复用 Rust runner，不能调用旧 Node 的 `startLocalServerAndWait`。
12. `tests/cli-args.test.ts` 仍预期 Node 默认；`tests/cli-rust-service.test.ts` 仍有“status 无 Agent/model”与“选中会话后拒绝 fake reply”的旧不可用断言。应增加真实正例，同时保留未配置、撤权、错误 hello、未知会话、无秘密 URL、终端控制字符、实例所有权、runner 退出等负例，不能只删旧断言求绿。

## 真实接口与分工

本轮 CLI 复用现有安全实例/token reader/runner；凭证只作 Authorization 请求头，WS URL 只带短期 ticket，不带服务 bearer。

根端负责 Rust、公共 DTO 和 mobile，CLI 当前等待以下正式接口契约最终确认/生成：

- `GET /lingxi/v1/mobile/bootstrap?agentId=&sessionId=`：白名单上下文，拟含 currentAgentId、agentName、currentModel `{id,provider}` 或 null、modelState `ready|unconfigured|unavailable`、thinkingLevel。
- `GET /lingxi/v1/chat/models`：相同目标 query；不得把旧完整配置/秘密直接透传。
- 会话列表用 `GET /lingxi/v1/sessions`（chat 权限且按 principal 过滤），不用内部要求 sessions.read 的 studio-list。
- 新建会话 `POST /lingxi/v1/sessions/new`；历史 `GET /lingxi/v1/sessions/{id}/history?all=1&reconciliation=1`。
- 审批走该会话真实 approvals HTTP，使用 confirmed/rejected；不能自动批准或借焦点任务。
- WS 用现有 ws-ticket、`/lingxi/v1/ws`、ClientHello/ServerHello；prompt 带 sessionId、native:sessionId、agentId、稳定 clientMessageId、snapshotVersion、clientAttemptId。
- 取消绑定准确 session/stream；恢复用 `resume_stream`，不重发 prompt。断档/reset 必须读规范历史，不能显示成成功连续流。

不要把后台来源伪造成 LocalUser，不扩大原权限。显式 Rust 错误不回退旧 Node。原维护 bundle/data 命令继续复用既有实现，不外发资料。

## 接手顺序与必读资料

1. 先读总交接 `docs/rust-tauri/R07/R07_NEXT_TOOL_HANDOFF_20261018.md`，确认根端 bootstrap/模型上下文进度与本轮实际工作区。
2. 读 `docs/rust-tauri/R07/T09_IMPLEMENTATION_READINESS.md`、本文件，以及两个只读方案：
   - `artifacts/rust-tauri/R07/T09/t09-cli-readonly-implementation-plan1.json`（SHA256 `557d7d458a5ea16f799101d046767fc4857d83d50aa974709fb2adb7facb3ee9`）
   - `artifacts/rust-tauri/R07/T09/t09-mobile-shared-context-readonly-plan1.json`（SHA256 `9cb1b178e2d28878f03da6e575c32b868096a7866413a3eb304798d719e59723`）
3. 先修上列队列、连接代次、恢复截止时间和目标上下文断点，再补 CLI 类型/单测；真实服务测试只替换自有外端响应，不替换认证、存储或执行结果。
4. 根端安排 Cargo/build；CLI 的真实 status/session/chat、审批、取消、断线恢复、重复输入、EOF/信号与资源回收分别留实际版本、命令、退出结果、证据。
5. 跨端 A17/A18 由根端整合：同一任务、历史、文件与权限必须真正一致。当前 CLI 初稿没有证明这些结果。

后续仍按 T09→T10→T11→T12 顺序，真实账号/其他系统等边界明确保留。不要因 T08h 接受就把 T09 初稿算通过，也不要覆盖前阶段失败记录。本次停止时没有提交、推送或发布。
