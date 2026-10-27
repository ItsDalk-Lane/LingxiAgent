# R07 切换编程工具交接（2026-10-18）

此文件是用户决定切换编程工具时的实施断点，不是阶段验收通过记录。请先核对磁盘实际内容，再继续实现。旧任务书、只读调查中的“未实施”是其写作时状态，应结合本交接及最新运行回执判断。

## 一、目标与不可变边界

继续完成 R07，按 T09 → T10 → T11 → T12 顺序推进；复用 T01—T08 已有实现，必要时修复受影响回归。R06 已由用户明确接受，无需重新跑整轮 R06；原来失败记录不能改写。

- 项目：`/Users/study_superior/Desktop/Code/LingxiAgent`。
- 当前分支：`codex/rust-tauri-migration`。
- 交接时 HEAD：`0c86675d49442fe0e38259bd3097423fb340eecc`。大量实现仍在未提交工作区；HEAD 本身不是全部实现版本。
- 沿用当前分支，保留现有修改、未跟踪文件与外部工具成果。禁止 reset/checkout/clean 覆盖工作区。
- 不提交、不推送、不建 PR、不发布，不使用真实账号、付费请求、对外发消息或切换真实数据。隔离本机真实服务、数据库、执行链和确定性外部响应替身已获授权。
- 普通实现、修复、局部验证直接做。不要反复请求继续确认；只暂停确实缺少授权的具体动作。
- 使用简体中文向非技术用户简短汇报，说明实际完成、验证结果、剩余限制。失败、未执行、受阻和通过必须分开。

## 二、阶段真实状态

权威进度入口：`docs/rust-tauri/R07/R07_PROGRESS.json`，逐功能结果：`FEATURE_PARITY_RESULTS.json`，原责任地图：`R07_LEAF_MAP.json`。进度标签不能取代原始证据。

| 阶段 | 交接时状态 |
|---|---|
| T01—T03 | 本地验证完成，真实账号/其他系统等边界保留。 |
| T04 | 本地份额验证，解锁后的实际接管补验已做；仍需最终 R07 回归。 |
| T05—T06 | 本地验证完成，真实付费供应商、其他系统、正式安装包未验。 |
| T07 | 独立工作台功能已验证；真实 Chromium 弹窗失败、cookie 设置后 Resume 导航失败保留，交 T12 排查，不能称整轮通过。 |
| T08 | `LOCAL_VERIFIED_FINAL_R07_PENDING`，候选 t08h 本地适用门禁和独立审阅通过。不能称原 343 条或整个 R07 全通过。 |
| T09 | 正在实施，CLI/mobile 只有未收口初稿；有测试失败；后端新接口尚未实现，不能验收。 |
| T10—T12 | 尚未实施；已有只读准备文档不算完成。 |

T08 重要依据：

- `artifacts/rust-tauri/R07/T08/RESULTS_t08h.json`：20 项命令门禁退出 0，服务 948/948，界面 718/718。
- `ACCEPTANCE_t08h.json` 与 `independent-final-review-t08h.json` 接受的是本地份额，后者 SHA256：`1bf0970a953b2a6e733f50cfcdacc8566f915c5291d11390ce97c1139a58d3b2`。
- adapter 为 223 通过、13 忽略；13 项本轮未执行，见 `t08h-adapter-ignored-scope-addendum1.json`，不是全部运行通过。
- `t08h-t12-clause-followup-actual1.json` 保留 17 项具体界面分支、4 类窄负例、minimax 原条款矛盾供 T12 清账。
- `docs/rust-tauri/R07/T08_FAILURE_HISTORY.json` 保留 e/f/g 和开发失败。g 为 20 门中 18 过 2 失败；旧 LAN 超时后来同源码通过，但原因未被证明，不能声称根治。
- 历史候选、日志、回执均不可覆盖。T08 本地通过不自动验证之后 T09 修改过的工作区。

## 三、T09 精确断点与接手顺序

先读：

1. `docs/rust-tauri/R07/T08_HANDOFF.md`、`T08_REPORT.md`。
2. `docs/rust-tauri/R07/T09_IMPLEMENTATION_READINESS.md`：原任务责任、39 个细分叶和 A17/A18。该文开头“只读/未实施”是较早调查时点，不应覆盖本交接的最新状态。
3. `artifacts/rust-tauri/R07/T09/CLI_IMPLEMENTATION_HANDOFF_20261018.md`。
4. `artifacts/rust-tauri/R07/T09/MOBILE_IMPLEMENTATION_HANDOFF_20261018.md`。
5. 当前源码差异和 `artifacts/rust-tauri/R07/T09/*.json/*.log`，逐个核实际退出码，不从文件名推断通过。

切换时已要求所有协作代理停止扩大实现，在完成当前原子编辑后只写交接，不再修复或启动新测试；下一工具不能依赖这些代理仍在运行。

### CLI 初稿

本轮修改 `cli/args.ts`、`cli/entry.ts`、`cli/rust-service.ts`，新增 `cli/rust-chat.ts`。只做过 3 条 `node --check` 语法检查（退出 0），未运行 CLI 测试、未证明真实聊天。

初稿已知待收口：settle 与 pump 顺序、重连 resumePending/超时、真实 WS 结束与 EOF/退出清理。旧 CLI 测试尚未改，不得以语法检查替代业务验证。详细行号与差异以代理交接和实际文件为准。

正式目标：默认 Rust，真实 status/sessions/chat/continue/serve；保留原交互及维护命令，复用现有私有实例/token 校验与 runner，Rust 失败不能悄悄回退 Node。WS hello、稳定 clientMessageId、流与工具事件、terminal、审批并发、abort、resume、EOF 清理必须实际验证。

### mobile 初稿和已知失败

`mobile-init.ts`、`services/chat-client.ts`、`utils/ui-helpers.ts` 及相应测试已有部分接线，新增 `desktop/src/react/__tests__/mobile/R07RustMobileRuntime.test.ts`。精确修改清单以 mobile 交接为准。

- `mobile-unit1.json`：两个既有局部测试文件退出 0，只是开发回执。
- `mobile-unit2.json`：退出 1，38 项中 36 通过、2 失败；源码前后未变。必须保留此原始失败。
- 失败 1：`MobileApp.test.tsx` 的旧“Rust 聊天尚不可用”预期不再匹配。应在真实实现后调整为正向及错误边界，不能直接删掉验证。
- 失败 2：新增 Rust 手机初始化测试报“聊天目标已切换，旧回复已丢弃”。需核连接/选择代次，不能简单移除防串线保护。

未实施的必要接点：共享 `session-actions.loadSessions` 手机分支还可能调用要求 `sessions.read` 的 studio-list，实际手机只能有原四 scope。应复用 GET `/lingxi/v1/sessions` 的真实 SessionView 投影，桌面保持原行为。ModelSlice 还未加只读 modelState/modelGeneration 镜像及 reset 清理。共享 ChatSidebar 的切换和 InputArea 的创建已有实际链，不另造一套无人调用的 helper。

手机原四 scope：`chat,resources.read,files.read,files.write`，不能加 `settings.read` 或 `sessions.read` 换页面可用。刷新/后台/重连仅恢复事件与快照，不重发 prompt；过期异步回复不能写入新连接、新身份、新助手或新会话。

### Root 后端与统一接口：尚未实现

目前只新增 `rust/crates/lingxi-protocol/src/client_context.rs` 并在协议 `lib.rs` 导出。**未注册 schema/golden/verify、未生成、未编译，后端路由未实现**。不要因 TS 初稿调用了地址便认为服务已提供。

计划的正式 GET：

- `/lingxi/v1/mobile/bootstrap?agentId=&sessionId=`：只读、按身份投影、脱敏、chat 权限。
- `/lingxi/v1/chat/models?agentId=&sessionId=`：供 CLI/mobile 的最小模型目录与实际选择；不调用 settings-only 全模型管理面。

协议初稿类型：ClientContextQuery、ClientModelSelection、ClientModelState（ready/unconfigured/unavailable）、ClientModelSummary、ClientModelContext、ClientAgentSummary、ClientAvatarAvailability、MobileBootstrapResponse。顶层 bootstrap 包含 models/activeModel/currentModel/modelState/modelGeneration/thinkingLevel/permissionMode。generation 为真实十进制字符串，不造假代次；nullable defaultThinkingLevel 在消费者需归一化。

必须复用真实助手/会话归属与配置：显式 session 先验权限、取实际 agent，显式 agent 不匹配应拒绝；删除、损坏、越权不能回退其它助手；未配置模型、不可用模型、配置损坏分清，不能吞错误返回空目录或换模型。不要透传 providers/headers/URL/密钥/自由 metadata/全 preferences。工作区路径需 files.read 及真实根授权，远程下载不能回退宿主 file 路径。清理过期持久化要调用实际清理器或明确失败。

可复用实际平铺模块：`agents_admin.rs`、`agent_config_readback.rs`、`chat_settings.rs`、`session_focus.rs`、`settings_ui.rs`、`userprefs.rs`、`auth.rs`、服务 `lib.rs`。源码不是按每个模块单独目录组织，先用 rg 查，不要猜不存在的路径。

- `chat_settings::effective_choice_for_agent` / `dispatch_choice` / `selected_thinking` 是实际模型选择链，须理解并复用其优先级，不另建配置权威。
- `agent_config_readback::gc_workspace_persistence` 当前私有，可最小扩为 pub(crate) 复用，不伪造已清理。
- GET `/lingxi/v1/sessions` 已用真实 `list_for(principal)` 过滤，只需 chat。SessionView 没有 workspace/revision，不能伪造；`native:sessionId` 是客户端定位。
- `/sessions/studio-list` 另要求 sessions.read，不要放宽此接口。
- 模型 set/switch/thinking 的实际 route scope 要核对，用原四 scope 设备真实验证控件。
- 统一协议生成器：`lingxi-protocol-gen.rs` 的 schema/golden registry 和 `lingxi-protocol-verify.rs` 的类型分发都须同步；`contracts/generated` 只由生成器生成，不能手改。

### T09 实际验收

使用隔离真实 Rust 服务、真实数据库、真实执行链，只替换外部平台响应：

- A17：CLI 发真实任务，工具实际创建并登记文件；授权手机实际页面看到同一 sessionId/runId/messageId/resourceId，下载同字节。不能直接塞数据库或只验证 mock。
- A18：另一有效设备缺 resources.read，即使知道 resourceId，GET/HEAD/带票据仍被拒绝；不能以无凭证 401 代替。撤销后 HTTP/WS/下载及时失效。
- 实际 renderer 构建、窄视口本地浏览器登录/会话/聊天/刷新/重连；其他系统和真实手机未测另列。
- `/desktop/` 托管 mobile.html 是既有行为，不擅自换 Electron index；`dev:web` 仍起 Node，不能拿它当 Rust 页面证据。
- 按 39 叶实际责任逐项绑定证据；CLI 维护/恢复/发布的跨阶段份额不要提前代领。

## 四、T10—T12 继续要求

- T10：用量查询、筛选、详情、导出共用真实调用记录，合计准确，旧会话关联保留。
- T11：开发入口仍受权限限制；插件禁用、更新、卸载后旧调用立即失效；两项扩展边界单独核验。保留 MCP legacy SSE 未实现、SDK 解析前分配上限未证明等已知边界，不凭描述抹掉。
- T12：全部功能真实闭环，包括既有 Git 环境能力；隔离停用旧核心后新链实际工作。补 T07 浏览器失败、T08 明确清账项、所有未收口本阶段责任。

详读 `docs/rust-tauri/R07/T12_GATE_READINESS.md`。正式验收应接入现有 `xtask verify-stage R07 --evidence <全新目录>`，当前仅注册 R02—R06，R07 地图/合同/生产者消费仍需接通。不能新造只读汇总历史通过便放行的替代门禁，不修改原验收账或放宽封印。

总范围：24 个基础场景、408 个补充叶（原第二账合计 1050 条原断言）、2 项扩展边界，以及移交清单单独闭合。**35 项移交来源已定位，不要再问用户**：

`artifacts/rust-tauri/R07/T12/35-handoff-source-resolved-20261018.json` 及配套 md。408 = 373 直接责任 + 35 移交，35 已在 408 内，不能重复加；35 为 29 条 R04/R07/R08 + 6 条 R04/R07，共 87 原断言。来源定位不等于验收通过。

最终交付阶段报告、后续交接、逐功能结果和外围执行器/旧依赖清单；本地原始证据保留版本、命令、退出码和摘要。最终同一工作区候选运行完整 R07，并独立复核。真实账号/付费供应商/其他系统/正式安装包未验明确保留，最迟 R10，不假称全平台通过。

## 五、运行与证据注意事项

- Node 当前 24.16；Rust/rustfmt 固定 1.98.1。先核实际环境与仓库脚本，不临时升级依赖。
- Cargo 由一个主控串行操作，避免多个构建改同一 target。当前 `CARGO_TARGET_DIR=/private/tmp/lingxi-r07-build`、`CARGO_BUILD_JOBS=4`。
- `TMPDIR=/private/tmp/lingxi-r07-tests/tmp`；测试 `LINGXI_HOME=/private/tmp/lingxi-r07-tests/home`；`LINGXI_PRODUCT_DIR=/Users/study_superior/Desktop/Code/LingxiAgent/lib`。首次启动夹具可能主动移除继承 home；通过实际 API 创建助手，不塞旧格式未盖章配置。
- Cargo test 可能覆盖同一 target 下 `debug/lingxi-service`。真实集成前显式 build、复制不可变二进制、记录 SHA256，并将每项检查绑定到该副本。
- 历史 T08h 保留二进制：`/private/tmp/lingxi-r07-tests/binaries/t08h-formal-build1/lingxi-service`，SHA256 `0aa124e8db5e2c791d2c568145e3b75a897df9ac988dbccb9ae01c7af0cdfb2b`。它不能代表 T09 新实现；临时目录若丢失，应如实说明并重建候选。
- 开发回执可用 `python3 artifacts/rust-tauri/R07/T09/evidence-tools/run_development.py <全新label> -- <command...>`，保存命令/环境/HEAD/源码指纹/日志/退出；它是开发证据，不是最终候选验收。
- 候选冻结后记录源码/工具/实际二进制前后指纹，执行期变动则该候选失效。已完成检查无新变动不机械重复；最终整体检查按原要求做。
- 不修改 `.sync-audit/verified-source-sha.txt`，不扩大白名单，不为变绿退役门禁。不提交也能验 dirty 工作区；提交相关封印仍未获授权。

先报告你核对到的真实断点，然后继续 T09，不停在计划或要求用户确认继续。
