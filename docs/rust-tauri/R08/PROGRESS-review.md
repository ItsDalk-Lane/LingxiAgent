# PROGRESS-review — 独立评审台账（T05：ws-ui-state；T06：回滚与迁移后新增数据保留）

## 2026-10-23 T05 独立评审：PASS 收官

- 评审对象：T05 迁移协调器（cutover 七子模块 + orchestrator + epoch 采用/启动门 + migrate 动词
  + --self-check 面 + legacy_import_runner 提取 + α1 种子守卫）+ P1 客户端对账 + 证据链。
- 评审资格：与 t02-core（T05 实现）、t02-a04（P1 实现）均无交集。评审期间未修改任何被评审文件。
- 方法：T05_DESIGN.md 16 节逐节对读实现全文；证据链收据抽查；rev-t05-* 独立复跑 8 组
  （r08_run.py 包裹、全新目录、串行执行、全部 sourceBindingsStable=true）。

## 复跑证据（rev-t05-*）

| 目录 | 内容 | 结果 |
|---|---|---|
| rev-t05-cutover-unit-01 | --lib cutover:: | 40/40 绿 |
| rev-t05-epoch-unit-01 | --lib epoch:: | 28/28 绿（含 incumbent byte-form 钉） |
| rev-t05-orchestrator-unit-01 | --bin lingxi-service | 7/7 绿 |
| rev-t05-selfcheck-unit-01 | --lib cutover::selfcheck 具名 | 7/7 绿 |
| rev-t05-seed-guard-01 | --test r08_t05_seed_guard | 4/4 绿 |
| rev-t05-migrate-e2e-01 | e2e happy 变体 | 23 断言 PASS |
| rev-t05-d2-full-01 | d2 full 腿 | 13 断言 PASS（committed、epoch 2、裁决书双副本、旧根字节相等） |
| rev-t05-p1-epoch-01 | vitest 13 例 | 13/13 绿 |

过程披露：orchestrator 首跑误用 --lib 目标（mod 声明在 main.rs，属 bin），0 命中；该目录已删除，
以 bin 目标在全新目录重跑。系评审人命令错误，非实现问题。

## 红证据诚实性抽查

epoch-adopt-01(101)→02(0)、parity-02/03(1)→04(0)、migrate-unit-02(101，家族 #6)→09(0)、
p1-baseline-01 vs p1-r02-regression-01（同根因逐行一致）、d2-01 红→α1→d2-03 绿 ——
全部红腿原样归档、闭合链完整、无粉饰。

## 结论与发现

- **PASS**：BLOCKER 0 / MAJOR 0 / MINOR 4。七项评审重点（fail-closed、相位权威、源零改动、
  单写者三层证据、纪元字节兼容、α1 零爆破、红证据诚实）全部成立。
- MINOR：N1 台账断言数低估（17 vs 实际 24/23）；N2 tmp 崩溃残渣整洁备注；N3 flag 值吞 `--` 前缀
  UX nit（仍响亮 exit 2）；N4 epoch_adopt 证据注入 id 的双钉不变量提请 T06 保持。
- 完整报告：docs/rust-tauri/R08/T05_REVIEW.md。

## T06 独立评审（2026-10-23）

- 评审对象：T06 全部实现（cutover_rollback.rs / rollback_delta.rs / classes.rs
  models-store.json 增补 / main.rs 双动词 / R02 注册表注记 / 6 个演习脚本 / runbook /
  t06-* 证据链 62 组）。评审资格：与实现者 t02-core 无交集；评审期间未修改任何被评审
  产品代码/脚本/设计文档；git HEAD 保持 86840d170。
- 方法：T06_DESIGN（J1–J9、⑤a、裁决①②、附加三钉）逐条对读实现全文；七缺陷修复
  逐条判定（正确性/新静默面/授权范围）；八组最终绿证据 candidateDiffSha256 比对；
  红证据签名对账 4 组；rev-t06-* 独立复跑 8 组（r08_run.py 包裹、全新目录、串行、
  演习前 pkill 孤儿）；独立工具深核 4 项（sqlite3/shasum/手工 JSON 解析）。
- **结论：PASS-with-MINORS** —— BLOCKER 0 / MAJOR 0 / MINOR 5；四项登记偏差全部接受。
  设计符合性（J1–J9、⑤a、①②、三钉）逐条核为落实；单写者/零触碰/无静默降级红线成立。
- 复跑证据（rev-t06-*，全部 exit 0、sourceBindingsStable=true、与实现者最终绿证同绑
  candidateDiffSha256=d5962b50…）：build 0 警告（rev-t06-build-01）、svcbin 16/16
  （rev-t06-svcbin-01）、adapters 177/177（rev-t06-adapters-01）、D1 38/38
  （rev-t06-d1-drill-01）、D2 27/27（rev-t06-d2-drill-01，RAM disk 自卸载）、
  D3 26/26（rev-t06-d3-drill-01）、registry amend 16/16（rev-t06-amend-01，v3 锚
  80a8d2c2 保持 + bulk 435/435/5206/40f42dc1 双跑全等）、一致性门 PASS
  （rev-t06-consistency-01）。bin/adapters 套件未见家族 #5 同签名红；lib 套件未复跑
  （不在 E 清单）。
- 深核抽样：F(i) 旧根印章四字段 {2,1,1,1}+lastVersion 0.1.43 盖戳/prepareWrites=5/4 个
  .orig 归档；F(ii) D2b 旧根零触碰（印章仍 {epoch:7}、无准备相产物、refused@preflight）；
  F(iii) secondImportZeroGrowth=true + 二次 applied=0 + pathScopedPreserved=3；
  F(iv) 16 条 payload 逐条 shasum -a 256 全等 + packageSha256 独立重算一致。
- MINOR：N1 台账记 amend-proof「17/17」实为 16/16（证据本身绿）；N2 rollback_delta.rs
  read_delta_package 空 if 死块（有效门是紧随的 packageSha256 重算）；N3 旧根 sqlite
  READ_ONLY 直开（observability 推导/⑤a 判别）在 WAL 模式库会静默建 -shm/-wal——
  夹具非 WAL 未触发，**移交 T07 核对真实旧根 journal_mode 或对称扩展 P6**；N4
  T06_DESIGN §6 模态 B 原文未随 automation_*/session_files 语义精化回写；N5 收据
  sessionRowsUpdated 字段名被 automation_* 复用（易误读，不建议 R08 内改 schema）。
- 与实现者宣称不符处：仅 N1 一处账面计数（17/17 vs 实际 16/16）；其余宣称
  （0 警告/16/16/177/177/38/27/26/一致性 PASS/红链根因）经复跑与抽查全部相符。
- 完整报告：docs/rust-tauri/R08/T06_REVIEW.md（含偏差裁定表与 T07/T08 移交注记）。

## 2026-10-24 R08 阶段终审：ACCEPT_HONEST_CLOSURE

- 终审对象：R08 整阶段（T01–T08 交付、R08_REPORT/R08_HANDOFF 宣称、证据链、xtask R08 注册、
  最终门）。终审人=独立代理（≠实现者，未修改任何被评审产品代码/脚本/文档；HEAD 全程
  86840d170 未动；未 commit/push）。
- **结论：ACCEPT_HONEST_CLOSURE —— BLOCKER 0 / MAJOR 0 / MINOR 4 / 观察项 4（不计级）。**
  CLOSED_HONEST_NOT_FULLY_GREEN、16 基础场景、126 叶（9 图钉+117 pending）、门 overall
  预期非 PASS 等宣称与证据一致；**未发现虚报**。完整报告
  `docs/rust-tauri/R08/R08_FINAL_REVIEW.md`。
- 治理修正 F-T07-1 核验成立：probe-02 红（conversational=0/unknownBlocks=4）→ probe-03 绿
  （3/0），sourceBindings 对拍锁定修复恰为 session_jsonl.rs/convert_sessions.rs 两文件
  （其余 664 绑定全等）；amend-proof 435/435/5206+双锚+doubleRunIdentical；「T04–T06 绿证据
  原本只对夹具自造格式成立」表述与事实相符（夹具 entryType vs 真实 Pi 嵌套格式双源核对）。
- T06 评审链成立：rev-t06-* 8 组 exit 0 同绑 d5962b50…；N2/N3 代码落地（死块移除 +
  stage_quiesced_copy 两处对称）+ t06-n2n3-* 6 组重绑绿；N1/N4 账面/回写到位。
- T07 抽查成立：ab-compare.json parity=12/differs=0/recorded=2/failedB=1 逐任务一致；
  drill-01/02/03 红链未删改；F-T07-2 翻译表+连接门代码在树（use-hana-fetch.ts L60-130/L179-183）。
- T08 注册成立：R08.json 18 命令/16 场景/126 叶（全 required_behavior_pending，9 叶含
  已实测份额图钉）+ main.rs/runner_identity 注册 + candidate.rs 删除条目跳过修复。
- 独立复跑 5 组（rev-r08final-*）全绿：build 0 警告 / svc-bin 16/16 / adapters 178/178 /
  registry PASS / inputs digest match。
- MINOR：M1 报告 §一「123 个 T07 域叶」与 9+117=126 不自洽；M2 HANDOFF「9
  stage_share_satisfied」措辞（实为 126 叶全 pending、9 叶图钉注记、门计 fail=9）；M3 HANDOFF
  rustBinarySha256 与 a16-versions.json 不一致（debug 重链接非可复现；基线权威=a16）；M4
  candidateDiffSha256 不含未跟踪文件（F-T07-1 修复在未跟踪 legacy_import/，红→绿绑定实际由
  sourceBindings 承载）。
- 最终门快照（终审时点）：PID 57977 运行中；r08_inputs_check PASS（digest match）；
  r08_t01_a01_a02_ws 套件 exit 0；其余排队。**不等待，最终结果由主会话登记。**
- 终审副作用披露：终审的 cargo build 在最终门窗口内重链接共享 debug 二进制（源未变、
  candidateDiffSha256=c99bb5c3… 稳定、行为等价）。
