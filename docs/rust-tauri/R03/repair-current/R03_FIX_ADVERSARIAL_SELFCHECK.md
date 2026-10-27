# R03 修复轮对抗性自查汇总（R03_FIX_ADVERSARIAL_SELFCHECK）

- 汇总人：EXECUTOR-REPAIR-R03-G07-E01（2026-09-30）。轮次级汇总/索引：G01–G06 各组逐 C-ID 对抗性自查（攻击窗口→观测→是否推翻→新回归）原文在各组文档；G07 本组的对抗性自查=对本轮门禁接入本身的攻击（Fake-green 全家族），全文在 §2。权威状态以 `R03_FIX_ISSUES.json` 为准。

## 1. 逐 F-ID：对抗性自查索引（G01–G06）

| F-ID | 组 | 对抗性自查文档 | 攻击面要点（barrier/固定调度/受控故障点，非 sleep 碰绿） |
|---|---|---|---|
| F01 | G01 | `G01-E01_ADVERSARIAL_SELFCHECK.md` | run_root_under/register_linked/child 三入口混用+不同深度取消；先取消后注册/注册与取消竞态窗口；隔离性与并发异因首因；三类泊车（Provider/审批/配额）超时收束 |
| F02 | G01 | `G01-E01_ADVERSARIAL_SELFCHECK.md` | 未首 poll/预留-登记各窗口停止；panic/迟到旧完成回调（身份栅栏防误清新 child）；drain 超时 abort 后实际退出的最终回收；后台 panic 双注册表对账 |
| F03 | G02 | `G02-E01_ADVERSARIAL_SELFCHECK.md` | 模型事件前后/提交入队前/事务提交前多序交错；fence 后持久化等待中取消；并发重复 fire 与 phase 倒退窗口；完成先赢后取消（TooLate 不倒退） |
| F04 | G03 | `G03-E01_ADVERSARIAL_SELFCHECK.md` | 外部确认前后 panic 均不臆测回执；中止/通道丢失/超时分类一致；可信负面（授权拒绝 dispatched=false/明确失败回执）不丢；恢复两次+幂等控制例只用原 key |
| F05 | G04 | `G04-E01_ADVERSARIAL_SELFCHECK.md` | 容量检查与 spawn 之间注入失败；队列满/事务 IO/受理窗口取消；同 key 同/异内容、跨主体/会话隔离对照；journal 提交前后丢响应；重启未知副作用显式拒绝 |
| F06 | G05 | `G05-E01_ADVERSARIAL_SELFCHECK.md` | 尾部反向标记（非靠回复猜内容）；跨 2000 边界的组合字符对（前 2000 相同尾部不同不被误合并）；日志摘要超限≠误拒合法输入 |
| F07 | G06 | `G06-E01_ADVERSARIAL_SELFCHECK.md` | 多次追加/不同顺序/循环多轮不重 drain；并行双后台会话；取消后快速新 Run 不误归属；有界容量并发竞争+10 连跑 80/80 |

## 2. G07（F08）本组对抗性自查：攻击自己的门禁接入

尝试推翻的目标：**"新增反例进入正式验收"这一接入本身是否可被绕过/稀释/伪造绿**。每项攻击均在 /tmp 隔离副本（git worktree @HEAD+未提交门禁文件）上执行真实 `cargo run -p xtask -- verify-stage R03` 或真实 `cargo test -p xtask`，原始退出码/日志全部在档（`artifacts/rust-tauri/R03/repair-current/G07-E01/negative-tests/`，可重跑脚本 `scripts/rust-tauri/r03_g07_gate_negative_tests.sh`）。

| 攻击窗口（adversarial_variation） | 观测（原始证据） | 是否推翻 | 结论 |
|---|---|---|---|
| **A1 删除一条 R00 补充叶映射**（把必需义务从门禁滤掉） | verify-stage 在任何命令前中止：`drops 1 REQUIRED_SUPPLEMENTAL leaf scenario(s) … [R00-T02-LA-00ECC9568490]`，exit 1 | 否 | R13/R14-F01 相等性检查把缺口点名并拒绝总 PASS（`n1a-delete-leaf-mapping/`） |
| **A2 删除 repair_suites 命令映射**（反例生产者从门禁蒸发） | 解析拒载：`scenario "R03-RP01" references unknown command "repair_suites"`，exit 2 | 否 | 场景→命令引用完整性在 parse 期硬拒（`n1b-delete-command-mapping/`） |
| **A3 删除 R03-RP01 修复场景**（命令保留但反例不再被场景消费——门禁自身看不见该缺口） | 门禁对基础场景本会全绿；但 xtask 钉图测试红：`R03 map dropped the G07/F08 repair scenario R03-RP01…`，exit 101 | 否 | 该洞由钉图测试闭合——而钉图测试跑在门禁自己的 `rust_test_workspace` 命令内（`n4-delete-repair-scenario/`）。如实登记：这是分层防护，非 verify-stage 单独可判 |
| **A4 匹配 0 的测试过滤器**（cargo test 对 0 匹配本就 exit 0 的经典假绿） | 生产者拒绝：`suite cancel_link_inheritance: filter matched 0 tests (running=0, pinned=7)` 等 9 条点名，exit 1；完整 verify-stage：repair_suites FAIL、overall FAIL、exit 1 | 否 | 空集合/过滤 0 必须失败（05 §3 契约；`n2-zero-match-filter/`） |
| **A5 过滤成子集/改名/删用例**（计数漂移） | 生产者钉 `executed N != pinned`（同 N2 机制，expect 精确相等）；xtask 钉表测试另在 cargo test 内镜像 9 组 (suite,count,F-ID) 集合相等 | 否 | 双层：命令内精确计数 + 工作区测试内钉表（`r03_repair_producer_pin_table_matches_the_registered_suites`） |
| **A6 声明一条生产者永不写的证据文件**（exit 0 但无内容） | 命令本身 exit 0（生产者真绿），verify-stage `missingEvidence: ["{EVIDENCE}/G07_REPAIR/missing-evidence-demo.json"]`，overall FAIL，exit 1 | 否 | exit0 无内容必须失败（`n3-missing-evidence-file/`） |
| **A7 陈旧证据根**（旧运行结果遮盖新运行） | verify-stage 拒绝启动：`evidence root … is not empty; preserving its prior evidence`，exit 1（命令一个都没跑） | 否 | F04 新鲜度绑定在根级也成立（`c03a-stale-evidence-root/`） |
| **A8 自查与审查之间更改执行输入**（复用旧 PASS） | 运行中修改受追踪源文件→`candidateSourceBinding.stable=false`、reason=「Candidate file bytes or HEAD changed during the registered stage checks…stage PASS is forbidden」、`finalChangedPathBytesHex` 点名 `rust/crates/lingxi-service/src/lib.rs`、overall 强制 FAIL，exit 1 | 否 | 原 PASS 不可复用（拒用=STALE 语义；`c03b-midrun-input-change/`） |
| **A9 稀释普通取消语义为重启语义**（forbidden_shortcut） | subagent_closeout 8 用例断言同进程终态/线程/配额（反复超上限后可再派发），无任何用例以 startup_scan/reboot 作为普通取消通过步骤；G01-R1 独立审查明查此点 | 否 | 红线保持（审查+门禁双重） |
| **A10 篡改生产门禁代码来"演示"负向行为** | 全部负向演示在 /tmp 隔离副本完成；主仓生产 R03.json/脚本在演示期间零改动（git status 可核） | 否 | 过程合规（派单 §2.2 要求） |

驱动器自身缺陷（如实登记）：A4 首跑的**驱动器断言**只搜门禁进程日志+result JSON，未含生产者证据文件——门禁本体首跑即正确拒绝（exit1/overall FAIL），驱动器 haystack 已修正并按首跑归档证据事后复评（`case-results.json` driverHaystackFix），未重跑、未放宽任何门禁断言。

## 3. 汇总裁决

7/7 负向案例全部非零退出且点名缺口（`allRefused: true`）；未发现可绕过门禁的路径；G01–G06 各组对抗性自查在其原文档逐 C-ID 结论均为未推翻。本轮未发现需移交总控的误判反证。
