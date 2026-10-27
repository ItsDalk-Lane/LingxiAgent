# T09 手机接线交接（实施中，未验收）

交接时间：2026-10-18T13:16:49.669587+00:00。用户要求转交其他工具后，已停止所有产品、测试和协议编辑；仅等已启动的单轮局部测试自然结束并保存本交接。未提交、推送、运行 Cargo、构建或真实账号请求。

## 当前已做的修改

- mobile-init：去除 Rust 固定不可用退出，改读正式 bootstrap；Rust 不另读 settings-only 权限默认值；当前助手不从其他助手兜底；会话列表读正式 /sessions 并以 native:sessionId 定位，revision 保持 null；列表、切换、创建与初始化增加连接/选择/登录周期检查。
- Chat client：模型目录改为 /lingxi/v1/chat/models，携带明确 agentId/sessionId；新会话用所选助手并排除旧会话。迟到正文还检查助手身份。
- 模型加载：校验模型列表形状、归一 null defaultThinkingLevel；增加可选择向调用者抛出真实错误的选项。
- MobileApp：初始化、登录和卸载增加代次检查；平台 URL 白名单允许正式资源内容地址，继续拒绝本机路径及 file: 回退。
- 新局部测试覆盖模型上下文定位、连接/助手/选择迟到、会话摘要、列表刷新与资源 URL。注意其内部请求替身属于局部组件/适配测试，绝非真实 Rust 服务联验。

## 当前文件指纹

| 文件 | SHA256 |
|---|---|
| `desktop/src/react/mobile/mobile-init.ts` | `2e1115c1e83ccc939de8b3dd50b23712468d11259927eb47fbdea96044e33936` |
| `desktop/src/react/mobile/MobileApp.tsx` | `9c3c5b3c441b491ae5c67480eebe680b5f263a5b158db5f2954bb48d15041d62` |
| `desktop/src/react/mobile/mobile-platform.ts` | `efdfe5a7d98c77de32383103ec937c869f06d53e44db52d3f8658d1260943c1d` |
| `desktop/src/react/services/chat-client.ts` | `b64dbaf0831d8ca1e1842ddd1166693ae843e9b0056f5c6311e23227a45c1f19` |
| `desktop/src/react/utils/ui-helpers.ts` | `d4c6e63e373794eacda51a291392ccb7efa8f0a88323acff2f049e02fdb799e9` |
| `desktop/src/react/__tests__/mobile/R07RustMobileRuntime.test.ts` | `68ca6a85d5b59aad3a2bcd38e9662459bb2d147aa7f7e7678894084c7d1c530c` |

上述6文件是本次手机实施所写；mobile-platform.test.ts 已有先前改动，本轮未写入，勿覆盖。session-actions.ts、model-slice.ts、session-connection-owner.ts、Rust/CLI/contracts/generated 本轮未编辑。

## 已运行且必须保留的原始结果

- `mobile-unit1`：2文件6项通过，exit0。仅既有坏会话响应与平台边界局部检查。
  - 命令：`node node_modules/vitest/vitest.mjs run desktop/src/react/__tests__/mobile/R02MobileSessionResponse.test.ts desktop/src/react/__tests__/mobile/mobile-platform.test.ts --maxWorkers=1`
  - 原回执：`artifacts/rust-tauri/R07/T09/mobile-unit1.json`；原日志 SHA256 `c6e35e0f44222814dd76d76fcf7ac54171e8340f0a266cbc0d7132c09d280fef`。
- `mobile-unit2`：2文件38项，36通过2失败，exit1；源码前后指纹相同。不是验收完成。
  - 命令：`node node_modules/vitest/vitest.mjs run desktop/src/react/__tests__/mobile/R07RustMobileRuntime.test.ts desktop/src/react/__tests__/mobile/MobileApp.test.tsx --maxWorkers=1`
  - 原回执：`artifacts/rust-tauri/R07/T09/mobile-unit2.json`；原日志 SHA256 `6218dc4b5926009bbd257fc97aa4254103c72ecb4452fd5a77c597f61cd1670b`。

mobile-unit2 的两项实际失败：

1. 既有 MobileApp.test.tsx 的 `uses the Rust auth routes and never enters the old mobile workspace` 仍期待固定“Rust 聊天尚不可用”。现已进入正式 bootstrap，旧夹具没有提供 `http://localhost:3000/lingxi/v1/mobile/bootstrap`，因此页面如实报 unexpected request，原268行期待失败。后续应改为正式 Rust 正向夹具和真实失败保留，不能删除测试或简单改成空成功。
2. 新 R07RustMobileRuntime.test.ts 的初始化正例失败于 chat-client.ts 正文归属检查，报“聊天目标已切换，旧回复已丢弃”。尚未定位是初始化 Promise.all 的列表/模型归属交错还是局部夹具准备；本轮停止后未修。需先诊断实际所变字段/代次，不能放宽连接/选择拒绝断言来刷绿。其他9项新增检查实际通过。

## 必须继续的窄待办

1. 先定位上述新初始化失败，并将旧固定不可用用例改为真正 bootstrap/chat-only 正例。保持旧失败日志原样。
2. **通用 session-actions.loadSessions 的手机 chat-only 分支尚未实施**。现役 ChatSidebar/InputArea/WS 事件共享路径可能仍访问 /sessions/studio-list 并要求 sessions.read；当前手机4scope不能扩大。应抽取同 SessionView 纯投影，手机复用 GET /sessions，桌面现路径保留。当前纯投影临时位于 mobile-init，后续可最小抽取，不造成循环 import。
3. **ModelSlice modelState/modelGeneration 镜像尚未实施**，连接切换清空也未实施。目前模型上下文能请求，尚未把 unconfigured/unavailable/真实代次完整存入 store；不得把空数组或旧模型当 ready。
4. root 已新增 client_context.rs 协议源，但当前手机仍用临时兼容接口，尚未引用最终生成 ClientModelContext/MobileBootstrapResponse；root 唯一生成后需消费。defaultThinkingLevel:null、string thinkingLevels/input 等须实际归一，勿仅类型强断言。
5. 未完成真实 Rust 子进程 mobile consumer child；未验实际登录→初始化→列表/创建→ChatPage 消息/刷新重连→CLI 同产物下载；未验实际 renderer dist、网络断开/后台恢复。A17/A18仍交主控整合，不以这里9项局部通过替代。
6. switchMobileSession/createMobileSession 虽做窄接线，但当前实际 ChatSidebar 使用 generic switchSession，InputArea 使用 generic pending creation；必须证明这两条现役真实链，不用未调用 helper 冒充用户操作完成。
7. 检查初始化/退出时连接和 WS 资源清理，确保未授权/过期旧主体不能继续显示。当前 invalidateMobileRuntime 只推进前端代次，并非关闭 WS。
8. 尚未运行本轮 typecheck、完整适用前端回归或任何固定候选门禁；所有结果仅 DEVELOPMENT，T09 未完成/未验收。

## 下一工具必读

- 用户边界：保留失败，不提交或推送，只推进 R07，当前已按用户决定停止并交接。
- `docs/rust-tauri/R07/T09_IMPLEMENTATION_READINESS.md`。
- `artifacts/rust-tauri/R07/T09/t09-mobile-shared-context-readonly-plan1.json`。
- `artifacts/rust-tauri/R07/T09/mobile-unit1.json`、`mobile-unit2.json`及各自原日志。
- root 新 client_context.rs 与统一交接：bootstrap 顶层 models/activeModel/currentModel/permissionMode；GET /chat/models 携明确 Agent/session；手机仍仅 chat/resources.read/files.read/files.write。不要加 settings.read/sessions.read，不恢复 Node fallback，不将宿主路径当浏览器 file URL。
- T08h 20命令全部0仅接受其本地适用份额，adapter另13ignored、17UI/4窄负例/原minimax矛盾留T12；T08旧e/f/g失败原样保留。
