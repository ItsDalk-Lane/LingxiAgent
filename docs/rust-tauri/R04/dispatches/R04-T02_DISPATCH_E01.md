# R04-T02 派单｜EXECUTOR-R04-T02-E01

派单时间：2026-09-30。派单人：R04 总控编排器（ZCode 会话内真实 Agent 工具派发）。
TASK_BASE_SHA：fd5fc2c2c583c6f4273ad3fa2122530edd8b152c（R04-T01 已独立 PASS 并推送确认）。

---

你是一次性执行代理，只负责 R04-T02：实现唯一工具执行网关。
基线：fd5fc2c2c583c6f4273ad3fa2122530edd8b152c；分支：codex/rust-tauri-migration；工作区：/Users/study_superior/Desktop/Code/LingxiAgent。

完整读取 R04 总控提示词、原阶段书、本 Task 全部 Steps/Deliverables、
R04-A03 与 R04-A04、补充义务 R04-SUP-02（journal/授权判定点复用）与 R04-SUP-05（R03 回归）、
R03 现行交接、R04-T01 报告（前置 Task），以及真实源码、注册入口、调用方、存储和测试。
不得依赖前一个代理的聊天记忆。

必读文件：
- /Users/study_superior/Downloads/Lingxi_R04_自动执行总控提示词_2026-09-30.md（重点 §4.1/§4.2/§7.T02/§9）
- Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R04_统一工具网关、四基础工具与沙盒.md（T02 节）+ 共同规范 01/02/05
- docs/rust-tauri/R04/R04_SCOPE_MATRIX.json（SUP-02/SUP-05 全文）
- docs/rust-tauri/R03/R03_HANDOFF.json interfaces[0]（authorization_boundary + journal_contract 原文）
- docs/rust-tauri/R04/R04-T01_REPORT.md + rust/crates/lingxi-kernel/src/toolcatalog.rs（T01 交付的 ToolRegistry/EffectiveArguments/prepare_invocation）
- 源码：rust/crates/lingxi-service/src/{lib.rs,runs.rs,invocations.rs,task_supervisor.rs,subagents.rs,background.rs}、lingxi-kernel/src/{ports.rs,invocation.rs,subagent.rs}、adapters/src/storage/、xtask
- core/tool-invocation-gateway.ts、lib/tools/session-permission-wrapper.ts、lib/tools/subagent-tool-policy.ts（现役语义参照）

任务要点（原 Task 全文仍为准）：
1. 将当前已存在的调用入口（直接、按需、MCP、插件、开发、子代理、后台）转为统一 InvocationRequest；principal 由可信入口生成；后续客户端只定义接入契约和调用级测试，不顺手完成整套客户端。
2. 依照总控提示词 §4.2 冻结 owner 与执行顺序：可信主体+RunContext→target/version/schema/代次与可用性→参数与实际资源范围→权限硬规则与父子授权交集→必要批准→执行前重检（撤销/代次/参数/资源）→已持久化合法执行意图→单次执行→可信回执或 Unknown→原 Run 负责人归并与持久化。接入现有 RunSupervisor、授权上下文与 journal（T05 写序：record_invocation_intent(prepared)→authorized→advance_invocation(started)→执行→record_invocation_receipt）；授权判定点=驱动在 authorized 步应用（RunGrant::Full 直授 / RunGrant::Subagent{tier} 经 kernel authorize_child_tool）。不得另立第二授权面、不得旧路径放行+新网关另判、不得写两套互相矛盾的收据。
3. PreparedInvocation 由服务端生成、不可由模型伪造，绑定主体/Run/目标代次/有效参数/资源/期限；执行时重验。禁止 "approved:true" 即通行。
4. 补齐策略 service ports 和真实入口接线；未经配置的授权机制应拒绝其不能安全判定的操作（不自动 Full 放行）；T03 才实现完整批准生命周期——本 Task 不用假策略证明网关真实安全，但策略端口缺失时的行为必须诚实（拒绝或显式需要批准，不静默放行）。
5. 对危险执行器入口做静态边界扫描和运行时负向验证：旁路调用、同源工具别名、嵌套 worker/MCP 回调。合法多层收窄允许存在，但不能出现任一路径单独放宽或写第二份调用收据。执行器函数不对业务入口公开；CI/xtask 边界检查覆盖禁止直接调用受保护执行器（白名单必须具备理由与负向测试）。
6. 对已有 subagent/subagent_reply/subagent_close 特殊分支检查 target 身份、可用性、参数和策略；特殊运行机制不绕过网关；不取消 T01 已交付的只读衰减、取消树或父子关系。

验收（两场景都必须真实执行）：
- R04-A03 各入口权限不变：固定主体/target/参数，逐条适用入口（直接/按需/MCP/插件/开发/子代理/后台中当前实际存在或本 Task 建立的）运行允许、拒绝、需批准三组，权限结论与实际副作用一致；某入口确需更严格策略时登记依据。
- R04-A04 伪造执行凭证失败：调用参数携带假 principal/capability/prepared/approval，不能覆盖宿主认证事实、扩大授权或形成实际副作用。
- 追加对抗检查：摘要对应 A、真实参数为 B；同一批准并发使用两次；跨 agent/session/执行节点复用句柄；禁用后缓存句柄调用；每次都由真实网关而非测试替身决定拒绝。

先检查用户已有修改（应为干净），再建立可重复验收。复用 T01 的 ToolRegistry/PreparedToolCall。
处理合法使用、拒绝、错误、撤销、取消、恢复和资源收尾。不扩大产品范围（T03 批准生命周期/T04 文件工具/T05 进程不在本 Task）。

验证命令（/Users/study_superior/.cargo/bin/cargo，rustup 1.98.1）：
- cargo fmt --manifest-path rust/Cargo.toml --all -- --check
- cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
- cargo test --manifest-path rust/Cargo.toml --workspace --locked（r00_management_leaves 为已知 macOS 防火墙环境失败，隔离复跑核证）
- cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
- cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries
- R03 回归：十套件钉数脚本 + A15/A16 关键链（沿用 T01 的回归口径）
- 如触碰 TS/Node 消费面：npm run typecheck 等

禁止修改需求让实现通过、删断言、吞错误、静默降级或伪造证据。禁止 commit/push/发布/真实数据迁移/未授权外发。

在 docs/rust-tauri/R04/R04-T02_REPORT.md 输出实现、调用链、修改范围、测试命令/退出码、逐项预期与实际、日志/产物/摘要、替身边界、未验证项、风险和回退。
证据放 artifacts/rust-tauri/R04/T02-E01/。更新 docs/rust-tauri/R04/R04_TEST_MAP.json 的 T02 条目。
自检全过只返回 READY_FOR_REVIEW，不自行判独立 PASS。结束本次代理。
