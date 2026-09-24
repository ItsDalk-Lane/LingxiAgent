# R00-T07 修复 R2（ZCode）

修复代理：REPAIR-R00-T07-R2（一次性修复代理，ZCode，只处理 R2 验收 PASS 之后、提交前差异检查发现的 BLOCKERS.md EOF 问题，不自评验收）
任务标题：R00-T07 修复 R2（ZCode）
基线：HEAD `4401afae2ee48df1512b8a8c9a1262be0efbe64f`（分支 `codex/rust-tauri-migration`，实测一致，未提交状态交付）
时点：2026-09-24（本机 macOS 27.0 darwin arm64，Python 3.14.3，Node v24.16.0）
修复结论：**EOF 缺陷已修，账本/自测/门禁复核全部通过 → READY_FOR_R3_REVIEW**（R2 的 PASS 绑定修复前候选；候选已变化，独立复验由全新代理 R3 执行，本报告不自评 PASS）

## 1. 范围与边界

- 只修总控在 R2 验收（`R00-T07_REVIEW_R2.md`，PASS）之后、提交前发现的一处差异检查问题：`git diff --cached --check` 报 `docs/rust-tauri/R00/BLOCKERS.md:37: new blank line at EOF`。未派生子代理、未建分支/worktree、未 commit/push/PR/tag/release、未执行 R00-T08。
- `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（总控未提交修改）零触碰（修复前后 `git diff --stat` 均为其原有 42+/17- 修改，本任务未读写该文件）。
- 既有验收报告零改动：`R00-T07_REVIEW_R1.md`、`R00-T07_REVIEW_R2.md` 未改一字；未回写 R1/R2 验收结论。
- 历史证据零改动：`confirm-rerun/`（20 文件）、`confirm-rerun-r1/`（23 文件）、顶层次终轮证据（23 文件）与 `R00-T07_RESULTS.json` 原样保留（本轮未改校验器/自测脚本，A13/A14 结果监控与全部证据哈希无需重绑，见 §3）。
- 未触碰其他任务（T01–T06 冻结文件、生产代码、依赖、既有测试零改动）。
- 动手前独立复核基线：按执行报告 §10 清单逐行比对磁盘 72/72 一致，聚合实算 `197371176b209f27be1968b6180f4a9fa5ef30501484427e53e93d9c914d9360` 与声称值相同——确认工作区即 R2 验收所核候选；`git diff --no-index --check /dev/null docs/rust-tauri/R00/BLOCKERS.md` 亲现 `BLOCKERS.md:37: new blank line at EOF.`，与总控描述一致。

## 2. 缺陷与修复（生成器尾部拼接）

- **根因**：`r00_t07_build_map.py` 的 `write_blockers_md` 在最后一行状态分布表格后追加空行元素（`lines.append("")`），与结尾 `"\n".join(lines) + "\n"` 叠加，使 BLOCKERS.md 以 `\n\n` 结尾（第 37 行为空白行）。
- **修复（最小、修生成器而非手改产物）**：删除该尾部 `lines.append("")` 拼接，并加注释说明「join+`\n` 已给出恰好一个结尾换行」。修复后 BLOCKERS.md 为 36 行，结尾恰一个换行、无空白行；重建后缺陷不再复现。
- 未改动 `write_blockers_md` 其余内容与三工具脚本任何其他逻辑（validate_ledger.py、selftest.py 字节不变，SHA 仍为 `08a37669…`/`2307fe30…`）。

## 3. 获准重建与产物同步

`r00_t07_build_map.py` 自身哈希记录在账本 `basis.input_sha256`，生成器变更后按 REPORT §8 的获准更新路径重建（`python3 -B docs/rust-tauri/R00/r00_t07_build_map.py`，exit 0，`MAP-BUILT scenarios=952 tasks=100 features=743 entries=832 tests=28 results=14 selftest=attached`）。

- **BLOCKERS.md**：对重建前后文件 diff，差异恰为删除第 37 行空白行，四节结构/登记项/计数表逐行不变。
- **ACCEPTANCE_MAP.json**：对重建前后做规范化逐字段 diff，差异仅两处——`generated_at`（时戳）与 `basis.input_sha256["r00_t07_build_map.py"]`（生成器自哈希 `ff6f6cb5d7495b03c47caf6195f0b36a856fc9d4e7c7a5ede88cd0a046401291` → `60bb4655713160f50405465ee75a88b632ae893eb80df7274abe3729b4aa9980`）。场景 200+736+16=952、入口 832、结果 14、checks=14829、状态分布（PASS 30/NOT_STARTED 185/SPECIFIED_NOT_EXECUTED 736/NOT_RUN_UNAUTHORIZED 1）与 A13/A14 行为全部不变。
- **结果/证据哈希无需刷新的原因**：A13/A14 结果的 `source_paths` 只监控 validate_ledger/selftest/RESULTS/四盘点 JSON（均未改动），全部证据文件未重写；R1 修复所需的「引导重建→重跑自测→终轮重建」链路本轮不适用，单次重建即完备（账本重嵌入的当前哈希全部与磁盘一致，校验器 §5 全量验证通过）。
- **确认重跑**：新增 `artifacts/rust-tauri/R00/T07/confirm-rerun-r2/`（23 文件）对修复后最终账本确认重跑（`--suite all --art-dir` 独立目录，沿用 R1 确立的模式），18/18+正面对照通过；正面对照输出 `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832`（与真实仓库 CLI 校验逐字一致）。

## 4. 验证命令与退出码（全部亲自执行）

| # | 命令 | 退出码 | 结果 |
|---|---|---|---|
| 1 | `python3 -m py_compile docs/rust-tauri/R00/r00_t07_{build_map,validate_ledger,selftest}.py` | 0 | 三工具语法全过 |
| 2 | `python3 -B docs/rust-tauri/R00/r00_t07_build_map.py`（修复后终轮重建） | 0 | `MAP-BUILT scenarios=952 tasks=100 features=743 entries=832 tests=28 results=14 selftest=attached` |
| 3 | `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py`（真实仓库，严格模式，CLI 真进程） | 0 | `LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832 spec_source=taskbook-strict`（与 R2 验收实测完全一致） |
| 4 | `python3 -B docs/rust-tauri/R00/r00_t07_selftest.py --suite all --art-dir artifacts/rust-tauri/R00/T07/confirm-rerun-r2` | 0 | `SELFTEST PASS variants=18/18 (+positive-control ok) temp_removed=19/19`；隔离副本 19/19 清理，候选零触碰 |
| 5 | EOF 差异检查：`git diff --no-index --check /dev/null docs/rust-tauri/R00/BLOCKERS.md` | —（no-index 有差异即 1） | **无任何 whitespace 错误输出**（修复前同命令输出 `BLOCKERS.md:37: new blank line at EOF.`）；文件 36 行、结尾 `|\n` 恰一个换行 |
| 6 | 全候选差异检查：`git add -N` 全部候选路径后 `git diff --check`（检查毕 `git reset -- <路径>` 还原，未留 staged 内容） | 0 | 无输出，无任何 whitespace/EOF 错误（含新增 confirm-rerun-r2 23 文件与两报告） |
| 7 | `npm run typecheck` | 0 | 三配置全过（本轮未改任何 TS/生产代码） |
| 8 | `npx vitest run tests/migration/r00-t05-replay.test.ts tests/migration/r00-a10-old-defect.test.ts` | 0 | 2 文件 14/14（A09/A10 既有保护未受干扰） |

A13/A14 关键反例在 confirm-rerun-r2 中逐日志核对（子串与退出码均符）：

- A13 canonical：`STATUS-PASS-WITHOUT-RESULT scenario=R00-T02-LA-000E6E1301C0 field 'result_ids'` + `EVIDENCE-MISSING-FILE result=RES-R00-A01 …startup-probe.json`（exit 1）；
- R1 反例①/③（v10）：`STATUS-RESULT-CONFLICT scenario=R00-A09 result=RES-R00-A09` + `EXIT-CODE-CONFLICT … status=FAIL but raw exit_code=0`；②（v11）：`RESULT-STATUS-ENUM … 'MAYBE_PASSED'`；③隔离态（v12）：仅 `EXIT-CODE-CONFLICT`；
- v9b 内嵌快照回退：`SPEC-FIDELITY scenario=R00-A04 … spec text tampered after import`；
- A14 v1：`ENTRY-COVERAGE-BIDIR entry=cli:lingxi-doctor … not mapped in ledger`；v3 补齐后 `LEDGER_VALID checks=14839 scenarios=953`；v4：`ENTRY-COVERAGE-BIDIR entrypoint_index … ['cli:ghost']`；
- 正面对照（防「永远失败」）：`LEDGER_VALID checks=14829 scenarios=952 results=14 entries=832`。

## 5. 修改文件清单（本轮全部改动）

| 文件 | 改动 |
|---|---|
| `docs/rust-tauri/R00/r00_t07_build_map.py` | `write_blockers_md` 删除尾部空行拼接 + 注释（唯一代码改动；SHA `ff6f6cb5…` → `60bb4655713160f50405465ee75a88b632ae893eb80df7274abe3729b4aa9980`） |
| `docs/rust-tauri/R00/BLOCKERS.md` | 按修复后生成器重建：仅删除第 37 行空白行（SHA `6f6945a8…` → `b27c69fbece8013e24e2c305fa60bb1fd2a53c932df62a2129fa46057cf5793c`） |
| `docs/rust-tauri/R00/ACCEPTANCE_MAP.json` | 按获准路径重建：仅 `generated_at` 与 build_map 自哈希两处变化（SHA `8a5956b4…` → `120973a39cf501589b2c2d06bfab3c30f676e12301f4589c201f5d25d540df33`） |
| `artifacts/rust-tauri/R00/T07/confirm-rerun-r2/`（23 文件，新增） | 修复后最终账本的确认重跑全套证据（18/18+正面对照） |
| `docs/rust-tauri/R00/R00-T07_REPORT.md` | §10 候选清单/聚合并引言计数刷新（脚本从磁盘实算，未手抄）+ 新增 §14 修复增补节；§1–§13 未回改（修复后 SHA `a67d51e71faeaa4fca59f9f83373e88cf2efd144df970b75e487a5d424498d71`，供总控重新绑定） |
| `docs/rust-tauri/R00/R00-T07_REPAIR_R2.md` | 本报告（新增） |

未改动：`r00_t07_validate_ledger.py`、`r00_t07_selftest.py`、`R00-T07_RESULTS.json`、顶层次终轮证据、`confirm-rerun/`、`confirm-rerun-r1/`、两份验收报告、T01–T06 全部冻结文件、总控账本、生产代码与依赖。

## 6. 候选清单与聚合 SHA

- 候选 = 95 文件（6 docs + 89 artifacts T07 证据），完整逐文件清单见执行报告 §10（脚本从磁盘实算生成）。
- **候选聚合 SHA-256**（95 行按「SHA256 + 双空格 + 路径 + 换行」拼接、路径排序，整体哈希）：

```text
2a26cb1f548f4390141ea8ed9c9ce68d4e87134a2f9a9b0e25114de1a7134a6c
```

- 本报告与执行报告均不自嵌自身哈希；以 `shasum -a 256 <文件>` 现算核对（执行报告现值见 §5）。

## 7. 未执行范围

- **commit/暂存交付**：修复代理未自行提交、未替总控提交；第 4 表 #6 的 `git add -N` 仅为差异检查，检查毕已 `git reset` 还原，最终未留任何 staged 内容。总控将在 R3 独立验收 PASS 后按用户既有授权提交并推送（本轮禁止的只是修复代理自行提交，非总控无授权）。
- **npm/CI 门禁接入、全量 `npm test`**：任务书 T07 未要求；本轮零 TS/生产代码改动，按受影响面只跑 typecheck + 2 个迁移测试文件（同 R1/R2 口径）。
- **R3 复验**：本轮修复使候选变化，R2 PASS 不再直接适用；R00-A13/A14 与任务放行由全新独立验收代理 R3 对当前候选复验决定。
- **其他平台/真实供应商/LIVE/发布**：非本修复范畴；R11-A14 维持 NOT_RUN_UNAUTHORIZED。
- **网络操作**：无（本机代理不可达不影响任何本地验证）。

---

**声明：修复完成，READY_FOR_R3_REVIEW。** 本报告为修复记录，不构成独立验收结论；不得自评 PASS。
