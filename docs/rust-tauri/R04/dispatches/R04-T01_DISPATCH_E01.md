# R04-T01 派单｜EXECUTOR-R04-T01-E01

派单时间：2026-09-30。派单人：R04 总控编排器（ZCode 会话内真实 Agent 工具派发）。

---

你是一次性执行代理，只负责 R04-T01：建立工具目录与参数契约。
基线：bf6450bcd722668188d1a3091681bf285875ef11；分支：codex/rust-tauri-migration；工作区：/Users/study_superior/Desktop/Code/LingxiAgent。

完整读取 R04 总控提示词、原阶段书、本 Task 全部 Steps/Deliverables、
R04-A01 与 R04-A02、补充义务 R04-SUP-03（RR-T02-F1..F4 评估）与 R04-SUP-04（目录面）、
R03 现行交接、前置 Task 报告，以及真实源码、注册入口、调用方、存储和测试。
不得依赖前一个代理的聊天记忆。

必读文件（含路径，均在本工作区）：
- 总控提示词：/Users/study_superior/Downloads/Lingxi_R04_自动执行总控提示词_2026-09-30.md
- 任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R04_统一工具网关、四基础工具与沙盒.md（T01 节）及 00/01/02/03/04/05/06/90/91 共同规范
- 责任矩阵：docs/rust-tauri/R04/R04_SCOPE_MATRIX.json（本阶段义务登记）
- R03 交接：docs/rust-tauri/R03/R03_HANDOFF.json（interfaces[0] ToolExecutorPort＝R04 交接主面）
- 源码起点：rust/crates/lingxi-kernel/src/ports.rs（ToolRequest L846/ToolExecutorPort L921/ToolOutcome L942）、rust/crates/lingxi-kernel/src/invocation.rs、shared/tool-categories.ts、core/tool-invocation-gateway.ts、lib/tools/ 下按需目录与现役工具注册
- 从真实注册表追踪按需工具、MCP、插件、PTY 和进程 helper；不猜测未读取的 API、文件名或参数。

任务要点（原 Task 全文仍为准）：
1. 从真实注册和旧工具类别盘点内置/插件/MCP 命名空间、别名、版本、代次、schema、权限说明、可用性、超时、幂等分类；保留 display name 与唯一 target ID 分离；同名不同来源不得覆盖，别名最终解析到同一权威身份 [S03]。
2. 建立真实 manifest/registry 和按需目录（单一 schema 源，不在多处手写）；将完整有效参数与结构化结果契约落地（原 ToolRequest 只有 args_digest/args_summary、ToolOutcome::Success 只有 content_digest 是 R03 测试边界，不是 R04 契约）——原始请求→schema 校验→明确默认值/规范化→同一份不可变有效参数→权限/批准摘要；摘要由可信边界对有效参数计算，不能信任模型自填摘要；参数摘要不能反推出路径/正文/命令。结果需包含实际内容块、错误、资源引用、截断/落盘引用、退出/运行状态，供 R03 运行器和 R05 消费，不能只有 digest。接口允许按职责增量演进，同步更新所有实际消费者与测试；保留 R03 行为不变量；不建平行 Protocol/Run。
3. 限制 schema/参数大小、嵌套深度、引用和验证成本；外部 schema dialect 不支持时明确失败，禁止默认联网解析任意引用。工具来源自称只读不等于已授予只读安全分类。
4. 定义安装/更新/禁用/卸载对 generation、描述快照、批准和执行句柄的影响；旧描述不得悄悄指向新含义。复用单一协议源生成 schema/TS 时做跨语言样本一致性测试（如适用）。
5. 评估 R04-SUP-03（RR-T02-F1/F2/F3/F4，canonical 序列化）在 T01 消费点的状态：逐项给出关闭证据或明确登记处理口径，不掩盖。

验收（两场景都必须真实执行，T01 可用最小计数执行器验证机制，最终阶段再真实能力复验）：
- R04-A01 目录与执行同源：注册两个来源不同的同名工具，经发现、描述、指定 target 调用，实际只命中正确目标；保留目录、调用及结果关联记录。
- R04-A02 陈旧目录拒绝错误目标：客户端持有 generation1，工具改成 generation2 后提交旧请求；明确刷新或拒绝，不执行语义已变的新对象。
- 追加对抗检查：别名与主名权限一致；畸形 schema、过深参数、非法必填类型、伪摘要均不触发副作用；规范化碰撞明确处理，不制造第二工具身份。

先检查用户已有修改与真实生产路径（当前 git status 干净），再建立可重复验收。
复用已存在的正确组件（lingxi-kernel/lingxi-service 现有结构、xtask 生成物检查），完成本 Task 全部当前责任并接通消费者。
处理合法使用、拒绝、错误、撤销、取消、恢复和资源收尾。不扩大产品范围，不把 T02+ 网关/批准/文件/进程提前做进来（目录与参数契约层的最小接线除外）。

先做普通逐项自查，再做对抗性自查，逐 A-ID/补充项记录实际结果。
测试替身只代替外部系统，不能代替被测网关/策略/存储/监督。
真实文件和进程要求必须实际执行，过滤 0 测试不是通过。

验证命令（经 rustup 生效 rust-toolchain.toml 1.98.1；PATH 中 ~/.cargo/bin 需先于 /opt/homebrew/bin）：
- cargo fmt --manifest-path rust/Cargo.toml --all -- --check
- cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
- cargo test --manifest-path rust/Cargo.toml --workspace --locked
- cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
- cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries
- 若触碰 Node/TS 共享消费面：npm run typecheck、npm run typecheck:core-contracts、npm run check:dependency-boundaries、npm run check:tool-invocation-boundaries 及相关 vitest

禁止修改需求让实现通过、删断言、吞错误、静默降级或伪造证据。
没有配置真实模型时不能伪造模型已接入。
禁止 commit/push/发布/真实数据迁移/未授权外发。

在 docs/rust-tauri/R04/R04-T01_REPORT.md 输出实现、调用链、修改范围、测试命令/退出码、
逐项预期与实际、日志/产物/摘要、替身边界、未验证项、风险和回退。
证据放 artifacts/rust-tauri/R04/T01-E01/。
同时更新 docs/rust-tauri/R04/R04_TEST_MAP.json 的 T01 条目（测试名与状态）。
自检全过只返回 READY_FOR_REVIEW，不自行判独立 PASS。
结束本次代理；下一 Task 和后续修复使用新代理。
