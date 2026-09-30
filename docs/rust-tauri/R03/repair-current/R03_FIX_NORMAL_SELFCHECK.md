# R03 修复轮普通自查汇总（R03_FIX_NORMAL_SELFCHECK）

- 汇总人：EXECUTOR-REPAIR-R03-G07-E01（2026-09-30）。本文件为**轮次级汇总/索引**：G01–G06 各组的逐 C-ID 普通自查原文在各组文档，此处不复制其结论、只建立可追溯索引并汇总 G07 本组的普通自查；逐 F-ID/C-ID 的权威状态仍以 `R03_FIX_ISSUES.json` 为准。
- 工具链：`~/.cargo/bin/cargo`（rustup 1.98.1），全部 `--locked`、离线；`rust/Cargo.lock` sha1 `3b659f41eb262eaf42efc2f91ec93f9989d42934` 全轮零变化。

## 1. 逐 F-ID：普通自查索引（G01–G06，原文在各组）

| F-ID | 组 | 普通自查文档（逐 C-ID 原文） | 关键正向核查 | 绿证据（门禁内机器核验） |
|---|---|---|---|---|
| F01 | G01 | `G01-E01_NORMAL_SELFCHECK.md` | 生产入口进入修改后代码（CancelRegistry/register_linked/run_root_under/子代理 timeout 链）；DB 行/事件/线程/配额一致 | `cancel_link_inheritance` 7/7（G07 门禁 repair_suites） |
| F02 | G01 | `G01-E01_NORMAL_SELFCHECK.md` | 同进程多次父取消超上限后配额回基线、busy 清除、合法终态；未首 poll/派发拒绝/panic/超时回收/后台 panic 各路径收尾 | `subagent_closeout` 8/8（同上） |
| F03 | G02 | `G02-E01_NORMAL_SELFCHECK.md` | 取消先赢不落 completed；完成先已确定→TooLate/AlreadyTerminal；重复取消首因+阶段单调；各边界零新外部调用 | `cancel_terminal_race` 13/13（同上） |
| F04 | G03 | `G03-E01_NORMAL_SELFCHECK.md` | 副作用后 panic→Unknown 非确认失败；中止/通道丢失/超时一致 Unknown；授权前拒绝 dispatched=false 与可信 failed 区分；重复恢复不重做 | `tool_receipt_unknown` 6/6（同上） |
| F05 | G04 | `G04-E01_NORMAL_SELFCHECK.md` | 后台 spawn 拒绝/起始事务失败→无幽灵 Replay；同 key 并发/异内容冲突/跨主体隔离；副作用后丢响应不删绑定；重启同 key 显式契约 | `admission_dedup_consistency` 5/5 + `admission_dedup_adversarial` 5/5（同上） |
| F06 | G05 | `G05-E01_NORMAL_SELFCHECK.md` | 1999/2000/2001 及预算内长输入前后台完整到达执行面；Unicode/规范化一致；超限显式拒绝零副作用 | `input_payload_fidelity` 5/5 + `input_budget_refusal` 2/2（同上） |
| F07 | G06 | `G06-E01_NORMAL_SELFCHECK.md` | 后台 Accepted 下一轮恰一次；跨会话/跨运行不串用；取消/终态边界契约；有界容量前后台一致 | `background_steering` 8/8（同上） |

## 2. G07（F08）本组普通自查（逐 C-ID）

### R03-FIX-F08-C01 逐问题的红绿回归可追溯 — PASS
- 矩阵已建立：`R03_FIX_ACCEPTANCE_RESULTS.json` `fIdMatrix`——每个 F-ID → 测试文件/逐用例名（9 套件 59 用例全列）→ 红基线证据路径+实测失败摘要（G0x-E01/adversarial-selfcheck/red-baseline-*.log）→ 绿证据（G07 门禁 repair_suites 逐套件 actual==expect）→ 三层审查状态（G0x-R1 全 PASS，引自总控账本，未重复执行已完成审查）。
- 红基线均为**未修代码上的真实失败**（各组隔离 worktree 实测，路径与失败行数在档）；未为制造红灯破坏无关代码。

### R03-FIX-F08-C02 漏项和空测试不能通过 — PASS
- 生产者 `scripts/rust-tauri/r03_g07_repair_suites.sh`：逐套件固定计数（expect==actual==图钉）+ 匹配 0 拒绝 + 无解析摘要拒绝 + 失败/忽略拒绝；`lingxi.r03-repair-suite-results.v1` 机器记录。
- 负向行为在 /tmp 隔离副本以真实 verify-stage 验证（N1a 删叶映射 exit1 点名叶 id；N1b 删命令映射 exit2 references unknown command；N4 删修复场景→xtask 钉图测试 exit101 点名 R03-RP01；N2 0 匹配过滤器→生产者 exit1 点名 suite+计数、完整门禁 overall FAIL；N3 缺证据文件→命令 exit0 但 missingEvidence 点名路径、overall FAIL）。全部原始退出码与日志在 `artifacts/rust-tauri/R03/repair-current/G07-E01/negative-tests/`，驱动脚本可重跑。
- 注（如实）：驱动脚本的点名断言首跑时只搜索门禁进程日志+result JSON，N2 的点名文本在生产者自身证据文件（gaps.txt/summary.txt）中——门禁本身首跑即正确拒绝（exit 1、repair_suites FAIL、overall FAIL）；驱动 haystack 已修正，N2 行按首跑归档证据事后复评（`case-results.json` driverHaystackFix 注记），无重跑、无新门禁执行。

### R03-FIX-F08-C03 候选与三层证据绑定 — PASS
- verify-stage R03 结果内 `candidateSourceBinding`：before/after digest（本轮 `7be6cdc5…`→同值）、15 个逐命令 checkpoint 全稳定、testedShaAtEnd=HEAD、runnerSourceBinding=PASS（编译内嵌源==磁盘字节）。
- 受控 STALE 演示（/tmp 副本）：C03a 预置陈旧证据文件→verify-stage 拒绝运行（is not empty，exit1）；C03b 运行中修改受追踪执行输入 `rust/crates/lingxi-service/src/lib.rs`→digest 漂移、`stable=false`、reason 点名 "Candidate file bytes or HEAD changed…stage PASS is forbidden"、`finalChangedPathBytesHex` 精确点名该路径、overall 强制 FAIL、exit1。执行输入与元数据区分：候选快照只哈希受追踪+非忽略文件字节，证据根被显式排除（excluded 策略在结果 JSON `excluded` 字段）。

### R03-FIX-F08-C04 重新接受范围正确且可交接 — PASS（材料就绪；裁决归阶段 Reviewer）
- 完整材料：本目录五件套 + `G07-E01_REPORT.md` + 递延表（R03_FIX_HANDOFF.md §3、R03_HANDOFF.deferred_registrations）+ Git 状态（G01–G06 已推送提交链+G07 未提交改动清单）。正式签名/真实供应商/完整 R07 UI 未作为本轮前置。

## 3. 门禁与回归实跑汇总（本轮 G07 真实退出码）

| 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|
| `cargo fmt --all -- --check` | 0 | 零 diff | `G07-E01/gate-commands/rust_fmt/` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警 | `G07-E01/gate-commands/rust_clippy/` |
| `cargo test --workspace --locked` | 0 | 72 suites / 709 passed / 0 failed（≥底线 72/704/0；+5 为 xtask 钉图测试） | `G07-E01/gate-commands/rust_test_workspace/` |
| `xtask check-contracts` | 0 | 零漂移 | `G07-E01/gate-commands/check_contracts/` |
| `xtask check-boundaries` | 0 | OK | `G07-E01/gate-commands/check_boundaries/` |
| `xtask verify-stage R03`（全新证据根） | 0 | overall PASS：15/15 命令、17/17 场景（16+RP01）、48 叶 17 pass+31 deferred、binding stable | `G07-E01/verify-stage-r03/` |
| `xtask verify-stage R02`（回归） | 1 | overall FAIL：19/20；唯一红 a16（E5 治理递延+1 条环境闪红，逐条归因） | `G07-E01/verify-stage-r02-regression/` |

受影响 R02 定向回归（认证/单写者事务/事件快照续读/备份损坏库/恢复演练/真实重启链/默认入口 E0–E4.5）：在 verify-stage R03 内 7 条定向命令全绿。
