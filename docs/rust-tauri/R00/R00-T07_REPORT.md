# R00-T07 报告｜建立可执行验收账本

任务：R00-T07（建立可执行验收账本）｜验收：R00-A13、R00-A14（均 REQUIRED）
执行者结论：**READY_FOR_REVIEW**（实施与自测完成；独立验收由总控另行指派，本报告不自称 PASS）

## 1. 范围与源码基线

- 只执行 R00-T07。未创建分支/worktree，未 commit/push/PR/tag/release，未派生子智能体，未执行 R00-T08，未修改总控账本。
- 基线 HEAD = `4401afae2ee48df1512b8a8c9a1262be0efbe64f`（分支 `codex/rust-tauri-migration`），与任务指定 Task base 一致；本地与 `origin/codex/rust-tauri-migration` tracking ref 一致（fetch 因本机代理 127.0.0.1:7890 不可达失败，不影响本地一致性判断；本任务无网络操作）。
- 前置核验：总控账本 R00-T06 = DONE/PASS，task_commit `4401afae2`（`ORCHESTRATOR_PROGRESS.json`，只读）；六项前置任务 tested_sha（7d1a0c6bc/16aeb380d/60dbe0384/ffcb85830/6f58b9351/64c302ec4）全部为 HEAD 祖先（merge-base 实测）。
- 工作区：`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 的预先存在未提交修改（总控维护）原样保留未触碰（`git diff --stat` 确认仍为其 14+/10- 修改，本任务零触碰）；本任务新增文件见 §10，无任何既有文件被修改。
- 任务书全文已读：00_README、01–06 共同约束、R00 阶段书（含 T07/A13/A14 全文）、90/91、stage-index/task-catalog R00-T07 条目、acceptance-catalog 200 场景分布与 R11-A14 条件授权条目。

## 2. 交付物

| 交付 | 位置 |
|---|---|
| ACCEPTANCE_MAP.json | `docs/rust-tauri/R00/ACCEPTANCE_MAP.json`（1.55MB：949 场景/100 任务/743 功能/832 入口/37 面/28 测试/14 结果/阻塞登记；requirements→tasks→tests→results→evidence 双向索引） |
| 验收账本校验器 | `docs/rust-tauri/R00/r00_t07_validate_ledger.py`（27 个规则族；CLI：`--root`/`--git-repo`；退出码 0/1/2） |
| 自测执行器 | `docs/rust-tauri/R00/r00_t07_selftest.py`（A13/A14 负向与增删反例；`--suite`/`--art-dir`） |
| 账本生成器 | `docs/rust-tauri/R00/r00_t07_build_map.py`（从任务书+已提交盘点+结果数据重建账本与 BLOCKERS.md） |
| 执行侧结果数据 | `docs/rust-tauri/R00/R00-T07_RESULTS.json`（A01–A14 结果记录：tested_sha/committed_in/命令/退出码/预期/实际/替身边界/证据/监控源码） |
| BLOCKERS.md | `docs/rust-tauri/R00/BLOCKERS.md`（active/条件授权/NOT_RUN_THIS_ROUND/预登记四节） |
| 自测证据 | `artifacts/rust-tauri/R00/T07/`（40 个文件：15+1 变体日志、canonical 明细、SELFTEST_SUMMARY、确认重跑全套） |
| 本报告 | `docs/rust-tauri/R00/R00-T07_REPORT.md` |

## 3. Steps 完成情况（对齐任务书 T07 四步）

**Step 1（导入+关联+新增子场景）**：`acceptance-catalog.json` 200 场景逐字导入（`scenarios[kind=base].spec` 十字段；来源哈希入 `basis.input_sha256`，校验器严格模式下逐场景逐字段比对任务书原文）；T02 预规格的 736 个叶子子场景（`formalization_task_id=R00-T07`）正式登记为 `kind=supplemental`（ID 集与 FEATURE_STAGE_ACCEPTANCE.json 精确相等，不重复展开正文）；T07 自身新增 13 个执行侧场景（`kind=t07_added`，A13/A14 同根因变体，`R00-T07-LA-*` 稳定 ID）。总范围 949，未缩减未篡改原始规格。

**Step 2（双向映射）**：账本五向索引可查询：场景→task_ids/feature_ids/test_ids/result_ids/entrypoint_ids；任务→scenario_ids（100 项含 depends_on/deliverables）；features_index 743 项（736 生产+7 非生产）→阶段/任务/验收/补充场景/入口；entrypoint_index 832 项（与 ENTRYPOINT_COVERAGE registrations 逐 entry 双向相等，经 feature→supplemental 场景可达）+37 个 EP-* 面；28 个现有测试/校验脚本（vitest/validator/scan/script 四类，带 SHA-256）↔ 场景。14 个已执行结果（A01–A14）每项绑定：tested_sha、committed_in、timestamp(+basis)、working_tree_digest(+scope)、platform、toolchain、dependency_lock_hashes、command、exit_code、expected、observed、stub_boundary、evidence[]（路径+SHA-256+角色）、source_paths+source_digests。

**Step 3（账本校验）**：27 个规则族（§5）；重复 ID（含 JSON 任意层级重复键）、孤立需求（场景无任务/生产功能无场景/补充场景无功能）、缺证据 PASS（STATUS-PASS-WITHOUT-RESULT / EVIDENCE-MISSING-FILE / EVIDENCE-HASH-MISMATCH）、缺环境（platform/toolchain/lockfile 字段）、跳过冒充（EXIT-CODE-CONFLICT）、结果过期（STALE-SOURCE 内容摘要 / STALE-BRANCH 祖先 / LOCKFILE-MISMATCH）均非零退出、错误行精确到 ID/字段/路径。

**Step 4（BLOCKED 区分）**：未来阶段场景保持 NOT_STARTED（185）/SPECIFIED_NOT_EXECUTED（736）不冒充；R11-A14 条件授权场景 = NOT_RUN_UNAUTHORIZED（未授权不阻塞技术交付也不算通过）；预登记仅绑定规格文本明示真实外部条件的场景（BLK-CREDENTIALS→R10-A09/R10-A10 共 2、BLK-PLATFORM→R04-A09/A10/A12、R09-A05/A07/A13、R10-A13 共 7，精确子串可复核），最晚消除阶段均 R10；BLK-LONGRUN-G1 登记为 NOT_RUN_THIS_ROUND（T06 冻结口径）。当前无 ACTIVE 阻塞。

## 4. 账本状态分布（构建时点）

| ledger_status | 计数 | 说明 |
|---|---|---|
| PASS | 27 | R00-A01..A14（14 个基础场景）+ 13 个 t07_added 变体场景，全部绑定结果 |
| NOT_STARTED | 185 | R01–R11 基础场景（未来阶段，未执行不冒充） |
| SPECIFIED_NOT_EXECUTED | 736 | T02 叶子子场景（规格就绪，实施阶段执行） |
| NOT_RUN_UNAUTHORIZED | 1 | R11-A14（发布授权未激活） |

## 5. 校验器规则族与实测

规则 ID 见校验器文件头（稳定契约）：MAP-INPUT、SPEC-IMPORT-COMPLETE（200 全量+ID 集相等+FSA 补充集相等）、SPEC-SOURCE-PROVENANCE（来源哈希）、SPEC-FIDELITY（严格模式字段级/回退模式摘要级）、NO-DUPLICATE-ID（JSON object_pairs_hook 拦任意层级重复键）、TASK-SCENARIO-BIJCTION、ORPHAN-REQUIREMENT、ENTRY-COVERAGE-BIDIR（A14 核心：registrations↔entrypoint_index 逐 entry 双向、功能可达场景）、FEATURE-INVENTORY-BIDIR、STATUS-PASS-WITHOUT-RESULT（A13 核心）、STATUS-NOTSTARTED-WITH-RESULT、STATUS-ENUM、RESULT-MISSING-FIELD（21 必填字段）、RESULT-BAD-FIELD（40-hex/ISO/int；committed_in 仅当前任务结果可为 null）、EVIDENCE-MISSING-FILE、EVIDENCE-HASH-MISMATCH、EXIT-CODE-CONFLICT、STALE-SOURCE（含账本自引用禁止）、STALE-BRANCH、LOCKFILE-MISMATCH、TESTS-MISSING/HASH、BLOCKER-REGISTRY（含条件授权状态、PASS 场景不得入阻塞表）、COUNTS-MISMATCH、LEDGER-SELF-STATUS（执行者不得自标 PASS/ACCEPTED）、STAGE-PREFIX。

正面运行（真实仓库，严格模式）：

```text
$ python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py
LEDGER_VALID checks=14761 scenarios=949 results=14 entries=832 spec_source=taskbook-strict   (exit 0)
```

防"永远失败"取巧：自测含正面对照（未篡改隔离副本必须 LEDGER_VALID，实测 exit 0）。

## 6. R00-A13 自测证据（无证据 PASS 被拒绝）

**canonical（规格正例）**：隔离副本中将补充场景 `R00-T02-LA-000E6E1301C0` 的 ledger_status 手改为 PASS、并删除 RES-R00-A01 引用的日志 `artifacts/rust-tauri/R00/T01/startup-probe.json` → 校验器 **exit 1**，输出（`T07/a13-negative-flip-status.log`）：

```text
LEDGER-ERROR EVIDENCE-MISSING-FILE result=RES-R00-A01 evidence file 'artifacts/rust-tauri/R00/T01/startup-probe.json' does not exist (deleted log)
LEDGER-ERROR STATUS-PASS-WITHOUT-RESULT scenario=R00-T02-LA-000E6E1301C0 field 'result_ids' empty: status=PASS requires >=1 result record (no-source PASS is rejected)
LEDGER_INVALID errors=2 checks=14618
```

非零退出 ✓，精确指出场景 ID+结果 ID+缺失字段+证据路径 ✓。同根因变体 10 个（均 exit 1 且子串命中，逐日志在 T07/）：v1 无结果 PASS（R01-A01）、v2 删证据、v3 篡改证据哈希、v4 删字段（tested_sha/exit_code）、v5 源码变更过期（r00-t05-replay.mjs 改动→RES-R00-A09 STALE）、v6 exit_code=1 冒充 PASS、v7 JSON 重复键、v8 任务声明场景缺失（R00-T02↛R00-A03）、v9 规格文本篡改（严格模式字段级：`scenario=R00-A04` `spec.then`）、v9b 同篡改在无任务书目录时由内嵌 spec_digests 快照回退检测。A13 套件 **11/11 变体符合预期**。

## 7. R00-A14 自测证据（新增真实功能不能漏验收）

隔离副本中向 ENTRYPOINT_COVERAGE.json registrations 增加真实形态入口 `cli:lingxi-doctor`（真实 `cli:*` 命名空间、feature_ids、source_ref）并向 FEATURE_INVENTORY.json 增加对应生产功能 `F-D99-CLI-DOCTOR`（保留分类、D20 域）：

- **v1 未映射**（账本不动）→ exit 1：`ENTRY-COVERAGE-BIDIR entry=cli:lingxi-doctor … not mapped in ledger` + `FEATURE-INVENTORY-BIDIR features_index … F-D99-CLI-DOCTOR`（`a14-v1-entry-unmapped.log`）；
- **v2 功能入索引但无场景/任务** → exit 1：`ORPHAN-REQUIREMENT feature=F-D99-CLI-DOCTOR no supplemental scenario`（`a14-v2-feature-no-scenario.log`）；
- **v3 补齐映射**（feature→场景 `R00-T07-LA-A14SIM-CLIDOCTOR`→任务 R07-T08、entrypoint_index、计数，并按获准路径重绑受影响结果摘要）→ **exit 0 LEDGER_VALID**（`a14-v3-mapped-passes.log`）；
- **v4 孤立映射**（账本引用盘点不存在的 `cli:ghost`）→ exit 1（`a14-v4-orphan-mapping.log`）。

A14 套件 **4/4 变体符合预期**。语义说明：v3 的"补齐后通过"包含对 RES-R00-A03/A04/A13/A14 的证据哈希与源码摘要重绑——这是获准路径模拟（等效于新增入口后重跑 `r00_t02_inventory.py` 扫描与 T07 自测再重建账本）；**只改映射不重绑**会被 EVIDENCE-HASH-MISMATCH/STALE-SOURCE 拒绝（v1/v2 的失败输出即证明）。即：新增生产入口的合规流程必然触发受影响检查重跑，不能静默混入。

## 8. 生产/测试接线与运行方式

- **未改任何生产代码、依赖、既有测试与 T01–T06 冻结文件**（git status 仅新增 7 个 docs 文件与 T07 证据目录）。
- 校验器/自测/生成器均为仓库内可运行工具（Python 3 标准库，无新依赖）：
  - 正面门禁：`python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py`（任务书 05 §3 的 `verify-stage` 思想在 R00 的落地形态；R02 建立 xtask 后可由其消费本账本，本任务不预建）。
  - 负向回归：`python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all`（约 40 秒；确认重跑用 `--art-dir` 指向独立目录）。
  - 重建：`python3 -B docs/rust-tauri/R00/r00_t07_build_map.py`（输入变化后的获准更新路径）。
- 自测的校验器调用为**进程内 import**（等效 argv `--root <隔离副本> --git-repo <真实仓库>`；退出码即校验器 main() 返回值，与 CLI 同源）——因本机 Mimosa 写入钩子拦截含动态 argv 的 subprocess 模式；CLI 形态本身在本报告 §9 命令表以真实进程实测（exit 0）。git 祖先校验在隔离副本中通过 `--git-repo` 指向真实仓库完成，未削弱。
- 隔离与清理：每个变体独立 `tempfile.mkdtemp` 副本（仅复制校验器读取的 78 个文件），16/16 全部删除；不读真实用户数据、无网络、无真实供应商调用、无付费请求、无外发。测试篡改只发生在临时副本。

## 9. 实际命令与结果

| 命令 | 退出码 | 结果 |
|---|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t07_build_map.py`（终轮） | 0 | MAP-BUILT scenarios=949 tasks=100 features=743 entries=832 tests=28 results=14 selftest=attached |
| `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py`（终轮，真实仓库） | 0 | LEDGER_VALID checks=14761 spec_source=taskbook-strict |
| `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all`（终轮） | 0 | SELFTEST PASS variants=15/15 (+positive-control ok) temp_removed=16/16 |
| `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir artifacts/rust-tauri/R00/T07/confirm-rerun` | 0 | 对最终账本（14 结果版）确认重跑：15/15+正面对照再次全过 |
| `python3 -m py_compile docs/rust-tauri/R00/r00_t07_{build_map,validate_ledger,selftest}.py` | 0 | 语法全过 |
| `npm run typecheck` | 0 | 三配置全过（本任务未改 TS，零干扰确认） |
| `npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts` | 0 | 2 文件 14/14（A09/A10 既有保护未受干扰） |
| `git merge-base --is-ancestor <tested_sha> HEAD`（6 个前置 SHA） | 0×6 | 全部为 HEAD 祖先 |

过程记录（保留事实，不掩盖）：首两轮自测 13/15——v4 因错误行措辞（`missing or empty` 插在 ID 与字段名之间）子串不匹配，已改错误格式为 `field '<name>' is missing or empty`；a14-v3 首版遗漏"盘点文件更新后重绑受影响结果摘要"的获准路径模拟，被自家 STALE-SOURCE 规则拒绝（该拒绝行为本身正确）。修正后全绿；中间失败轮的临时副本已清理，无过程日志残留（T07 目录只含终轮证据——自测启动时清理上一轮输出属声明行为，见脚本注释）。

## 10. 候选文件与摘要

- 候选 = 本任务交付 **95 个文件**（路径排序；R2 修复后刷新，构成与差异见 §13/§14）：6 个 docs（账本/阻塞/结果数据/三工具脚本）+ 89 个 artifacts T07 证据（23 个终轮日志与汇总 + 20 个原始确认重跑【R1 修复前历史证据，原样保留】+ 23 个 R1 修复确认重跑 + 23 个 R2 修复确认重跑）；**不含本报告自身**（报告哈希另行单列）、总控账本 `ORCHESTRATOR_PROGRESS.json`（预先存在修改，未触碰）。`docs/rust-tauri/R00/__pycache__` 为 gitignore 项。
- HEAD：`4401afae2ee48df1512b8a8c9a1262be0efbe64f`（未提交，候选以工作区文件交付）。

**逐文件 SHA-256（双空格分隔，路径排序）：

```text
42ac43c1dfab286c8f07738b432220fe49b308179ad3fec24e206780fecc0981  artifacts/rust-tauri/R00/T07/SELFTEST_SUMMARY.json
52a73e4522d1fada62203c98ad2ad47ac94547562422255b2336ec664ad64860  artifacts/rust-tauri/R00/T07/a13-canonical-detail.json
25d82f3c35cae9d990d30a396f7c315e0f38985ec4aac07fc6301f178cc34213  artifacts/rust-tauri/R00/T07/a13-canonical.log
25d82f3c35cae9d990d30a396f7c315e0f38985ec4aac07fc6301f178cc34213  artifacts/rust-tauri/R00/T07/a13-negative-flip-status.log
2138b7497e0ed25650b5b9fd47716d7ac2d1004fdc0f8897c940ef1ab0ecd9ac  artifacts/rust-tauri/R00/T07/a13-v1-flip-no-result.log
7fc235bf73436bad50b9ba826ae3cd0327a00d92b806d99e1f1a2a6f1c3550f3  artifacts/rust-tauri/R00/T07/a13-v10-scenario-result-conflict.log
ec8e8c55114c253869563317c54cefcb53090dc09c751a5576505dc6a606f0c1  artifacts/rust-tauri/R00/T07/a13-v11-result-status-enum.log
a66175cdfce11895fb0dd1b8d48da354a2cb4a847b4e92208af67baf00a73239  artifacts/rust-tauri/R00/T07/a13-v12-fail-exit-zero.log
32e343af3cdfe4d555e6d5a448f95a027908733b6e5574b23b0b113ba289a4cc  artifacts/rust-tauri/R00/T07/a13-v2-evidence-deleted.log
d3e8bc8b60a21c524f1b9ebcbd68d1dbf3595ffc9d1275fc3cf2b86c533b0d7f  artifacts/rust-tauri/R00/T07/a13-v3-evidence-tampered.log
d797e79283eb253b6ff7d03e8348a5d1060ad25f6a09dfddc98e63f2684fe6ee  artifacts/rust-tauri/R00/T07/a13-v4-missing-field.log
a6dda1d1cedb61c0ab546fe36694183bc69d47469676f98e9ab8322e54051c49  artifacts/rust-tauri/R00/T07/a13-v5-stale-source.log
65f21e8061c9e94d83f813e794e8f41a50e4d4d42369af3f8ca0ceddd56251cc  artifacts/rust-tauri/R00/T07/a13-v6-exitcode-conflict.log
071f30cec9f1d3ff1e1d69c96f1aeaa558822621d103b1fbc33f4ca90ec549d2  artifacts/rust-tauri/R00/T07/a13-v7-duplicate-id.log
f4c20e392ef78a5a431de75cf52266b519312c501ae91ccbb1755de024b78d97  artifacts/rust-tauri/R00/T07/a13-v8-task-ref-missing.log
1f78fbaab289b11c00f51b1ca579b92689646d510ee5587a96301639310569e3  artifacts/rust-tauri/R00/T07/a13-v9-spec-tamper-strict.log
7789336120ef2f3a259264def5be879832c68c2eb42d18a0b953135db99ba1fe  artifacts/rust-tauri/R00/T07/a13-v9b-digest-fallback.log
874c43973d32ceeb3be26688a04b28582a1708b2ec7603096369a65317119d16  artifacts/rust-tauri/R00/T07/a14-entry-add-remove.log
b57667733f85bad352d35a9bff732129e2561441dae203198553966edcb96aad  artifacts/rust-tauri/R00/T07/a14-v1-entry-unmapped.log
42820b8d0807d0b371ad771d194fda36ae123baf22ea4b7ee58227d7a36a9444  artifacts/rust-tauri/R00/T07/a14-v2-feature-no-scenario.log
e915bc1d2e5e48ce1b03d6b00e45225bce28242d52d1906d925a952924fe3231  artifacts/rust-tauri/R00/T07/a14-v3-mapped-passes.log
15109f33e2a34d3b9f21c75361c27f9bb2483f5649631d071122a65e4b5e7cf6  artifacts/rust-tauri/R00/T07/a14-v4-orphan-mapping.log
520d37ab9c22ecc903ea6b0756761258748ca4073c9236c6ceee84b8eb879cb6  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/SELFTEST_SUMMARY.json
52a73e4522d1fada62203c98ad2ad47ac94547562422255b2336ec664ad64860  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-canonical-detail.json
7ec6c10c0b6acb7820749f23e6515d8b2655892e2eb313ce668ce0961add459d  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-canonical.log
7ec6c10c0b6acb7820749f23e6515d8b2655892e2eb313ce668ce0961add459d  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-negative-flip-status.log
437f90876bc1c7709344e95ba722f4aa87c158b683f46545eded6c50bb98c136  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v1-flip-no-result.log
a71e79b64ae5bc919b8c4b7a135d1fef079b7a9ad620cb13a9834053b550cdfe  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v10-scenario-result-conflict.log
a03363cb2ad9f77f38e1dc5ffac9e55240af024c54630964f22f3a89f5cb3ab5  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v11-result-status-enum.log
f7db76358e87fb26ee56d27939aa10823f02cf9465ff8317f07e3acd31c72385  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v12-fail-exit-zero.log
645922eed59580f1a1b95e135d5948ec53f4f18a13727320817da149fd0d448d  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v2-evidence-deleted.log
5208157047b67090fb40129ec57d1d23cd033572303b69ffc0e7ad71a6a1f949  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v3-evidence-tampered.log
12583652517b398b219aec6c5a4a5eaa2f018c879408ca60e7585edb84010e34  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v4-missing-field.log
0909f37a18b68cb0d399ade0201ceaeb73757477766c1c64d0b190d715fe0a82  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v5-stale-source.log
0bfd86edb22b61db82c64b506e7b6bc50046657f31d4b6c6ba2ed9d40b632ad3  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v6-exitcode-conflict.log
75ea473106e7e9bcfb39ce1d8918dfa92168ed4ae121aee04548e67e1d7f5857  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v7-duplicate-id.log
61feda11e3e974d0490ee0e94acc36f6da4fa4ad49a7acf7811e04a26b086244  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v8-task-ref-missing.log
7159e5cf8908f95d84131bb05c275f65734017a07a641aa63c1a126840df55e0  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v9-spec-tamper-strict.log
a99831da7c808669a070e7f26457e89ee5a88669d40c43f3e985a094a0fe2c61  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a13-v9b-digest-fallback.log
874c43973d32ceeb3be26688a04b28582a1708b2ec7603096369a65317119d16  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a14-entry-add-remove.log
ad5aa1295e24cbe2aefba0dcb4d884dd62cd101dc993a50c302cbcc405523e32  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a14-v1-entry-unmapped.log
1f980dd2315efc8de83f65dc4b89fc6447be1acd47bfafddf02e9605a25c1c79  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a14-v2-feature-no-scenario.log
d79127db584758f52f81e2e8d83830c84b9e8b2001525a44800e07eff5cfc2c7  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a14-v3-mapped-passes.log
03a57e3c59c801dbfd4c23582b36a6bbf6029ff9cd56dcbce1579857c5f70f4c  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/a14-v4-orphan-mapping.log
a6cd6763c6c4d732ac49b16e1ae03469b6380190c96ecc9841470ade6d6c882a  artifacts/rust-tauri/R00/T07/confirm-rerun-r1/positive-control.log
19375a9bcf4e8384c78544cc7d189fb4da565683f12f0474dcafb1062f01981a  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/SELFTEST_SUMMARY.json
52a73e4522d1fada62203c98ad2ad47ac94547562422255b2336ec664ad64860  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-canonical-detail.json
7ec6c10c0b6acb7820749f23e6515d8b2655892e2eb313ce668ce0961add459d  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-canonical.log
7ec6c10c0b6acb7820749f23e6515d8b2655892e2eb313ce668ce0961add459d  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-negative-flip-status.log
437f90876bc1c7709344e95ba722f4aa87c158b683f46545eded6c50bb98c136  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v1-flip-no-result.log
a71e79b64ae5bc919b8c4b7a135d1fef079b7a9ad620cb13a9834053b550cdfe  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v10-scenario-result-conflict.log
a03363cb2ad9f77f38e1dc5ffac9e55240af024c54630964f22f3a89f5cb3ab5  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v11-result-status-enum.log
f7db76358e87fb26ee56d27939aa10823f02cf9465ff8317f07e3acd31c72385  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v12-fail-exit-zero.log
645922eed59580f1a1b95e135d5948ec53f4f18a13727320817da149fd0d448d  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v2-evidence-deleted.log
5208157047b67090fb40129ec57d1d23cd033572303b69ffc0e7ad71a6a1f949  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v3-evidence-tampered.log
12583652517b398b219aec6c5a4a5eaa2f018c879408ca60e7585edb84010e34  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v4-missing-field.log
0909f37a18b68cb0d399ade0201ceaeb73757477766c1c64d0b190d715fe0a82  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v5-stale-source.log
0bfd86edb22b61db82c64b506e7b6bc50046657f31d4b6c6ba2ed9d40b632ad3  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v6-exitcode-conflict.log
cf773076d2b44d6294bf350470f2f011c251165e847663d29caaa7a4d1616ba1  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v7-duplicate-id.log
61feda11e3e974d0490ee0e94acc36f6da4fa4ad49a7acf7811e04a26b086244  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v8-task-ref-missing.log
7159e5cf8908f95d84131bb05c275f65734017a07a641aa63c1a126840df55e0  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v9-spec-tamper-strict.log
a99831da7c808669a070e7f26457e89ee5a88669d40c43f3e985a094a0fe2c61  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a13-v9b-digest-fallback.log
874c43973d32ceeb3be26688a04b28582a1708b2ec7603096369a65317119d16  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a14-entry-add-remove.log
ad5aa1295e24cbe2aefba0dcb4d884dd62cd101dc993a50c302cbcc405523e32  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a14-v1-entry-unmapped.log
1f980dd2315efc8de83f65dc4b89fc6447be1acd47bfafddf02e9605a25c1c79  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a14-v2-feature-no-scenario.log
d79127db584758f52f81e2e8d83830c84b9e8b2001525a44800e07eff5cfc2c7  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a14-v3-mapped-passes.log
03a57e3c59c801dbfd4c23582b36a6bbf6029ff9cd56dcbce1579857c5f70f4c  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/a14-v4-orphan-mapping.log
a6cd6763c6c4d732ac49b16e1ae03469b6380190c96ecc9841470ade6d6c882a  artifacts/rust-tauri/R00/T07/confirm-rerun-r2/positive-control.log
0e43c48aa1ff5c4e3a12980b1b28d3c527307e0e3773a349e23d81f7daf0dda4  artifacts/rust-tauri/R00/T07/confirm-rerun/SELFTEST_SUMMARY.json
52a73e4522d1fada62203c98ad2ad47ac94547562422255b2336ec664ad64860  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-canonical-detail.json
664bdc4e7bb8347a4820a764430a36f317311a9233a225b28c4d17166a161e7a  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-canonical.log
664bdc4e7bb8347a4820a764430a36f317311a9233a225b28c4d17166a161e7a  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-negative-flip-status.log
f0e91cc6c3fbf6126784982d95924cea39be0c3f5235a3c9f983bd7d64f399d4  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v1-flip-no-result.log
7d0b385e23b43ea7130436bf44c613249727b9332f40dd183e7df11bf6077155  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v2-evidence-deleted.log
aa3ec8e344c444a1e495f2976db0f18877ab366414af2ba2226f1b31f36ef83b  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v3-evidence-tampered.log
a98417714614b36c58cbd6c173383af6c6ed665b736c5b18e0953f31e8b0e0ac  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v4-missing-field.log
e940af5e8bcb2e064673f1781bff594644da4971a75b0a368382afe62705cbcd  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v5-stale-source.log
e257f9f61399191fe42d182ead5afdbe7812d353667e33dec0058366cd371f1d  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v6-exitcode-conflict.log
8efde1fc6df508ed7f4e793adcc4ab4ea960a9f07771d29bc351295a3f0350d4  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v7-duplicate-id.log
c5153a8f5ee97990a22426172084428409bc8bd8966db8dce2d7db9c25b957e8  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v8-task-ref-missing.log
c861857e9c0eb7c78a2959cd7d1f1b0232d32f62643a736250e132b642e9d445  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v9-spec-tamper-strict.log
d261d9cb4575b3bcf1f29c2927c7e48321c61c0b68218b6e6fe6d7fda3a493bc  artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v9b-digest-fallback.log
bf184bbd7f20d508c4e0b31c82e50823e8c05d3df8311a5220c22602f24c4a73  artifacts/rust-tauri/R00/T07/confirm-rerun/a14-entry-add-remove.log
a2a36f40fc95dad38a1e3231ec136e2718b457ae2582580b473344e0fdfb67cf  artifacts/rust-tauri/R00/T07/confirm-rerun/a14-v1-entry-unmapped.log
410b2414321875aeea9a552ec5b60f928a22f01274dda8328c83394ca31709c0  artifacts/rust-tauri/R00/T07/confirm-rerun/a14-v2-feature-no-scenario.log
be6d300f86975d1ad27e74ee5aac16fd6d459e4344e0306f8d04b21345658536  artifacts/rust-tauri/R00/T07/confirm-rerun/a14-v3-mapped-passes.log
ad36f1896bd451eaaa0dad783ae81d4160eb633aea45fe3761b73ebbcc8e285d  artifacts/rust-tauri/R00/T07/confirm-rerun/a14-v4-orphan-mapping.log
bd52d450c211c766c995f1179e49bdaf952dcd8da4beca69675e54183b5736ab  artifacts/rust-tauri/R00/T07/confirm-rerun/positive-control.log
962db7e5b338b92cdfe4a4e0d2a8815644cf23f8c8a4311240660cae76049621  artifacts/rust-tauri/R00/T07/positive-control.log
120973a39cf501589b2c2d06bfab3c30f676e12301f4589c201f5d25d540df33  docs/rust-tauri/R00/ACCEPTANCE_MAP.json
b27c69fbece8013e24e2c305fa60bb1fd2a53c932df62a2129fa46057cf5793c  docs/rust-tauri/R00/BLOCKERS.md
8d39f81e00872a91100a46d62c9c40705c0a32a5fdd1e2339f2802081fd3f6dd  docs/rust-tauri/R00/R00-T07_RESULTS.json
60bb4655713160f50405465ee75a88b632ae893eb80df7274abe3729b4aa9980  docs/rust-tauri/R00/r00_t07_build_map.py
2307fe30ff8da1a84671e389492a45cf1dc10e9e60beaae9a073ee3924cea5c5  docs/rust-tauri/R00/r00_t07_selftest.py
08a37669e14b3d857e0b1d496c3b9af9d9fe4af9bfa0e4af001db44f067be1ce  docs/rust-tauri/R00/r00_t07_validate_ledger.py
```

**候选聚合 SHA-256**（上列 95 行按「SHA256 + 双空格 + 路径 + 换行」拼接后整体哈希；R2 修复后实算刷新）：

```text
2a26cb1f548f4390141ea8ed9c9ce68d4e87134a2f9a9b0e25114de1a7134a6c
```

**报告自身 SHA-256**：本报告不在候选集内，亦不自嵌哈希（嵌入即改变自身）；验收方对当前文件执行 `shasum -a 256 docs/rust-tauri/R00/R00-T07_REPORT.md` 可随时核对。

## 11. 未验证范围 / BLOCKED / 已知风险

- **未接入 npm/CI 门禁**：任务书 T07 未要求接入 `npm test`/CI；账本校验为独立 CLI。R02 建立 xtask `verify-stage` 时应消费本账本（05 §3 的既定路线），届时接入 Rust 侧门禁。
- **规格保真的双模式边界**：任务书目录存在时为严格模式（逐字段比对原文，v9 验证）；目录不存在（clone 后无本地任务书）时退化为账本内嵌 `spec_digests` 摘要快照（v9b 验证，场景级定位非字段级）。两种模式的检测能力都已负向验证。
- **历史结果的时间与摘要基准**：A03–A12 的 timestamp 绑定任务交付提交时间（`timestamp_basis` 逐项声明；T01 的 A01/A02 有机器时间戳）；working_tree_digest 为其受监控源码的路径排序聚合摘要（历史任务原始 tree digest 见各自报告）。这些是转录自 T01–T06 报告与 result JSON 的事实，独立验收可抽查对回。
- **预登记绑定的保守性**：只绑定规格文本明示真实外部条件的 9 个场景；阶段级依赖（R05 真实 provider LIVE、R07 平台真实账号、R08 真实数据副本授权、R10 正式签名）无逐场景明示文本，未做语义猜测绑定，由 04_阶段依赖的截止关卡约束（BLOCKERS.md notes 声明）。独立验收如发现应绑定而未绑定的场景，按账本更新路径补录。
- **自测篡改的等价性**：A13/A14 在隔离副本中模拟"手改账本/新增盘点项"，不修改真实生产代码与原始任务书（任务书禁止项）；隔离副本通过 `--git-repo` 复用真实仓库做 SHA 祖先校验，其余检查全部在副本内独立运行。
- **进程内调用**：自测以 import 方式调用校验器（Mimosa 写入钩子拦截动态 argv subprocess 所致）；退出码与 CLI 同源，CLI 形态已另行实测。若验收方要求纯进程隔离，可在任意机器直接运行 §8 的 CLI 命令复现。
- 网络受限（本机代理不可达）不影响本任务任何验证；无真实供应商/付费/外发调用。

## 12. 建议独立验收重点

1. 复跑 `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py`（应 LEDGER_VALID，14761 检查）与 `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir /tmp/acc-t07`（应 15/15+正面对照；写独立目录避免污染候选哈希）。
2. 抽查账本映射真实性：任取 `entrypoint_index` 一项（如 `bridge:dingtalk`）走 entry→feature→supplemental 场景→task 链；任取 t07_added 场景（如 `R00-T07-LA-A13-V2-EVIDENCE-DELETED`）核对其 parent_acceptance/结果绑定。
3. 核对 §6/§7 引用的日志原文与描述一致（`a13-negative-flip-status.log`、`a14-v3-mapped-passes.log`）。
4. 核对结果转录保真：抽 RES-R00-A09（对回 `artifacts/rust-tauri/R00/T05/REPLAY_SUMMARY.json` 与 T05 报告 §4）与 RES-R00-A11/A12（对回 T06 报告 §5/§6）的命令/退出码/替身边界。
5. 挑战过期检测：临时修改任一 `source_digests` 监控文件（如 `tests/migration/r00-a10-old-defect.test.ts`）后重跑校验器应 STALE-SOURCE 非零（注意恢复或用隔离副本，避免污染工作区）。
6. 复核 BLOCKERS.md 绑定：9 个预登记场景逐个对回 acceptance-catalog 原文确认关键词命中；确认 R11-A14 为 NOT_RUN_UNAUTHORIZED 且未冒充。
7. 复算 §10 聚合哈希（46 行拼接 SHA-256 应为 `a33fc436…`）。

## 13. R1 修复增补（REPAIR-R00-T07-R1，2026-09-24）

本节为 R1 独立验收（`R00-T07_REVIEW_R1.md`，结论 PASS 附 D-1 必须/D-2 建议）后的修复记录；逐项命令、退出码与证据见 `R00-T07_REPAIR_R1.md`。§1–§9、§11、§12 保留原始交付时点表述，未回改历史结论；当前候选状态以下列增量为准。

- **D-1（已修）**：§10 清单中 `confirm-rerun/a13-v3-evidence-tampered.log` 行 SHA 更正为实际值 `aa3ec8e344c444a1e495f2976db0f18877ab366414af2ba2226f1b31f36ef83b`（原为 3 字符轮转式转录笔误；磁盘文件未动，实测确认仍为该值）。
- **D-2（已修）**：校验器新增 2 个规则族并扩展 1 个——`RESULT-STATUS-ENUM`（result.status 合法集 = NOT_RUN/PASS/FAIL/BLOCKED/STALE/NOT_APPLICABLE，依据 01 通用约束 §6 场景状态集 + task-result 模板初始值；不含纯规划态 NOT_STARTED/IN_PROGRESS）、`STATUS-RESULT-CONFLICT`（场景 ledger_status=PASS 时每个绑定结果 status 必须为 PASS，错误行精确到场景 ID/结果 ID/字段）、`EXIT-CODE-CONFLICT` 增 FAIL 分支（status=FAIL 须非零退出）。FAIL/BLOCKED/STALE 仍为合法结果状态，真实失败/阻塞不会被误判为通过。
- **自测与账本同步**：自测新增 3 个负向变体（v10 场景 PASS 绑定 FAIL 结果＝R1 反例①/③状态、v11 非法枚举 MAYBE_PASSED＝R1 反例②、v12 场景与结果一致 FAIL 但 exit_code=0＝反例③规则隔离），套件 15→18 变体；账本登记 3 个新 t07_added 场景（R00-T07-LA-A13-V10-SCENARIO-RESULT-CONFLICT / V11-RESULT-STATUS-ENUM / V12-FAIL-EXIT-ZERO）。
- **获准更新路径**：修改三工具脚本 → 引导重建（12 结果账本）→ 重跑自测 18/18+正面对照 → 终轮重建（14 结果嵌入新证据哈希）→ 真实仓库 `LEDGER_VALID checks=14829 scenarios=952` → `confirm-rerun-r1/` 确认重跑 18/18。原 `confirm-rerun/` 20 文件为 R1 修复前确认重跑的历史证据，原样保留（其 a13-v3 日志即 D-1 所指文件）。
- **数量变化**：候选 46→72 文件（+3 新变体日志、+23 confirm-rerun-r1、三工具与终轮证据哈希刷新）；场景 949→952（t07_added 13→16，PASS 27→30）；校验器规则族 27→29（另 1 族扩展）；checks 14761→14829；聚合 SHA `a33fc436…` → `197371176b209f27be1968b6180f4a9fa5ef30501484427e53e93d9c914d9360`。
- **门禁复核**：`py_compile` 三工具、`npm run typecheck`（exit 0）、受影响 vitest（r00-t05-replay + r00-a10-old-defect，2 文件 14/14）全部通过；总控账本 `ORCHESTRATOR_PROGRESS.json` 未触碰。

## 14. R2 修复增补（REPAIR-R00-T07-R2，2026-09-24）

本节为 R2 独立验收（`R00-T07_REVIEW_R2.md`，结论 PASS）之后、提交前差异检查所发现问题的修复记录；逐项命令、退出码与证据见 `R00-T07_REPAIR_R2.md`。§1–§13 保留历史表述，未回改 R1/R2 验收结论；当前候选状态以下列增量为准。

- **触发**：`git diff --cached --check` 报 `docs/rust-tauri/R00/BLOCKERS.md:37: new blank line at EOF`。
- **根因与修复**：`r00_t07_build_map.py` 的 `write_blockers_md` 在末行表格后多拼接一个空行（`lines.append("")` 叠加 `join + "\n"`）。最小修正为去掉该尾部空行拼接，使 BLOCKERS.md 结尾恰一个换行、无空白行（现 36 行）。只修生成器而非手改产物，重建后缺陷不再复现。
- **获准重建**：重建一次 `r00_t07_build_map.py`（`MAP-BUILT scenarios=952 tasks=100 features=743 entries=832 tests=28 results=14 selftest=attached`）。本轮未改校验器/自测脚本，A13/A14 结果监控与全部证据哈希无需重绑——对重建前后 ACCEPTANCE_MAP.json 做规范化逐字段 diff，差异仅 `generated_at` 与 `basis.input_sha256["r00_t07_build_map.py"]`（生成器自哈希 `ff6f6cb5…` → `60bb4655…`）两处；BLOCKERS.md 差异仅删除第 37 行空白行。200+736+16 场景、832 入口、14 结果、checks=14829 与 A13/A14 行为全部不变。
- **确认重跑**：新增 `artifacts/rust-tauri/R00/T07/confirm-rerun-r2/`（23 文件）对修复后最终账本确认重跑，`SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19`；正面对照输出 `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832`（与真实仓库 CLI 校验一致）。原 `confirm-rerun/`、`confirm-rerun-r1/` 与顶层次终轮证据原样保留。
- **数量变化**：候选 72→95 文件（+23 个 confirm-rerun-r2；build_map/账本/阻塞登记三文件哈希刷新）；聚合 SHA `197371176…` → `2a26cb1f548f4390141ea8ed9c9ce68d4e87134a2f9a9b0e25114de1a7134a6c`。
- **R2 结论效力**：R2 的 PASS 绑定的是修复前候选；候选已变化，本轮修复只自评 READY_FOR_R3_REVIEW，R00-A13/A14 与任务放行由全新独立验收代理 R3 对当前候选复验决定。
