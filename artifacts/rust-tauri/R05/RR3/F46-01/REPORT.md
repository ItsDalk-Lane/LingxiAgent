# RR3 F46-01 生产修复与自检报告

**状态：SELF_CHECKED，C/F46 新联合独立审查 PENDING。**本人只实施和自检，不独立签收、关闭 F27/F46 或宣布 R05/R06 放行。历史 C-01 旧2/2与超3日志失败证据完整保留。本包只改 `rust/crates/lingxi-service/src/logging.rs`；C两测试/support、A/B、总控文档、锁、配置、原16A/103C、原阈值/160轮均未改。无 Git 写操作、系统过滤变更或真实供应商请求。

## 根因与最小修复

独立阅读 `RotatingLogFile::open/rotate/prune/open_current` 后确认：`prune`合同包含 active 文件，但旧启动路径先裁剪旧集合≤max_files、再创建新active；已有恰3份时返回4份。后续启动仍4，不是已证明无限泄漏；运行轮转原先先open再prune，正常轮转路径不具有此顺序偏差。C提供的线索没有当成验收结论。

修复仅将启动顺序改为 `open_current()?` 后 `prune()?`，与现有轮转顺序一致；不新增资源管理系统，不改变数量3或错误处理合同。启动创建失败不会先删旧日志；裁剪失败仍向调用者返回错误，由原正式入口明确stderr-only降级。

新增3个内联回归：已有恰3份时打开立即≤3、最旧移除/保留两份原内容/新写入；新目录连续6次无写入重开始终有界且序号延续；启动裁剪失败及下一次轮转创建失败显式返回。现有 lossless complete-line rotation、60次写入裁剪、重开不截断、0600/0700、脱敏fanout、非法配置明确拒绝均保留。真实 [source.diff](source.diff) 仅85行差异（82新增/3删除），主体行为只交换顺序。

## 本人真实旧红→新绿

旧源码先存 `logging-before.rs`，本人重新用锁定cargo build旧正式binary，再同一隔离home连续6次 `READY→SIGTERM→reap`。完整vector/PID/UTC/清单见 [reopen-before/result.json](reopen-before/result.json)，实际数 **[1,2,3,4,4,4]**，服务均exit0、探针准确F46 exit1，全部reap/home删除；这是本人真实红，不复用C结果代替本人运行。

修后重建正式binary，同样6轮计数 **[1,2,3,3,3,3]**，全部ready/服务exit0/探针exit0/reap/home删除。最终源绑定复跑见 [reopen-final/result.json](reopen-final/result.json)；早期修后复跑也完整保留于reopen-after。旧binary SHA256 `9628ba09d14e66ab9ba7824b4f3afeb1844e568ae1d66b88f95a57a8a02d119b`；最终 `b180d47abeeecef4e6d677b9501fde0828cd4061f74276d8e5e2a2a9afb33d81`。未用模拟函数替代此启动触发。

[error-explicit-binary/result.json](error-explicit-binary/result.json)另外使用本人临时目录的不可按普通文件删除条目，真实binary报告 `LINGXI_SERVICE_LOG_WRITE_FAILED`/`continuing stderr-only`，原可用服务仍ready且exit0、reap/home删除。错误路径的集合不能保证裁剪成功，本报告没有把它的4条目记为数量上限PASS。完整原始stderr保留；内联回归亦证明轮转打开错误向调用者返回。

## 新正式binary下完整原C负载

本人亲跑原两个同名测试的完整命令（不是部分筛选）：

```sh
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources -- --nocapture --test-threads=1
```

[resources-01/command.json](resources-01/command.json)：**exit0，2 passed/0 failed/0 ignored/0 filtered，335.40s**；新post-restart及owner-steady≤3断言真实运行。C test SHA256 `23b03f56bef900fc0563b3348596e61437995f7c05cc25a5173455f11a23e33f`、support `1f6601122e5d7841f3975ec68ea8c60adb653fc939ca4e65cc3da7565c269274`，前后相同；C-01旧测试摘要与最终摘要不同的历史边界不抹去。

[resources-01/f27-resource-series.json](resources-01/f27-resource-series.json)原始序列 SHA256 `bcc0620794f4384196d74b7844098c063a96947147be48897a1076b9d6a50b7b`；[RESOURCE_SUMMARY.json](RESOURCE_SUMMARY.json)只是原raw自检回读索引，不是新资源权威或独立结论。

- 原160逐轮保持：60预算取消+60错误+15正常+10×512KiB长响应+15真实worker，双会话，100+取消错误义务不缩减。cancelSettled0/dangling60保留为重启前running事实，恢复后消解和零重执行原断言保持，不把预算drop叫普通取消已终结。
- 54正式binary采样点：baseline1/cancel-live6/cancel6/error6/ok3/worker-live15/worker-released15/worker-final1/post-restart1。真实15worker存活每树2进程且ESTABLISHED≥2；释放每树1/TCP0。服务RSS27328–36656KiB、FD15–19；整个服务worker树RSS27328–39232KiB、FD15–22，原400MiB/400FD及增长限不改。
- owner生产进程内补证61点：baseline1+15真实worker和排队兄弟峰值+45连续3×100ms稳态。峰值activeSessionRuns/liveRunIds2、model/tool permit1、modelWaiter1；取消兄弟并join、放行worker并join，45稳态任务/permit/等待全0、live/backgroundIDs空。此生产组合根内owner观测与正式binary进程树明确分列，不宣称从binary内部观测所有Tokio任务。
- 全部home/workspace原清单保留。各采样日志数量只出现1/2/3，最后重启为3，45owner稳态≤3；最后重启日志为service-000003(65358B)、000004(33642B)、000005(0B)，实际正常写入及轮转仍工作。没有.tmp残留；数据库/身份/输入/日志是声明保留的持久数据，不叫泄漏。
- 真实最后service exit0/reap，owner storage close、stub结束，home/workspace/config三根删除成功；raw列清理前清单而非靠删除home猜资源回收。本人随后成功ps回读，所有raw已知参与PID均不在，见RESOURCE_SUMMARY pidReadback。

普通取消/晚到结果恢复引用完整保留C-01/I-MAPPING：本轮C-01两个具名精确测试已亲跑1/1，分别ordinary-cancel-subagent-01与ordinary-cancel-late-result-01；资源预算drop和重启证据不冒充这两个普通取消义务。媒体临时产物/其他恢复的已验引用仍以该映射和其具名证据为准，本logging修复不新签它们。

## 本人测量正反负控与pin兼容

完整原controls已在resources2/2中运行：FD已知3→6、真实TCP双端2→0、3件17B临时文件→空；dead/reaped/bogus PID、空/错身份、缺目录均UNKNOWN拒绝。又将本轮已编译resource测试装备按字节复制到本包隔离目录，仅副本进程PATH收到隔离lsof包装器，系统工具/主源码不改。正常1测exit0→假FD零exit101（invalid live-process sample）→假TCP零exit101（已知双端left0/right2）→无包装器恢复1测exit0。[sampler-negative-isolated/result.json](sampler-negative-isolated/result.json)记录真实vector/cwd/UTC/hash/退出及目标点名。失败留下的控制sleep仅在新独立进程组内清理并回读无成员，没有杀其他任务。被复制装备SHA256 `323456600dc961ee6e4d20b60cf7cf9964526bfcd4153090c7b4e89c0bf2e3a1`，副本逐字相同。

原权威pin-table/cid实际关系已核：27集成suite的固定计数、64 lib各`--exact`；没有logging库总数pin，resources仍2。本人真实`--list`列resources原两身份与当前service358库测试（原355+新3），并亲跑 `stage_map::map_tests::r05_stage_` **2/2 exit0/0 ignored/119 filtered** 核pin/cid镜像。新增logging测试不要求改权威pin/cid，也不把库总数代替功能覆盖。

格式check exit0；clippy `-p lingxi-service --lib --test r05_t08_resources --locked -- -D warnings` exit0；最终源logging内联**9/9，0ignored/349filtered**。fmt/clippy不是资源验收替代。

## 输入与每条命令绑定

macOS27.0.1 arm64、rustc1.98.1/cargo1.98.1，均使用 `/Users/study_superior/.cargo/bin`；根`rust-toolchain.toml`，Cargo编译/测试/clippy均--locked。分支codex/rust-tauri-migration、HEAD b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b。HEAD不代表工作树本轮改动；[FINAL_SOURCE_BINDING.json](FINAL_SOURCE_BINDING.json)存当前source差异/摘要和真实git状态。最终logging SHA256 `495608195d8c7702707b710781a4fb1be836ad6cf5904fef8558d2c1337ca301`。

每command.json有vector、cwd、UTC起止、exit、五项关键输入before/after、当次binary和原输出摘要。[resources-01/runtime-binding.json](resources-01/runtime-binding.json)捕获本轮实际service PID/command、严格磁盘config hash、合成材料脱敏后配置、真实worker和装备/两输入文件hash。扩展322项runtime/contracts/lock/schema/fixtures/pins/阈值清单在RUNTIME_INPUTS_DURING/AFTER中相同、digest `32f9fb59d396e3272a4e7c0a6e2810344632ccefd8435d39857cb43ece1c540b`。该扩展清单在运行中/之后捕获，五项关键输入在command开始/结束捕获；**不谎称整个仓库或A/B/E已冻结**。正式集成/负测/终审仍由总控冻结新候选后另跑。

资料读取：RR3_F46_BRIEF/RR3_BRIEF/RR3_REVIEW_BRIEF、RR1/RR2 MASTER、RR3矩阵/进度/交接、RR2 brief/交接与相关矩阵进度，原R05任务书和05验收性能全文、10-02适用§8与T08-C12/负测边界、C REPORT/I-MAPPING/原raw/负控/定向红与现有生产源码。继承原16A/100+3C/130适用叶/I01-I11/N01-N16及§6.1/6.2，不用本包限定自检降低全阶段规格。

下面包括有效红绿与两项无效尝试，**不把exit0零测试算通过**；完整stdout摘要与vector见每个链接。

| 记录 | 开始UTC | 结束UTC | exit | 原始汇总 |
|---|---|---|---:|---|
| [build-after](build-after/command.json) | 2026-10-07T00:38:24.696996+00:00 | 2026-10-07T00:38:38.479168+00:00 | 0 | 无 test result（命令用途见vector） |
| [build-before](build-before/command.json) | 2026-10-07T00:37:06.302183+00:00 | 2026-10-07T00:37:06.492656+00:00 | 0 | 无 test result（命令用途见vector） |
| [clippy-01](clippy-01/command.json) | 2026-10-07T00:43:26.429893+00:00 | 2026-10-07T00:43:38.876870+00:00 | 0 | 无 test result（命令用途见vector） |
| [error-explicit-command-01](error-explicit-command-01/command.json) | 2026-10-07T00:45:48.902674+00:00 | 2026-10-07T00:45:49.055016+00:00 | 0 | 无 test result（命令用途见vector） |
| [formatting-01](formatting-01/command.json) | 2026-10-07T00:40:14.732612+00:00 | 2026-10-07T00:40:15.884303+00:00 | 0 | 无 test result（命令用途见vector） |
| [logging-tests-01](logging-tests-01/command.json) | 2026-10-07T00:38:07.156701+00:00 | 2026-10-07T00:38:18.932802+00:00 | 0 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 349 filtered out; finished in 0.13s |
| [logging-tests-final](logging-tests-final/command.json) | 2026-10-07T00:44:02.692179+00:00 | 2026-10-07T00:44:02.948612+00:00 | 0 | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 349 filtered out; finished in 0.11s |
| [reopen-after-command](reopen-after-command/command.json) | 2026-10-07T00:38:58.982826+00:00 | 2026-10-07T00:39:00.027568+00:00 | 0 | 无 test result（命令用途见vector） |
| [reopen-before-command](reopen-before-command/command.json) **INVALID** | 2026-10-07T00:37:10.413655+00:00 | 2026-10-07T00:37:10.457483+00:00 | 1 | 无 test result（命令用途见vector） |
| [reopen-before-command-02](reopen-before-command-02/command.json) | 2026-10-07T00:37:26.311901+00:00 | 2026-10-07T00:37:27.170199+00:00 | 1 | 无 test result（命令用途见vector） |
| [reopen-final-command](reopen-final-command/command.json) | 2026-10-07T00:44:03.024781+00:00 | 2026-10-07T00:44:04.005541+00:00 | 0 | 无 test result（命令用途见vector） |
| [resources-01](resources-01/command.json) | 2026-10-07T00:39:00.092394+00:00 | 2026-10-07T00:44:52.354726+00:00 | 0 | test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 335.40s |
| [resources-list-01](resources-list-01/command.json) | 2026-10-07T00:43:35.004369+00:00 | 2026-10-07T00:43:38.971280+00:00 | 0 | 无 test result（命令用途见vector） |
| [sampler-negative-command-01](sampler-negative-command-01/command.json) | 2026-10-07T00:42:28.765183+00:00 | 2026-10-07T00:42:33.260195+00:00 | 0 | 无 test result（命令用途见vector） |
| [service-lib-list-01](service-lib-list-01/command.json) | 2026-10-07T00:41:39.791037+00:00 | 2026-10-07T00:43:34.928014+00:00 | 0 | 无 test result（命令用途见vector） |
| [stage-pin-check-01](stage-pin-check-01/command.json) **INVALID** | 2026-10-07T00:40:09.973723+00:00 | 2026-10-07T00:40:14.664752+00:00 | 0 | test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 121 filtered out; finished in 0.00s |
| [stage-pin-check-02](stage-pin-check-02/command.json) | 2026-10-07T00:41:51.890233+00:00 | 2026-10-07T00:43:26.350974+00:00 | 0 | test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 119 filtered out; finished in 0.02s |
| [stage-pin-list-01](stage-pin-list-01/command.json) | 2026-10-07T00:41:39.659069+00:00 | 2026-10-07T00:41:39.728248+00:00 | 0 | 无 test result（命令用途见vector） |

两项INVALID原因：reopen-before-command在binary未启动前因证据路径拼接类型错误失败，修正为reopen-before-command-02后才有有效六轮红；stage-pin-check-01误筛模块导致0测，先真实--list找到map_tests后stage-pin-check-02才有有效2测绿。另一次只读raw摘要探索用了不存在的phase名而StopIteration，未改raw、未计算验收PASS；最终RESOURCE_SUMMARY使用实际phase名回读11项全部true。这些不算产品失败或目标反例。

## 下一棒与未执行边界

交新全新C/F46联合审查者；不得由本人改名独立签收。应亲跑六次真实binary重启、完整原resources2测/160轮、存活worker/owner峰稳态/最后重启、假FD/TCP零红→还原绿；读取C完整报告与映射，核原阈值/未知拒绝/真实入口/候选lock/config/fixture/binary绑定。审查者自有新证据目录，注入限其隔离副本。

完整阶段命令/原16全负测/前序R04→R03→R02/RR1闭包属于新阶段审查，未由本包执行；A新失败不由本logging修复关闭。没有release优化比较、R10两小时/1000次、其他平台、真实收费供应商、系统过滤变更、提交/推送/发布、R06实施。本包SELF_CHECKED不改变R06_READY=false。若后续源/lock/config/fixtures变更，受影响证据须重新运行。
