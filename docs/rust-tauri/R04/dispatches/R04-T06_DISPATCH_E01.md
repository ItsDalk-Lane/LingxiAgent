# R04-T06 派单｜EXECUTOR-R04-T06-E01

派单时间：2026-09-30。派单人：R04 总控编排器。
TASK_BASE_SHA：84e6499fe（R04-T05 已独立 PASS 并推送确认）。

---

你是一次性执行代理，只负责 R04-T06：跨平台沙盒与逃逸防护。
基线：84e6499fe；分支：codex/rust-tauri-migration；工作区：/Users/study_superior/Desktop/Code/LingxiAgent。

完整读取总控提示词存档（docs/rust-tauri/R04/dispatches/R04_ORCHESTRATOR_PROMPT_2026-09-30.md，§7.T06）、原阶段书 R04-T06 节（R04-A11/A12）、T01-T05 报告、真实源码与测试。不得依赖前一个代理的聊天记忆。

必读：
- 任务书 R04-T06 节 + 共同规范 01/02/05
- docs/rust-tauri/R04/R04_PLATFORM_CAPABILITIES.json（平台承诺登记——你负责填充冻结后的实际能力矩阵）
- docs/rust-tauri/R04/R04-T04_REPORT.md（ResourceAccess）、R04-T05_REPORT.md（ProcessSupervisor/exectools 环境白名单）
- 现役沙盒实现（完整读取）：lib/sandbox/path-guard.ts、lib/sandbox/platform.ts、lib/sandbox/policy.ts、lib/sandbox/bwrap.ts、lib/sandbox/exec-helper.ts、lib/sandbox/managed-config-guard.ts、tests/sandbox-policy.test.ts、tests/sandbox-i18n-contract.test.ts、tests/sandbox-tool-wrapper.test.ts；从真实调用追踪 macOS 现役沙盒机制（seatbelt/sandbox-exec？或其他 helper）与 Windows/Linux 形态——以源码为准不猜测
- rust/crates/lingxi-service/src/{resourceaccess.rs,exectools.rs,procsupervisor.rs,toolgateway.rs}

任务要点（原 Task 全文仍为准）：
1. 从现役 PathGuard 和平台沙盒规则提取实际读写/网络/环境/子进程边界，复用可验证的成熟机制（不重造已可靠的系统隔离层；不声称平台语义完全相同）。
2. Rust 负责安全参数和最小授权组装；helper 路径、版本、可信来源、必要完整性按已选方案验证。配置错误、不支持策略、缺 helper 必须失败关闭（不自动裸跑、不静默降级）。
3. SandboxPort：沙盒策略接口接入 exec 链（exec_command 的隔离执行路径）；真实命令在沙盒内运行时资源判定与 T04 ResourceAccess/T05 环境白名单协同（约束交集，不出现独立兜底放行）。
4. 以受控外目录哨兵、网络回环服务和环境探针验证实际允许/拒绝行为（绝对路径、链接、继承句柄、网络范围、临时资源）——不能仅断言生成的命令行包含某选项就证明隔离生效。
5. 每个平台写明保证和不支持能力（更新 R04_PLATFORM_CAPABILITIES.json 为冻结矩阵）；运行时不能履行操作要求时明确拒绝，不向模型谎报"已在沙盒中"。

验收（本机 macOS 真实测得；Windows/Linux 按登记口径）：
- R04-A11 缺沙盒不裸跑：移除测试安装中的必要 helper 或换成不匹配版本，提交必须隔离的命令；拒绝、诊断清晰、外部哨兵不变，不回落无隔离执行。
- R04-A12 隔离保证实际成立：真实目标平台（本机 macOS）按冻结策略访问受限路径、网络和环境；允许对照成功、禁止项实际受限；未具备的保证明确未支持。其他平台真机验证缺失按现有递延登记（R03-WINDOWS-R09-R10）如实标 BLOCKED/递延，不冒称通过。
- 追加对抗检查：helper 替换、错误签名/版本、策略注入、绝对路径与链接改向、继承秘密环境；不能关闭 TLS、禁用全部工具或偷偷提权"解决"测试。

注意：网络回环探针只用本 Task 创建并登记的隔离回环服务；越权探针只打授权测试根内外哨兵，不碰真实用户文件；禁止通配清理共享 /tmp。

验证命令（/Users/study_superior/.cargo/bin/cargo）：fmt/clippy/test workspace --locked（r00 环境项隔离核证）/check-contracts/check-boundaries/R03 十套件+A15+A16/T01-T05 套件回归。

禁止修改需求让实现通过、删断言、吞错误、静默降级或伪造证据。禁止 commit/push。
报告 docs/rust-tauri/R04/R04-T06_REPORT.md；证据 artifacts/rust-tauri/R04/T06-E01/；更新 R04_TEST_MAP.json 与 R04_PLATFORM_CAPABILITIES.json。
自检全过只返回 READY_FOR_REVIEW。结束本次代理。
