# F02 批次0 域级独立审查（重绑语义对应 / 判据变化 / 证据链 / D18）

状态：**FAIL**（2026-10-22）。审查者未参与任何实现；只读审查＋可运行只读命令，未修改产品文件、未运行 cargo。
配套机器可读逐断言判定：[F02_BATCH0_DOMAIN_REVIEW.json](F02_BATCH0_DOMAIN_REVIEW.json)。

**结论一行：整体 FAIL —— 8 叶中 6 叶通过（run_code / lsp / security_scan 为 PASS_WITH_NOTES，ptc / ast_grep / learn_lesson 为 PASS），D17 两叶 `R00-T02-LA-09D567E99524`、`R00-T02-LA-D5514BBDBF88` 不通过：断言1『拒绝时目标状态不变』的后半句未被所绑案例 `bridge_settings_receipt_and_rich_streaming_toggles_persist` 实际断言（非法权限模式只断言 400，拒后无任何状态回读），而绑定 rationale 声称『非法权限模式拒绝后状态不变』——属 F02 要消除的超claim 错绑，按审查授权（断言原文与案例实际断言不对应一律 FAIL）判 FAIL。**

审查对象：[F02_REPAIR_PLAN.md](F02_REPAIR_PLAN.md) 批次0 执行记录；`docs/rust-tauri/R07/R07_LEAF_BINDINGS.json` 8 叶；`rust/crates/lingxi-service/tests/` 对应测试函数体（逐条读原文断言，不以绑定表摘要或准入复验 §3.1–3.5 为准，仅作对照）；`docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json` 原断言原文。

## 1. 逐叶判定

### 1.1 run_code `R00-T02-LA-C88F29B5114A` — PASS_WITH_NOTES

| 原断言 | 案例 | 函数体实际断言 | 判定 |
|---|---|---|---|
| A0 两次调用可共享变量 | `python_and_node_real_kernels_preserve…`（r08_admission_run_code.rs:426-486） | python/node 各先写入 x=40/plus/导入，第二次独立调用算出 42（:446-450）；Node 顶层 await/动态 import 跨调用保留；跨会话/Agent 不共享（:470-477） | PASS |
| A1 重启后旧变量消失 | 同上 + `real_thirty_second_cap…` | restart 后新 kernelId 且 `print(x)`→Failed『not defined』（:478-483；超时测试 :706-710 同证） | PASS |
| A2 取消清理内核 | `cancelling_real_run_stops_persistent_python_kernel_and_its_actual_grandchild`（:610-684） | 真实取消→Run Cancelled；内核与真实孙进程两 PID 全灭；`supervisor.live_handles()` 为空 | PASS |
| A3 语言无效、无会话、内核死亡或超时显式失败；重启不声称保留旧状态 | `absent_real_interpreters…`、`output_is_real_bounded…`、`real_thirty_second_cap…` | 缺解释器→RUN_CODE_LANGUAGE_MISSING＋安装指引＋不启内核（:362-403）；死亡→KERNEL_EXITED/DEAD 不静默重建（:587-600）；真实 30 秒 TIMEOUT、busy 保持、restart 后 not defined（:690-710） | PASS_WITH_NOTES |

备注：(a) 未支持语言名分支（RUN_CODE_INVALID_LANGUAGE，ruby）在套件内有绿样本（`real_gateway_status…:550-552`，leaf-cases ok=true）但未绑入 A3，A3 的语言分支由缺解释器分支立据；(b) 工具级无会话分支无样本为 rationale 如实披露，会话层对不存在会话的拒绝另有未绑定绿样本（:553-567）。均无虚claim，属可选增强。

### 1.2 run_tools `R00-T02-LA-0818574ABD43` — PASS

| 原断言 | 案例 | 函数体实际断言 | 判定 |
|---|---|---|---|
| A0 每个子调用独立权限判定 | `real_child_approval_is_separate…`（r08_admission_ptc.rs:356-405）＋ ptc_authority 三项（:55-195） | 外层批准≠子批准 approvalId，子 write 被拒→文件未写；批准等待中撤真实授权/禁用/换代→拒绝派发（`assert_not_dispatched`：journal Failed＋持久 Failed 收据＋dispatched=false，:655-663）且目标未写 | PASS |
| A1 外层只显示打印/返回值 | `real_parallel_children_only_return_selected_summary…`（:322-355） | 外层正文只含『selected summary』与返回值 2，SECRET_CHILD 不入正文；模型历史恰 1 条 ToolResult 无子内容 | PASS |
| A2 失败显示已执行子动作 | `later_program_failure_keeps…`（:406-418） | 外层 throw→Failed 含 outer failure；retained.txt 保留；subcalls[0].ok==true | PASS |
| A3 未知/自调用/无权限/超时/沙盒拒绝；不回滚 | `unknown_recursive_and_sandbox_failures…`、`real_default_ten_minute_timeout…`、`actual_vm_escape…` | 未知名与 run_tools 自调用→ToolCallError＋2 条 ok=false 持久子回执；沙盒外访问→RUN_TOOLS_EXCEPTION（:419-443）；vm 逃逸读/写 EPERM、TCP BLOCKED、零副作用（:522-570）；真实 ≥600s RUN_TOOLS_TIMEOUT、已完成动作保留、worker 回收（:572-625）；无权限见 A0 组、不回滚见 A2 组与超时案例 | PASS |

### 1.3 ast_grep `R00-T02-LA-196D50D8DD6E` — PASS（单案例覆盖三条，成立）

`r08_ast_structure_pagination_no_match_and_errors_use_real_run`（r08_admission_ast_lsp.rs:367-450）：

| 原断言 | 函数体实际断言 | 判定 |
|---|---|---|
| A0 命中结构而非同文字符串 | 文件内布置同文字符串 `'greet(99)'`（文本搜索会多命中），`greet($A)` 断言 matchCount==2，断言文案即『真实 AST 不得命中字符串』（:384-388） | PASS |
| A1 无匹配不写文件 | 全部操作（含无匹配与恶意 sgconfig）后 example.js 逐字节不变（:445-448）、敌意 sgconfig 未被消费（:403-406） | PASS |
| A2 无匹配为空结果；语言/解析失败或越权给错误 | `missing($A)`→matchCount 0＋『no matches』（:411-418）；invalid-language Failed（:419-425）；『Cannot parse query』Failed 且注明不得冒充无匹配（:426-436）；`../outside.js` Failed（:438-444） | PASS |

### 1.4 lsp `R00-T02-LA-2D896560381E` — PASS_WITH_NOTES

| 原断言 | 案例 | 函数体实际断言 | 判定 |
|---|---|---|---|
| A0 位置与源码一致 | `r08_lsp_real_clangd_positions_preview_rename_and_rejections`（:452-549） | 参考列由源码 UTF-16 偏移（同行含 emoji）计算；真实 clangd definition/hover/symbols/references 逐项命中 | PASS |
| A1 rename 仅改返回编辑范围 | 同上 | rename applied 后文件恰为全替换、收据恰 1 份（:519-525）；错位 prepare 拒绝 Failed（:526-530）；preview 分支 applied=false＋文件不变（:510-518） | PASS |
| A2 不可写时只预览不落盘 | `r08_lsp_read_grant_rename_returns_preview_without_writing`（:551-565） | 只读授权 rename→applied=false、LSP_WRITE_UNAVAILABLE、preview 含新名、文件逐字节不变 | PASS |
| A3 服务缺失/崩溃、不可重命名或写能力缺失报错；无编辑文件不变 | `r08_lsp_missing_and_crashed_server_are_explicit_errors`（:567-674） | missing→LSP_SERVER_MISSING Failed；crash→『exited』Failed；空 WorkspaceEdit→changedFiles==[]＋文件不变；cancel→真实进程树回收 | PASS_WITH_NOTES |

备注：『不可重命名』（:526-530）与『写入能力缺失』（LSP_WRITE_UNAVAILABLE）两分支由同叶 A1/A2 组案例实际断言，未列入 A3 自身案例组；批次0 规则允许同案例跨组复用，可零成本补列。叶内全覆盖，无虚claim。

### 1.5 security_scan `R00-T02-LA-CD5D7FC02D8E` — PASS_WITH_NOTES

| 原断言 | 案例 | 函数体实际断言 | 判定 |
|---|---|---|---|
| A0 发现与实际文件对应 | changes＋baseline（r08_admission_security_cases.inc:54-95、:3-31） | changes 模式 filesScanned==1 恰为被改 tracked.ts；baseline 在唯一含密钥的 credential.ts 上 findingCount>0 且密钥不出现在结果/审计 | PASS_WITH_NOTES（无 finding.path==实际文件的直接断言，由单密钥布置＋filesScanned 间接成立，准入独审 §3.5 已登记） |
| A1 范围不足明确 | baseline | binary/超限 filesSkipped>=2＋正文含 skip 说明（:20-23） | PASS |
| A2 无越权读取 | outside_root＋external_scanner（inc:32-53、:97-167） | 根外路径/符号链接拒、外部哨兵不泄漏、外部文件不变；扫描器进程沙盒内读/写失败、孙进程回收；changes 子根不枚举父 | PASS |
| A3 非Git/越权/上限/引擎失败标注，不伪称全量 | changes＋outside_root | 子根无 .git→Failed SECURITY_GIT_SCOPE_UNAVAILABLE（:87-90）；路径越权→Failed（:37-45）；文件上限/引擎失败无直接样本 | PASS_WITH_NOTES（两分支仅机制级核对，rationale 如实披露并登记 R08 跟踪缺口，不构成虚claim） |

### 1.6 learn_lesson `R00-T02-LA-771296A194E0` — PASS（单案例覆盖四条，成立）

`original_lesson_review_one_shot_content_binding_conflict_and_real_retrieval`（r06_skills_completion.rs:315-404）：

| 原断言 | 函数体实际断言 | 判定 |
|---|---|---|
| A0 技能文件存在且内容可读 | SKILL.md 存在且正文含教训内容（:343-346） | PASS |
| A1 风险未确认时不写 | 无 risk_accepted→requiresRiskConfirmation=true、目录不存在、config 不变（:321-327）；改内容＋旧 token→ok=false 仍不写（:328-336） | PASS |
| A2 索引与Agent启用状态一致 | experienceIndexed=true；agentA 启用、agentB 不含；Experience 索引含该课；运行时目录 enabled==true（:341-377） | PASS |
| A3 缺上下文/同名/写失败不生成伪课程；明确接受＋一次性token | 同名重交 err 且文件不变（:378-380）；missing Agent err（:381）；skills 池被堵 err 且无处落盘（:389-402）；token 一次性、内容改变即作废须重新接受（:328-339） | PASS |

### 1.7 D17 两叶 `R00-T02-LA-09D567E99524`、`R00-T02-LA-D5514BBDBF88` — FAIL

所绑案例 `r07_probe_access::bridge_settings_receipt_and_rich_streaming_toggles_persist`（r07_probe_access.rs:576-611，真实服务＋真实 HTTP）：

- **A1 前半句『字段缺省不改变原值』——成立**：只提交无关字段 `permissionMode:"auto"` → 200，同响应回读 receiptEnabled=false、richStreamingEnabled=true 均保持（:590-600）。
- **A1 后半句『拒绝时目标状态不变』——不成立**：非法 `permissionMode:"god"` 仅断言 `status==400`（:601-609），响应体被丢弃（`let (status, _)`），其后无任何回读/GET，测试结束。**函数体没有断言拒后状态不变。**
- 绑定 rationale（两叶同文）声称该案例『同时断言 true/false 保存、无关字段缺省不动原值、**非法权限模式拒绝后状态不变**』——最后半句为超claim。『状态不变』目前只有源码机制保证（`bridge_routes.rs settings_route` 先校验后写，:486-498），无测试样本。
- 附带发现（A0，NOTE 级）：两字段源码默认皆为 true（`bridge_routes.rs:204`，非显式 false 即 true）。receipt 叶只 exercised `false`（恰为非默认方向，保存＋持久真实成立，true 方向无样本）；**rich 叶只 exercised `true`——与默认同值，回读 true 无法区分『保存了 true』与『从未写过』，非默认 false 方向（该开关的实质切换）无任何绑定案例**，A0 证据强度显著弱于 receipt 叶。
- 判定：两叶 **FAIL**。修复方向（批次A，rust/ 解冻后）：同一探针在 400 后补一次读回、补 rich=false 方向写入；或改绑覆盖该语义的案例。

## 2. 判据变化审查（复用规则放宽）— PASS

- **单断言组内重复仍禁**：apply 脚本 `len(set(group))!=len(group)` 报错（r07_apply_leaf_bindings.py:102-104）；xtask 同构（stage_map.rs:1235-1240『repeats case … within original assertion』）；改写后测试第三段仍断言组内重复报错（stage_map.rs:1933-1941）。两处一致。
- **pinned 全部必须被指派**：stage_map.rs:1243-1253 `assigned.sort()+dedup` 后与 `contract.cases` 数量相等，否则『has pinned cases not assigned』；反向（引用必须在 pinned 内）在 :1225-1230。双向闭合。apply 脚本 pinned=`dict.fromkeys(flat)` 首现去重（:124），与 xtask dedup 语义一致。
- **空组/组数不符仍报错**：apply :94-107；xtask :1188-1215。一致。
- **空证据/零匹配不可绕过**（verify.rs 通读）：生产者命令必须本轮通过（:470-480，防旧证据回放）；证据文件必须存在、schema 合法、cases 数组非空（空数组即『exit-0-only fake-green』拒绝，stage_map.rs:601-607）；整份文件逐条案例 ok=true 且 actual==expect（含未被叶钉住的条目与重名拒绝，verify.rs:536-553）；每个图钉案例必须记录在案且 actual==map expect==file expect，缺失即 MISSING（:554-589）。图钉 expect=1 下零匹配或未记录均无法过门禁。
- **红绿证据如实**：xtask-tests-01 exitCode 101 是改写后测试的夹具不一致（错误『pinned cases not assigned』——测试自身钉了未指派案例，不是校验器拒绝合法跨组复用），与执行记录『首轮夹具错误』口径一致；-02 exitCode 0、128 passed。
- 备注：放宽仅限同叶跨断言组复用；每叶图钉集与指派集仍双向相等，不能借复用塞入不证明任何断言的案例。

## 3. 证据链核对 — PASS

- **suites-02**：8/8 exitCode=0（r06_skills_completion、r07_probe_access、r07_t12_closure、r08_admission_{ast_lsp,ptc,ptc_authority,run_code,security}）；8 份 output.log sha256 与 result.json logSha256 逐一相符；`r07_stage_inputs.py:91-107` 对 r08_admission_* 设 RUST_TEST_THREADS=1、ptc 设 TEST_TIME_LIMIT_SEC=660、ast_lsp 注入 ast-grep 固定 PATH。
- **leaf-cases**：60 案例 0 未持有；8 叶去重后恰 24 个 pinned 案例，全部 actual=1/ok=true/expect=1；绑定表与 `stage_maps/R07.json` 注入结果逐叶一致（groups 与 pinned 全等）。
- **绑定回执**：f02-batch0-binding-check-01 exitCode 0，candidateDiffSha256=d7fc8d13…；与准入候选 cc7f7052（admission-authority-run-01/candidate.diff sha 实测吻合）逐文件逐 hunk 比对：8 个共用文件（desktop/lingxi-kernel/lingxi-service/scripts/dev-env/tests）完全相同，新增仅绑定表＋xtask＋两脚本——『lingxi-service 源未变』属实；测试文件不在任一 diff 中，本审查所读函数体即实跑版本（sourceSha 86840d17 两轮一致）。
- **suites-01 红证据**：r08_admission_ptc exitCode=101（601.061s），`cancelling_actual_program_reaps_its_real_node_process` 于 r08_admission_ptc.rs:473 失败 left:3 right:1。读函数体（:444-487）：按 ppid==本测试进程＋args 含 r08-ptc-worker.mjs 过滤并断言恰 1 个外围进程——并行时兄弟测试的 worker 共享同一父 PID 必然≥2，『环境错配非产品缺陷』的解释成立；串行后同套件 602.326s 通过。红证据如实保留。
- **xtask**：-02 128 passed；-01 红证据见上节。

## 4. D18 未重绑决定 — 正确（CORRECT）

- `real_tool_suggestion_requires_apply_and_replays_after_expiry`（r07_automation_flow.rs:302-340）只证：模型建议不自行启用、apply→返回 job、**同 body 过期后重放 200 同 id 幂等（1 job）**；未证 095E0402EEEC A0 的『过期/冲突拒绝』（同 body 重放恰是成功路径）与 A1 的拒绝可见性。
- `slash_apply_uses_frozen_owned_suggestion`（:342-374）只证 5F394AA4C9A3 A0（写入自动任务＋返回标题＋恰 1 job）；未证『找不到建议/Studio 身份缺失』拒绝。
- 执行记录对两测试能力的描述与函数体一致；批次0 是纯重绑窗口（rust/ 冻结），补拒绝测试须留批次A；只重绑 A0 会留下半真半错的完整行为声明。不重绑的决定正确。
- 细节补正：(a) 『已应用时不创建重复任务』切片已由 cron 重放测试在 desk/cron 端点真实覆盖，批次A 只需补斜杠端点同语义与各拒绝分支；(b) 批次顺序节（执行前文本）曾写批次0 重绑这两叶，执行记录如实记录偏离及原因；(c) 批次A 完成前这两叶仍是 WRONG_ENDPOINT 旧绑定，不得计为批次0 成果。

## 5. 问题清单

| 编号 | 级别 | 叶 | 问题 |
|---|---|---|---|
| F02-B0-1 | **FAIL** | 09D567E99524、D5514BBDBF88 | 断言1『拒绝时目标状态不变』未被所绑案例实际断言（400 后无状态回读）；rationale 声称已断言，超claim |
| F02-B0-2 | MAJOR_NOTE | D5514BBDBF88 | A0 仅 exercised 默认值方向（true），非默认 false 方向无样本，保存效果不可与默认区分 |
| F02-B0-3 | NOTE | 09D567E99524 | A0 仅 exercised false 方向（成立）；true 方向无直接样本（默认即 true） |
| F02-B0-4 | NOTE | C88F29B5114A | A3 的 RUN_CODE_INVALID_LANGUAGE 与会话层无会话拒绝有套件内绿样本未绑定；rationale 无虚claim |
| F02-B0-5 | NOTE | 2D896560381E | A3 两分支由同叶 A1/A2 组案例断言，未列入 A3 自身组，可零成本补列 |
| F02-B0-6 | NOTE | CD5D7FC02D8E | finding.path 无直接断言（间接成立）；文件上限/引擎失败无直接样本（已披露） |

## 6. 诚实性声明

- 已执行：8 叶原断言逐条对照测试函数体与关键源码（bridge_routes.rs 默认值与先校验后写）；apply 脚本、stage_map.rs、verify.rs 门禁通读；全部回执/日志哈希核验；候选 diff 与准入 cc7f7052 逐文件逐 hunk 比对。
- 未执行：cargo/测试运行（任务约束）；批次0 之外 400 叶的既有绑定复核；非 macOS 平台。
- 本结论绑定当前候选（sourceSha 86840d17…、diff d7fc8d13…）；候选变化后须重新复验，不得沿用。
