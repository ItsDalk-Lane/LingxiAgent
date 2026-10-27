# R04 报告｜统一工具网关、四基础工具与沙盒（阶段报告，91 模板）

> **RR1 再接受附录（总控，2026-10-01）**：2026-10-01 对抗性复审（基线 `773d5a696`）确认五组
> 缺陷 F01-F05 并裁定先前接受依据（T05-R1-OBS-2 非阻塞登记等）不成立。本轮定点修复
> G01→G05 全部经"红复现→根因修复→两层自查→全新独立审查 R1 PASS"闭环收口，并由全新
> STAGE-REVIEWER-R04-RR1 阶段重审（五组反例独立旧红新绿、原 16 A-ID 门禁、内嵌+独立 R03
> 回归、R02 七链、七项接受依据纠正）**STAGE_VERDICT: PASS**。修复链：G01=`1692d2314`
> （F01 容量预留/RAII/回滚）、G02=`da15c4bd9`（F02 泵收口/归属门/reap 竞态）、G03=`614fab1af`
> （F03 StopUnconfirmed 全链保真）、G04=`1285c3bf6`（F04 字节级消费+F05 输出完整性）、
> G05=`629e15b85`（RR1 五场景入 R04 阶段门禁，24 场景全 REQUIRED，负向 9/9+4/4）。最终候选
> `629e15b85`，全部已普通推送并核对远程包含。现行结论：**R04 RE-ACCEPTED AFTER
> ADVERSARIAL REPAIR**；不启动 R05。明细：`repair-current/R04_RR1_FIX_ISSUES.json`（账本+
> 回执）、`R04_RR1_STAGE_REVIEW.md`（阶段重审）、`R04_HANDOFF.json` reaccepted_after_
> rr1_adversarial_repair（含七项纠正后的接受依据）。以下为 T08 执行代理时期的历史原文。

> 本报告由 R04-T08 执行代理（E02）在完成最后一个执行 Task 时按 91 模板编写；
> 阶段组合独立验收（全新阶段 Reviewer）**尚未发生**——本报告不是阶段 PASS 声明。

阶段与结论：**IN_PROGRESS（收口中）**——R04-T01..T07 各自经全新独立 Reviewer PASS 并由
总控 commit/push 确认（远程包含）；R04-T08 READY_FOR_REVIEW（两层自查完成，待独立任务
验收）。阶段组合验收与 `R04 ACCEPTED` 判定未开始。

范围：R04 仅本阶段八项（工具目录/参数契约、唯一执行网关、批准/撤销/只读、原生 read/write/
edit、命令/PTY/进程树取消、跨平台沙盒、MCP+worker 接入、工具结果/产物/全矩阵验收）。
与基线差异及获准 ADR：无新增 ADR；依赖沿用 R01 锁（rmcp 3.4.1 为 T07 前已进锁项）。

源码：
- 起止：`bf6450bcd`（R03 再接受点，R04 起点）→ 已提交 HEAD `ded3467bf`（T07）+ **工作区
  未提交的 T08 进展**（T08 通过独立验收后由总控提交；提交将包含
  `refactor(rust-tauri): R04-T08 verify tool execution and handoff`）。
- 已提交任务链：T01=`fd5fc2c2c`；T02=`1a1602c33`+补 `39168bfb0`；T03=`c72e0fa02`；
  T04=`80b4edbf5`；T05=`84e6499fe`；T06=`26251dc92`；T07=`ded3467bf`（全部在
  `codex/rust-tauri-migration`，远程包含已由总控确认）。
- 工作树摘要：T08 未提交改动清单见 `R04-T08_REPORT.md` §3（另含会话开始前已存在的任务书
  未跟踪目录与旧任务书删除——总控启动时登记的 initial_worktree，不混入 Task 提交）。
- 依赖锁摘要：`rust/Cargo.lock` 本 Task 零改动（--locked 全程）；工具链 rustup 1.98.1。

环境：macOS darwin 27.0.0 arm64；`/Users/study_superior/.cargo/bin/cargo`（rust-toolchain.toml
1.98.1 经 rustup 生效）；debug 构建；测试数据=每测试 `std::env::temp_dir()` 隔离目录+合成
fixture 子进程（r04_t07_fixture）+受控 rmcp stdio server；无真实外部账户（Provider 双仅
扮演 R05 模型发请求）。

完成项（逐 T-ID，关键实现与代码定位）：
- **R04-T01** 工具目录与参数契约：`lingxi-kernel/toolcatalog.rs`（manifest/registry/
  generation/别名/schema 预算/fail-closed 校验）；完整有效参数与结构化结果契约（ports.rs）。
- **R04-T02** 唯一执行网关：`lingxi-service/toolgateway.rs`（两阶段 prepare→execute、
  PreparedInvocation 服务端铸、执行前重验、无第二收据）；驱动 RunGrant 唯一授权点不变。
- **R04-T03** 批准/撤销/只读：`approval_service.rs`（operate/ask/read_only、预授权单次、
  ask 子代理 TOOL_APPROVAL_UNAVAILABLE——R03 递交义务 SUP-01 关闭）。
- **R04-T04** 原生文件工具：`filetools.rs`（read/write/edit、原子写、陈旧冲突、符号链接
  真实目标判定、ResourceAccess）。
- **R04-T05** 命令/PTY/进程树：`exectools.rs`+`procsupervisor.rs`（结构化 argv、write_stdin
  持续终端、进程组/孙进程有界清理、取消可观察）。
- **R04-T06** 跨平台沙盒：`sandbox.rs`（seatbelt/bwrap/Unsupported fail-closed；缺
  helper/错版本响亮拒绝不裸跑）。
- **R04-T07** MCP+worker：`mcpbridge.rs`（rmcp stdio、不可信输入处理、断连重连不盲重发）
  +`workerrpc.rs`（最小单操作、CSPRNG id、期限/取消/预算、宿主模型端口诚实拒）。
- **R04-T08**（本报告主对象）工具结果/产物/全矩阵：`artifactverify.rs`（产物核验双层）+
  网关登记审计+全矩阵测试与生产者+R04 stage map 正式门禁+遗留关闭（详见
  `R04-T08_REPORT.md`）。

行为变化：对用户**不可见**（Rust 工具面仍是显式 opt-in 组装；生产默认入口/旧 Node/Pi 栈
零改动——A16 保护语义）。这不是新增功能的理由：本阶段交付的是 R05 消费的工具执行底座。

验收（16 A-ID→命令/对象/预期实际/退出码摘要；逐项明细见
`R04_ACCEPTANCE_LEDGER.json` 与各 Task 报告 §4）：

| A-ID | 生产者（真实链） | 结果 |
|---|---|---|
| A01 目录与执行同源 | toolcatalog 单测+T01 套件+matrix family-count/身份格 | PASS（T01 轮） |
| A02 陈旧目录拒绝 | T01 套件+matrix generation-refusals=0 | PASS（T01 轮） |
| A03 各入口权限不变 | T02 套件（13）+matrix 35 格+route-consistency | PASS（T02 轮） |
| A04 伪造凭证失败 | T02 套件 r04_a04_*+对抗族 | PASS（T02 轮） |
| A05 批准后改参 | T03 套件（waiting+预授权形态） | PASS（T03 轮） |
| A06 等待期禁用 | T03 套件（disable+uninstall 零执行）+matrix | PASS（T03 轮） |
| A07 并发编辑不覆盖 | T04 套件全链+matrix edit-conflict | PASS（T04 轮） |
| A08 符号链接不逃逸 | T04 套件（真实目标判定） | PASS（T04 轮） |
| A09 孙进程清理哨兵存活 | T05 套件（真实 cancel_run+ESRCH 级 reap） | PASS（T05 轮） |
| A10 PTY 不退化 | T05 套件（输入/读取/resize/中断/退出） | PASS（T05 轮） |
| A11 缺沙盒不裸跑 | T06 套件（helper 缺失/换版/不支持后端） | PASS（T06 轮） |
| A12 隔离实际成立 | T06 套件（真实哨兵：fs/回环网络/env；macos-arm64 真机） | PASS（T06 轮） |
| A13 MCP 重连不重复 | T07 套件（断回执→Unknown→恰一次重连→计数=1） | PASS（T07 轮） |
| A14 worker 不绕权 | T07 套件（凭证/越界/白名单/主服务可用） | PASS（T07 轮） |
| A15 空产物不算成功 | T08：a15 双测试+artifactverify 单测+8 图钉案例 | PASS（T08 自查） |
| A16 禁用覆盖所有路线 | T08：a16 六路线 holes=0+历史保留+uninstall/generation 腿 | PASS（T08 自查） |

最终候选全量重验（T08 收口）：`cargo test --workspace --locked` exit 0（**888/0**——887 为最终前一编译轮数字，
最终候选（attempt3 门禁与 REVIEWER-R04-T08-R01 独立两轮）实测 888 通过；R1 审查 N1 校正）；
`verify-stage R04`（**7 命令全 0**：workspace/矩阵生产者/fmt/clippy/check-contracts/
check-boundaries/内嵌**完整 verify-stage R03 回归**——R03 侧 17/17 场景全 PASS；R04 侧
19/19 场景、124 叶=55 份额 PASS+69 递延、stable=true）exit 0，证据
`artifacts/rust-tauri/R04/T08-E01/verify-R04/`（首轮两轮尝试的失败/部分事实归档于
verify-R04-attempt1/2，见 T08 报告 §5）；9 例门禁负向全拒。

安全与数据：权限负向（read_only/ask 子代理/越界路径/越界声明/伪产物）全部真实执行零
副作用；真实进程树取消与哨兵存活（T05+T08 轻钉）；单写者（网关零授权零存储，journal
唯一收据——SUP-02）；无迁移（数据兼容零触碰）；回退=丢弃未提交改动（T08 报告 §8）。

完整映射：F-ID→T-ID→A-ID→具体测试→结果——`R04_ACCEPTANCE_LEDGER.json`（16 基础+
5 义务）+`R04.json`（124 R00 叶=55 份额图钉+69 递延）；未覆盖集合=R04.json 递延叶清单
（归属 R06/R07/R08/R09，REQUIRED 不丢失）。

已知缺陷：无未关闭的本阶段阻断缺陷。携带项：T05-R1-OBS-2（RegistryFull 孤儿路径，登记
非阻塞，归属后续裁定）；T04 §8.6 NFKC 折叠差异（R06）；r00_management_leaves 防火墙环
境项（间歇，非产品缺陷，处置口径在 T07 报告 §5.1）。

未执行/BLOCKED：无。真机平台验证按 PLATFORM_CAPABILITIES 登记口径（linux 真机/windows
后端拒绝形态归 R09/R10，非本阶段到期义务缺口——T06 冻结口径）。

回退：已演练（T08 报告 §8——工作区级回退，无新增数据丢失面）。

独立审查：T01-T07 各自 1-2 轮全新 Reviewer PASS（rounds 见 ORCHESTRATOR_PROGRESS；
T01/T02 两轮）；T08 待独立验收（本报告编写时未指派）；阶段组合验收待全部 Task PASS 后
由总控另派全新阶段 Reviewer。

下一阶段：R05（模型协议与真实供应商接入）——允许范围/必须输入/不允许事项见
`R04_HANDOFF.json`（allowed_next_scope/not_authorized）。**在 R04 阶段验收与收口完成前
不开始。**

远程/发布：T01-T07 已由总控普通 push 并确认远程包含；T08 与本报告未提交未推送（按
派单禁止，待独立验收后由总控执行）。发布/签名/公证未获准、未执行。
