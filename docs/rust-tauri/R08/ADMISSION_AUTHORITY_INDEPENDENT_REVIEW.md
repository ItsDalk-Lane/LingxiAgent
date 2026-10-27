# run_code / run_tools 授权边界独立复核

**状态：FAIL_NOT_CLOSED。** 本记录保存本轮独立只读发现、已核实的正向边界及待测条件，不授予 F01/F02 准入通过。用户已明确要求先封授权缺口，再独立复验。实现仍在并行修改，下面判断绑定独立保存的源码快照；后续修改必须重新核对和实测。

审查者：独立代理 `admission_evidence_review`，未实现这两个业务入口。记录日期：2026-10-22（Asia/Shanghai）；精确 UTC 时间见配套 JSON。基点 `86840d1700397d3dd620a41f2b884df4cddd5cc2`，分支 `codex/rust-tauri-migration`。该基点不是全部未提交候选的替身；13 件审查输入的逐文件摘要和不可变副本见 [inputs.json](../../../artifacts/rust-tauri/R08/authority-independent-review-01/inputs.json)。该源码快照审查时未运行Cargo；随后获主集成窗口完成默认组合根独立实测，见下文接续记录。

原准入结论仍见 [R07_ADMISSION_REVIEW.md](R07_ADMISSION_REVIEW.md)，错绑同因范围见 [ADMISSION_F02_SCOPE_REVIEW.md](ADMISSION_F02_SCOPE_REVIEW.md)。本记录补充授权链问题，不改 R00 原断言、阶段份额或历史回执，不重开已接受 R06、不转移到 R09。

## 原要求与证据口径

原文来自 [R00 FEATURE_STAGE_ACCEPTANCE.json](../R00/FEATURE_STAGE_ACCEPTANCE.json)，两条完整原叶亦复制到独立输入回执；不得按现有案例改写原断言。

| 原叶 | 原断言 |
|---|---|
| `R00-T02-LA-C88F29B5114A` run_code | 两次调用共享变量；重启后旧变量消失；取消清理内核；语言无效、无会话、内核死亡或超时显式失败，重启不声称保留旧状态 |
| `R00-T02-LA-0818574ABD43` run_tools | 每个子调用独立权限判定；外层只显示打印/返回；失败显示已执行子动作；未知工具、自调用、无权限、脚本超时或沙盒异常拒绝，已执行动作不因外层失败自动回滚 |

配套约束继续沿 [Rust/Tauri公共约束](../../../Lingxi_Rust_Tauri_Taskbooks_2026-09-23/03_功能与所有权矩阵.md)、[验收协议](../../../Lingxi_Rust_Tauri_Taskbooks_2026-09-23/05_验收与性能协议.md) 和 [R08原任务书](../../../Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R08_旧Electron客户端接入与数据迁移演练.md)。本轮重点是新入口不得绕开既有身份、工作流写目录、逐调用审核、撤权、取消、日志和恢复边界；不是要求重做原阶段。

实现者局部证据与独立验收分开使用：

- [run_code实现说明](RUN_CODE_IMPLEMENTATION.md) 与 [历史receipt](../../../artifacts/rust-tauri/R08/run-code-implementation-01/receipt.json)：历史四份日志摘要重新核算一致，包含旧候选 8 项通过及新增 Node await/import 后的 1 项定向复验。当时两份源摘要曾核对相符。**当前授权修复已改动实现和测试，两份当前摘要均已不同，旧绿不能覆盖新候选。**
- [PTC实现说明](PTC_IMPLEMENTATION.md) 和旧局部日志：`rust-test-03.log` 确有 7 过/0 失败；`rust-test-02.log` 的 6 过/1 失败保留。当前新增预算超时测试和新的父驱动授权修改不在旧7项证据覆盖内。本审查未重新执行这些套件，也不凭日志总数替原断言验收。

## run_code：源修复可见，但未独立关闭

### RC-A01｜工作流限制：原绕过确认，当前修复待实测

旧候选直接由 `ProcessSupervisor.spawn` 启动持久 Python/Node，未消费 `workflow_scope`。已有 `exec_command` 会在工作流中拒绝 PTY/无隔离执行，并把写目录求交；新解释器入口可绕过该已生效限制。这是新接线必须封住的同因授权问题。

当前快照在 `coding_run_code.rs::run` 已有非 status 的工作流拒绝，错误为 `RUN_CODE_WORKFLOW_UNSUPPORTED`；新增测试 `real_workflow_write_roots_cannot_be_bypassed_by_persistent_interpreter_run_or_restart` 从真实 workflow 节点进入，断言兄弟文件不存在、无内核启动、run/restart明确拒绝、status不启动。**这只证明修复与测试已在源码，尚未证明本轮独立运行通过。** 应同时核直接入口和 PTC 转调，不让外层包装丢失工作流范围。

### RC-A02｜目录一致性：待证问题，不冒充已复现漏洞

内核槽位按主体/Agent/session/language 保留，工作目录沿首次启动保存。执行时会重新授权 `kernel.cwd`，但未明确比较它与本次网关准备/冻结的工作目录是否一致。若真实会话工作区切换能到达这一情形，应拒绝或明确重启，而不能在展示新目录的批准下静默写旧目录。

本轮尚未从真实入口复现目录切换到达路径，故将其记为**独立测试缺口**，不虚报已经越权。需要实际切换、准备后换链接和撤目录权限的样本，核实际文件效果与内核状态。

### 已确认的局部正向边界及其限制

- 旧局部测试不是纯假 manager：Python/Node 真实解释器验证变量、import、函数跨调用和重启丢变量；真实 Run 取消 Python 与实际孙进程，检查两个 PID 消失；真实死亡拒绝静默重建、非法语言/不存在会话、实际30秒超时均有具体断言。
- `trusted_settings_context` 核实际主体、会话、Run、尝试和调用；槽位包含主体/Agent/session/language；`status` 经 Read，`run/restart` 经 Execute；解释器使用显式环境白名单。
- 局部套件手工调用 registrar 并注入自有真实 registry/gateway/supervisor；当前 `lib.rs` 能看到正式真实模型面＋workspace 内的默认登记。**登记源码存在不等于默认装配已经实测。**
- 历史测试在同一活 Storage 下调用 `ModelHistory.load` 两次，并读文件确认效果只发生一次；这是有效的历史读回证据，但没有关库或重启服务，不代领 R08 的真实持久恢复义务。
- 撤回一条早先提醒：旧 TypeBox 外部 schema 也要求 `code`，不能把新 schema 的相同行为判作新缺陷。普通交互解释器是否有 OS 隔离沿原执行权限语义，本审查不据此扩大为全阶段阻断。

## PTC：必须保留每个子目标的原边界

审查的是 `runs.rs::execute_program_child`、真实父 `spawn_linked_with_settlement` 注入 `PROGRAM_AUTHORITY` 的链，以及 `coding_ptc.rs` 如何回调父驱动。不是仅检查脚本返回值。

| 问题 | 快照中的实际状态 | 必要关闭条件 |
|---|---|---|
| **PTC-A01：早期拒绝独立回执** | 新增 `unresolved` 请求和 `refuse_child` 宏，部分未知/目录/来源拒绝可写独立 intent/receipt；但外围仍在 `name==run_tools` 时直接返回 Err，不进入 driver。其余 `?` 提前返回路径也需逐项核。 | 可信父下未知名称、直接/别名自调用、schema无效、失效说明、来源撤权均有各自 `dispatched=false` 回执及用户结果。伪造父上下文本就应零业务事实，不能为凑回执伪造合法调用。 |
| **PTC-A02：delegation未闭合** | 专用驱动暂以 `PTC_DELEGATION_REQUIRES_DIRECT_DRIVER` 明确拒绝，避免冒充普通executor；安全拒绝有用，但不是all-tools语义通过。 | 复用原子任务专用驱动，核谱系、独立权限、取消、子Run/收据不重复；不得修改原断言、隐藏目标或挪阶段取绿。 |
| **PTC-A03：自动审核与守卫缺层** | 主入口先做可取消 `review_and_validate_from_request_pinned`，还有 `on_tool_call` 守卫；helper只做prepare，父run_tools的守卫不能代替每个子调用。 | Phone/Cron/Heartbeat Ask 的每子动作走原审核允许/拒绝及取消；当前schema、禁用/变代、工具守卫均真实验证。 |
| **PTC-A04：Started后的撤权窗口** | helper在Authorized后复核Bridge/phone/workflow，随后await写Started，之后只查cancel；主链在Started写后再检查来源。 | 在Started写入等待窗口实际撤权，释放写入后目标零派发；批准等待期间撤权、别名及目录桥包装也不绕过。 |
| **PTC-A05：取消先到的结果围栏** | helper仅比较结果身份fence相等；缺主链 `CancelledBeforeWrite` 与 `audit_late_result` 的对应处理。 | 取消与结果同时ready、取消在结果写前先到：不写成功，已执行但未知如实保留，晚结果审计，不重复副作用。 |
| **PTC-A06：逐目标配额及收尾监督** | 子调用直接await gateway，无独立Tool配额/子scope及目标专属settle策略；父run_tools普通call可直接drop。browser/knowledge/web/html-to-pdf原可协作收尾的动作可能被提前丢弃。 | 配额1时不越额、不因父持许可自锁；明确编排许可与实际目标许可关系。需要结算的目标保留原期限和停写许可，实际结束才释放，不能只确认Node退出。 |
| **PTC-A07：日志/重放/模型可见范围缺实测** | 有源码反重放保护和局部文件/Ask拒绝/外层失败/单个历史外层结果；尚无默认装配、身份撤权矩阵、崩溃重开与副作用重放证明。 | 逐崩溃点恢复；同子seq无再次写/外发；原始子材料可供界面查看，实际模型请求及历史仅消费打印/返回，父子稳定ID一致。 |

主集成已说明正在修 Started后撤权、迟到结果、许可和监督；本快照记录这些仍未独立关闭。**补配额不能机械叠加：父run_tools若持全局唯一permit，子调用再申请同许可会自锁。** 应复用既有调度语义并用上限1的真实反向样本验证。

### 已确认的正向源码边界

1. `PROGRAM_AUTHORITY` 私有，只由真实父工具任务注入；脚本参数不能提供主体、Agent或权限。helper比较完整ctx/parent_call，并核父journal为Started、owner/attempt/generation相符、子序号尚未存在，拒绝重派发已有事实。
2. 别名/目录桥归一后，按真实 `prepared.local_name` 检查phone、guest及workflow允许范围；非驻留目标消费本轮实际正式说明，不信任程序自报代次。
3. 子调用仍经 `execute_prepared` 的单用handle、当前目标/代次、资源、review和MCP临时授权复核；这些是有效改进，不因仍有缺口而抹掉。
4. 测试源码包含Ask样本：先批准父run_tools，再期待不同approvalId的write子批准并拒绝，检查文件不存在；历史 `rust-test-03.log` 有同名案例通过，但没有该轮完整源码绑定，且当前父驱动继续变化。本轮仅确认源码中的独立批准结构，不将旧日志认证为当前候选实测；仍待独立执行及撤权/变代、各身份覆盖。
5. 外围不再仅依赖vm。既有逃逸红证据保留，当前代码加OS边界、自有进程组回收与许可保留。只承认具体受测样本；本记录不扩审整个沙盒或授予全平台通过。

## 独立实际复验接续条件

先由实现方完成授权修复并明确冻结候选，再由本独立审查者运行；不可让实现者的自测替代独审。Cargo窗口由主集成协调，本轮未抢占或排队构建。完整逐问题检查清单在 [配套JSON](ADMISSION_AUTHORITY_INDEPENDENT_REVIEW.json)。

1. 绑定冻结候选的完整必要源文件摘要、旧失败和新日志；新证据目录不可覆盖旧轮次。
2. 独立运行run_code原叶及真实workflow反例；补默认组合根、批准/目录变化和重启后历史读回，分别说明原叶和R08集成责任。
3. 独立运行PTC原叶、原预算超时入口和各身份拒绝/撤权、当前schema/代次、配额/协作收尾、崩溃重放；只替代外部协议，核心、真实存储及实际文件/进程效果须保留。
超时复验必须消费原预算入口并证明到期后的拒绝/保留效果/清理；不新增“每轮机械等满10分钟”的判据。可采用可信等价计时控制，只有默认真实期限本身待证时才等待该期限。

4. 按原断言分别记PASS/FAIL/BLOCKED，不以套件退出0或绿色数量代领未执行分支。授权缺口与必须项没有实际关闭前保持FAIL；仅源修复可称待复核。

本轮只新建本报告、配套JSON和独立源码/回执证据，未改实现、原账、用户数据或旧日志，未提交/推送/发布。

## 接续：默认组合根已独立实际复验

2026-10-22获冻结候选和Cargo窗口后完成两轮实际复验，结果见 [DEFAULT_COMPOSITION_INDEPENDENT_REVIEW.md](DEFAULT_COMPOSITION_INDEPENDENT_REVIEW.md)。五目标真实登记/准备、默认模型入口run_code/security_scan/run_tools、实际子日志/外部模型投影及关库重开不重复效果已在334件来源稳定的候选通过。这是本组明确边界的独立PASS；本报告列出的完整授权/撤权矩阵、协作收尾、崩溃重放和delegation等尚未由该宿主验证，整体继续FAIL_NOT_CLOSED。原快照判断和旧回执保留，不将旧源码结论强套到新的修复。
