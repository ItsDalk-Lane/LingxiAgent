# T04 独立只读复核（r7）

状态：本轮未发现新的可确认 P1/P2 源码阻断，**IR-01 在本机已关闭**。已独立核对 r7 全部 19 项计划门禁及七项实际故障回归，均通过且版本、命令、日志一致。额外窗口可见性检查 1 项通过；**A07 原生人工接管实际超时失败，因锁屏记为 ENV_BLOCKED，T04 尚未完整完成**。不将失败检查或旧版录像计入本候选通过。

## 范围与版本

- 候选：`SOURCE_MANIFEST_r7.json`，指纹 `612f1bdd45a4af24ec84e9956c3cd233fdec21d3883ac41bc7d26db1d5a6fc55`。
- 已独立重算清单 372 个文件的长度、SHA256 与清单指纹；审阅时均一致。
- 已逐项核对 `source-r7.tar.gz` 的条目名称、顺序、长度和内容摘要；归档 SHA256 为 `63b2aa50c18c16302386ecc62611de9c5e69e6ba0ee91524305eb409ca11dbbb`，与清单一致。
- 已从 r6、r7 两份完整源码归档逐字比较 `launcher.rs`、`runtime.rs`、`host.rs`、`startup_tests.rs` 的改动；`cdp.rs`、`managed_files.rs` 两版内容一致，并继续核对其被新路径调用时的行为。
- 本轮独立范围为其他实现者编写的 Chromium 启动、停止、删除和辅助进程清理。审阅者自己编写的服务装配开关、文件续读与相关 fixtures 不计入本轮独立代码覆盖；其运行回执只能作为实际结果记录核对。
- 未修改冻结源码，未执行 Cargo、未启动浏览器、未访问真实账号或付费服务。此报告为独立读取源码和已有本机证据的结果。

## IR-01 修复核对

1. **可失败的准备先于创建进程。** `adapters/src/browser/launcher.rs:166` 的 `Cdp::new` 完成非阻塞设置和读取线程创建后，`:180` 才 `spawn`。`runtime.rs:91` 的 `Live::prepare` 固定下载目录，同样发生在 spawn 之前。spawn 成功之后只构建持有实际 `Child` 的 `Process` 和不返回错误的 `Live::allocate`；不再存在旧的“创建进程后 CDP 准备失败，未知退出只能丢局部 Child”返回路径。
2. **实际 owner 先登记，再初始化页面。** `host.rs:590` 将 `Live` 同时放入 `pending_cleanup` 和该工作区 `entry.live`，之后 `:605` 才执行初始化和首 tab 创建。初始化、线程创建或首 target 失败统一调用 `stop_owned`。停止不确定时，`host.rs:321` 保留该 Live，封闭新执行并记录 `browser_cleanup_unknown`；`launcher.rs:228` 后的停止逻辑只在真实 `Child.try_wait` 确认退出后取走 Child。
3. **急停覆盖 owner 登记前的忙间隙。** `host.rs:994` 仅在本工作区同时没有 Live 且 busy 已释放时返回清空成功；不能把 live 暂为空解释为没有进程。等待超过实际期限则返回 Unknown 并保留撤权事实。同 epoch 重试仍可回收迟到的实际 Live。`Live::stop_until` 与 `Process::stop_until` 共用期限，等待锁或线程未退出时保留原句柄。
4. **删除只等自身并重核迟到 owner。** `host.rs:1133` 起只等待目标 entry 的 busy，不等待其他工作区全局 launching；`:1155` 在 purge 前再次拒绝任何剩余 Live/busy。迟到初始化清理 Unknown 不会被 launching 计数归零覆盖，profile、下载目录与 owner 均保留以便重试。退休事实继续阻止旧 scope 重新启动。
5. **辅助进程同样登记，未知时不清目录。** `host.rs:1290` 在辅助初始化前登记 `pending_cleanup` 与实际辅助目录对应的 Live；创建进程成功后即关闭析构自动清理标记。Unknown 保留目录和两处 owner。只有真实停止确认后，才沿已固定的 `auxiliary_root` 文件句柄和单一名称调用 `managed_files::purge`。原函数不跟随符号链接、不允许整个根目录清空，并有条目、深度和时间预算。
6. **整体停止不会漏掉迟到启动。** `host.rs:804` 收集正式、辅助和待清理 Live，之后等启动/辅助任务收口；即使第一次收集时还没有 owner，最终也会重核 `pending_cleanup` 和 `auxiliary`，有残留就返回 Unknown。

因此，r6 IR-01 描述的可确认源码缺口已修，并由下述 r7 显式故障组的实际运行证据关闭；没有借用普通测试中的 ignored 行。

## 七项故障回归的真实性

来源：`adapters/src/browser/startup_tests.rs`。fixture 选择本机真实 Chromium 程序，使用正式 `ChromiumHost`、管道 CDP、实际 Child 与隔离 profile；本地拒绝代理阻止外部请求。故障钩子只在测试构建存在，未替换整个 BrowserPort 或伪造退出事实。

| 检查入口 | 实际覆盖与断言 |
|---|---|
| `cdp_preparation_failure_happens_before_any_real_spawn`，178 行 | 在 CDP 准备阶段注入错误，断言 spawned 列表为空、无 Live/待清理项、launching 归零。此项刻意不创建 Chrome；不是实际操作系统 fcntl/线程创建失败的复现。 |
| `live_initialization_failure_keeps_real_owner_until_cleanup_retry_confirms`，195 行 | 实际 spawn 且已通过真实 Browser.getVersion 后注入初始化失败；首次停止注入 Unknown，核真实 pid 存活、Child 未回收、双 owner 和 profile 保留、新启动拒绝；实际重试后由 try_wait 确认回收。 |
| `first_target_failure_keeps_actual_process_and_target_until_cleanup_retry`，240 行 | 故障在真实 Target.createTarget 成功返回 targetId 后注入；核实际 targetId、原进程存活和封闭新进程；重试确认回收。 |
| `auxiliary_initialization_unknown_preserves_profile_and_owner_until_retry`，276 行 | 实际辅助进程初始化失败且停止 Unknown；任务退出后计数归零但两处 owner 和目录仍在；真实重试退出后目录才删除。 |
| `revoke_waits_for_real_spawn_owner_registration_and_confirmed_reap`，329 行 | 在真实 spawn 后、owner 登记前用受控暂停点停住；本 scope 急停不得提前成功；同时无 busy 的另一个 scope 删除不被该启动占住；释放后实际回收并清空。 |
| `revoke_startup_timeout_preserves_late_unknown_live_until_same_epoch_retry`，383 行 | 暂停跨过急停期限，核 Unknown、撤权及停止事实；释放后的迟到 owner 遇到注入 Unknown 仍保留实际存活 Child；同 epoch 重试才确认回收。 |
| `forget_rechecks_late_startup_unknown_before_purging_profile`，441 行 | 删除等候启动收口后遇到迟到 Live 清理 Unknown，核 profile/下载目录未删；重试真实回收后才删除并去除 owner。 |

计数应为 **7 项故障场景，其中 1 项证明没有 spawn，6 项持有真实 Chromium 进程**。退出不确定由停止故障钩子注入；真实 Child 存在性和后续 try_wait 回收不由钩子伪造。测试没有强迫操作系统真正拒绝杀死进程，不能把这些证据表述为真实 OS 清理超时已在用户环境发生。

这些测试都显式标有 `ignore`。r7 单列的 `native-startup-faults` 实际命令包含 `--lib browser::host::startup_tests:: -- --ignored --nocapture --test-threads=1`，已核对命令与计划逐参数一致、exit 0，原始日志明确 **7 passed / 0 failed / 0 ignored**，135 filtered out。回执前后指纹均为本 r7 指纹，`sourceUnchanged=true`，实际日志 SHA256 为 `62878d57bb7b2bf5b39d4b83847712c2594a1f5906bb157e0fb3da8e991ba4a5`，与回执相符。

普通 adapter lib 结果为 133 passed、9 ignored。其中 7 项就是上述已另行运行的故障组；另 2 项为 `manual_capture::same_page_native_human_takeover_records_only_owned_target` 和 `manual_capture::externally_restored_suspended_window_is_never_reaped_or_evicted`。这 2 项的独立结果如下，不能算入 adapter 的 133 项或七项故障组。

## 最终计划门禁证据与待定原生检查

已逐项对照 `r7_GATE_PLAN.json`、`r7_NODE_GATE_PLAN.json` 的全部 **19** 份实际回执：每个候选名、完整命令、exit 0、前后指纹、`sourceUnchanged=true` 和原始日志 SHA256 均匹配；复核时 372 个冻结文件仍无漂移。

| 实际门禁 | 核对结果 |
|---|---|
| `browser-flows` | routes 10、browser tools 13、web tools 8，合计 31 项通过。 |
| `service` | 32 组、727 项通过，其中 lib 476；包括原 MCP/worker 19、gateway 15、协作 9 与资源工具 3。 |
| `adapter` | 15 组、241 项通过，其中 lib 133、独立 Chromium 10；9 ignored 如上明确区分。 |
| `native-startup-faults` | 显式运行 7 项通过，0 ignored；只在这一回执计入七项故障验证。 |
| `kernel`、`protocol` | kernel 126；protocol 36 加验证器 1，通过。 |
| `generated`、`rust-roundtrip` | 146 份生成文件无漂移；66 份样本 Rust 解码再编码与 TS 结果逐字节一致。 |
| `clippy`、`format` | workspace/all-targets、禁止警告；全部格式检查，均 exit 0。 |
| Node 9 组 | UI 26 文件、293 项通过；三份类型配置、TS 66 样本往返、浏览器契约、契约类型、所有权、依赖边界、main/preload 构建均 exit 0。 |

上述统计是各命令实际执行数量，并非去重的全产品功能覆盖数；运行回执核对也不构成对审阅者自己实现部分的独立代码审查。

## 两项原生检查的实际结果

- `r7-native-visibility.json`：精确 `--ignored --exact` 命令实际 **1 passed / 0 failed / 0 ignored，exit 0**；前后 r7 指纹不变，日志 SHA256 `c550126e3922795956f29f216f1bc6c548670a24f26dec7c98032913f18e70cf` 与原始日志一致。已按日志的 `VISIBLE_REAP_EVIDENCE` 归档 `native-visibility-r7/result.json`、`cleanup.json` 及摘要清单；实际结果为保留原 target、拒绝第六工作区、仍有 5 个窗口，清理 `confirmed=true`。源码使用真实 `Browser.setWindowBounds` 恢复 native 窗口，刻意不更新宿主可见缓存，再核 idle/LRU；这是窗口管理层的外部恢复回归，不宣称通过鼠标或 Dock 人工恢复。
- `r7-native-handoff.json`：精确原生接管检查实际 **0 passed / 1 failed / 0 ignored，exit 101**，日志明确等待原生表单操作 120 秒后超时。命令环境虽传入 180 秒，源码上限为 120 秒，记录按实际等待时间。前后 r7 指纹不变，日志 SHA256 `4df6c45dcdbf5760e33acf8b8b333b0bfa64fac799b411fb4b132ae6fa5b9bc4` 匹配。主代理的实际操作记录表明 Mac 锁屏且 UI 工具未能解锁，未原生输入、未创建 continue 文件。已核对归档 `native-handoff-r7-blocked/cleanup.json` 为 `confirmed=true`；等待期间的页面帧不作为成功交互录像。锁屏原因来自主代理操作记录，本审阅者没有另行控制或解锁电脑。

A07 保留 **ENV_BLOCKED（原始运行 FAILED/101）**；解锁后必须在新的实际最终候选上重跑同页原生输入与交还，再核目标、正文、录像和资源回收。旧 `native-handoff-r1` 证据不替代本 r7 验证。源代码现已按主代理指示解冻进入后续开发，本结论只绑定上述 r7 归档和运行回执。

## 限制

本报告不替代完整 R07 验收，不覆盖其他操作系统、正式发布包、真实账号、付费供应商及未注册的 T05 开发代码。既有 r1—r6 失败记录和 R06 未通过记录继续保留。任一新增改动都需要新的候选与对应证据，不能沿用本 r7 指纹证明改变后的代码。
