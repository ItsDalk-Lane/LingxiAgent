# R06 最终阶段审查报告（STAGE-REVIEWER-R06-AR-R1）

- 审查者：STAGE-REVIEWER-R06-AR-R1（全新空历史，最终阶段审查）
- 基线：`c432a24ea38b3b0ca96f6248421ecb4995357bf9`（worktree `/tmp/lingxi-stage`，全程 `git status --porcelain` 为空）
- 工具链：`/Users/study_superior/.cargo/bin/cargo --manifest-path /tmp/lingxi-stage/rust/Cargo.toml --locked`（1.98.1），`CARGO_TARGET_DIR=/tmp/stage-target`，合成环境 `/tmp/stage-env`
- 结论：**CONDITIONAL_PASS（有条件放行进入 R07 门禁执行段）**；正式阶段状态在条件落地前仍为 R06_NOT_ACCEPTED。

---

## 1. 15 项 AR 发现核对

### 1.1 台账状态（亲读 AR_ISSUE_MATRIX.json @ c432a24ea）
- F01–F07、F09–F13、F15 共 14 项：`independent_review.status=PASS`，`remaining_scope=[]`，`DONE_INDEPENDENT_REVIEW_PASSED_STAGE_GATE_PENDING`；F08 `DONE_COMMITTED_PUSHED_REMOTE_VERIFIED`（repair_commit bc2ce51fd，W08 独立 PASS + 远端回执，deliveries/W08_PASS.md）。
- F14：`STAGE_GATE_EXECUTION_PENDING`，remaining_scope 仅「verify-stage R06 正式门禁执行（本发现由门禁本身关闭）」，与「门禁尚未跑」的实态一致——未虚关。
- F12 修复链完整：RV5 FAIL（RV5-F12-01）→ RV6 FAIL（2 处残留）→ RV7 PASS（7/7 append + 回归闸 + 负控）；RV7-F12-O1（minor 观察项）已在基线源码 `rust/crates/lingxi-service/src/knowledge.rs:2699-2710` 落实（账目错误在手时 stats/progress 失败以 append 出现，注释标 RV7-F12-O1），亲读确认。

### 1.2 独立审查报告抽验（rv1–rv7 全读，亲跑 3 簇锚点）
| 簇 | 报告判定 | 本人亲跑（c432a24ea） |
|---|---|---|
| F01 历史上下文（rv1） | PASS（844a10b2b 62/62） | `--test r06_ar_f01_history`：**8 passed / 0 failed**（c01–c04、五族 wire 角色、unknown、child 裁剪、archive/reset 竞态），真实二进制（CARGO_BIN_EXE_lingxi-service）+ 认证 HTTP + fixture probe |
| F12 缓存消费者（rv5/rv6/rv7） | 终局 PASS @180abc207 | `--test r06_ar_f12_cache_consumers`（LINGXI_HOME=/tmp/stage-env/home）：**6 passed / 0 failed**，含回归闸腿与 provider 腿（SYNTHETIC_PROVIDER_ERROR + cleanup append 断言） |
| 知识面（rv4/rv7 相邻面） | F13 PASS / knowledge 12/12 | `--test r06_original_knowledge_proofs`：**12 passed / 0 failed** |

三簇锚点数字与各 RV 报告逐一吻合，无漂移。

### 1.3 F14 裁定
**裁定：有条件放行——「verify-stage R06 在全新证据目录真实执行且 exit 0」即 F14 关闭；不要求在本审查内先跑门禁。**
理由：F14 的 C01/C04 腿已有可自动执行绑定且 C01 登记校验本人亲跑通过（`r02_registry_consistency.py`：`PASS: migrations.rs MIGRATIONS == registry (13 entries…)`，v8/v9 及 r06 v10–v13 全部登记，原「registry 止于 v7」缺陷已修复）；C02/C03/C04 由门禁与负控电池本体承担，属主控放行后的既定动作。
**但存在一个登记级阻断（必须先修，见条件 C1）**：F14-C02 的 `executionCommandRefs`（365 项）包含 real-gui 人工绑定 `r06_test_910e4bcfed2b7f485283`（`native_desktop_resource_roundtrip_real_gui`），其 argv **无 `--features e2e-test`**（r06_stage_inputs.py 生成 argv 时 binding 无 features 字段）。该套件首行 `#![cfg(feature = "e2e-test")]`（desktop_host.rs:1），无特性编译为 0 测试；执行器结算规则（r06_stage_inputs.py:588-592）要求精确 `running 1 / ok 1 passed`，「0 匹配……不能通过」。因此**按现行登记直接跑 verify-stage R06 会在该绑定上确定性红**。放行前必须把该绑定移出自动执行清单（改人工证据回执绑定，或随两叶转 R09 移除）。

## 2. 裁定项 a) GUI 证据口径（R00-T02-LA-2FB382A137FC / CAA876503B1B）

**裁定：接受该证据口径，两叶可改 READY——以「人工证据回执」绑定（gui-proof-final 证据包 SHA），不经自动重放；R09 重建桌面宿主后须在新宿主回归一次 OS 行为。**

亲验（非转述）：
- `gui-test4.log`：`test native_desktop_resource_roundtrip_real_gui ... ok`，1 passed / 180.23s；`NATIVE_UI_PHASE {"…phase":"COMPLETE","pid":2187}` 与 `proof/e2e-phase.json`（COMPLETE@2187）一致。
- OS 打开：`proof/os-open-observation.json` 记录 program=TextEdit、windowTitle=selected.txt、observedWindow=true，截图 `os-open-window.png` 在案。
- 废纸篓字节：`~/.Trash/lingxi-native-trash-2187.txt` 本人回读存在（27 字节 UTF-8，sha256 448d7977c53d…），字节恰为测试写入内容「真实系统废纸篓载荷」（desktop_host.rs:539 写、:577 读回断言），pid 与 e2e-phase 一致。
- 事件链：`proof/resource-events.json` 含真实 subscribe→外部修改→`resource.changed`（provider_watch, sequence 1..n）。
- 执行器拒绝自动重放是**正确**的反假绿行为（e2e-test 特性 + 真人点选不可无人重放；自动绑定零匹配会被结算规则拒绝）；该拒绝针对自动门禁，不否定人工证据本身。
- 负面路径互补：OS 打开叶的「相对路径、缺失、非文件拒绝」由可自动运行的 `native_resources.rs::real_file_read_write_copy_and_version_boundaries`（:139-141 `open_file("relative"/missing/目录)` is_err）覆盖，正负两面合围。

结论：证据真实、可独立复核、绑定到真实锚点测试与物理副作用；维持 GAP 反而会把已完成的真实证明降级为未完成。改 READY 时必须同步：登记形态为人工证据回执（绑定证据包各文件 SHA），不得留在 F14-C02 自动 commandRefs（否则触发 §1.3 的确定性红）。若门禁机制无法编码人工回执绑定，回退方案＝维持 GAP 转 R09 并随附证据包（此时该绑定同样必须移出自动清单）。

## 3. 裁定项 b) 87 项 GAP 宿主归属（A–L）

事实底座（亲算）：87 GAP 中 **33 叶 R00 账本归属为 R06-ONLY**（无任何后继阶段映射），其余 54 叶带 R07/R08/R09 份额。引文规则核对结论：**存在属「配置正确性」「知识来源约束」两类的 R06-ONLY 叶被 A–L 清单按类别整体推后，另有 EX-01（历史上下文×配置）核心份额未动——这三组不能靠登记放行，须回 R06 最小宿主**。嵌入一致性无 GAP 残留（F09–F12 全 PASS）；历史上下文/会话文件引用的发现级义务（F01/F04）已 PASS，残余为入口面与单句条款。逐类裁定：

| 类 | 叶簇（代表） | 裁定 |
|---|---|---|
| A 模型工具注册 | ls/grep/find/file/materialize（B97D、EC77、F88B、3CBC、AD7F）；learn_lesson/record_experience/notes/tasks/checkpoint（7712、A17D、6280、7244、6207） | 文件族：**R07 承接**（ResourceIO 局部证明保留、未虚关；缺的是模型目录注册面）。记忆/知识工具族：**R06 最小宿主**——由 EX-03/EX-06 承载，其 deadline 明示 R06 验收前核心份额，不能推后 |
| B WS 流 | D483（WS prompt run）、8166（resume）、C1FB（steer）、7CBF（abort）、9262（interject）、421E（WS compaction） | 服务端 WS 契约缺断言绑定的：**R06 补最小服务份额**（服务侧 WS 入口自 R03/R05 已存在，R08 只接旧客户端）；客户端/回执显示面 R08 |
| C slash | 2FAA、3FCB（/reset）、0FE8、5A6E（compact 族）、4EA8（stop） | **R08 slash 宿主承接**（入口面）；但 compact/fresh-compact 的**服务语义核心份额属 EX-01，必须 R06 内建**（见五类裁定） |
| D dream ticker | A9AD、B45E、76EE、0B48 | **R07 ticker 宿主承接**（B7b 已如实登记「ticker 宿主 R07 未移植」） |
| E summary store | 396F、53B6（总结来源清理） | 由 **EX-04 承载：R06 核心份额**（记忆整理业务闭环当期必需），界面/调度后续；不得整体推 R07 |
| F watch | 2382（file-changed IPC 接线）等 | **R09 承接**（服务侧 watch 已实证存在——resource-events.json 真实 provider_watch 事件；缺桌面接线） |
| G GUI 族 | 23+ 界面叶（2067、A02D、0DDB、99F、8E2D、D8D4、4173、BDE3、52A5、9CA8、94CB、B34F、A2C3、FBDE、81B2…） | **R08/R09 承接**（当前 Tauri frontend 仅 probe 页，属界面宿主，登记如实） |
| H 记忆遗留 | 04EE（tenets-changed）、0779（removedCount）、7563（agent-updated）、7432（检索状态）、DCF0（experience 刷新）等 | **服务端事件/字段补全＝R06 最小宿主**（R06-ONLY：广播事件与工具返回字段是服务侧行为，skills-changed 同型先例已在 R06 内完成）；UI 层转 R08 |
| I 知识面 | **01E1（笔记本删除+冻结引用）、8A62（reingest 归属）、9283（delete_source studio 核验）、76E2（jobs studio 限定+不可达状态）、5F53（outline_scope 越权/无源报错）** | **全部 R06 最小宿主，不得推后**（知识来源约束类、R06-ONLY、服务侧检查，亲读源码确认未实现：delete_notebook 仅删 notebook/membership、list_ingestion_jobs 全库查询无不可达状态、reingest/delete_source 未核 studio/membership） |
| J Office 桥 | 2F2B（docx-html）、3617（xlsx-html）、93D5（read 抽取） | 由 **EX-07 承载：R06 核心份额**（任务书明示「可达原格式当前份额必需，可使用受控成熟解析器」）；完整编辑/渲染 R08 |
| K 文件布局 | 31CDA（归档移回）、BB0A（edit-start 检查点）、4F04（沙盒条款） | 31CDA：R06 份额（sidecar 路径/冲突校验语义）**R06 最小宿主或显式产品决策**（DB 承载无文件布局已如实 PARTIAL），物理迁移 R08；BB0A：桌面宿主 R09；4F04：单句条款（OS 沙盒不可用拒绝）**R06 内裁决补证或显式降级** |
| （人格/头像族，归入 A-L 之「其他」） | 4870、7C69、856B（agent-updated/描述刷新）、4A56、7570、7F0F、BB4C、E4BE（role 头像/ETag）、3516（deleteSkills）、BA49（reload 广播）、BC67（config.yaml GET）、FF4F（sidecar 冻结提示词读取）、E162（rewind 用户批准）、980C（研究子代理开关） | 多为 **R06-ONLY 服务侧行为：R06 最小宿主**（事件广播/字段/GET 面）；其中 **BC675C664910（助手 config.yaml 完整 GET+全局注入+秘密掩码）属「配置正确性」，明确不得推后**；EX-12 已如实标注这是「冲突，不是已获延期许可」 |

**五类规则命中清单（不得靠登记到 R07 放行，须 R06 完成或维持 R06 未完成状态）：**
1. 配置正确性：`BC675C664910`（R06-ONLY，未实现）；EX-01 的 enabled/keepRecent/reserve 真实消费（常量→真实设置）。
2. 知识来源约束：`01E1E3CF7FA8`、`8A62470CC75D`、`9283371CDAD8`、`76E2F5AAF035`、`5F5365322CFF`（均 R06-ONLY）；EX-07/EX-08/EX-09 的核心份额为 EX 承载同域义务。
3. 历史上下文：发现级（F01）已 PASS；EX-01 核心份额（compact_now 真实提交入口）R06 到期。
4. 会话文件引用：发现级（F04）已 PASS；残余 `4F0466F7C090` 单句（沙盒不可用拒绝）与 `31CDA283A11A` 的 R06 份（sidecar 移回校验）。
5. 嵌入一致性：无 GAP 残留（F09–F12 全 PASS，我的 f12 亲跑 6/6）。

**登记卫生发现（不阻塞、须修）**：`269958F82F8A`（grep 越权）登记的 missingClause 称「越权source明确拒绝未实现」，与当前源码不符——knowledge.rs:1971-1975 `require_scope_source` 逐 id 整单 `KNOWLEDGE_SCOPE_VIOLATION`，且 r06_original_knowledge_proofs.rs:1440/1521/1538、r06_t07_knowledge_tools.rs:632/644/671 均有断言（我亲跑 12/12 绿）。该叶 gap 文案过时（保守方向的 under-claim，非假绿），须重锚；重锚后大概率可翻 READY。

## 4. 裁定项 c) 12 项 EX 义务

**裁定：如实、未虚关。** AR_ORIGINAL_OBLIGATIONS.json 12 个 extra_root_clusters 全部 `NOT_RUN`、`evidence_level=SOURCE_ONLY`、`runtime_evidence=[]`；deadline 统一为「R06 阶段验收前：这里只要求当前核心/服务份额」；current_vs_later 逐项写明「完整 UI/调度可后续，R06 核心须……」。AR_PROGRESS：`extra_open_count=12, extra_closed_count=0`，且「未减少15F或60C」。EX-12 甚至显式标注 persona/avatar 叶的 R07 映射是「冲突，不是已获延期许可」。无任何一项被虚报关闭。
注意：如实的另一面是——12 项核心份额按其自身 deadline 仍是 R06 验收前到期义务，构成放行条件而非可转移项（与 §3 五类裁定合并）。

## 5. 登记一致性与 spot check

- `python3 -B scripts/rust-tauri/r06_stage_inputs.py --check --evidence /tmp/stage-review/inputs-check`：**exit 0**（inputsDigestSha256=85d571554403f51f…，快照 280 requiredLeaves；即 R06.json/R06_INPUTS.json 与任务册/R00 账本/AR 矩阵/叶绑定再生成完全一致）。注：需先把 gitignored 的 `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/task-catalog.json` 拷入 worktree（脚本拒绝缺料是正确行为）。
- 192 READY / 87 GAP 亲数吻合。随机 5 叶 READY（F829AEA5931C、B097BDDC1E10、2CAF7E2573D8、EA26288DA2DB、24BC2E961E3E，seed=20261014）：断言原文与 R00 FEATURE_STAGE_ACCEPTANCE 逐字一致（5/5）、groups 锚点测试全部存在于登记套件（5/5）、coverage 标注 FULL。其锚点套件加跑 3 个：r06_original_memory_skill_proofs **19/19**、r06_original_skills_bundle_proofs **12/12**、r06_b7b_config_scope_prefs **2/2**。

## 6. 环境限制与未执行项
- 未执行：verify-stage R06 全量门禁与 R05→R02 闭包（属放行后主控动作，F14 由其关闭）；Windows/Linux；真实供应商 LIVE；正式打包；TS 三套 vitest 重跑（RV5 已 30/30，RV7 锚点抽查成立，本轮未重放）；lingxi-service 全 workspace 重跑（RV1 曾 62/62 于 844a10b2b，本轮按抽验口径跑 8 个套件全绿）。
- `/tmp/lingxi-stage`（worktree）、`/tmp/stage-target`、`/tmp/stage-env`、`/tmp/stage-review/`（inputs-check 快照、gap-leaves/gap-stages 清单、本报告与 VERDICT.json）保留备查。

## 7. 裁决与条件（详见 VERDICT.json）

**CONDITIONAL_PASS**。放行进入 R07 门禁执行段，条件：
- C1（门禁前置，登记修复）：把 `r06_test_910e4bcfed2b7f485283`（real-gui 人工绑定）移出 F14-C02 自动 executionCommandRefs——按裁定 a) 改人工证据回执绑定（绑定 gui-proof-final 证据包 SHA）并翻两叶 READY，或维持 GAP→R09 并移除绑定；否则门禁确定性红。
- C2（F14 关闭条件）：全新证据目录 `verify-stage R06` exit 0（含 C03 前序闭包与 C04 负控电池）；C01 登记腿本人已预验 PASS。
- C3（五类 R06 到期完成项，最终验收前）：EX-01 核心份额（compact 真实入口+真实设置消费）；BC675C664910 config.yaml GET；知识来源约束五叶（01E1/8A62/9283/76E2/5F53）服务侧最小宿主；269958F82F8A 重锚（过时文案，源码已修）；4F04 沙盒条款补证或显式降级；31CDA 的 R06 份裁决。
- C4（开放面如实延续）：33 项 R06-ONLY GAP 叶与 12 项 EX 核心份额维持登记开放、按 §3 归属逐类推进（R06 最小宿主或既有后继宿主），不得以登记代替实现；GUI 两叶按 a) 翻 READY 后 R09 新宿主回归一次。

在 C1–C4 落地前，正式阶段状态仍为 R06_NOT_ACCEPTED；本放行授权的是门禁执行与上述可执行完成项，不是整段验收追认。
