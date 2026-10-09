# REVIEWER-R06-T02-R3 第三轮独立对抗性审查报告

- **TASK_ID**: R06-T02(token 预算、压缩与长会话)
- **审查者**: REVIEWER-R06-T02-R3(一次性独立对抗性审查代理,未参加本 Task 的实现、修复与前两轮审查)
- **基点**: `b174e3ff44c61000abbb84595cf69dfad85dd531`(分支 `codex/rust-tauri-migration`),与修复者自报 TASK_BASE_SHA 一致
- **候选**: HEAD + 未提交工作树(无 commit)
- **候选摘要核对**: 自报 `3cabcb4647fe1d53d68f330ee0df6ed85b8602a0aa564d066c963f00dbac5023` —— **不可复现**(详见 R3-F-02 与证据 `00_digest_diagnosis.txt`)
- **工具链**: `/Users/study_superior/.cargo/bin/cargo`,rustc 1.98.1 (48a229cea 2026-09-01)(锁定版)
- **证据目录**: `artifacts/rust-tauri/R06/T02-REVIEW-03/`(`00_digest_diagnosis.txt` / `01_workspace_test.txt` / `02_probe_output.txt` / `probe/`)

---

## 裁决:**FAIL**

一项 MEDIUM 阻断(R3-F-01:摘要模型路由的架构级分叉未声明且误锚现役),两项 LOW(R3-F-02 候选摘要不可复现;R3-F-03 交叉锁鉴别力缺口+报告过述)。R1 六项与 R2 全部修复项经独立复证确认关闭;根因簇闭环真实;R05 回归全绿(1609/0)。MEDIUM 项触及「记录在案差异」完整性这一验收基石,按 R2 判例(线体分叉+误锚=MEDIUM 阻断)同型处理。

---

## 一、审查面覆盖声明(亲自查证,非转述)

| 面 | 范围 | 方法 | 结论 |
|---|---|---|---|
| A. 原始 Task 重审 | Goal/4 Steps/3 Deliverables/R06-A03/A04/Depends On=T01/生产调用链 | 报告 §一~§三 与源码逐行对照;生产链 bootstrap→drive_run→loop 顶触发→compact_exchange 逐跳核实 | 成立(除 R3-F-01 的锚定缺口) |
| B. 根因簇闭环 | 40 行矩阵、FIX-01..13、RC-3 五设计、§十 16 条 | 矩阵 40/40 行分类核对(15+ 行行为级实证);FIX 逐项复证;RC-3 鉴别力对抗实验;§十 逐条+现役锚点行号抽查 | 基本闭环;交叉锁有鉴别力缺口(R3-F-03) |
| C. 全新对抗探针 | 6 组(A/B/C/D1/D2/D3/E/F),全部新构造不重放 R1/R2 | 独立 probe 项目(path 依赖真 crate),loopback stub + 真 StoragePort | 全过;产出 1 个 CONFIRMED GAP(F1)与 R3-F-01 的行为级实证(D2/D3) |
| D. R05 回归 | diff 逐行 + 受影响套件 + 全量 | 14 个已跟踪文件 diff 逐行;r05_t05_timeouts(service 9/9 + adapters 13/13)、r05_t05_compat 1/1;`cargo test --workspace --locked` | **1609 passed / 0 failed / 0 ignored,exit 0** |
| E. 防虚假完成 | FIX-01 绕过面/FIX-02 续传/FIX-07 误触发/不修改面 | grep 全部 `AuxiliarySlot::Summarize` 消费点(唯一构造点);探针 E 两轮续传;探针 A/B2 边界与护栏;`summary_output_cap` 公式与回退方向未动 | 无虚假完成迹象 |

---

## 二、Findings

### R3-F-01(MEDIUM,阻断)

- **FINDING_ID**: R3-F-01
- **SEVERITY**: MEDIUM(行为分叉未声明 + 报告误锚;方向为能力退化/诚实降级信息丢失,不致 crash)
- **REQUIREMENT_ID**: 任务书步骤①(迁移现役压缩语义)/④;R06-A03
- **FILE_AND_LINE**:
  - `rust/crates/lingxi-service/src/compaction.rs:336-352`(摘要路由 = `AuxiliarySlot::Summarize` 独立槽)
  - `rust/crates/lingxi-service/src/compaction.rs:373-376`(fit 检查窗口 = summarize 路由 `compat.context_window`,`unwrap_or(0)`)
  - 现役对照:`core/session-compactor.ts:1985`(`runCachePreservingCompactionForSession`,`model = session?.model`)、`:2070-2076`(fit 检查用同一 `model` = **会话模型**)
  - 报告 `docs/rust-tauri/R06/R06-T02_REPORT.md` §十#12(把「fit 检查用摘要模型窗口」锚给现役 `session-compactor.ts:2064-2102`)
- **OBSERVED_BEHAVIOR**: Rust 压缩摘要调用走 **summarize 辅助槽**(R05 C07 独立解析、未配置响亮失败),fit 检查窗口、族分流、max_tokens 回填全部取自 summarize 路由快照。行为级实证(探针 D2/D3,`02_probe_output.txt`):
  - D2:summarize 路由存在但**未声明 contextWindow** → 每次触发都 `HardTruncated`(marker 摘要替换旧区、**0 次摘要模型调用**、0 台账行;第二次触发被无进展护栏拦为 NotTriggered)。
  - D3:summarize 槽**整体缺失** → 每次触发 `Failed{detail: "the summarize slot has no resolvable route…"}`、0 物理调用、原交换逐字不动、run 续跑(上下文无限增长)。
- **EXPECTED_BEHAVIOR(现役)**: 现役压缩摘要调用用 `session.model`(**会话模型本人**),fit 检查用会话模型窗口;现役 `AUXILIARY_SLOTS.summarize`(`core/auxiliary-slots.ts:83-88`)的消费方是 activity 摘要/autolearn(`core/engine.ts:4854/4866`、`lib/autolearn/autolearn-service.ts:166`、`core/agent-manager.ts:596`),**从不服务会话压缩**,且带 `fallback: "chat"`。同配置下(只配 chat)现役压缩正常工作。
- **REPRODUCTION**: `cd artifacts/rust-tauri/R06/T02-REVIEW-03/probe && cargo run`(探针 D2/D3 输出逐字如上)。
- **ROOT_CAUSE**: 执行者把「压缩摘要调用」绑到 R05 的 summarize 辅助槽是新架构决策(工程上合理:槽语义即「摘要」),但它不是现役语义的迁移——现役压缩根本不经辅助槽。决策本身可辩护,问题在:(a) §十 16 条无一声明该分叉;(b) §十#12 把 Rust 的 summarize 窗口来源锚给现役 `session-compactor.ts:2064-2102`,而现役该处窗口是会话模型的——误锚;(c) §五 锚定表无「摘要模型来源」行。
- **SAME_ROOT_CAUSE_PATHS**: 同槽消费的全部派生面:fit 窗口(已述)、族分流/max_tokens(D1 实证按 summarize 路由)、凭证/端点(可指向另一 provider,前缀缓存不命中)、能力门(tools:true 声明)。报告 §三 生产链描述如实写了 `resolve_dispatch(Auxiliary(Summarize))`,故非隐瞒,是「差异清单漏收 + 一处误锚」。
- **IMPACT**: 用户未配 summarize 槽 → Rust 压缩永不发生(每 turn Failed+warn,run 续跑至上下文爆炸);配了但未声明窗口 → 每次触发诚实硬截断(marker 替换真实历史,用户无感知丢上下文);绑异构模型 → 缓存/窗口/族判定口径分裂。三个面都生产可达(当前无默认 plane 模板内嵌 summarize 绑定)。
- **REQUIRED_FIX**: 二选一:(1) §十 新增条目声明「压缩摘要模型 = summarize 辅助槽(现役 = 会话模型)」及上述三后果,并修正 §十#12 的窗口来源锚定、§五 补「摘要模型来源」行;(2) 改实现走 chat 路由(设计回退,需重证族分流/fit 面)。推荐 (1)——决策本身合理,缺的是声明与锚定诚实。
- **REGRESSION_TESTS**: 现有测试已覆盖行为面(D2/D3 同形:`hard_truncation_when_the_summary_window_is_undeclared` 类 + `unresolvable_summarize_slot_fails_loudly` 类测试存在于 service 套件);需补「§十 条目 ↔ 测试引用」同步(交叉锁自动覆盖新条目)。

### R3-F-02(LOW,流程)

- **FINDING_ID**: R3-F-02
- **SEVERITY**: LOW(候选可验证性受损;实质正确性由本轮独立复跑与源码审查承担)
- **REQUIREMENT_ID**: 报告头部 CANDIDATE_DIGEST 自报字段;T01 post_acceptance_obligation F-03
- **FILE_AND_LINE**: `docs/rust-tauri/R06/R06-T02_REPORT.md:6`(自报值);`artifacts/rust-tauri/R06/T02-REPAIR-02/11_candidate_digest.txt:1-3`(实际执行命令与自证声明)
- **OBSERVED_BEHAVIOR**: 按报告声明口径与 11 号文件实际执行口径重算,及排除任何后写文件组合的 6 种变体,均 ≠ 自报值(证据 `00_digest_diagnosis.txt`)。mtime 时间线:报告含 digest 值且定型于 04:52:23,11 号(digest 载体)落盘 04:52:47 ——「全部文件写完之后计算」的声明被文件时间戳反驳。
- **EXPECTED_BEHAVIOR**: 候选摘要应在全部口径内文件定型后计算、可独立复现(T01 F-03 义务明文)。
- **REPRODUCTION**: `00_digest_diagnosis.txt` 内全部命令可重放。
- **ROOT_CAUSE**: 三层叠加:(a) `11_candidate_digest.txt` 自引用(在口径内且含 digest 字符串,数学上不存在不动点);(b) 声明口径(`grep -v '^docs/.../R06-T02_REPORT.md$'`)与实际执行(`grep -v 报告自身`,字面子串匹配不到任何文件 → 报告被算入)不一致;(c) 计算时点早于报告/11/12 落盘。
- **SAME_ROOT_CAUSE_PATHS**: T01 F-03(同型,已被 T01 验收列为义务);任何「自报摘要」字段。
- **IMPACT**: 审查者无法以摘要确认「所审即所交」;只能靠全量重跑+逐文件审查补位(本轮已做)。
- **REQUIRED_FIX**: 口径排除 digest 载体文件自身(或摘要不含载体内容);所有口径内文件定型后最后计算;声明口径与执行命令逐字一致。
- **REGRESSION_TESTS**: 无(流程项)。

### R3-F-03(LOW)

- **FINDING_ID**: R3-F-03
- **SEVERITY**: LOW(防再发机制的弱环 + 报告过述)
- **REQUIREMENT_ID**: RC-3 §五-4(报告-测试交叉锁)
- **FILE_AND_LINE**: `rust/crates/lingxi-service/tests/r06_t02_compaction.rs`(`report_deviation_register_matches_test_references`:missing 检查 = 条目→至少一次引用;stray 检查 = 引用 > max);报告 §十 引言(L174 附近)声称「引用集合 == 条目集合,每条目**恰好一次**引用」
- **OBSERVED_BEHAVIOR**: 探针 F1 重实现交叉锁逻辑实证:「删 §十 中间条目 N、代码中 `§十#N` 引用残留」的组合 **逃逸检测**(missing 只查 entries 内条目;stray 只查 > max,残留引用 ≤ max 不报)。另:报告声称「恰好一次」,实现是「至少一次」——过述。
- **EXPECTED_BEHAVIOR**: 交叉锁应双向:引用 ∉ 条目集合即红(含 ≤max 的残留);报告措辞与实现一致。
- **REPRODUCTION**: 探针 F1(`02_probe_output.txt`);静态读测试断言两行即见。
- **ROOT_CAUSE**: stray 检查用 `> max` 近似「∉ 集合」,省了一个 contains。
- **SAME_ROOT_CAUSE_PATHS**: 无其他同类锁。
- **IMPACT**: 防再发鉴别力打折——篡改组合「删中间条目+留引用」不变红;现状下无实际错误(条目与引用当前一致,本审查已核对)。
- **REQUIRED_FIX**: stray 改为 `!entries.contains(n)`;报告措辞改「至少一次」。
- **REGRESSION_TESTS**: 在测试中自证:构造篡改样例(临时目录)断言检测器变红。

---

## 三、确认关闭项(独立复证)

- **R1 F-01**(孤儿切点): `plan_compaction` 的 `suffix_min_owner` 无孤儿证明逐行核实;探针 B1 顺带确认摘要切点合法。
- **R1 F-02**(cache 双计): `context_tokens_from_usage` 的族 inclusion 仲裁逐行核实。
- **R1 F-03/F-04/F-05/F-06**: placeholder 恢复臂(探针 C 行为级)、`apply_plan(mid_run)` 参数化(探针 E 手动无 notice)、估算措辞、auxiliary 两臂文本(源码核实)——全部成立。
- **R2-F-01**(FIX-01 族分流): 探针 D1(outputCapRequired=true → max_tokens=4096 上线;false → 不豁免族清单)+ closed_loop 断言(optional 族线上无键)+ kernel `output_cap_required` 判定顺序与现役 `output-budget.ts` 逐字。**绕过面**: `AuxiliarySlot::Summarize` 全仓库仅 compaction.rs 两个消费点,唯一请求构造点 = `summary_request`,kernel 层无请求构造能力,不可绕过。
- **R2-F-02**(FIX-02 fileOps): 探针 C(恢复×enrichment 组合,placeholder 意图路径不污染清单)+ 探针 E(两轮手动压缩播种∪新增续传)行为级成立。
- **R2-F-03/F-04/F-05**(FIX-03/04/05): 回退方向注释与代码一致且未顺手改;repair 载荷=当轮 rawText(源码 L588 附近核实);split-turn 行位置与现役 410-414 逐字对照一致。
- **FIX-07**(fit+硬截断): 探针 A(window=0/1/MAX/85% 精确边界)、B2(护栏三态:仅摘要→NotTriggered、有内容→HardTruncated 且 0 调用、产物再进→NotTriggered 不互替循环)成立。**误触发面**: 估算口径 CJK×1.1 偏高(保守方向),窗口未声明硬截断与现役同臂——无误触发。
- **FIX-02/04/08 等 R2 其余项**: 已核。
- **「不修改」面**: `summary_output_cap` 公式(max(512, floor(0.8×reserve)),探针 A: 16384→13107、0→512)、`plan_compaction` 回退方向——均保持,未被顺手改。

## 四、矩阵与 §十 核对摘要

- 40 行矩阵:全行分类核对;行为级实证 ≥15 行(#2/#4/#6/#8/#10/#11/#15/#16/#21/#23/#24/#25/#26/#27/#31/#32);「已声明差异」行逐条对 §十 落位(#1→#9、#3→#16、#7→#8、#9→#9、#12→#10、#13/14→#11、#19→#13、#22→#6、#31→#12、#34→#14、#35→#15,不适用行 §十 末段声明)——除 R3-F-01 的架构分叉漏收外,归位完整。
- §十 16 条:现役锚点抽查(session-compactor.ts:377/1867-1875/1893-1915/1985/2064-2105、runtime:79/201、bridge:1710、compaction-utils.ts:36-38、output-budget.ts、agent-run.ts:122-135/523-538/601)全部真实;归因除 #12 窗口来源(R3-F-01)外合理;未发现其他漏报(端到端走查现役 runCachePreservingCompactionForSession 生产链)。
- RC-3 五设计:黄金线体 fixture(closed_loop 真实出站断言)、语义清单参数化、指令模板两臂、族矩阵三维——鉴别力属实;交叉锁缺口即 R3-F-03。

## 五、回归证据

- `cargo test --workspace --locked`(rustc 1.98.1,真实退出码): **1609 passed / 0 failed / 0 ignored,CARGO_EXIT=0**(`01_workspace_test.txt`)。已知同型 flake(r04_rr1_f05_spill_failure、cancellation_tree)本次直接全绿,未触发。
- fmt/clippy: EXIT=0(本轮源码零改动,前轮证据仍有效;本轮复跑确认)。
- R05 受影响套件: r05_t05_timeouts(service 9/9 + adapters 13/13)、r05_t05_compat 1/1 亲跑绿。
- 探针: 6 组全过,`02_probe_output.txt`,DIRECT_EXIT=0。

## 六、附加 informational(非 finding)

1. `cache_preserving_request_fits` 的 `saturating_mul` 在 window > u64::MAX/8500(≈2.17e15)时阈值失真为 u64::MAX/10000——理论边界,无生产意义(探针 A 记录)。
2. `output_cap_required` 的 provider/endpoint 匹配大小写敏感,现役 `output-budget.ts` 用 `lower()` 归一;方向为假 optional(线上无 cap 键),真实 required 端点(api.anthropic.com)由 endpoint 判定救回(探针 F2 记录)。
3. 矩阵 #6 措辞「严格大于阈值」不准——现役 `compaction.js:201` 与 Rust 同为 `>=`(边界 80_000 触发),结论「已对齐」正确。
4. 报告 §五「估算器对齐 hana CJK×1.1」与 §十#10 的自洽性已核;CJK 高估方向保守(风险 #3 已声明)。
