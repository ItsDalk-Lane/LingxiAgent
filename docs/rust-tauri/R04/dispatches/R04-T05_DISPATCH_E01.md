# R04-T05 派单｜EXECUTOR-R04-T05-E01

派单时间：2026-09-30。派单人：R04 总控编排器。
TASK_BASE_SHA：80b4edbf5（R04-T04 已独立 PASS 并推送确认）。

---

你是一次性执行代理，只负责 R04-T05：命令、PTY 与真实进程树取消。
基线：80b4edbf5；分支：codex/rust-tauri-migration；工作区：/Users/study_superior/Desktop/Code/LingxiAgent。

完整读取 R04 总控提示词（存档副本 docs/rust-tauri/R04/dispatches/R04_ORCHESTRATOR_PROMPT_2026-09-30.md，重点 §4.4/§7.T05）、原阶段书 R04-T05 节（R04-A09/A10）、T01-T04 报告、R03 交接、真实源码与测试。不得依赖前一个代理的聊天记忆。

现役语义参照（必须读取真实实现）：lib/sandbox/exec-helper.ts、现役 exec_command/write_stdin 工具（lib/tools/ 内搜索真实注册与实现）、lib/sandbox/bwrap.ts、持续终端/会话语义（现役 PTY 接线）。现役测试 tests/sandbox-*.test.ts 中进程/PTY 相关行为。

任务要点（原 Task 全文仍为准）：
1. 原生 exec_command 与现役 write_stdin/持续终端语义：cwd、shell 选择、参数、环境、输入输出分片、等待/返回句柄、退出码、截断。返回"进程已启动/仍运行"不冒称命令完成；write_stdin 校验调用者、会话和进程句柄授权。
2. 非 shell 操作用结构化 argv；确需 shell 的保留原功能（以权限/OS 隔离约束，不用关键词黑名单假装安全）；环境注入最小化（不把服务端全部秘密继承给工具——显式环境白名单机制）。
3. 真实 ProcessSupervisor：按平台可验证的进程归属（POSIX 进程组；Windows Job Object 按 cfg/登记）、句柄与实例身份（不按裸 PID/名称回收未知进程）；覆盖派发失败、取消前已退出、孙进程、管道、PTY 与清理超时。tokio kill_on_drop 不作为清理唯一保证。
4. 取消→终止进程树→等待退出→回收管道/PTY 的有界清理责任链；普通调用 Future 被丢弃也不丢失进程归属；持续终端跨调用存在时显式登记寿命（不把合法持续进程全杀，不把遗留进程冒称持续终端）。
5. 输出有界、截断与落盘引用；终端关闭、run 取消、应用退出分别按冻结策略处理。
6. 网关集成：exec_command/write_stdin 经 T02 网关与 T03 批准面（写类/执行类按策略）；ResourceScope 对 cwd/工作目录判定复用 T04 ResourceAccess。

验收（必须在真实目标 OS=本机 macOS 观察，不能只检查 CancellationToken）：
- R04-A09 真实孙进程被清理且哨兵存活：命令创建子/孙进程，另有无关测试哨兵；取消后受管树退出、哨兵存活、管道/PTY/许可回收、收据准确（PID/句柄探针与进程退出日志）。
- R04-A10 PTY 交互不退化：受控交互程序，输入、多次读取、resize、中断与退出；输入输出、尺寸、退出码和取消语义正确。
- 追加对抗检查：父提前退出但孙进程仍持有输出管道、输出风暴（有界+截断）、UTF-8 分片边界、无响应进程、stdout/stderr 同时阻塞、跨主体 write_stdin、句柄过期/伪造、取消与正常退出竞争；按声明隔离能力验证，不虚构平台保证。

验证命令（/Users/study_superior/.cargo/bin/cargo）：fmt/clippy/test workspace --locked（r00 环境项隔离核证）/check-contracts/check-boundaries/R03 十套件+A15+A16/T01-T04 套件回归。

注意：PTY 库选型（portable-pty 或等价）按 R01 依赖决策与锁文件核实；确需新依赖时核对锁定版本兼容性并记录必要变更（serde_json 先例：复用锁内版本不加新包）。清理窗口断言优先 barrier/明确握手，不靠随机 sleep。

禁止修改需求让实现通过、删断言、吞错误、静默降级或伪造证据。禁止 commit/push。
报告 docs/rust-tauri/R04/R04-T05_REPORT.md；证据 artifacts/rust-tauri/R04/T05-E01/；更新 R04_TEST_MAP.json T05 条目。
自检全过只返回 READY_FOR_REVIEW。结束本次代理。
