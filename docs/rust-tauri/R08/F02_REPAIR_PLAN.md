# F02 错绑修复计划（149 条同因复核集）

状态：BATCH0_CLOSED＋BATCHA_IN_PROGRESS（2026-10-22）。批次0 复验 PASS_WITH_NOTES（8/8 叶）。批次A 已完成 D18 错端点两条（新拒绝测试 13/13 绿＋重绑 4/4 pinned 绿）；剩余 D17×7、D04×2＋条件成功组、D16×3。批次B—C 未开始。本文其余部分为分析与执行记录；依据 [ADMISSION_F02_SCOPE_REVIEW.md](ADMISSION_F02_SCOPE_REVIEW.md)（2026-10-21 独立范围审查）与其配套 JSON 的 363 条逐叶记录。

## 批次0 执行记录（2026-10-22）

1. **复用规则调整**（判据变化，待域独审）：`r07_apply_leaf_bindings.py` 与 xtask `stage_map.rs` 同步放宽"叶内案例不重复"为"单断言组内不重复"；同一真实案例可证明多条原断言（F02 审查结论），pinned 案例按首现去重、全部必须被指派。xtask 128 项测试通过（含改写后的 `full_original_behavior_requires_distinct_cases_for_every_original_assertion`，回执 `f02-batch0-xtask-tests-02`；首轮夹具错误红证据保留在 `-01`）。
2. **套件注册**：r08_admission_{run_code,ptc,ptc_authority,ast_lsp,security} 与 r06_skills_completion 按同一生产者命令注册进 R07 地图；ast_lsp 注入 ast-grep 固定 PATH，ptc 设 TEST_TIME_LIMIT_SEC=660（独审已接受的机器保护器余量）。
3. **串行化红证据**：首轮经生产者跑 r08_admission_ptc 未串行（exit 101，`f02-batch0-suites-01/SUITES/r08_admission_ptc/output.log`：cancelling_* 断言"恰好一个外围进程"在并行下必然看到 3 个）——环境错配非产品缺陷；修复为 RUST_TEST_THREADS=1 后 `f02-batch0-suites-02` 全量 8/8 绿（600.87s→通过）。红证据保留。
4. **重绑 8 叶**：五工具叶→准入套件（映射取独立准入复验 §3.1–3.5）；learn_lesson→`r06_skills_completion::original_lesson_review_one_shot_content_binding_conflict_and_real_retrieval`；D17 两条（09D567E99524/D5514BBDBF88）断言1 从 A23 改绑 `bridge_settings_receipt_and_rich_streaming_toggles_persist`（A23 保留为附加路由证据）。D18 错端点两条**未**重绑：既有 automation_flow 测试只证成功幂等，不证"过期/冲突拒绝""找不到建议/Studio身份缺失"，部分重绑会复制造假——移入批次A 补测后绑定。
5. **验证**：`r07_apply_leaf_bindings.py` 408 叶注入退出0；xtask 128 绿；8 套件串行全绿（`f02-batch0-suites-02/SUITES/*/result.json`）；leaf-cases 60 案例 0 未持有；批次0 的 24 个 pinned 案例全部 actual=1/ok=true；绑定回执 `f02-batch0-binding-check-01`（候选 diff d7fc8d13…，与准入 cc7f7052 的差异仅 scripts/xtask/绑定表，lingxi-service 源未变）。
6. **待办**：批次0 域级独立审查（核语义对应与机械规则调整）；其后批次A。

## 数据来源与口径

- 范围：`sameCauseScreeningUnion=149`（40 UI-vitest、102 A23 替代、1 纯客户端、7 条件成功、4 错端点，集合交叉去重）。
- 每叶每断言的现绑定与不足标记在 `ADMISSION_F02_SCOPE_REVIEW.json` 的 `leaves[]`（含 `usesOnlyA23ForThisAssertion`、`reviewVerdict`、`proofBoundary`）。
- 绑定落点：`docs/rust-tauri/R07/R07_LEAF_BINDINGS.json` → `scripts/rust-tauri/r07_apply_leaf_bindings.py` 注入 `rust/crates/xtask/src/stage_maps/R07.json`。
- 修复必须保持：R00 原断言原文不改；A23 保留为路由/鉴权附加证据但不再充当业务断言唯一证据；一个真实案例可同时证明多条断言（见下"机械规则调整"）。

## 按域工作清单（叶数/断言数/标记不足断言数）

| 域 | 叶 | 断言 | 不足 | 主要在用套件 | 修复形态 |
|---|---:|---:|---:|---|---|
| D20 | 35 | 77 | 18 | probe_access、t08_model_authority、ui_vitest | 访问/设置：撤销、失败保留、刷新失败；多组 UI 需配 Rust 证据 |
| D21 | 31 | 76 | 26 | probe_skills、t08_skills_cards、t08_card_previews | 技能：11 条顺序错位重绑＋面板/设置补真实服务配对 |
| D19 | 17 | 44 | 13 | diary、phone_runtime、session_collab、probe_access | 会话/日记/手机：cookie 净化、Agent 误启、升格点击补 UI |
| D12 | 14 | 28 | 14 | media_admin | 全部"拒绝/无效输入"绑 A23；按端点补真实拒绝测试（见下） |
| D10 | 13 | 54 | 8 | t08_provider_http、t08_settings、t08_speech_bindings | 供应商/设置：错端点 2 条改真实 `PUT /api/config`；成功夹具 |
| D22 | 13 | 42 | 10 | t10_observability | 观测：健康/记录开关/分页/保留删除绑错子Tab/文件名 |
| D17 | 11 | 30 | 9 | bridge_flow、bridge_session_routes、probe_access | Bridge：缺省/冲突/身份分支；`bridge_settings_receipt…` 可复用 |
| D18 | 5 | 15 | 1 | scheduler | cron suggestion/slash apply 改绑 `r07_automation_flow` 既有真实测试＋补拒绝组 |
| D16 | 3 | 6 | 1 | office_tools | 少量补真实边界 |
| D04 | 3 | 8 | 2 | probe_access | QR 签发失败必调用端点；批准配对 secret 一次性 |
| D13 | 3 | 11 | 0 | 仅 ui_vitest | 纯 UI 层叶：配对真实服务案例 |
| D03 | 1 | 4 | 0 | 仅 ui_vitest | 同上 |

## D12 模式（最规整，建议首批）

全部 14 叶同构：断言0（成功读）已绑 `r07_media_admin` 真实案例；断言1（"拒绝/无效输入：无越权或错误持久化，错误对用户可见"）仅绑 A23。修复：在 `r07_media_admin`（或新套件）按端点写真实拒绝案例——无凭证/非法身份访问受守读端点断言 401/403 与无状态变化、无效 filter/ID 断言 4xx 与错误可见；重绑断言1，A23 移为附加证据。预计 1–2 个参数化测试函数覆盖 14 叶。

## 机械规则调整（需独审）

`r07_apply_leaf_bindings.py` 现强制"叶内 case 不重复"。F02 审查已确认一个真实测试可证明多条断言，该规则不得再迫使错绑。调整方案：允许同一 case 出现在同叶多个断言组，但要求 (a) 每断言组非空；(b) 逐断言列出"该案例实际断言了什么"的绑定理由（rationale 字段）；(c) 调整本身作为判据变化交独立审查复核，历史回执不回写。

## 修复循环（每域一批）

1. 只读核对：域内每叶原断言、现绑定、现有真实案例（含 `reusableCandidates` 14 条）。
2. 重绑 `R07_LEAF_BINDINGS.json`；缺真实覆盖的断言先补测试（rust/ 变更须在候选窗口内，等独立审查完成、候选解冻后进行）。
3. 生成器校验通过 → 注入 R07.json → `r08_map_integrity_probe.py` 或对应 map check 实跑。
4. 实跑受影响套件留回执（rustup 1.98.1、r08_run.py 记录）。
5. 按域独立审查：核"原断言→案例实际断言内容"语义对应，拒绝零匹配/空证据/关键跳过。
6. 全部域完成后更新 F02 状态，整体准入复审收口。

## 批次顺序

0. **批次0（证据已就绪，纯重绑）**：五工具叶 C88F29B5114A/0818574ABD43/196D50D8DD6E/2D896560381E/CD5D7FC02D8E 重绑到 `r08_admission_{run_code,ptc,ptc_authority,ast_lsp,security}` 真实套件（准入候选 cc7f7052 回执）；learn_lesson 771296A194E0 重绑 `r06_skills_completion::original_lesson_review_one_shot_content_binding_conflict_and_real_retrieval`。D18 错端点两条（095E0402EEEC/5F394AA4C9A3）重绑 `r07_automation_flow::real_tool_suggestion_requires_apply_and_replays_after_expiry` 与 `::slash_apply_uses_frozen_owned_suggestion`，另补找不到建议/Studio缺失/重复拒绝组。D17 两条（09D567E99524/D5514BBDBF88）按审查示例重绑 `r07_probe_access::bridge_settings_receipt_and_rich_streaming_toggles_persist`（真实缺省/无关字段/非法模式读回）。
1. **批次A（重绑为主）**：D18、D04、D17、D16——多数有现成真实测试或可复用候选。
2. **批次B（规则拒绝测试）**：D12（全不足但同构）、D22、D19、D10。
3. **批次C（UI-Rust 配对＋补测最多）**：D20、D21、D13、D03。
4. 条件成功 7 条与错端点 4 条并入对应域批次（D20/D17/D18/D10）。

### 批次A逐叶明细（22 叶）

| 域 | 叶 | 断言数 | 不足 | 形态 |
|---|---|---:|---:|---|
| D04 | 25C4FF66FEE5、8D3CEB6133E1 | 2/2 | 各1 | 会话权限默认：重绑 `session_permission_default_read_and_write`＋补缺省 ASK/必需身份边界 |
| D04 | CD5D7FC02D8E security_scan | 4 | 叶级 | 重绑 r08_admission_security（见批次0） |
| D16 | DA37966CFEAC desk beautify | 2 | 1 | 断言1（拒绝/边界）补真实案例 |
| D16 | 125C9E21A621、7747A6F59130 jimeng | 2/2 | 叶级(UI) | 配真实 Rust media 链（`r07_media::jimeng_image_and_video_use_real_supervised_cli…` 为协议替身边界内现成证据）＋UI 必需拒绝 |
| D17 | 9 条 SEMANTIC_EFFECT bridge（09D567E99524、2424DA9AE3A5、2F5ED5999431、57B2329825A0、5CB57741F1BB、C3A723B0BAB9、D5514BBDBF88、ECA3EAB38C12、F465ECC87EE3） | 2 | 各1 | 断言1 全为"拒绝/边界"绑 A23；其中两条可重绑既有探针，其余按端点补真实拒绝/缺省/冲突案例 |
| D17 | 7AD1B00D8158、DF1F77F2D776（UI） | 4/8 | 叶级(UI) | 初始化/二维码/owner/重置串位重排；同端点真实组件测试配对 |
| D18 | 095E0402EEEC、5F394AA4C9A3 | 2/2 | 错端点 | 重绑 r07_automation_flow 两条真实测试＋补拒绝组（见批次0） |
| D18 | 1FB3D8BC916D desk cron read | 2 | 1 | 拒绝路径补真实案例 |
| D18 | E6F239636060（Activity UI）、FA0FF4DC0779（Automation UI） | 4/5 | 叶级(UI) | 复用 ActivityPanel/AutomationPanel 既有组件错误测试按操作重排；升格点击补；服务侧升格测试已有（r07_automation_activity 两条） |

## 纪律

- 修复期间 R00 原断言、历史回执、旧证据目录不动；新证据一律新目录。
- 不以套件退出 0 代领逐断言通过；每域独审通过才记该域闭合。
- rust/ 源码变更集中在候选窗口（独立审查运行时冻结），变更后五准入套件需复跑绑定新候选。
- 与 R08 T01–T08 的关系：F02 闭合是 R08 依赖业务放行条件之一；修复期间可并行推进不依赖 R07 接口的 R08 准备（见 PROGRESS）。
