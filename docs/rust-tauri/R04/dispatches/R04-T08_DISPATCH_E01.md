# R04-T08 派单｜EXECUTOR-R04-T08-E01

派单时间：2026-09-30。派单人：R04 总控编排器。
TASK_BASE_SHA：ded3467bf（R04-T07 已独立 PASS 并推送确认）。

---

你是一次性执行代理，只负责 R04-T08：工具结果、产物与全矩阵验收。
基线：ded3467bf；分支：codex/rust-tauri-migration；工作区：/Users/study_superior/Desktop/Code/LingxiAgent。

完整读取总控提示词存档（docs/rust-tauri/R04/dispatches/R04_ORCHESTRATOR_PROMPT_2026-09-30.md，重点 §7.T08 与 §11 正式 Gate）、原阶段书 R04-T08 节（R04-A15/A16）、T01-T07 报告与各轮审查、真实源码与测试。不得依赖前一个代理的聊天记忆。

必读：
- 任务书 R04-T08 节 + 共同规范 01/02/05 + 91 交付模板
- docs/rust-tauri/R04/R04_SCOPE_MATRIX.json、R04_TEST_MAP.json、R04_PLATFORM_CAPABILITIES.json
- T01-T07 报告 docs/rust-tauri/R04/R04-T0*_REPORT.md 与 reviews/ 各轮（重点遗留：T07 F1 unique_dir 并行碰撞修复归本 Task；T04 OBS-1 edit fuzzy 重复计数差异需在全矩阵中登记复核；T05 OBS-2 RegistryFull 孤儿路径）
- xtask 阶段图机制：rust/crates/xtask/src/{stage_map.rs,verify.rs,main.rs}、stage_maps/R03.json 及其生成器脚本（scripts/rust-tauri/r03_*）；R03.json 的 repair_suites/deferred 机制作为格式参照
- 源码：lingxi-kernel/src/{ports.rs,toolcatalog.rs}、lingxi-service/src/{toolgateway.rs,approval_service.rs,filetools.rs,resourceaccess.rs,exectools.rs,procsupervisor.rs,sandbox.rs,mcpbridge.rs,workerrpc.rs,runs.rs}

任务要点（原 Task 全文仍为准）：
1. 统一 Success/Failed/Cancelled/Unknown 外部语义与内部收据：明确是否派发、是否有部分副作用、是否仍运行；保留内容块、资源身份、截断、可重试性。"请求已发送"不登记"文件已生成"。
2. 产物核验：文件产物登记前核验存在性、实际目标权限、基本格式/内容条件；资源读取复用权限；MCP/worker 只声明成功而无可验证产物时拒绝假成功；远端资源按其类型验证。
3. 全矩阵：从实际注册的 Rust 工具（read/write/edit/exec_command/write_stdin + MCP/worker 测试工具）× 权限（operate/ask/read_only×用户/子代理）× 调用路线（直接/按需/子代理/后台/MCP/worker）× 生命周期（禁用/卸载/代次/批准/取消）展开；不适用组合给理由；危险组合不 pairwise 省略。已承诺可用路径真可用；未迁移能力准确标识并保留期限（R06/R07/R08 递延不丢）。
4. 修复 T07 F1（unique_dir 每进程计数器后缀——先复现碰撞、修复、多轮并行验证稳定）；处理 T04 OBS-1（edit fuzzy 重复计数与现役差异：修复对齐或登记裁定，二选一并给依据）。
5. 【正式 Gate】按 R03 模式建立 rust/crates/xtask/src/stage_maps/R04.json：真实生成器脚本（逐字段镜像来自 R04 账本/测试）、注册 STAGE_MAPS+runner_identity（含 stale binary 检测）、覆盖 R04-A01~A16 全部 16 A-ID 与到期补充义务（SUP-01 ask 档、SUP-03 消费点关闭、SUP-05 R03 回归）；门禁负向测试（未知阶段/空集/漏 ID/删映射/0 匹配过滤器/陈旧证据/候选漂移→非零退出）；verify-stage R04 --evidence <目录> 真实退出 0 且证据落盘。
6. 交付：R04_REPORT.md（阶段报告，按 91 模板）、R04_HANDOFF.json（R05 交接：真实工具目录/schema、参数载荷、授权/审批端口、取消所有权、实际结果、ResourceRef、MCP/worker 能力与限制、完整注册和回归命令；不写自引用 SHA）、R04_ACCEPTANCE_LEDGER.json（16 A-ID+义务→证据指针）；更新 R04_TEST_MAP.json/PLATFORM_CAPABILITIES.json/ORCHESTRATOR_PROGRESS.json 的 T08 条目。
7. 真实闭环验证：Rust service/网关→真实文件/进程/MCP 合成服务→收据/结果闭环；测试 Provider 只发请求与检查真实结果（不按工具名伪造输出）；验证四基础工具、权限拒绝、取消、重连、崩溃 Unknown 结果以及 R03 回归；旧 Pi 工具工厂不经由 Rust 路径调用（生产默认入口保留不动——A16 保护语义）。

验收（两场景都必须真实执行）：
- R04-A15 空产物不算成功：worker 返回 success 但文件不存在/越权/结构不满足约定；网关不得登记为已交付有效文件，错误和实际副作用事实准确。
- R04-A16 禁用状态覆盖所有路线：缓存工具描述与句柄后禁用目标，从全部适用入口调用均不得新执行；真实已执行的历史结果保留。
- 追加：完整 verify-stage R04 门禁跑通（这是本 Task 的核心交付之一）。

验证命令（/Users/study_superior/.cargo/bin/cargo）：fmt/clippy/test workspace --locked（r00 环境项隔离核证）/check-contracts/check-boundaries/R03 十套件+A15/A16/完整 verify-stage R03（最终候选上重跑）/verify-stage R04（新）。

禁止修改需求让实现通过、删断言、吞错误、静默降级或伪造证据。禁止 commit/push。
报告 docs/rust-tauri/R04/R04-T08_REPORT.md；证据 artifacts/rust-tauri/R04/T08-E01/。
自检全过只返回 READY_FOR_REVIEW。结束本次代理。
