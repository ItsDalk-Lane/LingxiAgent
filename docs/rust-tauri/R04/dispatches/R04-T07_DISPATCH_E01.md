# R04-T07 派单｜EXECUTOR-R04-T07-E01

派单时间：2026-09-30。派单人：R04 总控编排器。
TASK_BASE_SHA：26251dc92（R04-T06 已独立 PASS 并推送确认）。

---

你是一次性执行代理，只负责 R04-T07：MCP 与插件 worker 接入。
基线：26251dc92；分支：codex/rust-tauri-migration；工作区：/Users/study_superior/Desktop/Code/LingxiAgent。

完整读取总控提示词存档（docs/rust-tauri/R04/dispatches/R04_ORCHESTRATOR_PROMPT_2026-09-30.md，§7.T07）、原阶段书 R04-T07 节（R04-A13/A14）、T01-T06 报告、真实源码与测试。不得依赖前一个代理的聊天记忆。

必读：
- 任务书 R04-T07 节 + 共同规范 01/02/05
- docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md D-06（rmcp 3.4.1 锁定决策与已验证能力：tokio duplex transport、initialize 握手、协议 2025-11-25 协商）
- docs/rust-tauri/R04/R04-T01_REPORT.md（ToolOrigin mcp 命名空间）、T02（网关/入口）、T03（批准面）、T05/T06（进程/沙盒）
- rust/crates/lingxi-spike（rmcp 用法参照）、rust/Cargo.lock（rmcp 3.4.1 已在锁内——复用锁内版本，不新增包版本）
- 现役 MCP 接入：搜索现役源码中 MCP client/server 注册（lib/ 或 core/ 内 mcp 相关实现与测试），只覆盖现役支持范围
- 现役插件 worker：lib/plugins/ 或等价位置的单操作执行器形态（按 R00 盘点追踪）

任务要点（原 Task 全文仍为准）：
1. MCP 适配：用 rmcp 3.4.1（锁内）接入现役所需 transport/初始化/版本协商/工具清单/变更通知/错误。外部 server 只视为不可信工具来源；SDK 存在不是集成完成证明——用合成 MCP server（受控测试实例）真实验证 transport、initialize、list、call、断连重连。
2. 身份与代次：来源+server+tool 身份稳定（复用 T01 ToolOrigin mcp 命名空间）；工具列表变化映射 registry generation、失效旧批准；重连/取消/限流/超大输出/结构化结果不符 schema 按契约处理；远程会话 ID/工具名/返回路径不成为本地授权凭证；不把宿主高权限 token 透传任意服务。
3. worker RPC：最小单操作请求（调用 ID、期限、取消、资源许可、大小上限）；stdin/stdout 或等价通道带长度限制；控制并发/启动/回收；worker 不读整个用户 home、provider 密钥库，不独自构建 Agent loop。未受信任代码进系统隔离边界（同进程 Rust 模块不是安全沙盒——worker 进程经 T05 supervisor/T06 沙盒约束按需）。
4. 受控宿主回调契约：worker 需模型能力时回调宿主 ModelGateway 端口；R05 未实现真实 ModelGateway 时合法返回"能力未配置"并测试拒绝/预算边界——不提前实现供应商、不让 worker 偷找 key 外发。
5. 网关集成：MCP 工具与 worker 工具经 T02 网关（prepare→策略/批准→执行）注册执行；执行类权限按 T03 面；结果回结构化 ToolSuccess。

验收（合成/受控实例，不自动启动用户外装 MCP）：
- R04-A13 MCP 重连不重复执行副作用：受控 MCP 服务完成一次计数动作但断开回执，客户端重连；不盲目重发写调用，按 verified idempotency/status 查询或 Unknown 处理，计数不盲增。
- R04-A14 worker 无法绕过权限：worker 请求超出 grant 的文件或模型凭证，真实宿主拒绝；grant 不扩大、秘密不泄露、主服务仍可用。
- 追加对抗检查：两个 server 同名工具、tools/list 变化、错误/超大输出、结构化结果不符 schema、取消后迟到响应、跨调用复用 ticket、伪造本地文件链接。

验证命令（/Users/study_superior/.cargo/bin/cargo）：fmt/clippy/test workspace --locked（r00 环境项隔离核证）/check-contracts/check-boundaries/R03 十套件+A15+A16/T01-T06 套件回归。

禁止修改需求让实现通过、删断言、吞错误、静默降级或伪造证据。禁止 commit/push。测试只对合成实例；禁止真实外发/付费调用。
报告 docs/rust-tauri/R04/R04-T07_REPORT.md；证据 artifacts/rust-tauri/R04/T07-E01/；更新 R04_TEST_MAP.json T07 条目。
自检全过只返回 READY_FOR_REVIEW。结束本次代理。
