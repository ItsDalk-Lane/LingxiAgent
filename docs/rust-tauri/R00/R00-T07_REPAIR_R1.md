# R00-T07 修复 R1（ZCode）

修复代理：REPAIR-R00-T07-R1（一次性修复代理，ZCode，只修 R1 报告所列 D-1/D-2，不自评验收）
任务标题：R00-T07 修复 R1（ZCode）
基线：HEAD `4401afae2ee48df1512b8a8c9a1262be0efbe64f`（分支 `codex/rust-tauri-migration`，实测一致，未提交状态交付）
时点：2026-09-24（本机 macOS 27.0 darwin arm64，Python 3.14.3，Node v24.16.0）
修复结论：**D-1 已修，D-2 已修，全部门禁通过 → READY_FOR_R2_REVIEW**（独立复验由全新代理另行执行，本报告不自评 PASS）

## 1. 范围与边界

- 只修 `R00-T07_REVIEW_R1.md` §10 所列 D-1（必须）与 D-2（建议增强，按修复指令一并执行）。未派生子代理、未建分支/worktree、未 commit/push/PR/tag/release。
- `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（总控未提交修改）零触碰（`git diff --stat` 仍为其原有修改，本任务未读写该文件）。
- 保留全部 T07 原有交付与验收报告：`R00-T07_REVIEW_R1.md` 未改一字；原 `artifacts/rust-tauri/R00/T07/confirm-rerun/` 20 个文件作为 R1 修复前的历史确认重跑证据原样保留（其中 a13-v3 日志即 D-1 所指文件，实测 SHA 未变）。
- 执行报告 `R00-T07_REPORT.md` 只做三类改动：§10 清单 D-1 行更正 + 清单/聚合整体刷新（磁盘实算）、§10 引言计数同步、新增 §13 修复增补节；§1–§9、§11、§12 历史表述未回改。
- 修复前独立复核 R1 事实：按报告原 §10 清单逐行比对磁盘，恰 45/46 一致，唯一不符行即 D-1 所指（报告 `…4142afba…`，磁盘 `…414af2ba…`）；按磁盘文件重算 46 行聚合恰为报告声称的 `a33fc436…`，按清单文字重算为 `cbd9224d…`——与 R1 §2 完全一致，确认「单行转录笔误、候选文件未被篡改」的定性。

## 2. D-1 修复：报告 §10 单行 SHA 笔误

- 位置：`docs/rust-tauri/R00/R00-T07_REPORT.md` §10 清单 `artifacts/rust-tauri/R00/T07/confirm-rerun/a13-v3-evidence-tampered.log` 行。
- 更正：`aa3ec8e344c444a1e495f2976db0f18877ab3664142afba2226f1b31f36ef83b` → **`aa3ec8e344c444a1e495f2976db0f18877ab366414af2ba2226f1b31f36ef83b`**（磁盘实测值，本修复前后各核一次，文件未动）。
- 因 D-2 引起候选文件变化，§10 清单整体以脚本从磁盘实算重写（72 行，杜绝再次手抄），聚合 SHA 由 `a33fc436…` 刷新为 `197371176b209f27be1968b6180f4a9fa5ef30501484427e53e93d9c914d9360`（见 §8）。
- 更正后执行报告自身 SHA-256：`9580f612c7e78f0f6c2a981730ab9173138baecc85a438895f1f9c0b531913f8`（供总控重新绑定；报告不自嵌自身哈希）。

## 3. D-2 修复：校验器三处矛盾状态放行（`r00_t07_validate_ledger.py`）

R1 用隔离探针证实三个缺口（探针 R1a/R1b/R1c 均被旧校验器 exit 0 放行）。本次新增 2 个规则族并扩展 1 个（规则 ID 为稳定契约，已同步文件头清单）：

| 缺口（R1 探针） | 新规则 | 语义与错误行格式 |
|---|---|---|
| ① 场景 `ledger_status=PASS` 但绑定 `result.status=FAIL` | `STATUS-RESULT-CONFLICT`（新） | PASS 场景的每个绑定结果 status 必须为 PASS。错误行：`LEDGER-ERROR STATUS-RESULT-CONFLICT scenario=<sid> result=<rid> field 'status' is '<v>' but scenario ledger_status=PASS (…)`——精确到场景 ID、结果 ID、字段名 |
| ② `result.status` 无枚举校验（`MAYBE_PASSED` 放行） | `RESULT-STATUS-ENUM`（新） | 合法集 = `NOT_RUN / PASS / FAIL / BLOCKED / STALE / NOT_APPLICABLE`。错误行：`LEDGER-ERROR RESULT-STATUS-ENUM result=<rid> field 'status' value '<v>' not in allowed result status set […]` |
| ③ `result.status=FAIL` 且 `exit_code=0` 不报错 | `EXIT-CODE-CONFLICT`（扩展 FAIL 分支） | `status=FAIL` 须非零退出。错误行：`LEDGER-ERROR EXIT-CODE-CONFLICT result=<rid> status=FAIL but raw exit_code=0 (…)` |

合法集依据（指令要求核对既有集合与历史记录，避免误判真实失败/阻塞）：

- 01 通用约束 §6 场景状态集 = NOT_STARTED、IN_PROGRESS、PASS、FAIL、BLOCKED、STALE、NOT_APPLICABLE；其中 NOT_STARTED/IN_PROGRESS 是未执行场景的规划态，不允许出现在执行结果记录中，故排除。
- `task-result.template.json` 执行前占位值 `NOT_RUN`，纳入合法集。
- 历史盘点（T01–T06 工件与账本）：验收级结果记录实际取值仅 PASS/FAIL/BLOCKED（如 T01 的 `R00-A01.result.json` PASS、`startup-probe-r2-attempt1.json` 内尝试记录 BLOCKED、`verify_candidate_r2.py` 按 PASS/FAIL/BLOCKED 分支校验）；`OBSERVED_STABLE`/`PENDING_MEASUREMENT`/`MEASURED` 是探针/阈值文件自身内部字段，不是 result.status 语义。
- **FAIL/BLOCKED/STALE 保持合法**：真实失败、阻塞、过期结果仍可如实登记，不会被判为非法；新规则只是禁止它们挂在 PASS 场景下（①）和禁止零退出支撑 FAIL（③）。当前真实账本 14 个结果全部 status=PASS、exit_code=0，新规则下不受影响（§5 正向校验通过）。

设计说明：探针①/③在账本中是同一篡改态（PASS 场景 + FAIL 结果 + 零退出）的两面，会同时触发 `STATUS-RESULT-CONFLICT` 与 `EXIT-CODE-CONFLICT`——同一篡改的两条真实矛盾各自报出，属预期而非误报；自测 v12 提供规则隔离态（场景与结果一致 FAIL、仅零退出矛盾）证明 FAIL 分支独立生效。

## 4. D-2 同步：自测变体、场景登记与账本再生成

**自测新增 3 个负向变体（`r00_t07_selftest.py`，全部在 `tempfile.mkdtemp` 隔离副本执行，不触碰真实仓库文件）**，与 R1 探针一一对应：

- `v10-scenario-result-conflict`：RES-R00-A09.status → FAIL（场景 R00-A09 保持 PASS、exit_code 保持 0）＝R1 反例①/③状态 → exit 1，双规则同报（实测输出）：
  ```text
  LEDGER-ERROR STATUS-RESULT-CONFLICT scenario=R00-A09 result=RES-R00-A09 field 'status' is 'FAIL' but scenario ledger_status=PASS (PASS scenario requires every bound result to be PASS; real FAIL/BLOCKED must not be presented as passed)
  LEDGER-ERROR EXIT-CODE-CONFLICT result=RES-R00-A09 status=FAIL but raw exit_code=0 (a zero exit cannot evidence FAIL; the failing signal itself is required)
  ```
- `v11-result-status-enum`：RES-R00-A09.status → `MAYBE_PASSED` ＝R1 反例② → exit 1：
  ```text
  LEDGER-ERROR RESULT-STATUS-ENUM result=RES-R00-A09 field 'status' value 'MAYBE_PASSED' not in allowed result status set ['BLOCKED', 'FAIL', 'NOT_APPLICABLE', 'NOT_RUN', 'PASS', 'STALE'] (a result may not invent ambiguous outcome values)
  ```
  （另伴生一条 STATUS-RESULT-CONFLICT：非法枚举同样 ≠PASS，两报均真。）
- `v12-fail-exit-zero`：场景 R00-A09 与 RES-R00-A09 一致改为 FAIL、exit_code 保持 0 ＝反例③规则隔离态 → exit 1，仅 `EXIT-CODE-CONFLICT result=RES-R00-A09 status=FAIL but raw exit_code=0`。

**账本同步（`r00_t07_build_map.py`）**：按既有模式登记 3 个新 t07_added 场景（`R00-T07-LA-A13-V10-SCENARIO-RESULT-CONFLICT` / `V11-RESULT-STATUS-ENUM` / `V12-FAIL-EXIT-ZERO`，parent_acceptance=R00-A13），并挂入 validate_ledger 与 selftest 两个 tests 条目的 scenario_ids。计数变化：场景 949→952（t07_added 13→16）、PASS 27→30、checks 14761→14829；总范围仍只增未减（200 基础 + 736 补充 + 16 新增）。

**获准更新路径**（工具脚本变更后结果必然过期，按账本自身规则的既定流程再生成）：

```text
rm artifacts/rust-tauri/R00/T07/SELFTEST_SUMMARY.json   # 回到引导态（A13/A14 不入账本）
python3 -B docs/rust-tauri/R00/r00_t07_build_map.py     # MAP-BUILT … results=12 selftest=pending (exit 0)
python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all
                                                         # SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19 (exit 0)
python3 -B docs/rust-tauri/R00/r00_t07_build_map.py     # MAP-BUILT … results=14 selftest=attached (exit 0)
python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py
                                                         # LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832 spec_source=taskbook-strict (exit 0)
python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir artifacts/rust-tauri/R00/T07/confirm-rerun-r1
                                                         # SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19 (exit 0)
```

顶层次终轮证据（SELFTEST_SUMMARY.json、a13-negative-flip-status.log、a14-entry-add-remove.log 等 23 个文件）由自测重写、账本重绑新哈希；`R00-T07_RESULTS.json` 零改动（A13/A14 的 observed 在账本内由汇总自动回填 14/14、4/14→4/4）；BLOCKERS.md 仅计数表随构建刷新。新增 `confirm-rerun-r1/`（23 文件）为修复后最终账本的确认重跑；原 `confirm-rerun/`（20 文件）不动。

## 5. A13/A14 正反例复跑与三个 R1 反例拒绝（独立命令/退出码）

| # | 命令（`--art-dir` 独立目录复跑，候选零触碰） | 退出码 | 结果 |
|---|---|---|---|
| 1 | `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all` | 0 | `SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19`；A13 14/14（canonical+v1–v9b 原语义不变 + 新 v10–v12）、A14 4/4（v1/v2/v4 拒绝、v3 补齐后 exit 0 LEDGER_VALID） |
| 2 | `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir artifacts/rust-tauri/R00/T07/confirm-rerun-r1` | 0 | 对最终账本（14 结果嵌入版）确认重跑：18/18+正面对照再次全过 |
| 3 | R1 反例①复现（=套件 v10） | 1 | `STATUS-RESULT-CONFLICT scenario=R00-A09 result=RES-R00-A09` + `EXIT-CODE-CONFLICT result=RES-R00-A09`（§4 引原文） |
| 4 | R1 反例②复现（=套件 v11） | 1 | `RESULT-STATUS-ENUM result=RES-R00-A09` + `'MAYBE_PASSED'`（§4 引原文） |
| 5 | R1 反例③复现（=套件 v12 隔离态；v10 为其复合态） | 1 | `EXIT-CODE-CONFLICT result=RES-R00-A09 status=FAIL but raw exit_code=0`（§4 引原文） |
| 6 | 正面对照（防「永远失败」取巧；未篡改隔离副本） | 0 | `LEDGER_VALID`（每轮套件内置，两轮均过） |
| 7 | `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py`（真实仓库，严格模式，CLI 真进程） | 0 | `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832 spec_source=taskbook-strict` |

canonical 行为不变核对：终轮 `a13-canonical.log` 仍同时报 `STATUS-PASS-WITHOUT-RESULT scenario=R00-T02-LA-000E6E1301C0 field 'result_ids'…` 与 `EVIDENCE-MISSING-FILE result=RES-R00-A01 …startup-probe.json`，A13 规格正例语义与 R1 验收时一致。

## 6. typecheck 与受影响测试

| 命令 | 退出码 | 结果 |
|---|---|---|
| `python3 -m py_compile docs/rust-tauri/R00/r00_t07_{build_map,validate_ledger,selftest}.py` | 0 | 三工具语法通过 |
| `npm run typecheck` | 0 | 三配置全过（本修复未改任何 TS/生产代码） |
| `npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts` | 0 | 2 文件 14/14（A09/A10 既有保护未受干扰） |

## 7. 修改文件清单

| 文件 | 改动 |
|---|---|
| `docs/rust-tauri/R00/r00_t07_validate_ledger.py` | 新增 `ALLOWED_RESULT_STATUS` 常量与 2 规则族、扩展 EXIT-CODE-CONFLICT；文件头规则清单同步 |
| `docs/rust-tauri/R00/r00_t07_selftest.py` | 新增 tamper_v10/v11/v12 与注册表断言（3 个负向变体） |
| `docs/rust-tauri/R00/r00_t07_build_map.py` | 登记新 3 个 t07_added 场景；两个 tests 条目 scenario_ids 同步 |
| `docs/rust-tauri/R00/ACCEPTANCE_MAP.json` | 按获准路径再生成（新场景/计数/新证据与源码哈希绑定；R00-T07_RESULTS.json 数据零改动） |
| `docs/rust-tauri/R00/BLOCKERS.md` | 随构建刷新计数表（t07_added 16、PASS 30），四节结构与登记项不变 |
| `artifacts/rust-tauri/R00/T07/`（顶层 23 文件） | 终轮自测证据重写（+v10/v11/v12 三个新日志） |
| `artifacts/rust-tauri/R00/T07/confirm-rerun-r1/`（23 文件） | 新增：修复后最终账本确认重跑全套 |
| `docs/rust-tauri/R00/R00-T07_REPORT.md` | D-1 行更正、§10 清单/聚合磁盘实算刷新、§13 修复增补节 |
| `docs/rust-tauri/R00/R00-T07_REPAIR_R1.md` | 本报告（新增） |

## 8. 最终哈希

- **候选 72 文件逐行 SHA-256 与聚合**：以脚本从磁盘实算生成并整体嵌入 `R00-T07_REPORT.md` §10（未手抄），修复后复核 72/72 行与磁盘一致；聚合 SHA-256 = `197371176b209f27be1968b6180f4a9fa5ef30501484427e53e93d9c914d9360`（拼接口径不变：「SHA256 + 双空格 + 路径 + 换行」按路径排序）。D-1 所指行现值 `aa3ec8e344c444a1e495f2976db0f18877ab366414af2ba2226f1b31f36ef83b`（文件实测复核两次，未动）。
- 直接修改文件 SHA-256：validate_ledger `08a37669e14b3d857e0b1d496c3b9af9d9fe4af9bfa0e4af001db44f067be1ce`；selftest `2307fe30ff8da1a84671e389492a45cf1dc10e9e60beaae9a073ee3924cea5c5`；build_map `ff6f6cb5d7495b03c47caf6195f0b36a856fc9d4e7c7a5ede88cd0a046401291`；执行报告（D-1 更正+清单刷新+§13 后）`9580f612c7e78f0f6c2a981730ab9173138baecc85a438895f1f9c0b531913f8`。
- 本报告自身 SHA 不自嵌（嵌入即改变自身，同执行报告 §10 约定）；以 `shasum -a 256 docs/rust-tauri/R00/R00-T07_REPAIR_R1.md` 现算核对。

## 9. 未执行范围

- **npm/CI 门禁接入**：任务书 T07 未要求（R02 xtask `verify-stage` 消费账本时接入），本次不扩范围。
- **全量 `npm test`**：本修复零 TS/生产代码改动，按受影响面只跑 §6 两组（typecheck + 2 个迁移测试文件）。
- **其他平台/真实供应商/LIVE**：非本修复范畴；R11-A14 维持 NOT_RUN_UNAUTHORIZED。
- **T01–T06 原始测量重跑**：其结果记录未被本次改动触及（tested_sha/证据绑定仍为原值，账本校验通过即为证）。
- **网络操作**：无（本机代理不可达不影响任何本地验证）。

## 10. 其他观察（未改动，如实记录）

- 探针①/③复合态下双规则同报属设计而非误报（§3 设计说明）；v11 的伴生冲突行同理。
- 校验器既有重复 result_id 检查的错误文案含自指短语 `(also at <同一rid>)`，检测行为正确、仅文案冗余；按最小改动原则本次不修，留待后续账本例行更新时酌情处理。

---

**声明：修复完成，READY_FOR_R2_REVIEW。** 本报告为修复记录，不构成独立验收结论；R00-A13/R00-A14 与 R00-T07 候选（含本修复）能否放行由 R2 复验代理判定。
