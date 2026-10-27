# R01 阶段修复 R7 — 修复报告

- 修复者：ZCode:R01 阶段修复 R7（ZCode 一次性任务）；非 R1–R6 修复者、非 R1–R7 阶段验收者、非任一 T 执行/评审代理。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`；仓库 `ItsDalk-Lane/LingxiAgent`。
- 验收输入：`/tmp/r01-stage-review-r7.md`，SHA-256 `e2bf485c725806ab8c052fec2a32a887d26611eb14e5c3df0c1201f15b3ea1c1`（本机实算吻合）。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01 修复完成并通过主仓与 /tmp 隔离副本双跑回归；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。

## 0. 开工状态核对（与交接一致）

- 本地 HEAD 与开工时 remote-tracking（`@{u}`）均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`；收尾 `git ls-remote` 因本机代理（127.0.0.1:7890）离线未能复测远端（如实记录，与 R6 同限制；全程零推送，开工时评审开工/收尾与本修复开工三处记录远端均为 `363999378…`）。
- 开工未提交候选 = 18 个已跟踪修改 + 67 个新文件 = 85 件；按 R5/R6/R7 评审同法（`git diff --binary` 字节串接按路径排序的未跟踪文件 SHA-256 清单后取 SHA-256）复算开工冻结指纹 = `04dea823b8ff24703241ec2d6128efcb991586565d585e9249e5f61044b3999f`（原像 307,095 B），与 R6 修复自报及 R7 评审冻结值逐字节一致。
- R7 修复在 R1–R6 候选之上叠加：R6 候选 85 件中 76 件与 R6 态逐字节一致（其中 STAGE_REPAIR_R6 证据 10 件对 R6 报告留档哈希清单 `/tmp/r01-repair-r6/r6-file-hashes.txt` 逐项匹配 10/10，该 10 件系 R6 rsync 后落位主仓、R6-iso 无参照）；未执行 reset --hard / clean -fd，未覆盖他人改动；主仓 `git diff --check` 通过（零空白错误）。

## 1. F01（BLOCKING）重复「最迟」中第二个相矛盾的期限被忽略 → 已修复

### 根因

检查器 `r01_t08_gate_check.py` 的「最迟 X」核验在 `_stage_binding_problems()` 内对折叠后正文调用 `LATEST_MARKER_RE.search(folded)`（修复前第 291 行）——`search()` 只返回第一个匹配，只有首处「最迟」参与「存在性 + 与结构化 `resolve_latest_stage_id` 一致」两项核验。正文写出两处互相矛盾的强制期限时（如 `R09（宿主集成）最迟 R09；最迟 R10（虚构放宽关卡）`，结构化 `resolve_by_stage_id=R09`、`resolve_latest_stage_id=R09` 保持原样），第二处「最迟 R10」被完全忽略：真实 CLI exit 0 / PASS_WITH_CONDITIONS（R7 评审实测），风险交接正文与机器接受的最迟期限不一致。`_extract_stage_references` 虽提取第二处的 `R10`，但 R10 真实存在于 R00–R11，通用 token 存在性核验不会核对它与结构化最迟的矛盾。单处「最迟 R10」与顺序交换「最迟 R10；最迟 R09」（首处即矛盾）均被正确拒绝——方向性证据正说明仅首处受检。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.6-stage-repair-r6 → 1.7-stage-repair-r7，规则族 G6d）

1. **逐处核验、失败关闭**：`LATEST_MARKER_RE.search()` 改 `finditer()` 遍历折叠后正文**每一处**可识别的「最迟 X」标记（带序号诊断：第 N 处）——任一标记引用不存在于真实阶段索引（R00–R11）的阶段 → `risk-stage-unknown`；任一标记 ≠ 结构化 `resolve_latest_stage_id` → `risk-stage-text-mismatch`。正文每一处「最迟」都是强制期限声明，任一相矛盾即拒。
2. **多处一致的清晰契约（评审第七行许可的契约选择）**：正文可写多处「最迟」，全部与结构化最迟关卡一致方可放行（`最迟 R09；最迟 R09` → exit 0）；选择「全部一致才放行」而非「重复即拒」，与 R6 评审表第七行现状一致且不误伤重复一致书写。
3. **不误伤正常上下文（评审第六行）**：与「最迟」语法无关的正文阶段提及不构成最迟标记——`R09（宿主集成）；R10（后续平台事项）最迟 R09` 中 R10 是真实存在的普通引用，唯一最迟标记 R09 与结构化一致 → exit 0。
4. **权威不放宽**：结构化 `resolve_latest_stage_id`（与冻结契约 `latest_stage`）仍是判定权威，正文核验只加严不放宽；冻结契约、REAL_STAGE_INDEX（R00–R11）、全角折叠（G6c）、完整标识边界（G6b）一字节未动——R6→R7 检查器 diff 159 行中对 FROZEN_CONTRACT/REAL_STAGE_INDEX 零命中（diff 留档 `/tmp/r01-repair-r7/gate-check-r6-to-r7.diff`）。

### 修复后行为矩阵（真实 CLI 实测，主仓+iso 双跑；仅改 /tmp 登记副本 RR-T05-X1 正文，结构化坐标 R09/R09 不动）

| 正文变体 | 修复前（R7 评审实测） | 修复后 |
|---|---|---|
| `R09（browser worker 边界实现与门禁）`（原文正向对照） | exit 0 / PASS_WITH_CONDITIONS | exit 0 / PASS_WITH_CONDITIONS（不变） |
| `R09（宿主集成）最迟 R10（虚构放宽关卡）`（单处矛盾） | exit 1 / NO-GO / risk-stage-text-mismatch | exit 1 / 同（不变） |
| **`R09（宿主集成）最迟 R09；最迟 R10（虚构放宽关卡）`（F01 原样反例）** | **exit 0 / PASS_WITH_CONDITIONS（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch（第 2 处点名）** |
| `R09（宿主集成）最迟 R10；最迟 R09`（顺序交换） | exit 1 / NO-GO（首处即矛盾） | exit 1 / 同（不变） |
| `R09（宿主集成）最迟 Ｒ０９；最迟 Ｒ１０`（全角混用） | exit 0 / PASS_WITH_CONDITIONS（误放） | exit 1 / NO-GO / risk-stage-text-mismatch（折叠后第 2 处矛盾） |
| `R09（宿主集成）；R10（后续平台事项）最迟 R09`（上下文提及 R10） | exit 0 | exit 0（不误伤，不变） |
| `R09（宿主集成）最迟 R09；最迟 R09（重复但一致）`（多处一致） | exit 0 | exit 0（清晰契约放行，两处均实检） |

证据：`STAGE_REPAIR_R7/f01-stage-repro-r7.txt`（13 例 R7 评审登记复测，主仓）、`f01-iso-recheck-r7.txt`（iso 双跑）。

### 负向/正向覆盖（进程内与真实 CLI 双层，只增不删）

- **进程内自测 72/72**：positive-control + N1–N65（R1–R6 全保留）+ **N66–N68 新增 3 个重复最迟反例**（首处一致/后处冲突→text-mismatch、顺序交换→text-mismatch、全角混用→text-mismatch）+ **P3-positive-double-latest-consistent**（多处一致放行，TRACKED 命中）+ **P4-positive-contextual-r10-not-marker**（上下文提及 R10 不误伤，TRACKED 命中）。证据：`f01-gate-selftest-r7.txt`（主仓）、`f01-iso-recheck-r7.txt`（iso）。
- **真实 CLI 电池 4 正向 + 46 负向**：`r01_t08_gate_cli_regression.sh` 在 2 正+43 负之上追加 5 个 R7 变体（3 负向全部 exit 1 且类别正确 + 2 正向 exit 0）；正向对照 exit 0 不变。证据：`f01-cli-regression-r7.txt`（主仓）、`f01-iso-recheck-r7.txt`（iso）。
- **R7 评审原样反例+对照 13/13**（评审留档 `/tmp/r01-r7-*.json`，与评审同法仅改临时登记副本）：两条 F01 原样误放修复后 exit 1/NO-GO（text-mismatch），全角混用/顺序交换/单处矛盾 exit 1，两处一致正向与上下文 R10 正向 exit 0，R4/R5/R6 原样反例 4 例类别不变（unknown/disguised/text-mismatch），R6 规范化正向 exit 0。
- **R1–R6 全套原样反例回归 25/25**（登记副本 `/tmp/r01-repair-r5/repro/` 18 例 + `/tmp/r01-repair-r6/repro/` 7 例，主仓+iso 双跑）：control 与两个规范化正向 exit 0；22 负向全部 exit 1 且类别逐项不变。证据：`f01-r1-r6-regression-r7.txt`、`f01-iso-recheck-r7.txt`。
- 未删任何测试、未放宽任何关卡、未改任何既有断言（全部为追加）；未改任何风险登记正文/结构化坐标（`RISK_REGISTER.json` 本轮一字节未动，SHA-256 保持 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307`）。

### 设计决策的如实说明

- **选「逐处核验 + 全部一致放行」而非「重复即拒」**：评审允许两者（「可按清晰契约放行或拒绝但不能漏检」）；多处一致书写（`最迟 R09；最迟 R09`）在真实登记书写习惯中属冗余而非矛盾，拒绝会造成无误伤余地的过紧契约。失败关闭由「每一处均须 == 结构化 latest」保证——任何更晚/更早/不存在的期限都被逐处拒绝。
- **诊断带序号**（第 N 处）：多处标记时逐处定位，单处文本诊断语义不变；既有测试只钉类别码（`BLOCKED(risk-stage-*)`），不受消息文案影响。
- **标记语法不变**：`LATEST_MARKER_RE` 本身（含完整标识边界与全角折叠前置）未动，伪装形态（最迟 R09.5、最迟 R099 等）仍由 G6b/G6c 既有路径拒绝。

## 2. 修改范围与指纹

R7 叠加改动（R6 候选 85 件中 76 件逐字节未动，逐文件证明见 `/tmp/r01-repair-r7/r7-file-hashes.txt`）：已跟踪修改 8 项 + 既存未跟踪脚本就地更新 1 项（`r01_t08_gate_cli_regression.sh`，R2 引入）+ 新增 `STAGE_REPAIR_R7/` 证据 10 件。候选总计 **95 文件 = 18 修改 + 77 新增**（67 前轮 + 10 R7）。`git diff --check` 通过。

逐文件 SHA-256（R7 触碰 9 项；完整清单 `/tmp/r01-repair-r7/r7-file-hashes.txt`）：

```
M docs/rust-tauri/R01/r01_t08_gate_check.py            （G6d finditer 逐处核验 + 自测 N66–N68/P3/P4 + 契约 1.7）
U docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh   （既存未跟踪，就地更新：+3 负向 +2 正向 → 4 正+46 负）
M docs/rust-tauri/R01/R01_REPORT.md                    （R7 进展叙述 + 独立审查段）
M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json       （description + A15 stage_repair_note 补 R7 链）
M docs/rust-tauri/R01/R01_HANDOFF.json                 （stage_review_r7/stage_repair_r7 坐标 + stage_gate_position + artifact_hashes 重算）
M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json           （R01 stage 块：round 7 / verdict FAIL / 复验与修复坐标）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json   （1.7 契约重生成，判定不变 PASS_WITH_CONDITIONS/15 TRACKED）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log     （1.7 契约重生成）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log  （1.7 契约重生成，72 用例）
N artifacts/rust-tauri/R01/STAGE_REPAIR_R7/（10 件，逐文件哈希见 /tmp/r01-repair-r7/r7-file-hashes.txt）
```

各项实际哈希值以 `/tmp/r01-repair-r7/r7-file-hashes.txt` 为准（避免报告内嵌哈希转录错误）。

未触碰确认：`RISK_REGISTER.json` 仍为 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307`；F01 为纯检查器侧修复，真实登记正文/结构化坐标一字节未动。

**候选工作树冻结指纹**（同 R5/R6/R7 评审法：`git diff --binary` 字节直接串接按路径排序的新文件 SHA-256 清单后取 SHA-256；原像 325,821 B 留档 `/tmp/r01-repair-r7/r7-final-fingerprint-input.bin`）：

```
1c2bd57d3f973cc49e428fc6476af61e4ff0bbc18706702bc87ca0a2be624d29
```

HANDOFF `artifact_hashes` 20 项钉住哈希在最终候选状态逐项实算全部匹配（5 项按 R7 改动重算：R01_REPORT.md/R01_ACCEPTANCE_LEDGER.json/r01_t08_gate_check.py/r01_t08_gate_cli_regression.sh/gate-report.json；15 项未触碰逐字节复核，RISK_REGISTER.json 保持 `87e34d0e…`）；`working_tree_digest` 维持 T08 执行时点快照不改（同 R1–R6 惯例，钉住对象为已入库的 T08 交付）。JSON 重写方法：HANDOFF / ORCHESTRATOR_PROGRESS / LEDGER 均为 `json.dumps(ensure_ascii=False, indent=2)+"\n"` 字节级可重现格式（本轮编辑后三文件逐一验证 raw==canonical）。

## 3. /tmp 隔离副本验证矩阵（iso=/tmp/r01-repair-r7/iso，rsync 自主仓候选工作区 + APFS 克隆 .git/node_modules）

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 | gate_check --self-test（主仓+iso 双跑） | **72/72 OK** | f01-gate-selftest-r7.txt / f01-iso-recheck-r7.txt |
| F01 真实 CLI 电池 | r01_t08_gate_cli_regression.sh（双跑） | **4 正+46 负全过** | f01-cli-regression-r7.txt / f01-iso-recheck-r7.txt |
| F01 R7 评审原样反例+对照 | --register /tmp/r01-r7-*.json（13 例，主仓+iso 双跑） | **13/13 按预期**（两条原样误放 exit 1/NO-GO，顺序交换/全角混用/单处矛盾 exit 1，两处一致与上下文 R10 正向 exit 0，control exit 0） | f01-stage-repro-r7.txt / f01-iso-recheck-r7.txt |
| F01 R1–R6 全套反例回归 | --register /tmp/r01-repair-r5/repro + r6/repro（25 例，双跑） | **25/25 类别不变** | f01-r1-r6-regression-r7.txt / f01-iso-recheck-r7.txt |
| F01 正向真实数据 | gate_check（1.7 契约，双跑） | exit 0 PASS_WITH_CONDITIONS，15 递延 TRACKED | gate-report.json / f01-iso-recheck-r7.txt |
| A15 覆盖自测+真实数据 | coverage_check --self-test / 真实数据（iso） | 6/6 OK + COVERAGE-CLOSED（736 F-ID 双向映射闭合） | a15-a16-recheck-r7.txt |
| A16 隔离检查 | isolation_check（iso） | ISOLATED（生产目录零原型文件/生产入口零原型引用） | 同上 |
| 闭包再生成零漂移 | iso: node scripts/compute-cli-closure.mjs | exit 0，`git status --porcelain -- build/` 零输出 | f02-closure-census-r7.txt |
| census 单文件 | iso: vitest run tests/cli-closure-census.test.ts | 23/23（含原位重写用例），build/ 零漂移 | 同上 |
| 四文件门禁（候选态） | iso: vitest run upstream-sync-matrix+audit-seal+round2+round3 | **68/74，6 红=预期封印前红集合**（audit-seal diff guard 1：旧坐标；round2 3：R10-03/R10-04/R10-09；round3 2：manifest/replay）——与 R5/R6 候选态同集；upstream-sync-matrix 7/7 绿 | f02-fourfile-iso-r7.txt |
| 全量 npm test（候选态） | iso: npm test | **14943 passed / 6 failed / 15 skipped**（总 14964，exit 1）——6 红与四文件门禁同集，与 R5/R6 候选态同数同集，R7 零新增红 | f02-npm-test-iso-r7.txt |
| typecheck | iso: npm run typecheck（tsc×3） | exit 0 | typecheck-lint-iso-r7.txt |
| lint | iso: npm run lint | 0 errors / 10896 warnings（与 R4/R5/R6 持平） | typecheck-lint-iso-r7.txt（摘要） |
| 主仓守卫 | git diff --check | 通过（零空白错误） | — |
| artifact_hashes | HANDOFF 20 项逐项实算 | 全部匹配 | — |

主仓只运行了不写盘/不重写交付的检查器与一次性 CLI（gate/coverage/isolation/--self-test/电池/反例复测，gate 证据三件按既有惯例在主仓重生成）；`compute-cli-closure.mjs` 再生成、census、四文件门禁、全量 npm test、lint、typecheck 全部只在 /tmp 隔离副本运行；**主仓全程未运行任何会重写交付 gzip 的脚本**（round2/round3 证据测试仅由 vitest 在 iso 内执行）。

时点说明（如实）：隔离副本 rsync 于全部代码与文档四件改动完成之后、R7 证据落位之前；iso 全部验证跑在该时点上。最终候选相对 iso 树仅多 `STAGE_REPAIR_R7/` 下 9 个证据归档文件（`f01-iso-recheck-r7.txt` 在 iso 内生成并逐字节拷回主仓）——全部为证据文本；已 grep 核实 `tests/` 与 `scripts/` 无任何测试/脚本读取 R01 关卡检查器与文档四件，故证据/文档增量不影响任何测试结果。iso 测试全程工作树零漂移（测试前后 `git status --porcelain --untracked-files=all` 比对，唯一增量为该 iso 内生成的 1 个证据文件）。

## 4. 正式提交后复验矩阵（交接总控/复验代理）

1. 由另一全新 Codex 子代理对完整 R01 阶段做只读复验：核对本报告指纹 `1c2bd57d…` 与开工/结束工作树一致；复验 F01——R7 原样反例（`R09（宿主集成）最迟 R09；最迟 R10（虚构放宽关卡）`、`…最迟 Ｒ０９；最迟 Ｒ１０`）exit 1/NO-GO/risk-stage-text-mismatch，顺序交换（`最迟 R10；最迟 R09`）exit 1，多处一致（`最迟 R09；最迟 R09`）与上下文提及 R10（`；R10（后续平台事项）最迟 R09`）exit 0；R1–R6 全套正负电池保留（自测 72/72、CLI 4 正+46 负、原样反例 25/25 类别不变）；复核证据哈希/范围（R7 触碰 9 项逐文件 SHA、R6 候选 76 件未动证明、R6 证据 10 件对 R6 清单 10/10）与 16 项 REQUIRED 证据链。
2. 阶段复验 PASS 后，总控按 PROGRESS.md seal 工作流冻结真实候选（本报告 §2 指纹对应工作树）提交，再在真实最终提交上执行：绿色电池 → round2/round3 分片补丁 VERIFIED → 坐标推进（纯审计提交，guard 仅 6 审计文件）→ 四文件门禁 **74/74** → **全量 npm test 0 失败** → 可达集对象审计 0 个 ≥100MiB → 推送后 ls-remote 核对 SHA → 工作树零漂移。
3. 台账回填：ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r7.report_sha256 由后续账本提交补登本报告哈希。

## 5. 失败与限制（如实清单）

1. 候选未提交未推送（无授权）；`report_sha256` 字段为 null 待总控回填。
2. 封印未执行；候选态 6 红（四文件门禁 68/74、全量 npm test 6 failed）如实保留——它们是坐标未推进的预期封印前红（VERIFIED_SOURCE_SHA 仍指 R00 C3 `f2b8c687…`），不由本修复伪造转绿；预演/候选态 SHA 不作正式坐标。
3. 本轮实测为 macOS 27.0 arm64 单机；Windows/Linux/macOS x64、真实供应商/凭证、正式打包/签名、T04/T05/T06 可见原型与 PDF 全量重渲染、授权态录音/屏幕、真实数据迁移未在本轮重跑，结论沿用各 T 独立报告与前轮评审声明。
4. Rust 工作区本轮零改动，未重跑 cargo test；沿用 R4 隔离副本 19 binary 45/45 证据。
5. 收尾时 `git ls-remote` 因本机代理（127.0.0.1:7890）离线未能复测远端 HEAD；本轮全程未执行任何 push（无授权），开工时与 R7 评审开工/收尾三处记录远端均为 `363999378…`，本地 HEAD 全程同一值。
6. lint 全量输出本轮只保留摘要日志（exit 0、0 errors/10896 warnings），未归档 1MB+ 全文；如需全文可在 iso 复跑（约 2 分钟）。
7. 首次 iso 反例记录的 exit 采集曾被 shell 展开顺序污染（`$(basename)` 先于 `$?` 求值使全部显示 exit=0），已作废并以修正采集重跑（证据文件内如实标注）；主仓记录与 iso 修正记录均为有效数据。
8. 全程未 commit/push/PR/tag/release、未 force push、未触真实用户数据/外发/付费操作；未扩大审计白名单、未退役门禁/测试、未改断言、未虚报验证坐标；未改任何风险登记正文（RISK_REGISTER.json 一字节未动）。

## 6. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 72/72
bash docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                     # 4 正+46 负
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# R7 评审原样反例与变体（13 例登记副本在 /tmp/r01-r7-*.json，仅改 RR-T05-X1 正文）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-r7-R7-double-latest-conflict.json   # exit 1
# R1–R6 全套反例回归（18 例在 /tmp/r01-repair-r5/repro/，7 例在 /tmp/r01-repair-r6/repro/）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-repair-r6/repro/fw-latest-r10.json  # exit 1
# A15/A16
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --self-test        # 6/6
python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py                   # ISOLATED
# 闭包（仅限 /tmp 隔离副本；主仓勿运行）
node scripts/compute-cli-closure.mjs && git status --porcelain -- build/    # 零输出=零漂移
npx vitest run tests/cli-closure-census.test.ts                             # 23/23
# 四文件门禁（仅限 /tmp 隔离副本）
npx vitest run tests/upstream-sync-matrix.test.ts tests/post-verification-audit-seal.test.ts \
  tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts   # 68/74（6 红=预期封印前红）
# 指纹复算
git diff --binary | cat; git ls-files --others --exclude-standard | sort | \
  xargs shasum -a 256   # 串接后取 SHA-256（原像：/tmp/r01-repair-r7/r7-final-fingerprint-input.bin）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R7/`（10 件）；仓外 `/tmp/r01-repair-r7/`（iso 副本、指纹原像、逐文件哈希清单、检查器与电池 R6→R7 diff、iso 原始日志含四文件门禁与全量 npm test 全文）。

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
