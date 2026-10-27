# R00-T07 独立验收 R1（ZCode）

验收代理：REVIEWER-R00-T07-R1（一次性独立验收代理，ZCode，不参与执行或修复）
候选：R00-T07「建立可执行验收账本」（执行者报告 `R00-T07_REPORT.md`，执行者自评 READY_FOR_REVIEW）
Task base/HEAD：`4401afae2ee48df1512b8a8c9a1262be0efbe64f`（分支 `codex/rust-tauri-migration`，实测一致）
验收时点：2026-09-24（本机 macOS 27.0 darwin arm64，Python 3.14.3）

## 结论

**R00-A13：PASS。R00-A14：PASS。任务 R00-T07 验收结论：PASS（附 1 项必须修复的报告文档瑕疵 D-1 与 1 项建议增强 D-2，均不改变 A13/A14 判定）。**

候选核心证据自洽且全部独立复现：实际 46 文件的聚合 SHA-256 恰为报告声称的 `a33fc436…`（报告 §10 清单中 1 行有转录笔误，见 D-1）；正向校验、15/15+正面对照自测、200/736/13/832/743/37/100 全部映射计数、历史 14 项结果的 SHA/时间/证据绑定均独立复核通过。执行者未自评任务通过（账本 `ledger_self_status=READY_FOR_REVIEW`，校验器有 LEDGER-SELF-STATUS 强制规则）。

## 1. 验收范围与执行方式

- 只审查候选，未修改任何候选文件、总控账本或其他仓库文件；未 commit/push/PR/tag/release/建分支；试验全部在 `/tmp/t07_review_probe` 与 `/tmp/acc-t07-review-r1` 隔离目录；无网络、无真实供应商、无付费 API、无外发。
- 通读：AGENTS.md、任务书 00/01–06、R00 阶段书 T07/A13/A14 全文、90/91、acceptance-catalog（200 场景分布与 R11-A14 条目）、执行报告、ACCEPTANCE_MAP.json（1.55MB 结构化核对）、三工具脚本全文、BLOCKERS.md、T01–T06 已提交报告与证据抽查、SELFTEST_SUMMARY 与全部关键日志。
- 独立重算与复跑，不采信执行者结论。

## 2. 候选完整性与哈希重算（含根代理初核两个疑点的定性与根因）

实测候选构成：**46 个文件**（docs 6 + artifacts T07 顶层 20 + confirm-rerun 20），与报告 §10 一致；报告自身 SHA-256 实测 `27335bc6bcadb87988448d0cdd6080d4913b241102c2307ee4c7d96094d4ff8a`，与总控期望一致。

| 项 | 报告声称 | 验收方独立重算 | 一致 |
|---|---|---|---|
| 46 文件聚合（SHA+双空格+路径+换行 拼接） | `a33fc436572ae1daa06624f8c9970e2033cd8faf4df1dd125a85af1e8b75f549` | **实际文件行拼接 = `a33fc436…f549`**（相同） | ✅ |
| 按报告 §10 所列 46 行文字拼接 | （隐含应等于上行） | `cbd9224d72b3c56ddc336b8605db64e2dba4d92ab3cd52dd02dff1586affdc25`（与根代理初核一致） | ⚠️ 见 D-1 |
| 逐文件比对 | 46 行 | 45/46 一致，1 行不一致 | ⚠️ 见 D-1 |

**D-1【必须修复｜报告 §10 清单单行转录错误】**
- 位置：`docs/rust-tauri/R00/R00-T07_REPORT.md` §10 清单中 `artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v3-evidence-tampered.log` 一行。
- 事实：报告所列 `aa3ec8e344c444a1e495f2976db0f18877ab3664142afba2226f1b31f36ef83b`（第 42–44 字符为 `2af`）；文件实际 `aa3ec8e344c444a1e495f2976db0f18877ab366414af2ba2226f1b31f36ef83b`（第 42–44 字符为 `af2`）。3 字符轮转式笔误，两串均为 64 hex。
- 根因定性：**报告清单单行转录笔误，候选文件未被篡改**。决定性证据：以实际文件字节重算的聚合哈希恰等于报告声称的 `a33fc436…`，即执行者计算聚合时基于的就是当前磁盘上的这些文件；仅清单文字里的这一行抄错。这同时解释了根代理初核的两个观察（该行 SHA 不一致、按清单重算得 `cbd9224d…`）。
- 影响：验收方或后续审计按报告清单逐行核对会在该行得到假阳性「哈希不符」，损害报告自述精确性；不影响账本（账本 evidence 哈希由校验器独立验证通过）、不影响 A13/A14 判定、不构成候选证据不自洽（候选文件与聚合声称自洽）。
- 修复要求：提交前将该行更正为实际值 `aa3ec8e344c444a1e495f2976db0f18877ab366414af2ba2226f1b31f36ef83b`（报告自身 SHA 将随之变化，总控重新绑定即可）。

## 3. R00-A13 逐条判定（REQUIRED）

规格通过条件：「非零退出，精确指出 ID 和缺失字段」。

| # | 验证 | 命令/方式 | 退出码 | 结果 |
|---|---|---|---|---|
| 1 | 执行者自测套件独立复跑（canonical+10 变体+正面对照） | `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir /tmp/acc-t07-review-r1` | 0 | `SELFTEST PASS variants=15/15 (+positive-control ok) temp_removed=16/16`；候选文件零触碰（重算 SHA 不变） |
| 2 | canonical 正例复核（场景手改 PASS + 删日志） | 上述套件内 `a13-canonical` | 1 | 输出同时含 `STATUS-PASS-WITHOUT-RESULT scenario=R00-T02-LA-000E6E1301C0 field 'result_ids'…` 与 `EVIDENCE-MISSING-FILE result=RES-R00-A01 …startup-probe.json does not exist`，`LEDGER_INVALID errors=2`——精确到场景 ID、结果 ID、字段名、证据路径 ✅ |
| 3 | 同根因变体（v1 无结果 PASS / v2 删日志 / v3 篡改哈希 / v4 删字段 / v5 源码过期 / v6 exit 冒充 / v7 重复键 / v8 任务引用缺失 / v9 规格篡改严格 / v9b 内嵌快照回退） | 套件日志逐份核对 | 各 1 | 11/11 与 SELFTEST_SUMMARY 一致，子串精确命中 |
| 4 | **验收方独立新反例**（全新探针 `/tmp/t07_review_probe/probe.py`，不复用执行者代码）：缺 platform、缺 toolchain、重复 result_id、未来场景偷挂 result、非法 timestamp、lockfile 哈希不符 | 隔离副本 + 单一篡改 | 各 1 | 全部被拒且错误行精确到 result ID 与字段名（如 `RESULT-MISSING-FIELD result=RES-R00-A07 field 'platform'`、`LOCKFILE-MISMATCH`、`STATUS-NOTSTARTED-WITH-RESULT scenario=R05-A01`） |
| 5 | 正面对照（防「永远失败」取巧） | 未篡改隔离副本 | 0 | `LEDGER_VALID checks=14761` ✅ |

**判定：A13 PASS。** 真实仓库严格模式校验器独立复跑：`python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py` → `LEDGER_VALID checks=14761 scenarios=949 results=14 entries=832 spec_source=taskbook-strict`（exit 0），CLI 子进程形态可用（执行者因本机 Mimosa 写入钩子只能进程内调用——**验收方构造探针时亲身复现了同一拦截**，该环境约束声明属实）。

## 4. R00-A14 逐条判定（REQUIRED）

规格通过条件：「缺少任务或场景会失败；补齐后通过」。

| # | 变体 | 退出码 | 关键输出 |
|---|---|---|---|
| 1 | v1 新增真实形态入口+功能、账本未映射 | 1 | `ENTRY-COVERAGE-BIDIR entry=cli:lingxi-doctor … not mapped` + `FEATURE-INVENTORY-BIDIR … F-D99-CLI-DOCTOR` |
| 2 | v2 功能入索引但无场景/任务 | 1 | `ORPHAN-REQUIREMENT feature=F-D99-CLI-DOCTOR no supplemental scenario` |
| 3 | v3 补齐 feature→scenario→task 映射（含获准路径摘要重绑） | 0 | `LEDGER_VALID` |
| 4 | v4 账本映射盘点不存在的入口（孤立映射） | 1 | `ENTRY-COVERAGE-BIDIR entrypoint_index … cli:ghost` |
| 5 | **验收方独立入口** `http:POST/api/v99/ghost-widget`（不同命名空间/形态，证明非自测脚本专属路径） | 1 | 三重拦截：ENTRY-COVERAGE-BIDIR + EVIDENCE-HASH-MISMATCH + STALE-SOURCE |

「补齐后通过」的语义核验：v3 的重绑是获准路径模拟（等效重跑 T02 扫描后重建账本）；验收方 R1g 实测证明**只改映射不重绑摘要会被 EVIDENCE-HASH-MISMATCH/STALE-SOURCE 拒绝**——即新增生产入口无法静默混入，必然触发受影响检查重跑。任务/场景/测试映射补齐后通过的链路完整。

**判定：A14 PASS。**

## 5. 映射完整性与计数独立核对（不信计数，逐集重算）

| 维度 | 声明 | 独立重算 | 一致 |
|---|---|---|---|
| 基础场景 | 200 | acceptance-catalog 200 场景（R00–R11 各 16/24，199 REQUIRED + 1 条件授权）全量导入，任务书目录存在时严格模式逐场景逐字段比对（v9 篡改 `spec.then` 被拒可证） | ✅ |
| 补充叶子场景 | 736 | 与 `FEATURE_STAGE_ACCEPTANCE.json` supplemental ID 集**精确相等**（集合比较 True） | ✅ |
| T07 新增场景 | 13 | 13 个 `R00-T07-LA-*`（A13×9 + A14×4），parent_acceptance/task/结果绑定齐全 | ✅ |
| 入口 | 832 | ENTRYPOINT_COVERAGE registrations 键集与账本 entrypoint_index **逐 entry 相等、逐 entry feature 集相等（0 不符）**；832 入口全部经 feature→supplemental 场景可达（0 无场景） | ✅ |
| 功能 | 743（736 生产） | 与 FEATURE_INVENTORY（736 生产+7 非生产）双向相等 | ✅ |
| 面 surfaces / 任务 | 37 / 100 | 与 ENTRYPOINTS.json / task-catalog 相等；任务↔场景双向无缺失 | ✅ |
| 状态分布 | PASS 27 / NOT_STARTED 185 / SPECIFIED_NOT_EXECUTED 736 / NOT_RUN_UNAUTHORIZED 1 | 逐场景重数一致（PASS=14 base+13 t07_added） | ✅ |

**总范围未缩减**：200 基础场景 + 736 补充 + 13 新增 = 949，账本顶层 `ledger_self_status=READY_FOR_REVIEW`（执行者不自评 ACCEPTED）。

## 6. 历史 14 项结果真实性抽查（tested_sha/时间/环境/退出码/证据）

- **SHA 链**：7 个 tested_sha/committed_in（7d1a0c6bc、16aeb380d、4b4a1d98f、60dbe0384、ffcb85830、6f58b9351、64c302ec4）实测全部为 HEAD 祖先；映射链自洽（每任务 tested_sha=其 base、committed_in=其交付提交）。
- **时间**：A03–A12 采用交付提交时间并逐项 `timestamp_basis` 诚实声明「原始运行墙钟未机器记录；运行先于提交」；4 组提交时间与 timestamp_utc **精确换算一致**（如 4b4a1d98f `2026-09-23T23:02:32+08:00` = `15:02:32Z`；HEAD `10:22:09+08:00` = A11/A12 `02:22:09Z`）。
- **证据对回**（抽查）：RES-R00-A09 → `T05/REPLAY_SUMMARY.json` 三 run exitCode 全 0、diffs identical、断网隔离声明在位；RES-R00-A01 → `T01/startup-probe.json` exit_code=0、sandbox-exec + /tmp 隔离 home；RES-R00-A11 → `T06/BASELINE_BENCHMARK.json` 含 serverLeg/ptyLeg/desktopLeg(browserMulti/pdfConvert)。账本绑定 55 个 evidence 文件与 43 个监控源码的哈希已由校验器全量验证（exit 0 即全符）。
- **辅助声称复现**：`npm run typecheck` exit 0；`npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts` → 2 文件 14/14；三工具 `py_compile` 通过。
- **未执行/条件授权诚实性**：R11-A14 = NOT_RUN_UNAUTHORIZED（未冒充）；BLK-LONGRUN-G1 诚实登记 NOT_RUN_THIS_ROUND；无 ACTIVE 阻塞。BLOCKERS 预登记 9 场景（BLK-CREDENTIALS 2 + BLK-PLATFORM 7）**独立重算关键词命中与声明完全一致**，逐场景命中词可对回 catalog 原文。
- **两轮自测 checks 差异自洽**：顶层终轮日志 checks=14619（12-result 账本轮，A13/A14 结果尚未嵌入），confirm-rerun checks=14761（14-result 最终账本）——确认重跑正是覆盖「嵌入自测结果后仍 VALID」的回归，不是矛盾。

## 7. 执行者自评边界核查（验收指令专项）

- 任务书 01 §6 明确区分**场景状态**（执行侧证据记录，PASS 须含命令/预期/实际/tested SHA/环境/退出码/证据——A13/A14 及 13 个变体场景的 result 绑定满足全部要素）与**阶段状态**（执行者只可 READY_FOR_REVIEW，ACCEPTED 由独立验收决定）。
- 账本顶层 `ledger_self_status="READY_FOR_REVIEW"`，且校验器 LEDGER-SELF-STATUS 规则强制该值域（执行者想自标 PASS/ACCEPTED 会被门禁拒绝）。
- 执行报告结论亦为 READY_FOR_REVIEW，未自称验收通过。
- **判定：不违反「执行者不得自评通过」。** 场景级 PASS 是测试结果记录；任务验收状态由本独立验收（R1）判定。

## 8. 校验器/生成器可重复运行与不放行验证

- 生成器可重建（`r00_t07_build_map.py` 输入齐全、幂等路径声明清晰；验收方未在真实仓库重跑以免覆盖候选——这是验收约束而非缺陷，构建器在隔离副本中被自测的正面对照等效覆盖）。
- 校验器 27 规则族实测有效拒绝：无证据 PASS、删日志、缺字段（含验收方独立试的 platform/toolchain）、哈希篡改、源码过期、分支过期、lockfile 不符、重复键/ID、孤立需求/映射、任务引用缺失、规格篡改（严格+回退双模式）、exit 冒充、未来场景偷挂结果、正面对照防「永远失败」。
- **D-2【建议增强｜不阻塞】**：验收方发现 3 个任务书列举之外的规则缺口（探针 R1a/R1b/R1c 全部 exit 0 放行）：① scenario `ledger_status=PASS` 与绑定 `result.status=FAIL` 可脱节；② `result.status` 无枚举校验（非法值 `MAYBE_PASSED` 放行）；③ `result.status=FAIL` + `exit_code=0` 不报错。影响有限：攻击者无法借此伪造证据/退出码/源码摘要/分支（这些全部锁死），只能让 result 状态字段语义含糊。任务书 T07 Step3 明确列举的六类必拒项均有规则且验证有效，故不构成 A13/A14 FAIL 依据。建议下次账本更新时增加 RESULT-STATUS-ENUM 与 scenario↔result 状态一致性规则并补自测变体。
- 网络限制无掩盖：任务全程无网络操作；本机代理不可达不影响任何验证；未发现借环境限制跳过必须验收内容的迹象。

## 9. 门禁结果汇总与未执行范围

已执行并全部通过：正向校验（真实仓库，严格模式，exit 0）；A13/A14 自测 15/15+正面对照（/tmp 独立目录复跑）；验收方独立探针 10 例（7 拒绝符合预期 + 3 缺口如实上报 + 正面对照）；typecheck、迁移测试 14/14、py_compile；全部计数/映射/哈希/时间/分支独立重算。

未执行（及理由）：
- **npm/CI 门禁接入**：任务书 T07 未要求接入 `npm test`/CI（报告 §11 已声明；R02 xtask `verify-stage` 时消费本账本）。
- **其他平台/真实供应商/LIVE**：非 R00-T07 范畴；R11-A14 未授权保持 NOT_RUN_UNAUTHORIZED；跨平台为 T06/R09/R10 关卡。
- **T01–T06 原始测量重跑**：各任务已有独立验收轮（REVIEW_R1 系列），本轮核对的是 T07 对其结果的转录保真（哈希/时间/祖先/退出码声明），全部成立。
- **Rust 侧验证接口**：`rust/` 尚不存在（R02 范畴），账本已预留。

## 10. 修复清单（不阻塞 A13/A14 判定）

1. **必须**（D-1）：更正 `R00-T07_REPORT.md` §10 中 `confirm-rerun/a13-v3-evidence-tampered.log` 行的 SHA 为 `aa3ec8e344c444a1e495f2976db0f18877ab366414af2ba2226f1b31f36ef83b`；更正后报告自身 SHA 变化需由总控重新绑定。
2. **建议**（D-2）：校验器补 RESULT-STATUS-ENUM + scenario↔result.status 一致性规则，自测补对应负向变体（可与 R01 前的账本例行更新合并执行）。

## 11. 验收声明

本报告为独立验收记录，只判定 R00-A13/R00-A14 与 R00-T07 候选可否放行，不构成执行阶段完成声明；未修改候选与任何仓库文件；无未披露的环境限制。验收过程产物在 `/tmp/t07_review_probe/` 与 `/tmp/acc-t07-review-r1/`（临时，可清理）。
