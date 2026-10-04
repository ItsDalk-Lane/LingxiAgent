# R05_NEGATIVE_GATE_REPORT — 附录 C 十六项故障注入（正反对照）

- 执行者：EXECUTOR-R05-T08。执行窗口：2026-10-04。脚本：`scripts/rust-tauri/r05_t08_negative_gate.sh`（注册的负向门禁生产者）。
- 结果：**16/16 全部按目标断言失败关闭（fail-closed）且缺口被点名；两个同环境正常对照全绿**。机器记录：`artifacts/rust-tauri/R05/T08-E01/negative/case-results.json`（`allRefused: true, controlsGreen: true`），逐例日志同目录 `<case>/`。
- 隔离方式：副本在 `$HOME/r05t08-work/negcopy`——对主仓库**只读**的本地 clone（HEAD `c549ff654`）+ 未提交工作树 overlay（rust/、scripts/、docs/rust-tauri/，rsync --delete、排除 target/）；主工作树零注入（每例前 `reset_copy` 从 pristine 快照还原全部被改文件；脚本结束保留最后状态仅为诊断）。副本 cargo 全部 `CARGO_NET_OFFLINE=true` 且剥离代理环境变量——负向验证本身零真实外发（N14 的语义正是禁止未经授权外发）。

## 对照组（同环境、注入前）

| 对照 | 命令 | 退出码 |
|---|---|---|
| xtask R05 镜像单测 | `cargo test -p xtask --bin xtask r05_`（副本内） | 0 |
| 二进制接线套件 | `cargo test -p lingxi-service --test r05_t01_binary_wiring`（副本内） | 0 |

## 逐例结果（case-results.tsv 原样；每例均为「单一目标变异 → 预期失败的检查真的失败且原因归因于对应保护」）

| 注入 | 变异（唯一目标） | 判定检查 | 退出码 | 实测失败原因（点名） | 判定 |
|---|---|---|---|---|---|
| R05-GATE-N01 | 从 R05.json 删除 R05-A16 场景 | xtask `r05_production_map_keeps_the_sixteen_a_scenarios_verbatim` | 101 | 断言失败点名 `R05 map dropped original scenario R05-A16` | OK |
| R05-GATE-N02 | pin 表一条 lib 路径改为匹配零测试（cargo 自身 exit 0） | 生产者完整运行 | 1 | `filter matched 0 tests (running=0, pinned=1) — empty test collections must not pass`（记于 suites/gaps.txt；生产者 fail-fast 于 cid 装配步先拒：`run … is not in the pin table`/`cid table owns 0`） | OK |
| R05-GATE-N03 | 钉数 7→6（svc:r05_t01_model_plane） | xtask `r05_stage_pin_table_matches_the_registered_suites` | 101 | `suite registrations drifted (dropped, added, or re-counted)` 点名该 run | OK |
| R05-GATE-N04 | ① 一个 lib 测试真实断言翻转失败；② 生产者的 `RUN_EXIT` 捕获被硬编码 0（吞掉子进程退出码） | 生产者完整运行 | 1 | 吞码后仍失败：`passed 0 but the stage pin is 1` + 失败测试被 cid 装配点名（`handle_refusal_texts_carry_no_material` has no `... ok` line） | OK |
| R05-GATE-N05 | 预置外来陈旧内容到 verify-stage 证据根 | `verify-stage R05` | 1 | `evidence root … is not empty; refusing to reuse it` | OK |
| R05-GATE-N06 | R02 注册门禁运行中向 kernel/lib.rs 追加一行注释（等证据根出现后才注入，非快照前） | `verify-stage R02`（副本） | 1 | `overall=FAIL` + `candidateSourceBinding.stable=False`（候选前后绑定不一致即禁止 PASS）。注：STABLE 机制在 candidate.rs/main.rs 为全阶段共享，以 R02 注册门禁演示（时长可承受）；R05 门禁消费同一实现 | OK |
| R05-GATE-N07 | 删除 r04_regression_gate 命令（场景仍引用） | `verify-stage R05` | 2 | `scenario R05-SUP-R04REG references unknown command "r04_regression_gate"`（命令存在性亦由镜像单测钉住） | OK |
| R05-GATE-N08 | lib.rs 接线条件反转（无注入 provider 时永不装配真实链——只剩测试构造器路径） | `--test r05_t01_binary_wiring` + `--lib` 对照 | 101 | 二进制链 `c02_real_binary_full_chain_through_authenticated_endpoint` 失败（未达 completed）；**同树 lib 单测 exit 0**（证明是正式启动接线被移除而非无关编译错） | OK |
| R05-GATE-N09 | tool_render 的 Success 臂改为固定字符串 `done` | `--test r05_t03_protocol_adapters c05_runtime_nonce` | 101 | 运行时 nonce 断言失败（下一请求不再含真实文件内容） | OK |
| R05-GATE-N10 | openai 适配器把工具结果配到常量 callId | `--test r05_t01_binary_wiring` | 101 | `call_bin_1` 配对断言失败（left/right 可见错配值） | OK |
| R05-GATE-N11 | StopUnconfirmed 状态臂压平成 `[process exited with code 0]` | `--lib models::tool_render::tests::running_and_stop_unconfirmed_statuses_are_honest --exact` | 101 | `[process stop unconfirmed …]` 诚实词表断言失败 | OK |
| R05-GATE-N12 | runs.rs 在工具执行前不释放模型 permit（1341 行 drop 移除） | `--test r05_t06_worker_model c06_` | 101 | 配额=1 嵌套链失败（callback 等待超时，非静默死锁） | OK |
| R05-GATE-N13 | R04.json 的一个 deferred 叶删除 basisKind（改判形态） | `verify-stage R04` | 2 | 阶段图解析/叶镜像校验硬拒（`deferred_to_later_stage` 义务不可改判） | OK |
| R05-GATE-N14 | R05.json 偷加读取 LIVE_API_KEY 的命令与场景 | xtask `r05_map_declares_no_live_lane_and_only_offline_commands` | 101 | 命令集漂移点名 `live_probe`（未授权 LIVE lane 不能注册；负向验证全程零真实外发） | OK |
| R05-GATE-N15 | 凭证拒绝 Display 泄露合成秘密 `sk-N15-SYNTHETIC-SECRET` | `--lib credentials::tests::handle_refusal_texts_carry_no_material --exact` | 101 | 无料断言失败（`sk-` 出现在错误文本） | OK |
| R05-GATE-N16 | 干净 R02 门禁 run-a → 改源码 → run-b；再对已用证据根重放 | `verify-stage R02` ×2 + 重放 | 1 | `digestsDiffer=True`（两次候选绑定摘要不同，机器可核验——旧结果不能冒充新树）+ 重放被拒 `is not empty`。注：run-a/run-b 的 R02 整体结果受已登记间歇 LAN 停驻环境项影响，与绑定机制正交（判定只取绑定属性） | OK |

## 负向门禁自身抓到并修复的真实缺口（fail-closed 于门禁作者）

1. **R05.json 漏注册 runner_identity**（run 3 N16 run-a 拒收 `stale xtask binary: source inventory differs; added=["stage_maps/R05.json"]`）：阶段图必须在 xtask `main.rs` 的 STAGE_MAPS 与 `runner_identity.rs` 的 EMBEDDED/SOURCE_INVENTORY **双处**注册——已补齐（这正是防“旧 runner 评估新图”的机制按设计工作）。
2. **生产者 FAILED 摘要解析回归**（run 3 N04）：为兼容 libtest 双语序补解析时误删一处捕获组（BSD sed `\1 not defined` → fail-closed 但报文误导）——已修复并主树全量复跑（82/82 绿）。
3. **N11 首版变异打错臂**（run 3/4）：顶层 `ToolOutcome::Unknown` 臂不被目标测试覆盖（该测试钉的是 Success 内的 StopUnconfirmed 状态臂）——变异已重定向到正确臂。

以上三点均先有正常对照、再做单一变异、失败原因归因到对应保护；未把无关编译失败当作证明（每例的变异都通过编译且同树相关对照保持绿）。

## 主树清洁声明

- 主工作树在全部负向运行期间与之后**零注入**：被改文件仅存在于副本；`git status` 与 R05 门禁的候选绑定摘要（working-tree-manifest）可复核。副本 `$HOME/r05t08-work/negcopy` 保留至阶段收口后由用户处置（含 .git 的本地 clone，约 6 GB）。
