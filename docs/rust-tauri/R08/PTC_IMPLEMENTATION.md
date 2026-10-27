# run_tools 最小前置修复

状态：LOCAL_SELF_TESTS_PASS_PENDING_INDEPENDENT_REVIEW。新冻结候选8项自测已覆盖，未判原叶独审通过或R07准入通过。

## 原叶与实际分工

原叶 `R00-T02-LA-0818574ABD43` 四条原断言保持：每子调用独立权限判定；外层只显示打印/返回值；失败显示已执行子动作；未知工具、自调用、无权限、超时或沙盒异常拒绝且已完成动作不自动回滚。

`coding_ptc.rs` 只负责纯程序外围与有界展示材料。`r08-ptc-worker.mjs` 直接复用既有 `lib/ptc/ptc-runtime.ts`，没有引入旧工具 wrapper/Pi。每次调用均回真实 RunSupervisor 的私有父驱动，使用真实父授权、批准、调用日志和取消树，不直接调用 gateway.execute_prepared。父驱动与正式默认装配由主集成负责；本文件不能代领其审查。

保留旧字段 description/subcalls/durationMs；子行 seq/name/argsSummary/ok/ms/error/args/argsOmitted/output/outputTruncated/outputOmitted。预算：参数4KB，参数摘要120字符、错误摘要200字符，子输出单条8KB/合计64KB、正文50KB；程序最多8并发，真实墙钟10分钟、工作线程256MB、日志128KB。子调用在同一 Rust future 内并行，不能用 tokio::spawn 遗失真实父驱动私有 scope。

## 已发现并修复的外围安全问题

纯运行时把 structuredClone 等宿主函数直接交给 vm，实测 `structuredClone.constructor('return process')()` 可取 Node worker 能力，完全未调用 tools 就写合成私有目标。`escape-negative-01.log` 原始退出1保留；不把“vm没有process全局”当授权隔离。

当前外围加严格 OS 边界：macOS固定 `/usr/bin/sandbox-exec` deny-default，系统运行库及node/script/runtime/package精确只读，仅 `/dev/null` 写，禁止网络；工作目录固定 `/`，不让沙盒读取仓库目录。Linux固定 bwrap，独立根只读绑定所需文件与运行库、unshare-all；缺能力拒绝。Windows目前明确拒绝，未声称本轮平台验证。没有修改只读范围的 lib/ptc。

Node可信外围在done后继续存活，Rust收齐已发子调用后整组kill并实际wait；执行future取消时进程拥有者把Child与会话许可交给回收者，2秒未确认只记错误，继续持许可等待，不能提前开放停写闸。程序先返回但已有子调用仍在执行时，仍收齐并记录实际事实；不重发。

## 已执行与当前缺口

- `node artifacts/rust-tauri/R08/ptc-implementation-01/node-adapter-check.mjs`：退出0，仅纯运行时适配，不证明Rust权限/存储。
- `node artifacts/rust-tauri/R08/ptc-implementation-01/node-contained-adapter-check.mjs`：严格边界下退出0，最新 `node-contained-adapter-05.log`。真实读/写私有合成文件为EPERM；实际 `/bin/sh` 子写退出1且目标不存在；实际本机TCP为EPERM且监听器未接到；TS/log/return、日志限额和不可擦除TS拒绝仍通过。早期strict-01/02因cwd错误而红，修正为 `/` 后绿；历史保留。
- 首轮未加严格OS边界的真实Rust六组测试：直接执行统一编译产物 `rust/target/debug/deps/r08_admission_ptc-5d41adb521f581e6 --nocapture --test-threads=1`，退出101，5过1失败，日志 `rust-test-01.log`。预算样本对同一文件连续read被真实工具去重，未形成10条大输出；现改10个不同真实文件，保留预算原断言待复验。
- `r08_admission_ptc.rs` 当前八组实际链覆盖：真实文件并行与日志/返回；外层批准后子写仍单独批准且拒绝不写；外层异常保留已写及收据；未知/递归/沙盒异常；实际取消并回收Node；元数据预算/UTF-8；vm逃逸后的实际私有读写、子进程写及网络拒绝；默认10分钟真实程序超时与已完成动作保留（新补，待跑）。

尚待本轮重编实测与独审。主集成已指出的完整all-tools/delegation语义、早期拒绝的独立持久收据、以及子输出是否在实际外部模型投影中完全剥离，仍需关闭，不能由局部适配通过替代。真实10分钟超时入口尚未跑；纯运行时默认预算存在不能代替实际门禁证据。最终默认组合根、UI与R08迁移/回滚/前滚也未由本文件验收。

续跑说明：首轮预算fixture改用不同文件后，第二轮仍因把32KB模型投影当完整UI材料而红（8记录对预期11）；已改只读真实SQLite中原始提交后tool_call_completed，从正确UI收据核11记录及预算。旧driver下随后7项实际Rust测试全部通过（rust-test-03.log，2.01秒）；它只证明旧driver+严格OS外围的局部链，不能覆盖新driver的quota/自动审核/撤权fence/取消settlement等修复。当前所有名字包括递归均交新driver裁决并留独立拒绝journal，Arc接口已配合调整。已冻结两模块/两test/worker，等待主集成统一编译后的新候选实际执行。

## 2026-10-22 新冻结候选执行断点

主集成统一四组编译后开始新候选实际执行，起始源码和二进制摘要固化于 `final-candidate-01-start.json`。新driver的七项功能/权限/真实OS反向样本逐名执行退出0，7通过、0失败、0忽略、1过滤；过滤的唯一项是已在另一进程实际执行的默认10分钟样本，未使用skip/todo或把过滤项记PASS。精确七个名称与命令见最终回执（待本轮完成生成），原日志 `rust-functional-final-candidate-02.log`。

首次全套执行被机器保护器 `TEST_TIME_LIMIT_SEC=600` 在600.04秒中止，退出134；它与产品必须实际等待600秒的默认预算相撞，不是默认超时已通过。`rust-test-final-candidate-01.log` 红证据保留。只读进程观察 `post-guard-abort-processes-01.json` 中没有遗留PTC外围候选，未杀外部进程；失败合成根保留，未把崩溃中的Run写成完成。

现在用仅本次命令的 `TEST_TIME_LIMIT_SEC=660` 重跑唯一默认10分钟测试，内存保护仍为8GiB、不关闭保护，不改产品600秒预算、不改原断言，也不使用假钟。命令：`TEST_TIME_LIMIT_SEC=660 rust/target/debug/deps/r08_admission_ptc-5d41adb521f581e6 real_default_ten_minute_timeout_keeps_completed_action_and_reaps_worker --exact --nocapture`。日志 `real-default-timeout-02.log`，当前尚未结束，不能判PASS。扩大的是机器保护器对整个测试进程的运行余量，产品超时断言不变；仍须独审核此差异与实际证据。

补充只读OS路径别名反例：相同合成秘密通过 `/System/Volumes/Data` 路径别名访问仍被EPERM拒绝，`node-contained-alias-adapter-01.log` 退出0。仅证明本机严格OS外围，不证明Linux/Windows或完整原叶。

## 本轮最终本域结果（取代前面“仍在执行”的断点）

默认10分钟单项已真实执行完，退出0，1通过/0失败/0忽略/7明确过滤，耗时600.30秒。7项功能分组退出0与该单项合计覆盖全部8项，没有关键跳过。机器保护器只为本次单项命令设660秒，产品仍600秒，8GiB内存保护保留；原全套134红证据未删除或改写。此命令层余量差异待独审。

列明源码/二进制的前后SHA-256全部一致。`ptc-implementation-01/final-candidate-01-result.json` 固化全部精确命令、实际退出、日志/源码/二进制摘要、覆盖分区、原始失败、环境与替身边界。外部模型使用替身，服务元数据使用固定ManualClock；默认600秒、停止/取消、真实内核/网关/存储与实际OS进程均未使用假实现。

仅为实现者本域自测。公共driver的完整权限、自动审核、quota、撤权/fence、settlement、all-tools/delegation、默认组合根及模型实际投影仍须独审确认；已备证据不能替代独立结论。完整R08仍未执行。根主任务本轮被宿主标记内容风险而errored，本域没有绕过该状态接管公共实现或宣告整体完成；记录已保存供恢复。

## 2026-10-22 续跑：ptc_authority 五项补验完成（同一冻结候选）

上会话遗留的 `tests/r08_admission_ptc_authority.rs` 本轮首次编译并执行：编译退出0（回执 `admission-authority-build-01`），执行 `--test-threads=1` 退出0，5通过/0失败（1.24s，回执 `admission-authority-run-01`）。真实链：批准等待中撤真实会话授权→批准后 `dispatched=false` 持久拒绝回据且目标文件未写；等待中禁用/换代→`availability is now disabled`/`target generation moved from 1 to 2` 拒绝；配额=1→编排父释放配额、两子调用串行真实写、journal 两条 Succeeded；同 requestId 运行中/完成后幂等返回原 Run、三次事件 resume 续读零副作用零重复收据。对应授权独审 PTC-A04/A06/A07 实测缺口；A01/A03/A05 机制在主链（refuse_child! 持久回执、逐子 review_and_validate、fence/audit_late_result）但未逐一有专项样本；PTC-A02 delegation 仍显式拒绝。均为实现者本域结果，不代领独审。
