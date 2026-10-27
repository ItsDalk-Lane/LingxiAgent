# DISPATCH: EXECUTOR-R03-T01-E01（执行子代理，一次性）

- TASK_ID: R03-T01
- TASK_NAME: 实现运行与尝试状态机
- TASK_BASE_SHA: 526f7770f1eff6be289b8c34faeccc1b95e181fd
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- ACCEPTANCE_IDS: R03-A01, R03-A02
- REPORT_PATH: docs/rust-tauri/R03/R03-T01_REPORT.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/T01-E01/
- 派发时间: 2026-09-29
- 派发者: R03 总控编排器（ZCode 会话）

## 派单正文（原样发给子代理）

你是一次性执行代理，只负责 R03-T01：实现运行与尝试状态机。
本次基线：526f7770f1eff6be289b8c34faeccc1b95e181fd。
分支：codex/rust-tauri-migration。
工作目录：/Users/study_superior/Desktop/Code/LingxiAgent。

先完整阅读 R03 总控共同约束、原 R03 阶段书、
本 Task 全部记录、R03-A01/R03-A02、R02 现行交接、
前置 Task 报告（无前置），以及真实源码、调用方和测试。

不要依赖别的代理聊天记忆。
先检查用户既有修改，禁止覆盖或混入。

先明确实际 owner、生产入口与存储写者，
再建立可复现验收，完成当前 Task 的全部 Steps/Deliverables。

已有正确实现复用并验证，不复制平行状态机。
允许模型/工具外部替身；不允许替身承担内核决定权。
处理失败、取消、清理、权限、持久化及重启边界，
不只实现 happy path。

只执行本 Task。
必要跨文件改动必须说明因果关系，不提前完成下一个 Task。
不得修改需求使实现通过，不删断言，
不将失败改成跳过或可选。

不得 commit/push、修改进度为 DONE、
进行真实外发或用户数据迁移。

在 docs/rust-tauri/R03/R03-T01_REPORT.md 报告：
实现与调用链、修改文件、
逐验收预期与实测、命令退出码、
证据位置及摘要、替身边界、
候选输入摘要、未验证项和独立复核重点。

自检通过只返回 READY_FOR_REVIEW。
实际失败或环境阻塞如实报告。

返回后结束本次代理，
不接下一个 Task，也不自行继续修复。

## 总控补充的定向输入（同一派单的组成部分）

必读文件（按序）：
1. /Users/study_superior/Desktop/Code/LingxiAgent/AGENTS.md
2. Lingxi_Rust_Tauri_Taskbooks_2026-09-23/00_README_总入口.md、01_通用执行约束.md、02_目标架构与强制契约.md、03_功能与所有权矩阵.md、04_阶段依赖与接口交接.md、05_验收与性能协议.md、91_交付与独立验收模板.md
3. Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R03_运行状态机、并发、取消与恢复.md（本 Task 全文）
4. docs/rust-tauri/R03/R03_SCOPE_MATRIX.json 与 R03_TEST_MAP.json（本阶段责任矩阵）
5. docs/rust-tauri/R02/R02_HANDOFF.json（interfaces + allowed_next_scope）
6. docs/rust-tauri/R02/R02_IMPLEMENTATION_MAP.json（handoff_to_r03）
7. docs/rust-tauri/R02/SERVICE_START_AND_SHUTDOWN.md
8. 源码：rust/crates/lingxi-kernel/src/{lib.rs,ports.rs}；rust/crates/lingxi-protocol/src/lib.rs（RunStatus/RunId/AttemptId）；rust/crates/lingxi-service/src/{lib.rs,sessions.rs,events.rs,inject.rs(如存在)}；rust/crates/lingxi-adapters/src/storage/{run_store.rs,queue.rs,migrations.rs}；相关测试文件。

本 Task 规格要点（任务书 R03-T01 原文为准）：
1. 定义 queued/running/waiting_approval/cancelling 与 completed/failed/cancelled/interrupted 等状态及合法转换；外部协议保持兼容映射。
2. RunId 在任务创建时固定，attempt 重试递增；ModelCallId/ToolCallId 独立产生，不因一次 provider 重连另建用户任务。
3. 终态通过单一 finalize 路径持久化；重复 finalize 幂等，冲突终态拒绝并诊断。
4. 空回复、只有过程内容、工具部分失败与取消分别有明确 outcome，不编造最终答案。
必须交付：RunStateMachine（真实接线）、转换表、运行结果契约。

已知现场事实（须核实）：
- lingxi-kernel/src/lib.rs 已有纯函数 RunStateMachine::transition 与 RunContext；RunStatus 在 lingxi-protocol。R02 的 sessions execute 链路可能存在"立即成功"式临时执行入口——本 Task 必须把真实运行生命周期接进去，且不能让旧临时入口与新生命周期分别拥有同一 Run 终态；若旧测试依赖旧内部事件数量，可基于原公开不变量改夹具，但须在报告中列出每一处及理由（独立 Reviewer 将复核未降低保护）。
- 终态持久化复用 StoragePort/RunDatabase 同事务边界（run_store.rs），事件经 EventService；不要另建第二套存储或事件面。
- R02 递延到 R07 的九项叶保持递延，不得倒灌本 Task。

验收（任务书原文）：
- R03-A01 多模型调用只有一个任务终态：替身按工具→继续→最终回复三次模型调用；一个 Run、三条 ModelCall、唯一终态；中间模型结束不算任务结束；证据=事件及数据库断言；本机隔离环境、确定性替身。
- R03-A02 重复终态不重复结算：相同 settled 消息重复+冲突消息乱序到达；只持久一次终态/结算，冲突记录可诊断；证据=属性测试及结果计数。

测试替身边界：Provider/Tool 可用确定性替身（不引入真实外发）；不得替身写终态、不得 mock 被测核心（状态机/finalize/存储/事件）。
本 Task 验证命令（定向，verify-stage R03 未注册前不得伪造其成功）：
- cargo fmt --manifest-path rust/Cargo.toml --all -- --check
- cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
- cargo test --manifest-path rust/Cargo.toml --workspace --locked（至少覆盖本 Task 新增测试 + 受影响 R02 测试子集；报告列出过滤器与命中数，0 命中不算通过）
- cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts
- cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries

证据与报告落位：
- 日志/测试产物：artifacts/rust-tauri/R03/T01-E01/（自建子目录，含命令、退出码、摘要）
- 报告：docs/rust-tauri/R03/R03-T01_REPORT.md（按 91 模板精神：实现与调用链、修改文件清单、逐验收预期/实测、命令与退出码、证据位置、替身边界、候选输入摘要=基线 SHA+修改文件+digest、未验证项、独立复核重点）
- 不得提交 git；工作树改动留待总控冻结候选。

工程红线（AGENTS.md）：禁止静默降级；不把密钥写进代码或报告；不改 pinned-keyset；不动 desktop/ 生产默认入口；npm 侧非本 Task 范围。
