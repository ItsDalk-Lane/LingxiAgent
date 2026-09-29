# DISPATCH: EXECUTOR-R03-T08-E01（执行子代理，一次性）

- TASK_ID: R03-T08
- TASK_NAME: 状态机验收与 R04 接口交接
- TASK_BASE_SHA: dc42a01e37d1110293ec137402fd3239bd5b7d9d
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- ACCEPTANCE_IDS: R03-A15, R03-A16
- REPORT_PATH: docs/rust-tauri/R03/R03-T08_REPORT.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/T08-E01/
- 派发时间: 2026-09-29
- 前置：T01–T07 全部 DONE 并推送

## Task 规格（阶段书原文为准）

做什么：证明真实调度器，而非替身自身在控制结果。
1. 替身只模拟外部响应，不 mock RunSupervisor/Storage/授权上下文；测试调用真实 service 入口。
2. 至少运行正常、多工具、取消、超时、重复、乱序、跨会话、崩溃恢复组合；记录任务/调用/终态计数。
3. 将测试失败生成最小可重放种子，加入长期状态机回归，而不是新建另一个特殊执行分支。
4. 交付可供 R04 注册工具、R05 注册模型、R07 接入后台入口的公开 ports。
必须交付：R03_REPORT.md；R03_HANDOFF.json；状态机属性及集成证据。

验收：
- R03-A15 真实入口驱动状态：替身提供协议输出但不写状态；通过 service 执行任务；所有状态/事件来自真实 Supervisor 与存储；证据=调用观测与集成日志。
- R03-A16 故障种子可重放：一次随机属性测试发现异常序列；固定 seed 重跑；稳定复现，修复后同 seed 与全组通过；证据=seed 和测试前后日志。没有现成异常时，用隔离测试变异证明捕获和重放机制，不能编造发现历史。

## 总控细化（同一派单组成部分，全部必做）

1. 阶段图注册与正式 Gate：
   - 新建 rust/crates/xtask/src/stage_maps/R03.json（以 R02.json 为 schema 模板）：commands（R03 全部定向验证命令）+ scenarios（R03-A01..A16 全 REQUIRED）+ supplementalLeafScenarios（按 R03_SCOPE_MATRIX.json 的 4 项补充义务：SUP-01 stage map 注册自身、SUP-02 R02 接口消费回归、SUP-03 CLI 聊天叶 R03 份额、SUP-04 R07 递延九叶保持——分类与 R00 账本相等性检查规矩一致）。
   - 注册进 STAGE_MAPS（main.rs）并同步 runner_identity.rs 钉住 R03.json。
   - 负向自证：未知阶段/空集/缺证据/超时/假成功仍硬失败（xtask 已有机制，跑一次负向探针证明对新图同样生效）。
   - 真实运行 cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 --evidence artifacts/rust-tauri/R03/T08-E01/verify-stage-r03（全新证据目录），退出码 0，证据落盘。
2. 强制修复递延项（T04 R1-F1 + T05 V3 + T06 V4，同根因）：
   - docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json 登记 V2（64d7edfd…）/V3（3bd5388f…）/V4（371d4154…）指纹（以你独立重算的盘上指纹为准，V1 已发布值逐字节不动）。
   - scripts/rust-tauri/r02_t04_storage_tx.sh S4 硬编码 version==1 修复为按注册表/实际版本判定。
   - 修复后重跑 r02_t04_storage_tx.sh 全程 + R02 回归链。
3. R02 基础链回归（阶段收口）：认证与隔离、单写者与事务、事件快照与续读、损坏库与备份、关闭与恢复、A15 真实重启链、A16 默认入口与相关旧栈回归——经 verify-stage R02（全新证据目录）或等效定向命令重跑，结果落证据；npm 侧按 R02 惯例跑受影响定向（完整 npm 与审计封印状态由总控另记，不在本 Task 伪造）。
4. R03_REPORT.md + R03_HANDOFF.json + R03_ACCEPTANCE_LEDGER.json（按 91 模板 + R02 同名文件格式）：
   - 16 A-ID → 有效证据映射；8 Task 摘要；修复轮摘要（T03 D1 于 T06 修复等）；替身边界声明；R02 回归结果。
   - 接口交接（真实可调用名称+签名+错误/取消语义，不接受拟用名词）：R04 ToolExecutor port+调用上下文（含 T06 O-1 ask 档继承坍缩为审批策略面义务）；R05 Provider port（TurnProviderPort 现状）；R07 任务提交/监督入口（execute_background_for/cancel/事件订阅面）。schema/协议版本、数据 epoch、依赖锁。
   - 递延登记：R07 九叶保持 REQUIRED；V2–V4 登记完成态；R04/R05/R07 各自义务。
   - 注意防自引用：HANDOFF 中不写本 Task 自己的 commit SHA（总控提交后由后续记录登记）。
5. A16 种子机制：现有属性测试（T01 finalize/T04 fence）均未发现自然异常——按阶段书允许路径，用隔离测试变异（临时注入受控缺陷到隔离副本/测试内探针，不进生产代码）证明「异常序列→捕获 seed→固定 seed 稳定复现→移除变异后同 seed+全组通过」机制，如实记录为机制证明而非自然发现；落 seed 与前后日志。
6. 组合矩阵测试（新增持久集成测试或汇编既有测试的组合运行）：正常、多轮模型、多工具、超时、取消、重复、乱序、跨会话、崩溃恢复——替身只给外部响应，状态/事件全部真实链路产生，记录任务/调用/终态计数。
7. 环境红线：不引入 tokio test-util；npm/桌面栈不触碰；生产默认入口不变。

## 现场事实

- T01–T07 各验收测试已存在（run_lifecycle/run_finalize_property/session_serialization/cancellation_tree/late_result_*/request_dedup/invocation_journal*/subagent_*/background_*/recovery_*）；A15 大部分可由既有测试+新组合矩阵汇编，但必须有「调用观测与集成日志」级证据（本 Task 汇编运行，不重写）。
- r02_t04_storage_tx.sh 当前树上 S4 exit 1（已知项）；verify-stage R02 的命令在 stage_maps/R02.json。
- runner_identity.rs 钉 R02.json——加 R03.json 需同步更新该文件（参考其对 R02.json 的处理，含 stale binary 检测文案）。
