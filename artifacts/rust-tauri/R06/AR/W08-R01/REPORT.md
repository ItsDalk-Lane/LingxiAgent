# R06-AR-W08 / F08 全新独立验收

VERDICT: PASS

此结论只接受固定候选的 **R06-AR-F08 当前完整修复份额**。本次没有新增未关闭的 F08 缺陷。R06 阶段仍未通过；不接受其它 F-ID、EX-ID、原 Task 的完整业务迁移或全部原叶。既有 F14 登记门禁已亲跑失败，仍由原到期 Task 处理。

Reviewer 是本 Task 全新一次性独立角色，未执行本修复、未参与上一轮审查，不复用执行者结论。本次只读产品/配置/原测试，没有 git 写、修复、提交、用户设置访问或真实资料迁移。所有输入为普通合成材料，仅在 loopback 模拟外部模型协议回复；没有外发、付费服务、账户/系统权限变化或 R07 工作。证据仅写 W08-R01。

## 固定候选与执行边界

- 固定根：`/Users/study_superior/.codex/worktrees/r06-w08-review/LingxiAgent`；base HEAD `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`。
- 四 owned 文件完整 SHA 在 [final-binding.json](final-binding.json)。排序的 `path NUL sha LF` 聚合 `55c32488cc123116c9c91ec514a2daa0215a9dc1e575d9b7ef13d448f514df06` 与绑定相等。
- 原固定 `rust/Cargo.lock` SHA `d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4`；不含 W06 依赖。执行者 JSON 算法摘要 4ec… 与历史组合锁 9e91… 均不是本次候选/算法/锁。
- 356 个 Rust 非 target 文件开始、结束全部相等；四 owned、lock、9 个实际候选可执行文件最终再次核对相等，见 [end-binding-check.json](end-binding-check.json)。目标目录只使用固定副本自己的 `rust/target`。实际 service SHA `149a00f8ec86e66ed14af3346839f405be6807fc73732c3e9c4c71b1e39afc9f`。
- Cargo `/Users/study_superior/.cargo/bin/cargo` 1.98.1；所有 Cargo 编译/测试/运行采用 `--locked -j8`，fmt 是不编译的格式检查。实际 Rust 测试与真实 service 日志都显示 8192MB/600s guard armed；父 Python/exec 监督没有关闭。Python 外层自身无 guard marker 不等于实际 service 未守卫。
- 每个命令的实际 cwd、命令、起止、退出码、独立合成 LINGXI_HOME、源与可执行文件摘要在对应唯一 `rNN-*.json/.log`。业务状态的 data_home、库位置及真实 binary ready line 证明生效。Darwin arm64 本机结果，不等于其它平台/正式包/LIVE 验收。

## 已阅读与原范围裁决

已完整读取 AGENTS、不可变完整规格（含 F08 四 C、边界、§6—13 角色流程）、Reviewer 模板、原 R06-T02 四 Steps/三交付物/A03/A04 和 R06-T06 五 Steps/三交付物/A11/A12、相关通用架构/交接/验收约束、SCOPE-01 相关到期原叶。W08-E01 REPORT/results/manifest 仅作找证据线索。

[original-19-clause-review.json](original-19-clause-review.json) 逐一保留 19 个原 ID、原 owner、原截止、完整期望、本次实测/推断/未验。当前必须完成的 F08 份额是 T02-S02 工具输入预算、T02-S03 工具对/身份、T02-S04 原始记录与明确失败、T06-S04 hooks 的成功结果警告分支。这些份额本次全部实测通过。T02-S01、其它整体预算/压缩交付、T06-S01/02/03/05 与全部原业务交付没有因这次修复被关闭；A03/A04/A11/A12 的原有本机回归本次实际通过，完整阶段集成义务仍 OPEN。

[original-101-leaf-review.json](original-101-leaf-review.json) 完整保留 101 个原 T06 叶的原 ID、owner、原截止、期待/原断言、当前关联与后续依据。**完整叶 PASS 数为 0**：关联套件通过、共同投影机制源码可达、若干记忆/技能断言实际执行，均不冒充每叶完整业务验收。其它 F05/F06/F07、EX03/04/05/06/12 等原剩余内容留给原当前到期 Task，未改名延期或移到 R07。[upstream-relevant-shares.json](upstream-relevant-shares.json) 保留 90 个原 upstream R06 份额；显式原映射到 T02/T06 为 0，不因此豁免共享事件消费者的影响。

## 生产调用链与根因全族

源确认：`runs.rs:1268` 同一宏统一成功/失败结果；9 个闭合入口均先权威持久化 raw 完成事件，再进入该宏。唯一 `exchange.push(ToolResult)` 在 `runs.rs:1363`。正常生产 bootstrap 于 `lib.rs:1510` 启用扩展 hooks。

| runs.rs 行 | 真实入口 |
|---|---|
|2459|批次拒绝；含前置 loop guard 阻止路径|
|2582|参数摘要拒绝|
|2673|Gateway prepare 拒绝|
|2804|子代理运行授权拒绝|
|2901|批准不可用|
|3070|批准拒绝/中止|
|3266|子代理委托结果|
|3444|Gateway 执行拒绝|
|3528|实际工具执行结果|

`hooks.rs:236` 的 `project_tool_success` 消费已经 guarded 的内容，警告最多 2048B，警告与截断说明计入每成功结果的 **实际 UTF8 渲染总量 32768B**；Text/Reasoning 单块先按 L1 32KiB 守卫。`memory.rs:1131` 头尾窗口在 UTF8 边界内截断。resource/opaque/进程身份保留；固定身份自身超过预算时明确返回模型投影 InvalidMessage，raw 成功档案仍完整。错误结果保持原不截断规则。

真实共同 renderer 位于 `lingxi-adapters/src/models/tool_render.rs:32`，模型协议实际请求消费 exchange。`runs.rs:1336—1360` 经现有正常事件存储提交新 `tool_result_model_projection`，再进入 exchange；实测 rawEventId/toolCallId/providerCallId/digest/result/resource/process/truncated/renderedBytes 与数据库和下一 wire 一致。该事件描述模型上下文投影，不代表供应商已处理/已接收的回执；判断实际发送仍依据捕获 wire。

`history_projection.rs:137` 保留未知扩展事件于事件面、不作为普通历史项；`history_projection.rs:204` 工具卡来自原 ToolCallCompleted。因此 raw 工具卡不是“模型看到全部 raw”的证据。新事件没有实现 F01 跨 Run 模型历史恢复。

## F08 四 C 与真实入口实测

[r13-four-c-binary-bound.json](r13-four-c-binary-bound.json) / [日志](r13-four-c-binary-bound.log) 亲跑 7 个测试，exit 0；其中四 C 和 error 是真实 ServiceState.bootstrap → SessionService.execute_for → RunSupervisor → Gateway → 存储/消息组装 → 下一实际 provider 请求；另外 2 条是辅助边界测试，不能替代四 C。

|ID|真实输入与实际结果|结论|
|---|---|---|
|F08-C01|真正 read 普通 >32KiB 文件；下一 wire 26302B，有头尾/省略说明，truncated 与档案一致|PASS|
|F08-C02|真正 read 注入警告大文件；原中段唯一标记存在于 raw，下一 wire 26451B 中不存在，警告保留|PASS|
|F08-C03|经真实 Gateway 执行的合法外部合成多块 Text/Resource 响应连续三轮；业务 loop guard 真正触发。wire 各 26363/26363/26398B，资源身份及警告保留|PASS|
|F08-C04|真实 read/raw 完成事件、投影事件、下一 wire、后续 history 查询按 ID/seq/digest/截断属性对账，明确各自来源|PASS|
|error 原合同|两轮真实失败结果的 wire 42055/42180B，失败序列警告保留，未无依据截断错误全文|PASS|

C03 的外部响应替身仅提供合法合成工具材料；当前 guard、Gateway、Run、存储及消息构造没有 mock。C01/C02/C04 是真实普通文件工具 read。实际输入全文、provider 请求、事件保存在 [observations](observations)；[independent-evidence-reconciliation.json](independent-evidence-reconciliation.json) 另用 Python 亲读 8 个真实 SQLite 库，独立复算 canonical SHA/seq/调用关联/投影与 wire，而非只看 PASS 日志。

入口补强：真正独立 service 可执行程序、真实配置 bootstrap、生成的隔离本地认证 token、真实 HTTP execute、真正文件 read、真实库、下一 provider wire 均亲跑通过，见 [binary-observation-r19.json](binary-observation-r19.json)、[r19 命令](r19-binary-input-bound.json)。4 次实际模型请求，3 组 raw/投影档案，对应实际工具身份正确；第一次大结果 26451B 并保留注入警告，第二/第三同文件 read 按现役生产去重规则返回小结果，第三轮真实循环警告存在。不能把该 binary 试验声称“第三轮仍是大 read”；大多块循环由 C03 单独证明。SIGTERM exit 0、子进程已收割、binary 前后 SHA 相同。

同 synthetic home 干净停服后重新启动真实 binary，只读认证历史导出/投影档案，内容相等且没有新模型或工具 replay，两次实际 guard 都 armed。该结果只验归档可重开，不关闭 F01，不验崩溃恢复/未完成 Run 重放。

## 独立新增边界、存储失败与红绿对照

[reviewer_probe_input_bound.rs](reviewer_probe_input_bound.rs) 仅为独立探针，不改候选。它直接链接固定副本用 locked Cargo 构建的真实库；编译命令/日志独立保存。 [r18](r18-reviewer-input-bound.json) 3/3 PASS，实际候选链及下一 wire 未被 mock。

1. 巨大 Reasoning 含控制字符/中文与注入字样，同时 opaque 签名、remote resource、running process handle。真实 wire 总量受限、中段消失、警告和所有身份保持；raw 完整。
2. 不可截断 opaque 身份 40000B 超量。真实 wire 明确 `tool error [invalid_message]` 和 raw archived，投影 failed；原成功 raw 与身份全文不改。
3. 真正 SQLite 插入触发器对新投影事件 `RAISE(ABORT)`。execute_for 返回实际 Storage 错误，raw 完成档案留下，投影事件没有落库，只有最初一次 provider 请求、没有下一请求。既有 RegistrationGuard (`runs.rs:507—545`) 的存储异常合同保持最后 durable running 状态并撤注册/取消，不编造终态；未完成 Run 的重启恢复另属原 R03/F01 义务。

上述输入在 execute_for 前保存了完整材料与 canonical 内容 digest，真实 binary 文件 SHA 也在起进程/提交前保存；[input-before-reconciliation.json](input-before-reconciliation.json) 独立复算与真实 raw/文件一致。[independent-input-bound-reconciliation.json](independent-input-bound-reconciliation.json) 再亲读新三个真实库对账。

[old-red-validity.json](old-red-validity.json) 亲核执行者 attempt04 正常大结果有效 exit 0、attempt05 生产旧 warning 分支有效行为红 exit 101，均实际 1 测试，原三源 SHA 等于 base，不是不编译/0 匹配。该历史证据使用组合锁，明确不冒充本次固定锁亲跑。

另亲跑完整旧根因隔离变异 [r04](r04-old-root-mutation.json)：独立临时源码和独立 COW target（inode 不同），恢复 warning 非空时直接拼 raw 的旧成功分支，**移除该分支新总预算投影保护**，保留新事件以单独暴露旧根因。不是只把第一 guard 参数改成 raw。真实 C02 跑 1 条、exit 101，因被省略中段进入下一实际 wire 而红，候选相同 C02 绿。候选源未改。变异源 SHA/根目录见 [mutation-path.json](mutation-path.json)，可执行 SHA/inode/清理见收据。

失败历史没有隐藏：probe-compile 初始若干失败是 rlib 元数据版本不配；compile07 是新增证据输出错误地要求 ToolSuccess 可序列化，compile08 改为序列化实际内容字段后成功，均未改变产品。r03 独立探针误预期“请求已受理”，实际 Storage 错误正确；r05 误等终态，违反现役存储异常最后 durable 状态合同；r06/r18 按原合同改探针后 3/3 通过。r07 binary 探针误要求每次重复 read 仍返回大文件；原生产去重导致该 oracle 失败，r10/r16/r19 按真实既有去重规则验证并通过。所有日志和失败观察保留；这些探针错误不是旧根因行为红，未冒充产品失败或抹去历史。

## 受影响回归与门禁

|亲跑范围|结果与证据|
|---|---|
|kernel-lib 122 / compression 44 / service-lib 387 / T02 binary 2 / compaction 26 / 原 hooks 4 / memory 7 / skill context 4 / skills 6|602 PASS，r02；r17 再绑定运行前后实际可执行 SHA 后 602 PASS。重复不累计成独立覆盖数|
|F08 套件|7 PASS，r01/r13，独立新边界另 3 PASS r06/r18|
|adapters-lib 119 / service-lib 387 / R04 Gateway 13 / R05 model plane 24 / protocol adapters 12 / closed loop 11 / R06 history 6|572 PASS，r14。service-lib 与前表重复，不假加成新的 572 项|
|fmt / clippy workspace all targets -D warnings / xtask check-contracts / check-boundaries|exit 0，r08/r09/r11/r12|
|原 Python R02 登记门禁|**FAIL exit 1**，r15：迁移 9 条，登记 7 条；缺 v8/v9。源码/登记/脚本都与 base 原样相同，本修复未引入|
|完整 verify-stage R06 / 前序 R05→R04→R03→R02 正式闭包 / 全平台 / 正式包 / LIVE|NOT_RUN；本证据不赋予阶段 PASS|

原 `r06_t06_hooks.rs` byte SHA `e4b2f8257c7f61d732da85830402c87404500accb01232700acd3a7ece803e2f` 等于 base；四原名称、断言未减少，亲跑通过。测试数只说明命令实际执行，不能关闭 101 原叶。

## 影响与仍未关闭的问题

当前 F08 必需项（全部四 C、错误原规则、真实入口、同族额外反例、完整旧根因红、投影持久化失败、必要回归、候选绑定）均已满足，没有新增 W08 阻断项。

关联既有问题 **R06-AR-F14 / W08-R01-GATE-01**：违反原 F14 必修1/2/4 与 F14-C01；位置 `docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json` 和 `lingxi-adapters/src/storage/migrations.rs`。复现为原 Python 校验 r15，实际 9/7 且缺 v8 r06_t03_session_tree、v9 r06_t05_session_files_resources；期待名称/版本/SQL摘要严格相等。根因是新增迁移未按原机制登记及 R06 正式图未完成。原 F14 需要原 SQL/inspector 收据登记并完整前序门禁及负向电池，必重跑 F14-C01—C04 和 FINAL-R06；禁止本 Reviewer 修复、降低门禁或关闭该问题。该问题保留原 F14/R06-T08/R02-T04 owner 和 R06 原截止。

新投影事件影响 F01/F04 与 R02—R05 的存储/事件/usage/历史/模型消费链：本次相关 source 审查与局部回归通过，**完整最终集成仍 STALE**，必须由相应当前 Task 与 FINAL-R06 在最终组合候选重跑。F01 跨 Run 上下文消费仍未实现，不能用登记新事件或本次只读重开冒充完成；F04 分支历史/租约等完整验收不被本次关闭。并发取消/崩溃恢复/跨 Run 重载的完整电池没有新增亲跑；本修复没有增加权限或跨 Run 恢复机制，宏的存储先于 exchange 亲测，原串行生产调用及 guard 语义作源码判断，其它完整竞态仍留原 F03/F04/R03 到期验收，不写 PASS。没有凭此豁免本 F08 存储失败或归档可重开。

## 清理与交付

[cleanup-receipts.json](cleanup-receipts.json) 保存 19 次本 Reviewer 合成 home、实际文件/库 SHA 与逻辑表计数、变异 target 独立 inode/二进制摘要、两个探针可执行文件清理收据。真实库先亲读对账再清理。未删除用户资料或其它任务目录；固定候选源/target 保留。没有复制整套源码/.git/target 到证据目录。报告完成后结束本一次性 Reviewer，不自行 Repair/提交或接下一任务。
