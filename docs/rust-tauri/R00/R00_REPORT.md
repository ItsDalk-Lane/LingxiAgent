# R00 阶段报告｜封存并交接基线（R00-T08）

阶段与结论：**READY_FOR_REVIEW**（R00 全部 8 项任务执行完毕；T08 为执行者自评完成，独立验收由总控另派全新任务执行，本报告不自称 PASS/ACCEPTED）

## 1. 范围

- 本报告覆盖 R00 阶段收口任务 **R00-T08（封存并交接基线）** 及其验收 **R00-A15（预存失败可重现）、R00-A16（基线审阅可独立复查）**；T01–T07 结论转录自总控账本（`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`，本任务只读未改）。
- 未兼任独立验收；未派生子代理；未创建分支/worktree；未 commit/push/PR/tag/release。
- 与基线差异：无获准 ADR 需求——本任务全部改动均为 T08 规格内取证、登记于验收报告的 follow-up 修复（§6）与账本获准更新路径（§8.4）。

## 2. 源码

- Task base = HEAD = `e0b7be6108c4d5bc873061dee279b7163ca78a41`（分支 `codex/rust-tauri-migration`）；stage base = `7d1a0c6bc28062ff455adcf8f68c3a80b117e90d`。
- 工作区：总控账本 `ORCHESTRATOR_PROGRESS.json` 的预先存在未提交修改原样保留（未触碰）。T08 候选 = 173 文件（§10）。
- 依赖锁：`package-lock.json` = `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`（R00 全程未变）。

## 3. 环境

macOS 27.0（Build 26A428）/ Darwin 27.0.0 / arm64（Mac15,14, 96GB）；Node v24.16.0、npm 11.13.0、Python 3.14.3、vitest 4.1.10。快照：`artifacts/rust-tauri/R00/T08/environment-r1.txt`。本机代理 `127.0.0.1:7890` 不可达（环境既有事实，本轮全部验证不依赖外网）。测试数据：全部为 `os.tmpdir()` 隔离目录 + 合成夹具；外部协议：确定性本地替身（模型 stub、127.0.0.1 端点）；零真实供应商 / 零付费 API / 零真实用户数据 / 零外发。

## 4. 完成项（T01–T08 概览 + T08 详情）

| 任务 | 交付（固定 SHA 见 R00_HANDOFF.json artifact_hashes 与 §10） | 独立验收 |
|---|---|---|
| T01 固定基线与隔离工作区 | BASELINE.json / BASELINE_DELTA.md / 隔离探针 | PASS（R4） |
| T02 重建完整保留功能清单 | FEATURE_INVENTORY.json（736 生产叶子）/ EXCLUSIONS / FEATURE_STAGE_ACCEPTANCE / ENTRYPOINT_COVERAGE | PASS（R10） |
| T03 盘点 Pi 与 Node 隐含职责 | PI_REPLACEMENT_MATRIX / RUNTIME_DEPENDENCIES / 导入图 | PASS（R3） |
| T04 盘点入口、身份、数据与权限 | ENTRYPOINTS（35 现役+1 休眠+1 残留）/ STORES（69 存储）/ OWNERSHIP_CURRENT | PASS（R2） |
| T05 建立旧行为与故障夹具 | FIXTURE_MANIFEST + 9 组夹具 + OLD_BEHAVIOR_ORACLE + 回放驱动 | PASS（R1） |
| T06 测量旧版本并冻结性能协议 | BASELINE_BENCHMARK / PERFORMANCE_THRESHOLDS / R00-T06_PROTOCOL + raw | PASS（R2） |
| T07 建立可执行验收账本 | ACCEPTANCE_MAP（952 场景/16 结果）+ 校验器/自测/生成器 + BLOCKERS | PASS（R3） |
| **T08 封存并交接基线** | 本报告 + R00_HANDOFF.json + 基线证据摘要（R00_EVIDENCE_SUMMARY.md + §10 清单）+ 验证组合原始日志（artifacts/rust-tauri/R00/T08/，物理 163 文件 = §10 清单内 162 + 清单自身） | **待独立验收** |

**T08 做了什么：**①按 05 §2 运行全量验证组合并保留原始 stdout/stderr/退出码（§5）；②失败分类与同条件复现（§7，A15）；③核对本阶段零生产行为改动、零真实用户数据接触（§8）；④实施验收登记的 follow-up 修复（§6）；⑤按获准路径重建验收账本并绑定 A15/A16 结果（§8.4）；⑥计算全部成果摘要并填写阶段交接模板（R00_HANDOFF.json）；⑦A16 三抽查点自验（§9）。

## 5. 验证组合（全部真实运行，原始日志在 artifacts/rust-tauri/R00/T08/）

| 命令 | 退出码 | 结果 | 日志 |
|---|---|---|---|
| `npm run typecheck` | 0 | tsc×3（root/node/test）全过 | typecheck-r1.log |
| `npm run typecheck:core-contracts` | 0 | 28 文件严格契约通过 | typecheck-core-contracts-r1.log |
| `npm run check:dependency-boundaries` | 0 | 2031 源文件通过 | check-dependency-boundaries-r1.log |
| `npm run check:tool-invocation-boundaries` | 0 | 2250 源文件通过 | check-tool-invocation-boundaries-r1.log |
| `npm run lint` | 0 | 修复后 0 错误（10887 处预存 warning 原样保留，其中 33 处可 --fix 自动修复；修复前 exit 1 见 lint-r1.log） | lint-r2.log |
| `npm run build:renderer` | 0 | vite 构建成功（7.05s） | build-renderer-r1.log |
| `npm run test:knowledge-platform-smoke` | 0 | 15 文件 / 125 用例全过 | knowledge-smoke-r1.log |
| `npm test`（全量） | 1 | 文件 1463 通过/3 失败/3 跳过（共 1469）；用例 14895 通过/4 失败/15 跳过（全口径共 14914）；4 预存失败（§7） | npm-test-r1.log + npm-test-r1.exitcode |
| `node scripts/rust-tauri/r00-t05-replay.mjs --out artifacts/rust-tauri/R00/T08/replay-t08` | 0 | 三次重放规范化逐字节一致（强化守卫下复证 A09） | replay-t08-driver.log + replay-t08/ |
| 受影响测试（migration 三文件） | 0 | 21/21（9 回放+5 反例+7 守卫负向） | affected-migration-tests-r1.log、a16-test-rerun.log |
| `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py` | 0 | LEDGER_VALID checks=14945 scenarios=952 results=16（含 A15/A16 重建后） | ledger-validate-final.log；终稿编辑后复核 ledger-validate-final2.log；R1 勘误修复轮重绑 HANDOFF 后重建复核 ledger-validate-final3.log（均同输出 exit 0；修复前 RES-R00-A16 STALE-SOURCE 失败留证 ledger-validate-stale-r1.log） |
| `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir …/T08/ledger-selftest-r2` | 0 | 18/18 变体 + 正面对照过 | ledger-selftest-r2/ + driver 日志 |

**未运行项（如实声明）：**`npm run pack`/`dist`（打包与签名——涉及真实产物构建，非 T08 规格，R10 范围）；Windows/Linux/macOS x64 平台（无真机，BLOCKERS 预登记，R10）；真实供应商 LIVE（未授权，R10）；桌面 GUI 安装包（R09/R10）。T06 的长时资源增长（BLK-LONGRUN-G1）维持 NOT_RUN_THIS_ROUND（R10）。

## 6. 行为变化与 follow-up 处置

**生产行为变化：无。** R00 全程（7d1a0c6bc→e0b7be610 + 本任务工作区）零改动 `desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json` 等生产运行面（`git diff --name-only` 过滤实测为空）；`scripts/` 变更全部为新增 `scripts/rust-tauri/r00-*` 测量工具（A=新增）；`.gitignore` 仅移除旧任务书跟踪。本任务改动的 `eslint.config.js` 是 lint 工具配置（见下），不影响任何运行时行为。

验收报告登记 follow-up 的处置（详见 §11 决策表）：

- **T05 R1-F01（MEDIUM，要求至迟 T08 封存前修复）— 已修**：`tests/migration/network-guard.ts` 强化——补丁 `net.createConnection`（tls 的 `createConnection` 导出在当前 Node 不存在，按存在性补丁）、isPipe 判定改为「仅显式 `options.path` 或含 `/` 的字符串」（`net.connect(443, "host")` 位置参数形不再放行）、覆盖 `dns.promises.lookup` 与 `dns.resolve*` 族（回调+promises）；新增永久负向回归 `tests/migration/network-guard-negative.test.ts`（7 用例：三类绕过逐类验证阻断 + 管道/本地放行面 + `http.Agent→createConnection` 路径实测同步拦截）；`FIXTURE_MANIFEST.json` replay.offline 声明同步改精确（列出覆盖面与残留局限）。修复后受影响检查全量重跑（§5 migration 21/21 + 三次重放一致）。
- **T05 R1-F02（LOW）— 已修**：FIXTURE_MANIFEST auth 条目补第 5 条散文（`connection-not-allowed`/custom_remote），与 `auth/expected.json` 五用例一一对应并在条目内显式列出用例名。
- **T05 R1-F04（INFO，验收方明示「不构成缺陷」）— 裁定为接受限制**：normalize tmp-path 正则理论性过屏蔽，当前冻结证据全为纯路径（验收方实测影响零）；封存期不改动冻结规范化语义，登记于 HANDOFF unresolved_items，留待夹具演进。
- **T06 F08（LOW，"T08 或阶段评审"）— 以阶段交接收口**：不改冻结测量证据 `BASELINE_BENCHMARK.json`；在 R00_HANDOFF.json `office_chain_pointer` 与本报告提供显式指针（raw/summary 路径 + 数值所在表），G2/G3 消费入口不缺失；汇总器并入留待 R08/R10 汇总演进。
- **T06 F10（LOW，措辞更正）— 勘误收口**：不回改冻结 T06 报告；勘误如下——T06 报告 §3/§9 所称「逐样本记录的 serverBundleSha256」实际记录粒度为 run summary 单处（互证实质成立，盘上字节==summary 值，R2 审查方已独立核实）。
- **T06 F09/O1**：维持原处置（后续 bench 演进 / R08 前加注），入 HANDOFF unresolved_items。

**T08 自发现缺陷（本任务修复）：**

1. `npm run lint` exit 1：单错误 `docs/rust-tauri/R00/r00_t03_import_graph.mjs:254 'console' is not defined`。根因：T03 新增的 docs 下 .mjs 使用裸全局，而 eslint.config.js 的 node-globals 块只覆盖 scripts/tests/.sync-audit（.ts 文件因 typescript-eslint 关闭 no-undef 不受影响，故仅此文件报错）。修复：`docs/**/*.{js,mjs}` 纳入既有 node-globals 块（与 .sync-audit 同理，3 行）。修复后 exit 0，0 错误 / 10887 处预存 warning 原样保留（其中 33 处可 --fix 自动修复；lint-r1/r2 双日志留证）。
2. 账本隐含缺陷（相对 T08 预存、T07 遗留）：`RES-R00-A13/A14` 的 `committed_in=null` 仅在 T07 提交前合法；T07 交付已提交为 e0b7be610 后任何在当前 HEAD 的账本重建都会触发 `RESULT-BAD-FIELD`（首次重建实测暴露，失败轮完整留证：ledger-rebuild-r1.log + ledger-selftest-r1/ 17/18）。按事实修复：两条记录 `committed_in` 补绑 `e0b7be610…`（tested_sha 仍由构建器回填自测验证树 4401afae2，历史不变）；重建后 18/18 + LEDGER_VALID。

**过程事故（如实记录）：**本任务曾误以无参形式运行 `r00-t05-replay.mjs`，其默认输出目录为冻结证据目录 `artifacts/rust-tauri/R00/T05/`，覆写了 15 个已提交证据文件；立即 `git restore` 从 HEAD 逐字节恢复（恢复后 `git status` 该目录 0 改动），随后改用 `--out` 隔离目录正确重跑。该驱动无 `--help`/参数校验即执行默认路径的问题登记为风险（§11），未修改 T05 冻结交付。

## 7. R00-A15｜预存失败可重现（分类表）

全量 `npm test` 的 4 个失败用例（3 文件）属同一根因家族，两次同条件复现（同命令/同环境/同 HEAD）失败集合完全一致，比对由 `r00_t08_a15_verify_repro.py` 机器验证（exit 0，`A15-REPRO-VERIFIED failures=4 identical_runs=2`）：

| 失败用例 | 分类（相对 T08） | 根因 | 对后续影响 |
|---|---|---|---|
| post-verification-audit-seal「audit-only allowlist enforced」 | **预存**（R00 首任务提交即失败） | VERIFIED_SOURCE_SHA=`46f12ab1c…` 审计白名单不含 R00 迁移增量（docs/artifacts/tests/scripts 新文件与 .gitignore、旧任务书删除） | 总控按 PROGRESS.md 封印流程重绑坐标前，全量 npm test 恒含此 4 例失败；不阻塞 R00 技术交付判断（AGENTS.md 明文：不为变绿扩白名单/退役门禁/虚报坐标） |
| round2 R10-03（源码 manifest 绿色门禁） | 同上 | 同上（同一 `verify-post-verification-diff.mjs` 失败） | 同上 |
| round2 R10-04（manifest 覆盖排除规则） | 同上 | 同上 | 同上 |
| round3（源码 manifest 可复算） | 同上 | 同上 | 同上 |

处置：**未将其写为 PASS、未删除任何用例、未修改白名单**；原始双份日志 + 校验器输出留证（audit-seal-repro-r{1,2}.log、a15-repro-verify-r1.log）。其余 1463 文件/14895 用例通过，无环境类/未知类/新增类失败。**关键兼容/安全证明受阻项：无。**

**取证中另发现的两项工具级预存失败（非 npm test 用例，同属 A15 分类纪律）：**①`r00_t02_inventory.py --negative-checks` 在 HEAD 报 STALE——经诊断三份冻结交付与再生成输出唯一差异为 `tested_sha` 戳（16aeb380d→e0b7be610），736 功能/映射/覆盖内容逐字节零漂移，STALE 为该工具「树移动需重绑戳」设计语义；②`r00_t03_validate.py` 退出 1——冻结导入图 1664 文件 vs 当前受控源集 1677（T03 冻结后 T05+ 新增源文件），scope 断言按设计失败。二者均先于本任务会话存在于 HEAD（预存，相对 T08）、均不阻塞 R00 放行（历史 PASS 绑定各自验证树；重生成/再绑属后续任务获准路径）；③同组对照 `r00_t04_scan.py --validate` 在 HEAD 仍 exit 0。证据与诊断方法：`t02-negative-checks-at-head.log`、`t03-validate-at-head.log`、`t04-validate-at-head.log`、`t02-t03-checker-diagnosis.md`（均在 artifacts/rust-tauri/R00/T08/）。

**附带发现（新增风险，非测试失败）**：round2/round3 用例失败前会重写 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch.gz`（22MB→25MB，因 manifest 现含 R00 增量），跑全量测试会弄脏工作区；本任务已 `git restore` 恢复并登记 ROUND2-TEST-SIDEEFFECT（建议后续修测试卫生，属 R00 外范围）。

## 8. 安全与数据

1. **未改生产行为**（§6）：R00 范围 diff 实测零生产运行面文件改动。
2. **未接触真实用户数据**：全部验证使用 os.tmpdir() 隔离目录与合成夹具；唯一涉及真实目录的取证是 T01 已验收的只读元数据清单（本轮未重复）。本任务 npm test 期间出现的唯一工作区副作用（§7 patch.gz 重写）为仓库内已提交文件的测试重生成，已恢复，非用户数据。
3. **敏感扫描**：R00 全范围 diff 与 T08 新增证据零命中（PEM/私钥/常见 token 模式）；T06 raw 下 2 处 `.mimosa/hook-state/` 审查方残留为 .gitignore 覆盖项（实测 check-ignore 命中 `.gitignore:129`），不入库。
4. **账本获准更新路径**（T07 §13 记录的既定流程）：改受监控源（守卫/夹具清单/新测试）→ 受影响检查重跑（migration 21/21 + 三次重放 + migration 三文件）→ `r00_t07_build_map.py` 重建（952 场景/16 结果/31 测试索引）→ 自测 18/18 → 真实仓库 `LEDGER_VALID checks=14945`。RES-R00-A09 扩展 source_paths（+network-guard.ts/+negative.test.ts）并追加 3 条 T08 重验证据；RES-R00-A15/A16 新记录（21 必填字段齐全，committed_in=null 且 tested_sha==basis.head，合法待提交态）。
5. **单写者/权限**：R00 无新存储写入；账本由 r00_t07_build_map.py 单一入口生成。

## 9. R00-A16｜基线审阅可独立复查（三抽查点自验）

`python3 -B docs/rust-tauri/R00/r00_t08_a16_spotcheck.py`（输入仅 R00_HANDOFF.json spot_checks 段 + 仓库，模拟「另一执行者只拿交接与仓库」）→ **exit 0，三点全过**（R1 勘误修复后同命令复跑仍 exit 0：a16-spotcheck-r2.log）：

1. **一个功能**：`F-D09-…-FILE-HISTORY-FILES--AF8E24`（列出有历史的文件）→ 盘点映射（R04/R06、任务 R04-T04/R06-T05）↔ 源码锚点 `server/routes/file-history.ts:30`（`/file-history/files`）↔ 账本补充场景 `R00-T02-LA-AF8E24A2F524` 双向链全在位。
2. **一条数据链**：`session-jsonl` 权威存储（epoch-managed/user_data）→ 6 个读写方文件在位 → `multi-turn-basic` 夹具输入/预期 SHA-256 与交接固定值一致。
3. **一个测试**：交接记录的命令（`LINGXI_MIGRATION_BLOCK_NETWORK=1 vitest run tests/migration/{r00-t05-replay,r00-a10-old-defect,network-guard-negative}.test.ts`）真实重跑 exit 0 / 3 文件 / 21 用例（a16-test-rerun.log 摘要与内容双校验）。

执行者自验通过 ≠ 独立验收：独立验收者应按 R00_HANDOFF.json 直接复跑上述命令与抽查器，并另行抽选不同功能/数据链/测试复核。

## 10. 完整映射与候选摘要（基线证据摘要）

- **需求→任务→测试→结果→证据映射**：`ACCEPTANCE_MAP.json`（SHA-256 `5eeac37ce3cb19e1eaaf123a8ce3b1f3cbd286538eda5a75fed7e4095eec288a`；952 场景=200 基础+736 补充+16 t07_added；16 结果=A01–A16；832 入口；31 测试索引；PASS 32 / NOT_STARTED 183 / SPECIFIED_NOT_EXECUTED 736 / NOT_RUN_UNAUTHORIZED 1；R1 勘误修复轮经 `r00_t07_build_map.py` 重建重绑 A16 的 HANDOFF 摘要，语义差异仅 `generated_at` 与 RES-R00-A16 的 `source_digests`/`working_tree_digest` 三处）。未覆盖集合=NOT_STARTED 183 个未来阶段基础场景 + 736 补充场景（实施阶段执行）。
- **T08 候选 = 173 文件**：逐文件 SHA-256 清单 `artifacts/rust-tauri/R00/T08/CANDIDATE_MANIFEST.txt`（其 SHA-256 即聚合 SHA；已与 git status 全集交叉核对零遗漏/零多余）：

```text
d83ae896ce19d1ee5a8c0c8084244c44a32516cee5d589a35ead9fff2a448645
```

  构成：7 个已跟踪文件修改（eslint.config.js、tests/migration/{network-guard.ts,fixtures/FIXTURE_MANIFEST.json}、docs/rust-tauri/R00/{ACCEPTANCE_MAP.json,BLOCKERS.md,R00-T07_RESULTS.json,r00_t07_build_map.py}）+ 166 个新文件（artifacts/rust-tauri/R00/T08/ 162 个原始日志/重放/自测/诊断证据〔含 R1 勘误修复轮新增 28 个：修复前 STALE 失败留证、A16 复跑、账本重建/自测/终验〕 + docs 3 个新工具/交接 + 1 个新测试）。**不含**：本报告与 R00_EVIDENCE_SUMMARY.md（成文于聚合之后，哈希由验收方现算）、总控账本 ORCHESTRATOR_PROGRESS.json（预先存在修改，未触碰）、CANDIDATE_MANIFEST.txt 自身（自引用规避，其哈希即聚合值）、R00-T08_REVIEW_R1.md 与 R00-T08_REPAIR_R1.md（验收/修复轮文档，不属 T08 候选）。
- **单列 SHA-256**：R00_HANDOFF.json = `68877ff6980993e34e8fd5405b93eb0c20495b5fa242bcc41c782c06796c62a5`；本报告与 R00_EVIDENCE_SUMMARY.md 的哈希由验收方 `shasum -a 256` 现算（不自嵌）。- 其余 T01–T07 交付固定哈希见 R00_HANDOFF.json `artifact_hashes`；原始证据目录 `artifacts/rust-tauri/R00/T01..T08/`。

## 11. 已知缺陷 / 风险 / 决策表

| 项 | 严重度 | 状态 | 归属 |
|---|---|---|---|
| 审计封印 4 用例失败（§7） | 门禁性 | 预存，未修绿，留总控封印流程 | R00 阶段验收提交后 |
| T02/T03 检查器 HEAD 预存失败（§7 附注） | 低 | 预存：T02 纯 tested_sha 戳（内容零漂移已证明）、T03 冻结图 vs 树增长；再绑/重生成留后续任务 | 后续任务 |
| T07 账本 A13/A14 committed_in 隐含缺陷 | 中 | **已修**（补绑 e0b7be610，重建后全绿） | 本任务 |
| T05 R1-F01 守卫覆盖 | 中 | **已修**（强化+负向回归） | 本任务 |
| T05 R1-F02 夹具散文 | 低 | **已修** | 本任务 |
| lint docs/**.mjs node-globals 缺失 | 低 | **已修**（配置 3 行） | 本任务 |
| round2/round3 测试重写 22MB patch.gz | 低 | 登记风险（ROUND2-TEST-SIDEEFFECT），未修（R00 外范围） | 后续任务 |
| 回放驱动无参即写默认（冻结）目录 | 低 | 登记风险；建议加参数校验（本任务事故已恢复） | 后续任务 |
| T05 R1-F03 模型断点未演练 / R1-F04 正则加固 | 低/INFO | 维持原处置（R03 / 夹具演进） | R03 等 |
| T06 F08/F09/F10/O1 | 低 | §6 处置（交接指针/勘误/登记） | R08/R10 |
| BLK-CREDENTIALS/PLATFORM/LONGRUN-G1/RELEASE-AUTH | 条件 | BLOCKERS.md 预登记，未伪造 | R10/R11 |

## 12. 未执行 / BLOCKED

见 §5 未运行项与 §11；无新增 ACTIVE 阻塞。四类授权标志（远程写/生产数据/付费外测/发布）均为 false 且本轮零动作（HANDOFF 同步）。

## 13. 回退

本任务改动回退演练路径：`git restore` 工作区 7 个修改文件 + 删除 169 个新文件 = §10 清单内 166 个 + 清单外 3 个（本报告、R00_EVIDENCE_SUMMARY.md、CANDIDATE_MANIFEST.txt 自身）（无数据丢失风险——全部为本任务新建证据/工具）；账本可由 `r00_t07_build_map.py` 从任务书与盘点重建。已实际演练的恢复：T05 冻结证据 15 文件（§6 事故）与 patch.gz（§7），均恢复至 HEAD 字节。不回滚用户原有修改（总控账本未触碰）。

## 14. 独立审查

**待总控指派。** 建议最小动作（91 §5）：①复跑 §5 表中任一命令对日志；②复跑 `r00_t08_a15_verify_repro.py` 与 `r00_t08_a16_spotcheck.py`；③按 R00_HANDOFF.json spot_checks 另选不同功能/数据链/测试复核；④审阅 §6 三处修复的根因与影响面（守卫强化→重放一致性、eslint 配置→仅 docs 全局声明、账本补绑→A13/A14 历史不变）；⑤核对 CANDIDATE_MANIFEST.txt 聚合哈希。

## 15. 下一阶段（R01 允许开始条件）

输入齐备：功能/入口/存储/Pi 依赖清单、协议夹具、性能阈值、阻塞表（全部固定 SHA 见 HANDOFF）。允许范围=R01 任务书 8 项任务；**接口版本与目标契约选择在 R01 决定，R00 未升级任何依赖/协议**；R00 独立验收 ACCEPTED 前不开始 R01 实施。

## 16. 远程 / 发布

未获准、未执行：零 commit、零 push、零 PR、零 tag、零 release、零外部发送。提交推送由总控在独立验收通过后处理。
