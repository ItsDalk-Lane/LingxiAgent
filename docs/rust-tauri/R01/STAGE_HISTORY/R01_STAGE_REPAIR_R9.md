# R01 阶段修复 R9 — 修复报告

- 修复者：ZCode:R01 阶段修复 R9（ZCode 一次性任务）；非 R1–R8 修复者、非 R1–R9 阶段验收者、非任一 T 执行/评审代理。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`；仓库 `ItsDalk-Lane/LingxiAgent`。
- 验收输入：`/tmp/r01-stage-review-r9.md`，SHA-256 `b98763a11c1875b0e3ebf28c78e9d48e10f2fbd6dbec8e9e054d115a5277ad58`（本机实算吻合）。R8 修复报告 `/tmp/r01-stage-repair-r8.md` SHA-256 `a0daa60cb0c98b6e323b95368f250083129fb5bf539cc4a6b18065a217a4c653` 一并核对吻合。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01 修复完成并通过主仓与 /tmp 隔离副本双跑回归；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。

## 0. 开工状态核对（与交接一致）

- 本地 HEAD `363999378482dfed42733a3c4e8ad20ac82fd0fc`；开工 `git fetch` 因本机代理（127.0.0.1:7890）不通失败，经 `-c http.proxy= -c https.proxy=` 直连 `git ls-remote origin refs/heads/codex/rust-tauri-migration` 实测为 `363999378…`，与 R9 评审记录一致；收尾前再测仍为 `363999378…`；全程零推送。
- 开工未提交候选 = 18 个已跟踪修改 + 87 个未跟踪新增 = 105 件，与任务书/评审一致；按评审同法复算冻结指纹 = `0c70bc5360375b7ea7982e33209a95666a6b75f561d6b4c2b5481b4d2b4e8845`（原像 352,485 B，与评审留档 `/tmp/r01-r9-start-fingerprint.bin` 同源）。不符即停——本轮开工核对完全相符，未发生覆盖。
- R9 叠加在 R1–R8 候选之上：以 R9 评审 iso 留档（`/tmp/r01-review-r9-iso`，即 R8 候选 105 件的逐字节复制）为基准复算，R9 触碰 9 件（全部属于 R1–R8 已改清单，见 §2）、其余 **96 件逐字节未动、0 缺失**；未执行 reset --hard / clean -fd；`git diff --check` 通过。
- R8 报告范围措辞问题（评审 §其余边界复核 3 已独立查明）本轮如实沿用不改写旧报告：完整 R7 候选 95 = 85 主表 + 10 R7 证据；R8 改 9、实际保留 86 = 76 主表 + 10 R7 证据；R8 候选 105 件正确。本轮范围记法见 §2。

## 1. F01（BLOCKING）单个期限声明子句内的「原定→改期/顺延/或/斜杠」仍只校验首个阶段 → 已修复

### 根因

R8 的 G6d 标记语法把「触发词 + 有界连接段 + 完整阶段 ID」绑定到触发词后的**首个**阶段 ID：`LATEST_MARKER_RE` 在首个 R/r 处停止（旧 r01_t08_gate_check.py:178–181），核验只检查捕获阶段（:354–377），触发词只要落在已匹配 span 内即视为已解析，其余真实阶段仅检查存在性（:327–334）。于是同一期限子句内首个阶段之后的**第二阶段**——改期「（原定 R09，现改 R10）」、顺延「由原定的 R09 顺延至 R10」、选择「在 R09 或 R10」、斜杠并列「R09/R10」——不参加任何与结构化 `resolve_latest_stage_id` 的比较，仅由通用阶段扫描核验其在 R00–R11 存在。R9 评审实测（仅改 /tmp 登记副本 RR-T05-X1 正文，结构化坐标保持 R09/R09）：四变体全部 exit 0/PASS_WITH_CONDITIONS 误放，人工交接期限（R10）与机器接受的冻结期限（R09）相矛盾。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.8-stage-repair-r8 → 1.9-stage-repair-r9，G6e 期限子句唯一一致 + 失败关闭）

1. **每个触发词治理「从触发词起到子句终结符止」的完整区域**：终结符集与 G6d 连接段同一字符族（`；;。．.！!？?` 换行回车）。区域内每个**完整阶段引用**（G6b 完整标识边界、G6c 全角折叠后）都必须与结构化 `resolve_latest_stage_id` 一致；出现任何其他阶段即无法机器判定唯一一致期限，按 `risk-stage-text-mismatch` 失败关闭拒绝（问题追加在既有类别之后，`stage_problems[0][0]` 首类别不变，R1–R8 全部钉住断言保持）。
2. **不枚举改期/顺延/选择语言**：中文「或」、拉丁「or」、半/全角斜杠「/／」、括注、逗号续写及其任意组合，由「触发词区域内出现第二阶段」这一**语言无关**的结构性判定统一覆盖——不把 R9 评审点名的句子黑名单化，也不无限枚举自然语言同义词。全角变体（Ｒ０９／Ｒ１０ 等）经既有 1:1 折叠后同规则。
3. **区域边界 = 语义边界（不误伤上下文）**：触发词**之前**的阶段是截止坐标上下文（既有首坐标校验管辖，如真实登记 `R09 最迟 R10` 的 R09）；**终结符之后**的阶段是普通上下文提及（既有存在性校验管辖，如 R7 评审正向 `；R10（后续平台事项）最迟 R09`）；与触发词语法无关的正常提及不构成期限标记（R7 以来契约保持）。
4. **引用词名豁免（R9 评审披露限制的最小处理，不扩大范围）**：触发词被成对引号**紧包**（“最迟”/「最迟」/『最迟』/"最迟"）时是引用词名/字段名而非期限声明（如评审例 `R10 文档介绍“最迟”字段` 曾被误拒为 risk-stage-latest-unresolved），不触发解析义务、不治理期限子句。**该豁免不可能隐藏任何期限**：闭引号紧邻触发词末端，被包裹内容不可能携带阶段 ID；紧包之外形态（如 `“最迟” R10`）仍由标记语法照常核验。未引入其他引号/注释类豁免。
5. **既有断言零改动**：FROZEN_CONTRACT、REAL_STAGE_INDEX（R00–R11）、CURRENT_STAGE、全角折叠、完整标识边界、G6d 标记/逐处核验/失败关闭全部保持（diff 留档 /tmp/r01-repair-r9/gate-check-r8-to-r9.diff，+159/−17 行均为 G6e 与自测/文档，对 FROZEN_CONTRACT/REAL_STAGE_INDEX 零命中）；新增问题类别为零（复用 risk-stage-text-mismatch，语义即「正文期限与结构化最迟不一致」，信息文本区分改期/顺延/或/斜杠形态）；自测新增 N78–N84 七个负向 + P7/P8 两个正向，CLI 电池新增 8 负 + 2 正，全部为追加。

### 修复后行为矩阵（真实 CLI 实测，主仓+iso 双跑；仅改 /tmp 登记副本 RR-T05-X1 正文，结构化坐标 R09/R09 不动）

| 正文变体 | 修复前（R9 评审实测） | 修复后 |
|---|---|---|
| `R09（browser worker 边界实现与门禁）`（原文正向对照） | exit 0 / PASS_WITH_CONDITIONS | exit 0 / 同（不变） |
| **`R09（宿主集成）最迟（原定 R09，现改 R10）完成`（F01 原样 1，改期）** | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| **`R09（宿主集成）最迟由原定的 R09 顺延至 R10 完成`（F01 原样 2，顺延）** | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| **`R09（宿主集成）最迟在 R09 或 R10 完成`（选择「或」）** | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| **`R09（宿主集成）最迟 R09/R10 完成`（斜杠并列）** | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| `…最迟（原定 Ｒ０９，现改 Ｒ１０）完成`（全角改期，新增对照） | （未测） | exit 1 / NO-GO / risk-stage-text-mismatch |
| `…最迟 Ｒ０９／Ｒ１０ 完成`（全角斜杠，新增对照） | （未测） | exit 1 / NO-GO / risk-stage-text-mismatch |
| `…最迟在 Ｒ０９ 或 Ｒ１０ 完成`（全角选择，新增对照） | （未测） | exit 1 / NO-GO / risk-stage-text-mismatch |
| `…最迟在 R09 or R10 完成`（拉丁连接，新增对照——区域规则语言无关） | （未测） | exit 1 / NO-GO / risk-stage-text-mismatch |
| `…最迟 R09；最迟 R10`（R7 原样） | exit 1 | exit 1 / 同（不变，首类别 risk-stage-text-mismatch） |
| `…最迟 R09；最迟：R10`（R8 原样全角冒号） | exit 1 | exit 1 / 同（不变） |
| `…最迟于第三阶段完成`（触发词无阶段 ID） | exit 1 | exit 1 / 同（不变，risk-stage-latest-unresolved） |
| `…最迟 R09；最迟 R09（重复但一致）`（多处一致正向） | exit 0 | exit 0（不变） |
| `…最迟 R09 完成（R09 复核）`（同子句内重复一致，新增正向） | （未测） | exit 0 / PASS_WITH_CONDITIONS |
| `R09（宿主集成）；R10（后续平台事项）最迟 R09`（上下文提及正向） | exit 0 | exit 0（不变） |
| `…不晚于 R09 完成`（否定连接式一致正向） | exit 0 | exit 0（不变） |
| `R09 最迟 R09 完成；R10 文档介绍“最迟”字段`（引号词名引用，评审披露误拒） | **exit 1 / risk-stage-latest-unresolved（误拒）** | **exit 0 / PASS_WITH_CONDITIONS** |
| `R09（宿主集成）；R10 前必须完成`（无触发词裸后缀，文档化边界） | exit 0 | exit 0（不变；不在触发词族内，由结构化坐标+首坐标+存在性三方约束） |
| `R09（宿主集成）；最晚于 R10 完成`（同义词不在触发词族，文档化边界） | exit 0 | exit 0（不变；不逐词枚举同义词——见 §5 边界） |

证据：`STAGE_REPAIR_R9/f01-stage-repro-r9.txt`（25 变体重放 25/25，复现器 /tmp/r01-repair-r9/rerun-adversarial.py 硬编码内联评审变体表）、`f01-iso-recheck-r9.txt`（iso 双跑 25/25）、`f01-realdata-r9.txt`（真实登记正向）。

### 负向/正向覆盖（进程内与真实 CLI 双层，只增不删）

- **进程内自测 92/92**：positive-control + N1–N77（R1–R8 全保留）+ **N78–N84 新增七个负向**（改期/顺延/或/斜杠/全角改期/全角斜杠/拉丁 or）+ **P7 同子句内重复一致正向** + **P8 引号词名引用正向**。证据：`f01-gate-selftest-r9.txt`（主仓）、`f01-iso-recheck-r9.txt`（iso）；关卡证据 `gate-selftest.log` 按 1.9 契约重生成（92 用例）。
- **真实 CLI 电池 8 正向 + 62 负向**：`r01_t08_gate_cli_regression.sh` 在 6 正+54 负之上追加 8 负（改期/顺延/或/斜杠/全角改期/全角斜杠/全角选择/拉丁 or）+ 2 正（同子句重复一致/引号词名）。证据：`f01-cli-regression-r9.txt`（主仓）、`f01-iso-recheck-r9.txt`（iso）。
- **R9 评审 25 变体 25/25**：R8 已拒 13 例保持 exit 1；四误放变体转 exit 1；quoted-context 误拒转 exit 0；其余正向不变（逐条含首阻断类别，/tmp/r01-repair-r9/r9-adversarial-matrix.json）。
- **R1–R8 留存 CLI 反例 38/38**（R1–R6 25 + R7 13，基准为 R9 评审留档 /tmp/r01-r9-old-repros.json）：退出码与首阻断类别逐项不变。证据：`f01-r1-r8-regression-r9.txt`（主仓+iso 双跑一致）。
- 未删任何测试、未放宽任何关卡、未改任何既有断言（全部为追加）；`RISK_REGISTER.json` 本轮一字节未动（SHA-256 保持 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307`；真实登记仅有的两处触发词文本 `R09 最迟 R10`、`R09（宿主集成）最迟 R10（跨平台产物）` 的触发词区域只含 R10、与结构化 R10 一致，修复不改变真实数据判定）。

### 设计决策与边界（如实说明）

- **选「触发词区域结构判定」而非「枚举改期词」**：改期/顺延/选择在自然语言里有无穷写法（原定/现改/顺延至/推迟到/调整为/or/或者/斜杠……），逐词枚举必再漏（R7→R8 的教训正是枚举连接词形态）；「区域内不得出现第二阶段」是语言无关、机器可判定且失败关闭的完整契约：只要唯一一致期限无法确定即拒。
- **代价（严格契约方向，非误放）**：触发词同一子句内出现与结构化最迟不同的任何阶段引用都会被拒——包括字面上属补充说明的写法（如「最迟 R09 完成（R10 再复审）」）。这是「无法确定唯一一致期限即拒」的显式失败关闭方向：此类歧义书写应拆分为独立子句（终结符分隔）或改写为与结构化一致。区域判定不回溯触发词之前（截止坐标上下文）也不跨过终结符（普通上下文），真实登记与 R7–R9 全部正向不受影响。
- **引号豁免刻意收窄**：仅「开引号紧邻触发词首 + 对应闭引号紧邻触发词尾」的成对紧包形态豁免（词名引用的语法事实），不豁免引号内含阶段的完整声明（`“最迟 R10”` 的触发词后是空格非闭引号，不构成豁免，标记照常核验），也不新增其他引用语法。评审披露的误拒限制由此最小化处理，未扩大范围。
- **仍不构成期限标记的形态（文档化边界，非本轮缺陷，与 R8 契约一致）**：不含触发词族（最迟/不迟于/不得迟于/不晚于/不得晚于）的裸阶段时间后缀（「R10 前完成」「最晚于 R10 完成」的「最晚」）仍是上下文提及——由首坐标=结构化 deadline、全部 token 存在性、结构化坐标权威三方约束；把它们捕为期限标记需枚举同义词，正是任务书禁止的方向。R9 评审已将此类列为文档化边界非阻断。

## 2. 修改范围与指纹

R9 叠加改动（R8 候选 105 件中 96 件逐字节未动，对照 /tmp/r01-review-r9-iso 留档逐文件复算，证明见 /tmp/r01-repair-r9/r9-file-hashes.txt 与 §0）：已跟踪修改 9 项 + 新增 `STAGE_REPAIR_R9/` 证据 11 件。候选总计 **116 文件 = 18 修改 + 98 新增**（87 前轮 + 11 R9）。`git diff --check` 通过。

```
M docs/rust-tauri/R01/r01_t08_gate_check.py            （G6e 期限子句唯一一致 + 引号词名豁免 + 自测 N78–N84/P7/P8 + 契约 1.9）
M docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh   （+8 负向 +2 正向 → 8 正+62 负）
M docs/rust-tauri/R01/R01_REPORT.md                    （R9 进展叙述：评审 FAIL + 修复 + 范围更正记法）
M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json       （description + A15 stage_repair_note 补 R9 链）
M docs/rust-tauri/R01/R01_HANDOFF.json                 （stage_review_r9/stage_repair_r9 坐标 + stage_gate_position + artifact_hashes 重算）
M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json           （R01 stage 块：round 9 / verdict FAIL / 复验与修复坐标）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json   （1.9 契约重生成；与 R8 候选态语义 diff 仅 contract.version 一行，判定不变）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log     （1.9 契约重生成）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log  （1.9 契约重生成，92 用例）
N artifacts/rust-tauri/R01/STAGE_REPAIR_R9/（11 件：f01-gate-selftest / f01-cli-regression / f01-realdata / f01-stage-repro / f01-r1-r8-regression / f01-iso-recheck / a15-a16-recheck / f02-closure-census / f02-fourfile-iso / f02-npm-test-iso / typecheck-lint-iso，均 -r9.txt；逐文件哈希见 /tmp/r01-repair-r9/r9-file-hashes.txt）
```

**候选工作树冻结指纹**（同 R5–R9 评审法：`git diff --binary` 字节直接串接按路径排序的新文件 SHA-256 清单后取 SHA-256；原像 380,578 B 留档 `/tmp/r01-repair-r9/r9-final-fingerprint-input.bin`）：

```
39e9bdd7a4576e7b7437400b321302c5608c5e9cc06cfff62bf04bba73a853e3
```

HANDOFF `artifact_hashes` 20 项钉住哈希在最终候选状态逐项实算全部匹配（5 项按 R9 改动重算：R01_REPORT.md / R01_ACCEPTANCE_LEDGER.json / r01_t08_gate_check.py / r01_t08_gate_cli_regression.sh / gate-report.json；15 项未触碰逐字节复核，RISK_REGISTER.json 保持 `87e34d0e…`）；`working_tree_digest` 维持 T08 执行时点快照不改（同 R1–R8 惯例）。JSON 重写方法：HANDOFF / ORCHESTRATOR_PROGRESS / LEDGER 均为 `json.dumps(ensure_ascii=False, indent=2)+"\n"` 字节级可重现格式（本轮编辑后三文件逐一验证 raw==canonical）。

## 3. /tmp 隔离副本验证矩阵（iso=/tmp/r01-repair-r9/iso）

iso 由真实 HEAD `363999378` 克隆 + 116 候选路径逐字节复制组装；node_modules 单次 APFS `cp -cR`（无嵌套，R8 事故形态复查为零）；收尾全量哈希比对 116 件与主仓零差异；主仓未运行任何会重写交付 gzip 的脚本（round2/round3 证据测试仅由 vitest 在 iso 内执行）。

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 | gate_check --self-test（主仓+iso 双跑） | **92/92 OK** | f01-gate-selftest-r9.txt / f01-iso-recheck-r9.txt |
| F01 真实 CLI 电池 | r01_t08_gate_cli_regression.sh（双跑） | **8 正+62 负全过** | f01-cli-regression-r9.txt / f01-iso-recheck-r9.txt |
| F01 R9 评审 25 变体 | 登记副本在 /tmp（双跑） | **25/25 按预期**（四误放转 exit 1、引号误拒转 exit 0、其余不变） | f01-stage-repro-r9.txt / f01-iso-recheck-r9.txt |
| F01 R1–R8 留存反例 | 38 例（基准 R9 评审留档，双跑） | **38/38 退出码+首类别逐项不变** | f01-r1-r8-regression-r9.txt / f01-iso-recheck-r9.txt |
| F01 正向真实数据 | gate_check（1.9 契约，双跑） | exit 0 PASS_WITH_CONDITIONS，15 递延 TRACKED | f01-realdata-r9.txt / f01-iso-recheck-r9.txt |
| A15 覆盖自测+真实数据 | coverage_check --self-test / 真实数据（iso） | 6/6 OK + COVERAGE-CLOSED（736 F-ID 双向映射闭合） | a15-a16-recheck-r9.txt |
| A16 隔离检查 | isolation_check（iso） | ISOLATED（生产入口零原型引用） | 同上 |
| 闭包再生成零漂移 | iso: node scripts/compute-cli-closure.mjs | exit 0，`git status --porcelain -- build/` 零输出 | f02-closure-census-r9.txt |
| census 单文件 | iso: vitest run tests/cli-closure-census.test.ts | 23/23（含原位重写用例），build/ 零漂移 | 同上 |
| 四文件门禁（候选态） | iso: vitest run upstream-sync-matrix+audit-seal+round2+round3 | **68/74，6 红=预期封印前红集合**（audit-seal diff guard 1：旧坐标；round2 R10-03/R10-04/R10-09 3；round3 manifest/replay 2）——与 R5–R9 评审候选态同集，逐项列出；upstream-sync-matrix 7/7 绿 | f02-fourfile-iso-r9.txt |
| 全量 npm test（候选态） | iso: npm test | **14943 passed / 6 failed / 15 skipped**（总 14964，exit 1）——6 红与四文件门禁同集，与 R7/R8/R9 评审候选态同数同集，R9 零新增红 | f02-npm-test-iso-r9.txt（全文 /tmp/r01-repair-r9/iso-npm-test.log） |
| typecheck | iso: npm run typecheck（tsc×3） | exit 0 | typecheck-lint-iso-r9.txt |
| lint | iso: npm run lint | 0 errors / 10896 warnings（与 R4–R8 持平） | 同上（全文 /tmp/r01-repair-r9/iso-lint.log） |
| 主仓守卫 | git diff --check | 通过（零空白错误） | — |
| artifact_hashes | HANDOFF 20 项逐项实算 | 全部匹配 | — |
| iso 零漂移 | 收尾 116 候选文件哈希比对 | 与主仓逐项零差异；build/ 零漂移 | — |

主仓只运行了不写盘/不重写交付的检查器与一次性 CLI（gate/coverage/isolation/--self-test/电池/反例复测，gate 证据三件按既有惯例在主仓重生成）。

## 4. 正式提交后复验矩阵（交接总控/复验代理）

1. 由另一全新 Codex 子代理对完整 R01 阶段做只读复验：核对本报告指纹 `39e9bdd7…` 与开工/结束工作树一致；复验 F01——R9 评审原样四变体（改期 `最迟（原定 R09，现改 R10）完成` / 顺延 `最迟由原定的 R09 顺延至 R10 完成` / 选择 `最迟在 R09 或 R10 完成` / 斜杠 `最迟 R09/R10 完成`）exit 1/NO-GO/risk-stage-text-mismatch，全角变体（全角改期/全角斜杠/全角选择）与拉丁 or 连接 exit 1，引号词名引用（`R10 文档介绍“最迟”字段`）exit 0，R7/R8 原样反例 exit 1，多处一致（含同子句内重复一致 `最迟 R09 完成（R09 复核）`）、上下文提及 R10、否定连接式一致 exit 0，无触发词裸后缀边界（`R10 前必须完成`/`最晚于 R10`）仍为文档化边界 exit 0；R1–R8 全套正负电池保留（自测 92/92、CLI 8 正+62 负、R9 评审 25 变体 25/25、R1–R8 留存反例 38/38 类别不变）；复核证据哈希/范围（R9 触碰 9 项 + STAGE_REPAIR_R9 11 件逐文件 SHA、R8 候选 96 件未动证明）与 16 项 REQUIRED 证据链。
2. 阶段复验 PASS 后，总控按 PROGRESS.md seal 工作流冻结真实候选（本报告 §2 指纹对应工作树）提交，再在真实最终提交上执行：绿色电池 → round2/round3 分片补丁 VERIFIED → 坐标推进（纯审计提交，guard 仅 6 审计文件）→ 四文件门禁 **74/74** → **全量 npm test 0 失败** → 可达集对象审计 0 个 ≥100MiB → 推送后 ls-remote 核对 SHA → 工作树零漂移。不得用预演 SHA 虚报正式封印，不改 allowlist 或删除门禁。
3. 台账回填：ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r9.report_sha256 由后续账本提交补登本报告哈希。

## 5. 失败与限制（如实清单）

1. 候选未提交未推送（无授权）；`report_sha256` 字段为 null 待总控回填。
2. 封印未执行；候选态 6 红（四文件门禁 68/74、全量 npm test 6 failed）如实保留——它们是坐标未推进的预期封印前红（VERIFIED_SOURCE_SHA 仍指 R00 C3 `f2b8c687…`），不由本修复伪造转绿；预演/候选态 SHA 不作正式坐标。
3. 触发词族/区域判定的文档化边界（§1 设计决策）：不含触发词族的裸阶段时间后缀（`R10 前必须完成`、`最晚于 R10` 的「最晚」）不构成期限标记——同义词不枚举（任务书明确禁止方向），由结构化坐标权威 + 首坐标 + 存在性三方约束；触发词同一子句内的非一致阶段补充说明会被拒（失败关闭方向，应改写为独立子句或与结构化一致）；连接段 >16 字符或含 r/R/终结符按既有 unresolved 拒（R8 契约保持）。
4. iso 重型套件（typecheck/lint/闭包/census/四文件/全量 npm test）在候选功能态（代码+脚本+三件关卡证据+当时账本）运行，账本/报告/证据文件终态以哈希同步并逐项零差异复核（测试不读取这些文件，功能态与终态等价）；Python 类证据（自测/电池/25 变体/38 反例/真实数据/A15/A16）在 iso 终态重跑全绿。
5. 本轮实测为 macOS 27.0 arm64 单机；Windows/Linux/macOS x64、真实供应商/凭证、正式打包/签名、T04/T05/T06 可见原型与 PDF 全量重渲染、授权态录音/屏幕、真实数据迁移未在本轮重跑，结论沿用各 T 独立报告与 R9 评审声明（R9 评审已复核的 A01–A16 证据链、Rust 锁定工具链 45/45、任务链/推送链等本轮未重做，不冒称重验）。
6. Rust 工作区本轮零改动，未重跑 cargo test；沿用 R9 评审锁定工具链证据。
7. 开工 `git fetch` 因本机代理离线失败，直连 ls-remote 实测远端 `363999378…`（开工/收尾两测一致）；全程零推送。
8. 全程未 commit/push/PR/tag/release、未 force push、未触真实用户数据/外发/付费操作；未扩大审计白名单、未退役门禁/测试、未改断言、未虚报验证坐标；未改任何风险登记正文（RISK_REGISTER.json 一字节未动）；R9 评审的 R8 报告范围措辞更正（95=85+10、保留 86=76+10）按其独立查明结果记录于 §0/本报告，原 R7/R8 报告不改写。

## 6. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 92/92
bash    docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                  # 8 正+62 负
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# R9 评审 25 变体（复现器硬编码内联评审变体表，登记副本仅写 /tmp）
python3 /tmp/r01-repair-r9/rerun-adversarial.py                             # 25/25
# R1–R8 留存反例（38 例基准 /tmp/r01-r9-old-repros.json）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-repair-r6/repro/fw-latest-r10.json   # exit 1
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
  xargs shasum -a 256   # 串接后取 SHA-256（原像：/tmp/r01-repair-r9/r9-final-fingerprint-input.bin）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R9/`（11 件）；仓外 `/tmp/r01-repair-r9/`（iso 副本、指纹原像 380,578 B、逐文件哈希清单、检查器与电池 R8→R9 diff、25 变体矩阵与逐例输入/输出 JSON、iso 重型套件全文日志）。

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
