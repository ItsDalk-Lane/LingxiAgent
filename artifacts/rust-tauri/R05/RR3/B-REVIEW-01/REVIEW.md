# RR3 B / F45 — 第 1 轮全新独立验收

**结论：PASS（仅 B/F45 与 N03 可复跑、汇总完整性范围）。mustFix：无。**

验收者 `/root/rr3_b_review_01`，未参与实施或修复。2026-10-07 08:22–08:26（Asia/Shanghai；逐命令 UTC 时间见 receipts）。HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`，带未提交 RR3 overlay。A/C 仍在实施，主树总体没有冻结；本报告不宣布阶段放行，不改变 `R06_READY=false`。

## 读取与约束

全文读取 RR3_BRIEF、RR3_REVIEW_BRIEF、RR1_MASTER_PROMPT_2026-10-04、RR2_MASTER_PROMPT_2026-10-06，以及 RR3 ISSUE_MATRIX/PROGRESS/HANDOFF、B-01/REPORT 与三个被验脚本。核查 B-01 两次 N03 的原始日志、有效正常/目标红/恢复日志、15 项自检与 binary-bound-phases/result，确认首次精确路径误写导致恢复零匹配的失败仍保留、未用于通过结论。核查 RR2/G-R2/NEG-GATE-RR2 的历史 16/16 与过期锚点说明、RR2/B-R2/R2/REVIEW-r2；历史结论不替代本轮亲跑。

对应义务：F45、原 N03、I11、F26 的测试身份/归属/实际执行核对；相邻 T08-C13/C14 的门禁范围保留。本轮只降低 TSV 计数字段，不修改权威表的正式值、生产镜像预期、测试身份、最低门槛或生产入口。

主树只读；新证据仅本目录。注入仅官方脚本生成的自有新副本 `/Users/study_superior/r05t08-work/negcopy.qcldNA` 和本目录手写 TSV 夹具。未编辑被验对象或总控台账；未提交、推送、切分支或创建标签。官方脚本使用本地 clone 写隔离副本，其对主树仓库只读。

## 亲跑结果

| 命令 / 检查 | exit | 实际观察 |
|---|---:|---|
| `bash -n scripts/rust-tauri/r05_t08_negative_gate.sh` | 0 | 语法通过；`bash-n-receipt.json` |
| `python3 scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py` | 0 | 实施侧 15 项自检全部符合期望；实际拒绝腿 exit 1，合法腿 exit 0；`implementation-selfcheck.log` |
| `bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/RR3/B-REVIEW-01/n03 --case N03` | 0 | 官方脚本原入口；真实正常 8/0/0，动态注入 24→23 恰一次，目标红 exit101，逐字节恢复精确 1/0/0 绿 |
| 自有 `review-driver.py` 额外精确三腿与注入 | 0/0/101/0 | 正常镜像绿、注入成功、目标红、还原绿；每腿完整输入清单、前后摘要、真实二进制与日志 SHA；`phases-result.json` |
| 自有 `adversarial-checks.py` | 0 | **41 项**手写独立检查通过：15 组注入输入、26 组汇总正负控；`adversarial-with-times/result.json` |

官方正常镜像：**8 passed / 0 failed / 0 ignored / 113 filtered**。目标红：**0 passed / 1 failed / 0 ignored / 120 filtered**，exit101；恢复：**1 passed / 0 failed / 0 ignored / 120 filtered**，exit0。独立精确三腿均为 1 项目标、120 filtered；正常和恢复 ignored=0，零匹配=0。

真实目标命令（三腿相同，cwd 为上述隔离副本）：

```bash
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites -- --exact
```

红腿点名 `stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites ... FAILED`，断言原因是 `the R05 pin table's suite registrations drifted (dropped, added, or re-counted)`；左右注册集明确含 `svc:r05_t01_model_plane` 的 23 与 24。正常前置的全部 8 个 R05 镜像通过，红腿真实运行 1 项且仅此项失败，恢复真实运行 1 项通过。不是编译错误、认证前置失败、其他断言偶然失败或空过滤。

## 独立正负控与代码核查

- 注入器从目标 `pin svc:r05_t01_model_plane` 的**唯一**行读取 old；new=old−1，receipt 保存 `matches=1`、`mutations=1`、行号76、24/23、oldRow/newRow 及前后 SHA。审查者另逐字节比对差异，只有计数字段变化，表其他字节不变。
- 独立手写旧固定锚点 **7→6**、未来计数 **42→41**、混合空白/CRLF/其他行 **17→16** 全成功。结果直接对照手写预期字节，不由实现输出反推预期，也未 import 注入器内部函数。当前 24 由官方原入口与额外三腿检验。
- 缺失、同值重复、冲突重复分别 exit1，点名唯一锚点要求与实际匹配数0/2/2；每例目标 TSV 字节不变、无 mutation 成功 receipt。非法字符串、小数、带正号、负数、0、1、缺筛选字段、多字段、非 ASCII 数字均 exit1、不改表、不写成功 receipt。
- 直接取主脚本原 `write_results` 函数执行：单项正确=1 case，其他15列 `unexecutedCases`；空集、错项、重复拒绝。完整16项夹具通过，单项冒充完整、空集、重复、BAD判定、错误身份拒绝；**逐一移除 N01…N16 的16种缺项形状全部拒绝**。这些是汇总函数夹具，不是实际执行16项注入的证据。
- 默认 `CASE_SCOPE=ALL` 保留；源码中16个 `record_case` 精确覆盖 N01…N16 各一次，默认链保留 N03 调用，单项分支只允许 N03。与 HEAD diff 对照，其他15项故障注入及目标检查正文未删除或缩减。新增汇总使用完整有序身份集合相等，不能仅凭 `all([])` 或总行数放行。
- 生产镜像对 suite 集及计数仍做完整相等断言，64项 lib 每项1测要求保留。B 的三处变更没有改变这一生产检查或正式 TSV。

## 输入、工具链与二进制绑定

工具链 `/Users/study_superior/.cargo/bin/{cargo,rustc}` 均 **1.98.1**，`--locked`，`CARGO_NET_OFFLINE=true`；macOS arm64，Node v24.16.0 / npm11.13.0。详细版本、HEAD、开工工作区状态见 `environment.json`。

保存生产输入清单范围：隔离副本 rust（排除构建 target）、scripts/rust-tauri、docs/rust-tauri、contracts、根 rust-toolchain.toml；Rust manifest、lock、schema/config/fixture 内容均在逐文件清单中。原始扫描包含 `.pyc`；另保存 `*-production.json` 明确剔除派生 `__pycache__/.pyc`，**两套原始材料均保留**，不把派生缓存当生产输入。

独立三腿生产输入摘要：

| 腿 | 摘要 |
|---|---|
| 正常 / 字节恢复 | `da23d5027f6be20a115737bd8db7bf66b02673ebcffe1c7db4c9404455e564fc` |
| 24→23变异 | `806051eabf0509616ce79f9635665f7aeec7602cd2d343ca97ee97f23c57d73a` |

三腿每次输入前后相等；正常与还原完全相等，变异腿唯一不同输入为正式 TSV。原始含派生缓存摘要分别 `1f53b2408722d4c73bf08a6d6358b7383003d3bcdeb04f943c7599073c06a7bd` / `20f54dfc6ad700b80f863df0349a346664f664997d48fdc9c9acfe4fe4202bd3`；详见各 phase receipt 与清单。

三腿实际二进制：`/Users/study_superior/.cache/lingxi-r05-neg-target/debug/deps/xtask-ad766de8ea03e62c`，均 SHA256=`ae943888174036c35957afaf810f3055c6e0f2037bc436a1ccbd49ca4bf8acf4`。本镜像运行时读取 TSV，变异不需换二进制；cargo 实际 Running 行与逐腿二进制读取一致，未替换被测入口。Cargo.lock SHA256=`259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3`。

B 三文件与权威表在来源扫描前/后及隔离副本逐字节一致：

| 输入 | SHA256 |
|---|---|
| negative_gate.sh | `4853e129e2d0cf7dbd6c93c8c158186fad23e6f231a03ff64f140cca61e0042f` |
| mutate_pin.py | `7785b41ebadfdc6d6415ee1d337ea48621e25d1bbb1ed1c16456004545279609` |
| negative_gate_selfcheck.py | `f94f76721e77bf5a738b1270bd875569fd3f9ba3cc552795033ac057a8ee1867` |
| 还原 TSV | `fdacb2c2401ccd0a276abc7ab945cad57ec307f08e24516bac9227e76031e6e5` |

来源生产树前摘要=`46f49ecca89baf0c9bdcc596266ddff02d14b98c3a42a5eb5aa6d8d85cedbfac`，后摘要=`adfe233936e71e585b70714aadfd7af54c3a8199f397ccc35f29ec4b86226a2d`。A 在生成副本期间修改了 candidate/其测试与绑定辅助脚本，另有 Python 派生缓存变化；完整路径差异见 `source-copy-boundary.json`。**没有声称主树或副本与当时整个生产树相等**：本轮绑定到自有副本的稳定输入，B三文件/TSV来源明确不变。最终冻结候选须重新建立全树绑定。

## 边界、保留与下一步

没有使用供应商替身、真实账号、收费请求、HTTP 服务或用户数据。注入器对手写 TSV 的测试与汇总 TSV/控制退出码夹具只证明对应算法/汇总边界；真实 N03 核心检查完全由正式 cargo/xtask 原入口执行。

审查证据记录器首次扫描遍历构建缓存过慢，尚未执行任何检查或注入时停止；改用遍历时跳过 target 后重新启动，正式 `review-driver.py` exit0。没有把该停止记为测试通过。独立41项首次完成后，仅为补齐汇总腿逐条时间字段再执行一次，原 `adversarial/` 保留，带完整时间记录版本位于 `adversarial-with-times/`，两次均exit0。

**本轮未实际重跑其他15项、完整 workspace、正式 R04→R03→R02/RR1 闭包、完整 verify-stage R05、LIVE 或其他平台。**RR2 历史16/16保持原样。B 的唯一副本/共享汇总变化与 A 的绑定链变化会影响 N05/N06/N16；最终不能仅复用历史16/16放行。

按总控本轮指示：A/C实施完成、所有包独立通过并冻结后，由**新的 G 负测审查者**完整亲跑默认 N01–N16，并核目标原因与还原，随后新的阶段审查者亲跑原§5.3。这个集成依赖不是 B/F45 当前的实现缺口；未完成前阶段仍 NOT_ACCEPTED / R06_READY=false。

下一命令（由新的最终负测审查者在冻结候选、全新目录执行）：

```bash
bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/RR3/G-NEG-01/negative
```

所有本审查新文件摘要索引见 `evidence-sha256.json`；具体命令、时间、退出码、实际数量、输入/二进制/log 摘要均在相应 receipts。实施者没有参与本结论签署。
