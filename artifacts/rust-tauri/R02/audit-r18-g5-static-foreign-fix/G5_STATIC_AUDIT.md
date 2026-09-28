# R02 G5 十个注册脚本静态复核（R18）

本记录只说明源码和语法检查。服务监听在当前环境出现 `bind PermissionDenied`，本轮没有运行任何真实服务测试；不得把本记录当成 R02 动态验收通过。首次失败及旧轮日志均保留原位。

| 脚本／任务 | 正常停止与异常退出 | 启动失败／辅助进程 | 本轮结论 |
| --- | --- | --- | --- |
| `r02_t01_service_smoke.sh`／A01 | TERM→限时轮询→归属复核→必要时 KILL；仅 `exited` 后 `wait`；EXIT 用同一有界清理 | 负面启动 `run_refusal` 最多轮询 10 秒；非 `exited` 直接失败，交 EXIT 清理 | `foreign` 不再被当作干净退出 |
| `r02_t02_dual_instance.sh`／A03 | 多实例各自保留 PID；正常 TERM 与崩溃 KILL 均有界；EXIT 逐 PID 清理 | 等待 READY 失败即退出并触发有界清理 | 崩溃和正常停止仅接受 `exited` |
| `r02_t02_path_priority.sh`／A04 | 每个案例的服务 PID 在 `wait` 后退役；停止与 EXIT 有界 | 未 READY 时只有 `exited` 可回收；活进程由 EXIT 清理 | `foreign` 报错并保留 scratch |
| `r02_t03_auth_matrix.sh`／A05/A06 | TERM→KILL 有界；正常停止只在 `exited` 后 `wait` | 启动期间死亡／无 READY 会失败并触发 EXIT | `foreign` 报错并保留 home |
| `r02_t04_storage_tx.sh`／A07/A08 | 优雅停止和崩溃检查分别验证退出码；KILL 与 EXIT 有界 | 启动失败触发 EXIT；没有直接等待存活 PID | `foreign` 不再算停止或崩溃成功 |
| `r02_t05_events_matrix.sh`／A09/A10 | 正常停止与 EXIT 都使用有界 TERM→KILL | 启动失败触发 EXIT | `foreign` 报错并保留 home |
| `r02_t06_backup_restore.sh`／A11 | 正常 TERM、崩溃 KILL 与 EXIT 均有界；WS probe 另有 35 秒结束期限 | 启动失败触发 EXIT；probe 超时不直接 `wait` | 服务与 probe 的 `foreign` 均判失败 |
| `r02_t06_recovery_drill.sh`／A12 | 活跃 PID 集合仅保留未回收子进程；停止与 EXIT 有界 | 无 READY 仅在 `exited` 后回收；兄弟服务停止只接受 `exited` | `foreign` 不再算任何恢复/崩溃成功 |
| `r02_t07_redaction_scan.sh`／A13 | 服务与外部锁助手分别限时停止；EXIT 逐 PID 清理 | 启动失败触发 EXIT；锁助手有 11 秒结束期限 | 两类 PID 的 `foreign` 均判失败 |
| `r02_t07_slow_subscriber.sh`／A14 | 正常 TERM 最多 15 秒、KILL 后最多 10 秒；EXIT 独立有界清理 | 订阅探针完成标记后限时确认退出；启动失败走 EXIT | 服务与探针的 `foreign` 均判失败 |

十个脚本的进程探测都区分 `exited`、`owned`、`foreign`、`unobservable`。本轮在每处实际发 TERM/KILL 前复查当前归属；`foreign` 和 `unobservable` 都不会收到信号，`foreign` 也不会被等待。正常路径、启动失败和 EXIT 只有确认 `exited` 才执行 `wait`。限时轮询遇到 `foreign` 可以提前停下轮询，但后续判定会失败；不能由轮询的 `break` 推断通过。

`xtask verify-stage` 的十项命令均登记在 `rust/crates/xtask/src/stage_maps/R02.json`。`verify.rs::run_command` 给命令分配新目录、独立 session、运行期限；超时后按进程组 TERM/KILL 并以有期限的 `try_wait` 回收。脚本正常退出时由脚本自己的 EXIT 清理负责子进程。静态核对没有修改 xtask。

原始静态检查记录在同目录 `static-checks.json`：十次 `bash -n` 与一次限定十文件的 `git diff --check` 均 exit 0；起止 UTC、stdout、stderr、命令逐条保存。对应源文件 SHA-256 在 `candidate-sha256.json`。这些只是 G5 子候选摘要；后续 A07 脚本若继续修改，须为最终候选重新计算摘要。

剩余限制：没有运行需要本地监听的十项服务检查，因此真实进程退出、超时与失败分支仍为动态 **BLOCKED**。`ps` 观察与发信号之间不能原子化；当前源码把检查贴近信号并在归属不可证时失败，但这一点也需要获准的动态复核。
