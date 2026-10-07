# RR3 G-REVIEW-03 独立集成负测审查（默认全量十六项）

**结论：包级 PASS（G 集成负测范围）。default16-03 完整真实默认运行 16/16 fail-closed 且逐项点名，controls 绿，N03 恢复绿，N16 两次不同绑定且旧根拒收，full R02 与 full E5 真实执行，最终 Node verify PASS、12 文件逐字节恢复，默认 shell 真实 exit=0 已落盘（G02 的 UNKNOWN 缺口在本轮有效轮补上）。此前两次尝试（default16-01 宿主侧终止、default16-02 共享缓存污染）按失败原样保留，不拼进有效结论。本 PASS 仅覆盖 G 包级，不等于阶段终审；R05 仍需全新 FINAL 亲跑 §5.3，R06_READY 不由本包改写。mustFix：无新增产品/检查器缺陷。**

审查者为全新空历史独立会话（rr3_g_review_03），未参与任何实施/修复/前轮审查，未派子代理。只写本目录（含主脚本自建的隔离副本 `/Users/study_superior/r05t08-work/negcopy.IYrM8z`、`negcopy.n7dHPH`、`negcopy.f8U5qz`）；主树源码/脚本/docs 只读；无 Git 写操作；未删用户或历史证据；唯一删除物是本审查自己创建后被变异编译污染的 `~/.cache/lingxi-r05-neg-target`（见 §3）。

## 1. 权威与对象

全文读取 RR3_G_R3_BRIEF、RR1_MASTER_PROMPT、RR2_MASTER_PROMPT、RR3_BRIEF、RR3_REVIEW_BRIEF、RR3_G_R2_BRIEF、最新 RR3_ISSUE_MATRIX/PROGRESS/HANDOFF，A-REVIEW-02、B-REVIEW-01、C-F46-REVIEW-01、H-REVIEW-02、I-REVIEW-01、J-REVIEW-02、E-REVIEW-04 各完整 REVIEW.md，G-REVIEW-01 原始 case 数据、G-REVIEW-02 REVIEW/STOPPED/I-MAPPING、G-INTERRUPTION-01 REPORT、RR2/G-R2 NEG-GATE-RR2，以及生产脚本 negative_gate.sh / stage_suites.sh / prepare_git_copy.py / prepare_node.py / mutate_pin.py / legacy_entry_regression.sh 全文。

- 分支 `codex/rust-tauri-migration`，HEAD `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`；候选＝HEAD＋未提交工作树（overlay 子树 dirty 72 项逐项含 SHA 记录于 [overlay-dirty-manifest](metadata/overlay-dirty-manifest.json)）。
- 工具链：rustup 1.98.1（绝对 `/Users/study_superior/.cargo/bin/cargo`）、Node v24.16.0、npm 11.13.0、macOS arm64；`CARGO_NET_OFFLINE=true`、`--locked`。
- 环境修正核对：会话 HOME 已是 `/Users/study_superior`（无需 export，前后解析路径相同）。`R02_LEGACY_REGRESSION_MODE` 显式 `env -u` 清除（默认 full）。`CARGO_INCREMENTAL=0`、`PYTHONDONTWRITEBYTECODE=1`（与 G02 同款，如实记录；不改变断言/版本/测试范围）。
- **磁盘与暖缓存事实**：开工 df＝583Gi 可用（Data 卷）；brief 声称的 `~/.cache/lingxi-r05-neg-target` 21G 暖缓存**实际不存在**（TASK0/cargo-clean-receipt.txt 尾行的 "retained (21G)" 与实机不符，该回执是文字声明无存在证据）——default16-01 从零冷缓存构建。该差异已如实记录，不影响判定（空间充足）。

## 2. 三次执行尝试（全部真实生产入口，同一命令）

```bash
bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/RR3/G-REVIEW-03/<dir>
```

| 轮 | 目录/副本 | 结果 | 保留状态 |
|---|---|---|---|
| 1 | default16-01 / negcopy.IYrM8z | N01–N15 十五项有效目标红；N16 run-a 完整 R02 后，run-b 执行 a05 的 WS-matrix 步骤时**宿主侧终止了承载后台任务**（系统通知 status=killed），wrapper 未能落盘 exit（进程组被杀，UNKNOWN）；N16 行/reuse/Node verify/最终 reset/write_results 未到达 | 原样保留，[default16-01-interruption](metadata/default16-01-interruption.json) |
| 2 | default16-02 / negcopy.n7dHPH | **CONTROL 失败**：control-xtask 6/8 两项镜像测试看到 N14 变异形状（R05-LIVE-SNEAK、live_probe），而该副本 R05.json 与干净主树逐字节相同（sha a1e49423…）。根因＝共享 NEG_TARGET 残留 default16-01 N14 变异时编译的 xtask 二进制（指纹 66c0206f…，mtime 07:52Z），rsync -a overlay 保留主树旧 mtime，cargo 判 fresh **跨副本复用变异产物**（该 cargo 输出 0 条 Compiling）。真实 exit=1 落盘 | 原样保留，[default16-02-cache-pollution](metadata/default16-02-cache-pollution.json)、[neg-target-before-deletion](metadata/neg-target-before-deletion.json) |
| 3 | default16-03 / negcopy.f8U5qz | **完整有效**：删除被污染的本轮自建 NEG_TARGET 后冷缓存全量重编（control-xtask 16 条 Compiling），16/16 全绿判定，真实 exit=0 落盘（结束 2026-10-07T10:59:20Z） | 本轮签收依据 |

default16-02 的缓存污染是 brief 红线「缓存复用须核实际重新编译来源；保旧 mtime 的旧二进制复用不得充作还原绿」的实证反面形态（跨轮共享 NEG_TARGET + mtime 保留 overlay），属**流程/基础设施教训**，非产品或检查器缺陷：负测控制检查自身正确地在第一时间拒绝了污染运行（fail-closed 生效）。处置只删本轮自建缓存，不触历史证据。

## 3. default16-03 有效轮逐项结果（N01–N16）

准备段：git-preparation 完整 CoW clone（38,066 tracked + overlay，dirty=72）与 node-preparation（64,760 依赖，ws 真实解析、vitest 真实启动）均 PASS。Controls：xtask r05 镜像 8 passed/0 failed/0 ignored/113 filtered exit0；r05_t01_binary_wiring 2 passed/0 failed/0 filtered exit0（全量真实重编，[recompile-evidence](metadata/recompile-evidence.json)）。

| 项 | exit | 点名（gap-named） | 有效性依据（本轮原证） |
|---|---:|---|---|
| N01 | 101 | R05-A16 | 删 A16 后 `r05_production_map_keeps_the_sixteen_a_scenarios_verbatim` 单测红点名 `R05 map dropped original scenario R05-A16`；0/1/120 filtered，非编译失败 |
| N02 | 1 | filter matched 0 tests | 完整 producer（stage_suites.sh 全量构建+27 suites+64 pins）真实运行，gaps 点名 `lib-adapters/...the_buffer_bound_is_loud_XYZ: filter matched 0 tests (running=0)`——G02 ENOSPC 缺口本轮有效抵达 |
| N03 | 101 | drifted + svc:r05_t01_model_plane | 权威表唯一目标恰一次 24→23（mutation.json：matches=1/mutations=1/old/new/前后 SHA）；镜像 0/1/120 红；字节恢复后精确 1/1 绿（PASS N03 restore） |
| N04 | 1 | handle_refusal_texts_carry_no_material + count-check | 真断言失败（credentials 翻转）+ 生产者吞退出码（RUN_EXIT=0）后，parsed-count/cid-ownership 检查仍拒绝并点名失败测试 |
| N05 | 1 | not empty | 预置 stale.json 的证据根被 verify-stage R05 拒收：`evidence root ... is not empty; preserving its prior evidence` |
| N06 | 1 | overall=FAIL + stable=False | sync-exit=0（初始绑定完成且权威首命令启动信号先于变异捕获）；kernel 追加后 stable=false、reason=候选字节/HEAD 在检查期间变化禁止 PASS；合法前置抵达绑定机制，非业务失败代偿 |
| N07 | 2 | references unknown command | 删 r04_regression_gate 后场景引用硬拒 `references unknown command "r04_regression_gate"` |
| N08 | 101 | c02 + never reached | 接线条件反转后 binary 套件 `c02_real_binary_full_chain_through_authenticated_endpoint` 红且 panic 于 never reached；lib 358 全绿（lib-exit=0 对照保持） |
| N09 | 101 | c05_runtime_nonce | 固定 "done" 后运行时 nonce 检查具名红 |
| N10 | 101 | call_bin_1 | tool_call_id→常量后配对断言 `left: "call-fixed-mispaired" / right: "call_bin_1"` 红（1 passed/1 failed 对照形状） |
| N11 | 101 | honesty 单测 | StopUnconfirmed 压平为 exit-0 文本后 `running_and_stop_unconfirmed_statuses_are_honest` 红 |
| N12 | 101 | c06 嵌套链 | 删 drop(model_permit) 后 `c06_main_run_worker_and_callback_complete_under_global_model_concurrency_one` 红（c06_a_held… 有界拒绝对照 ok） |
| N13 | 2 | basisKind 缺失 | R04 deferred 叶删 basisKind 后 verify-stage R04 硬拒 `supplemental leaf ... missing basisKind` |
| N14 | 101 | live_probe | 私开 live lane 后 offline-only 镜像红，命令集 diff 点名 `live_probe` |
| N15 | 101 | no-material | HandleRefused Display 拼入 sk-N15 合成秘密后 `handle_refusal_texts_carry_no_material` 红 |
| N16 | 1（reuse 腿） | digestsDiffer + reuse refused | run-a/run-b 两次完整 R02 真实写出绑定：digestA `a3ec4a31…` ≠ digestB `dce6f0e8…`（独立复算相等）；已用根 run-a 再用被拒 `is not empty`；exit 三腿 1/0/1 |

汇总判定（生产脚本自写）：`{"cases": 16, "allRefused": true, "controlsGreen": true}`；RESULT 行 16/16 controls green。逐项 exit-code/named 原证见 default16-03/ 各 n* 子目录（exit-code.txt、test.stdout.log/run.log、gaps.txt）。

## 4. full R02 与 full E5 的真实结果（G02 notRun 范围的覆盖）

默认 16 项本身即涵盖这些生产入口：N02/N04 走完整 R05 producer；N06/N16 共三次完整 `verify-stage R02`（20 注册命令）；a16 命令即 `r02_t08_legacy_entry_regression.sh` 默认 full 模式（E5 全量 npm）。

- **N16 run-a（干净副本，stable=True）**：19/20 命令 PASS；a16_legacy_regression FAIL——E5 全量 npm 真实运行 198.2s，`Test Files 3 failed | 1471 passed | 3 skipped`、`Tests 6 failed | 15031 passed | 15 skipped (15058)`，其中 6+15031+15=15052≠15058：一次 `Worker exited unexpectedly` 使 6 个测试脱离组件计数，R5-F01 parseability 判 UNPARSEABLE **fail-closed**（`components sum != total`）。分类器行为正确；失败属 vitest worker 崩溃的环境偶发，非绑定失败、非产品逻辑失败。
- **N16 run-b（kernel 追加注释后，stable=True，绑定摘要不同）**：**20/20 命令 PASS，overall=PASS**。a16 的 E5 全量 npm 完整执行且 seal-family 分类成功：候选红 ⊆ baseline replay 红 ∪ 登记预存族（seal trio），`E5-no-new-reds`、`E5-cause-classification`、`E5-block/cause-coverage` 全 PASS，最终行 GREEN（candidate raw exit 1 保留登记、"registered, not formal green"）。**full E5 在 run-b 以登记分类口径真实通过**；run-a 的失败被 run-b 证为环境偶发。
- **F47/F48/F50 修复后的 R02 业务面**（多轮一致真实结果）：a01_smoke PASS、a13_redaction_scan PASS、a05_a06_auth_matrix PASS、supplemental_client/cli_rust/cli_sessions_matrix PASS——G01 四轮中的 7 项失败在当前候选上全部消失（default16-01 与 03 合计五轮 R02 均如此）。
- **已知环境项如实区分**：default16-01 run-a 中 supplemental_management_matrix 曾 FAIL（`request to 192.168.3.5:56376 stalled ... 0 bytes read`，测试自身标注 macOS 防火墙/proxy TUN 可能拦截未签名测试二进制的非回环入站）——与已登记 BLOCKED 的 R05-ENV-R00/D 同族；default16-03 两轮中该命令 PASS（ALF 拦截随二进制/路径偶发）。该偶发不构成新 mustFix；D 的最终解除仍按总控计划待用户侧允许入站操作。
- Node verify（`verify_node_dependencies`，N16 后末尾执行）：`node-verification/result.json` status=PASS，checkedFiles=64,765——G02 notRun 的 Node 终末核验完成。
- 最终恢复：N16 后 `reset_copy` 消费同一 12 文件表逐项 cp+cmp（脚本内），本审查另独立复算 12 文件 copy↔pristine SHA 全等、copy dirty=72 与 overlay 候选一致（[independent-final-restore](metadata/independent-final-restore.json)）——G02 notRun 的最终 reset 恢复完成。

## 5. 输入、程序与证据身份

- 关键 23 项输入前后 SHA（negative_gate.sh=7ca614a7…、stage_suites、pins/cids/registry TSV、R02/R04/R05 stage map、Cargo.lock、toolchain、package 输入）见 [source-before](metadata/source-before.json)；主树 HEAD/分支/期间未变（本轮全程主树只读）。
- 复用输入相等性（[reuse-input-equality](metadata/reuse-input-equality.json)、[j02-source-final-equality](metadata/j02-source-final-equality.json)）：
  - H-REVIEW-02 全部 375 项逐字节相等 → H02 的资源 160 轮/54+61 点/15 存活 worker/45 稳态、F46 六轮、F47/F48、两个普通取消具名测试按原范围复用。
  - A1 17 项（A-REVIEW-02 与 J-REVIEW-02 双重核对）全等 → A1 的 121 检查/内外 checkpoint 复用。
  - I-REVIEW-01 21 项中 20 项相等，唯一差异 negative_gate.sh＝J02 准备段修改（7ca614a7… 与 J-REVIEW-02 main-sources 三时点一致），已由 J-REVIEW-02 独立新验；I 的恢复/同步/N03 机制证据按此边界复用。
  - A delivery 19 项中 18 相等，唯一差异 r02_t08_legacy_entry_regression.sh＝J02 BASE 构造邻接（J-REVIEW-02 已新验 legacy 段）。
  - J-REVIEW-02 main-sources-final 1016 项中 998 项相等；18 项差异全部是 docs/rust-tauri（E03 文档轮 12 份 owned 文档＋总控台账 6 份），已由 E-REVIEW-04 PASS 验收；**rust/ 与 scripts/ 执行代码零差异**。
- 三轮 console/exit/case 行索引见 [run-index](metadata/run-index.json)；观察记录 [observations](metadata/observations.jsonl)；df 全程 ≥566Gi（每大步骤前后核）。

## 6. I01–I11 与边界

逐项映射见 [I-MAPPING.md](I-MAPPING.md)。要点：本轮新增的完整默认 16+恢复把 I11（门禁自证）从「未完成」变为「本轮亲跑 16/16+恢复绿」；I01–I09 的完整 producer 组合消费路径与 G02 判断一致仍待新 FINAL 的 verify-stage R05 全链亲跑（旧 91 runs/427 过属 H 变更前输入；本轮 N02/N04 的 producer 是注入态下的 fail-closed 证据，不代正常全绿）；I10 按 H02 375 项输入相等原范围复用。

## 7. 失败、保留与 mustFix

- 新增产品/检查器必需缺陷：**无**。E5 worker 崩溃与 management ALF 拦截均如实归因为环境偶发/登记环境项；缓存污染为流程教训（已在 §2 记录并实证），控制检查 fail-closed 行为正确。
- 保留不改：G-REVIEW-01 默认 FAIL（exit2/15 行/PRIMARY 并行改写）、G-REVIEW-02 BLOCKED_BY_STORAGE、G-INTERRUPTION-01 归档、default16-01/02 两轮失败现场、旧 raw npm seal trio 登记红（非 formal green）、D 的 LAN 阻断（R05-ENV-R00 BLOCKED）、LIVE 未授权与 Windows/Linux 平台延期。
- 本包 PASS 仅 G 级；阶段仍 NOT_ACCEPTED/R06_READY=false，待新 FINAL（RR3/FINAL-01 原文 §5.3）与新 E 回填/新审。本轮全程无 Git 提交/推送，无系统权限修改，未调用真实收费供应商。
