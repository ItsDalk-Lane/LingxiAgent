# R02-A07 运行中存储故障：静态修复与未执行验收

- 截止：2026-09-28T10:23:05Z；当前工作区仍在改动，以下只绑定本组文件摘要，不是阶段候选封印。
- 原缺口：T04-R1-F05。旧 A07 只证明启动时目录写失败、成功提交后的崩溃恢复；没有运行中的 HTTP 500/503、无伪完成事件、重启读库的同一证据链。
- 新检查：`execute_concurrency.rs::http_running_storage_fault_has_no_success_event_or_restart_run` 先对运行中的 SQLite 持有写锁，要求 HTTP 503、零 run/事件；随后用仅在合成库中创建的触发器阻止终态写入，要求 HTTP 500、允许已提交的开始事件但没有完成事件、磁盘保持 `running`，并重启核对相同 run 身份与行数。
- 机器门禁：R02-A07 新增 `a07_live_fault_01_test` 生产结构化案例，`a07_live_fault_02_check` 读取案例与原始测试日志逐字段核验；原 A07/A08 脚本仍为必需，不以新检查替换。
- 实际执行：`cargo test -p lingxi-service --test execute_concurrency --no-run` 退出 0，原始输出 `no-run-gate.log`；`cargo test -p xtask embedded_r02_map -- --nocapture` 2/2 PASS，原始输出 `embedded-map-unit.log`；Python 编译和 JSON 解析退出 0。第一次误加 `--lib` 的 xtask 命令退出 101，原始输出 `stage-map-unit.log` 保留；随后错误过滤词导致 0 项运行，原始输出 `stage-map-unit-corrected.log` 保留，不能作为通过证据。
- 动态状态：**BLOCKED，未运行**。先前真实端口绑定得到 `Operation not permitted`，按用户审批边界不再等效重试。HTTP 500/503、事件及重启对照目前仅是写在检查中的断言，不能声称已观察通过。
- 恢复条件：平台明确允许当前候选启动本地服务后，用全新证据目录执行完整 A07 及阶段门禁；保存实际退出、原始日志、源摘要和当前候选摘要，再由独立评审核验。

本组代码摘要（SHA-256）：

```text
044eb740706c04f47a01ed3ff8de8c4c05c717926e258b9a0c8141e38c4b4e78  rust/crates/lingxi-service/tests/execute_concurrency.rs
47023e233e307c34ad0b6c632e6f4b388bbd3ed3ea17fd2469fcb925e54ec0f6  scripts/rust-tauri/r02_t04_live_fault_evidence.py
a4a4ebfa6be66e049eb7a1b96db9123fb68fc5253004b61e3510a65e68f5ef89  rust/crates/xtask/src/stage_maps/R02.json
```
