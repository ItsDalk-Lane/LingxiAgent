# R01 阶段修复 R8 — 修复报告

- 修复者：ZCode:R01 阶段修复 R8（ZCode 一次性任务）；非 R1–R7 修复者、非 R1–R8 阶段验收者、非任一 T 执行/评审代理。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`；仓库 `ItsDalk-Lane/LingxiAgent`。
- 验收输入：`/tmp/r01-stage-review-r8.md`，SHA-256 `8e926451df0ef4b9a3f03c9a50d07710493c48e795b3318950b8d130c160ebba`（本机实算吻合）。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01 修复完成并通过主仓与 /tmp 隔离副本双跑回归；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。

## 0. 开工状态核对（与交接一致）

- 本地 HEAD `363999378482dfed42733a3c4e8ad20ac82fd0fc`；开工与收尾两次 `git ls-remote origin refs/heads/codex/rust-tauri-migration`（本机代理 127.0.0.1:7890 不通，经 `-c http.proxy= -c https.proxy=` 直连）均为 `363999378…`，与 R8 评审记录一致；全程零推送。
- 开工未提交候选 = 18 个已跟踪修改 + 77 个未跟踪新增（git 可见）= 95 件，与任务书一致；磁盘另有两件 gitignore 的 `.mimosa/hook-state` 会话残file（R4/R6 目录内工具状态，`git ls-files --others --exclude-standard` 不可见，不入候选，已如实区分）。按评审同法复算开工冻结指纹 = `1c2bd57d3f973cc49e428fc6476af61e4ff0bbc18706702bc87ca0a2be624d29`（原像 325,821 B，与 /tmp/r01-repair-r7/r7-final-fingerprint-input.bin 逐字节同源）。
- R8 叠加在 R1–R7 候选之上：R7 候选 85 件清单（/tmp/r01-repair-r7/r7-file-hashes.txt）中 R8 未触碰的 76 件逐字节复验一致、0 变化、0 缺失；R7 触碰的 9 件即本轮 R8 触碰集。未执行 reset --hard / clean -fd，未覆盖他人改动；`git diff --check` 通过。

## 1. F01（BLOCKING）带常见连接词或标点的「最迟」声明绕过逐处核验 → 已修复

### 根因

R7 修复把「最迟」核验从 `search()` 改为 `finditer()` 逐处核验，但 `LATEST_MARKER_RE` 的标记语法仍是 `最迟\s*+阶段 ID`——「最迟」后只允许空白。正文写出「最迟：R10」「最迟: R10」「最迟于 R10」「最迟为 R10」等常见自然书写（全角/半角冒号、连接词）时，这些同样明确的期限声明完全不构成标记：不参加存在性核验、不与结构化 `resolve_latest_stage_id` 比较。通用阶段扫描虽提取到真实存在的 R10，但只核验其在 R00–R11 索引中存在。R8 评审实测（仅改 /tmp 登记副本 RR-T05-X1 正文，结构化坐标保持 R09/R09）：四种冒号/连接词变体全部 exit 0/PASS_WITH_CONDITIONS 误放；而 R7 原样（最迟 R09；最迟 R10）、顺序交换、全角混用均被正确拒绝——证明缺陷恰在标记语法对连接形态的漏识别，逐处核验机制本身有效。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.7-stage-repair-r7 → 1.8-stage-repair-r8，G6d 连接形态扩展 + 失败关闭）

1. **标记 = 触发词族 + 有界连接段 + 完整阶段 ID**（在 G6c 全角折叠后的正文上匹配）：触发词族 `最迟 / 不迟于 / 不得迟于 / 不晚于 / 不得晚于`——同族否定连接式与「最迟」语义相同（「不晚于 R10」同样是明确期限声明），一并纳入；连接段为触发词与阶段 ID 之间 ≤16 字符的自然书写杂讯（空白/全半角冒号/连接词/其他非阶段起点字符），**不枚举连接词**——「最迟于/为/是/至/到/即/在…R10」等任意单字或组合连接词由有界杂讯段统一覆盖，不再逐词打补丁；阶段 ID 沿用 G6b 完整标识边界（R+数字，紧邻前后非标识延续字符）与 G4b 全角折叠。
2. **子句局部性（防跨句误捕）**：连接段内不得出现大写 R 或小写 r（可能的阶段/伪装起点——出现即视为该触发词在本子句内无法解析，不得静默跳过）、不得跨越子句/句子终结符（；;。．.！!？?换行）——期限声明必须落在同一子句内，跨子句的阶段是上下文提及，不是该触发词的期限（保留 R7 契约「与触发词语法无关的正常阶段提及不构成标记、不误伤」）。
3. **失败关闭（新类别 `risk-stage-latest-unresolved`）**：正文中每个触发词出现都必须解析出一个阶段 ID（触发词 span 被某标记 span 包含即视为已解析）；解析不出（如「最迟于第三阶段完成」的自然语言期限、或连接段内先遇到 r/R/终结符）→ 拒绝。无法机器核验一致性的期限不得替代结构化坐标放行——这关闭了「把明确 R-ID 期限改写成自然语言期限」的绕过面。
4. **逐处核验语义不变**：任一标记引用不存在阶段 → `risk-stage-unknown`；任一标记 ≠ 结构化 `resolve_latest_stage_id` → `risk-stage-text-mismatch`；多处标记全部一致方可放行。结构化坐标与冻结契约 latest_stage 仍是判定权威，正文核验只加严不放宽——FROZEN_CONTRACT/REAL_STAGE_INDEX（R00–R11）/全角折叠/完整标识边界零改动（diff 留档 /tmp/r01-repair-r8/gate-check-r7-to-r8.diff，154 行新增均为 G6d 扩展与自测，对 FROZEN_CONTRACT/REAL_STAGE_INDEX 零命中）。
5. **既有断言零改动**：全部为追加（N69–N77 + P5/P6 + 8 个 CLI 变体）；新增问题类别在 problems 列表中排在既有类别之后，`stage_problems[0][0]` 首类别不变，既有测试钉住的类别码全部保持。

### 修复后行为矩阵（真实 CLI 实测，主仓+iso 双跑；仅改 /tmp 登记副本 RR-T05-X1 正文，结构化坐标 R09/R09 不动）

| 正文变体 | 修复前（R8 评审实测） | 修复后 |
|---|---|---|
| `R09（browser worker 边界实现与门禁）`（原文正向对照） | exit 0 / PASS_WITH_CONDITIONS | exit 0 / 同（不变） |
| `R09（宿主集成）最迟 R09；最迟 R10（虚构放宽关卡）`（R7 原样） | exit 1 / NO-GO / text-mismatch | exit 1 / 同（不变） |
| `R09（宿主集成）最迟 R10；最迟 R09`（顺序交换） | exit 1 | exit 1 / 同（不变） |
| `R09（宿主集成）最迟 Ｒ０９；最迟 Ｒ１０`（全角混用） | exit 1 | exit 1 / 同（不变） |
| **`R09（宿主集成）最迟 R09；最迟：R10（虚构放宽关卡）`（F01 原样，全角冒号）** | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| **`…最迟 R09；最迟：Ｒ１０…`（全角冒号+全角阶段）** | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch（折叠后核验）** |
| **`…最迟 R09；最迟于 R10…`（连接词）** | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| **`R09（宿主集成）最迟：R10（虚构放宽关卡）`（单处冒号）** | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| `R09（宿主集成）最迟: R10（虚构放宽关卡）`（半角冒号，新增对照） | （未测） | exit 1 / NO-GO / risk-stage-text-mismatch |
| `R09（宿主集成）最迟为 R10（虚构放宽关卡）`（连接词族，新增对照） | （未测） | exit 1 / NO-GO / risk-stage-text-mismatch |
| `R09（宿主集成）不晚于 R10（虚构放宽关卡）`（否定连接式，新增对照） | （未测） | exit 1 / NO-GO / risk-stage-text-mismatch |
| `R09（宿主集成），不得迟于 Ｒ１０（虚构放宽关卡）`（否定+全角，新增对照） | （未测） | exit 1 / NO-GO / risk-stage-text-mismatch |
| `R09（宿主集成）最迟于第三阶段完成`（触发词无阶段 ID，失败关闭） | （未测） | exit 1 / NO-GO / **risk-stage-latest-unresolved** |
| `R09（宿主集成）最迟 R09；最迟 R09（重复但一致）`（多处一致正向） | exit 0 | exit 0（不变） |
| `R09（宿主集成）最迟：R09；最迟于 R09（重复但一致）`（连接形态一致正向，新增） | （未测） | exit 0 / PASS_WITH_CONDITIONS |
| `R09（宿主集成）不晚于 R09 完成`（否定连接式一致正向，新增） | （未测） | exit 0 / PASS_WITH_CONDITIONS |
| `R09（宿主集成）；R10（后续平台事项）最迟 R09`（上下文提及 R10 不误伤） | exit 0 | exit 0（不变） |

证据：`STAGE_REPAIR_R8/f01-stage-repro-r8.txt`（R8 评审 10 例主仓复测）、`f01-cli-regression-r8.txt`、`f01-iso-recheck-r8.txt`（iso 双跑，含全部 10 例）。

### 负向/正向覆盖（进程内与真实 CLI 双层，只增不删）

- **进程内自测 83/83**：positive-control + N1–N68（R1–R7 全保留）+ **N69–N77 新增 9 个负向**（全角冒号第二处冲突/全角冒号+全角阶段/「最迟于」/单处冒号/半角冒号/「最迟为」连接词族/「不晚于 R10」否定连接式/「不得迟于 Ｒ１０」否定+全角/触发词无阶段 ID 失败关闭）+ **P5 连接形态一致正向** + **P6 否定连接式一致正向**。证据：`f01-gate-selftest-r8.txt`（主仓）、`f01-iso-recheck-r8.txt`（iso）。
- **真实 CLI 电池 6 正向 + 54 负向**：`r01_t08_gate_cli_regression.sh` 在 4 正+46 负之上追加 8 负（全角冒号/全角冒号+全角阶段/最迟于/单处冒号/半角冒号/最迟为/不晚于/触发词无阶段 ID）+ 2 正（连接形态一致/否定连接式一致）；正向对照 exit 0 不变。证据：`f01-cli-regression-r8.txt`、`f01-iso-recheck-r8.txt`。
- **R8 评审原样反例与对照 10/10**（评审复现器 /tmp/r01-r8-repro.py 同法重放）：四种误放形态全部转 exit 1/NO-GO/risk-stage-text-mismatch；R7 三反例仍 exit 1；正向与多处一致/上下文提及仍 exit 0。证据：`f01-stage-repro-r8.txt`、`f01-iso-recheck-r8.txt`。
- **R7 评审原样反例与变体 13/13**（/tmp/r01-r7-*.json）：exit/verdict/类别逐项与 R7 留档一致。**R1–R6 全套反例 25/25**（/tmp/r01-repair-r5/repro 18 例 + /tmp/r01-repair-r6/repro 7 例）：退出码与类别逐项不变（含 N57「最迟 R09.5」仍以 risk-stage-disguised 为首类别——新增的失败关闭问题排在伪装检出之后，不改变既有断言）。证据：`f01-r1-r7-regression-r8.txt`、`f01-iso-recheck-r8.txt`。
- 未删任何测试、未放宽任何关卡、未改任何既有断言（全部为追加）；`RISK_REGISTER.json` 本轮一字节未动（SHA-256 保持 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307`，真实登记仅有的两处「最迟 R10」均无连接词且与结构化 R10 一致，修复不改变真实数据判定）。

### 设计决策与边界（如实说明）

- **选「有界杂讯段」而非「枚举连接词」**：R8 评审点名的冒号/全角冒号/「最迟于」之外还有「为/是/至/到/即/在」等大量等价连接写法，逐词枚举必再漏（R7 的教训正是枚举了「空白」一种形态）；≤16 字符杂讯段对任意连接组合统一捕获，漏检面收敛为「触发词与阶段 ID 相距 >16 字符或跨子句」——后者按失败关闭处理（见下）。
- **选「解析不出即拒」而非「解析不出即忽略」**：契约声明「每一处显式期限都是强制期限声明」，无法核验的期限声明不能默认与结构化一致而放行；失败关闭方向宁可拒绝含糊书写，也不给「把 R-ID 期限改写成自然语言期限」留通道。代价：连接段超 16 字符、或段内先出现 r/R（如「最迟由 risk owner…」）、或触发词后紧跟终结符的书写会被拒——这是文档化的严格契约，不是误放。
- **仍不构成标记的形态（文档化边界，非本轮缺陷）**：不含任何触发词的裸阶段提及带时间后缀（如「R10 前完成」「于 R10 内」而无「最迟/不晚于」等字样）仍是上下文提及——由首坐标=结构化 deadline、全部 token 存在性、结构化坐标权威三方约束；把它们捕为期限标记会与「R10 前后」「前述」等用语冲突造成误伤，R7 以来契约即如此，本轮不扩大。
- **「最迟 R10 前完成」这类触发词+阶段+后缀形态已覆盖**：触发词后首个完整阶段 ID 即被捕获核验，后缀不参与。

## 2. 修改范围与指纹

R8 叠加改动（R7 候选 95 件中 76 件逐字节未动，逐文件证明见 /tmp/r01-repair-r8/r8-file-hashes.txt 与上节复验）：已跟踪修改 8 项 + 既存未跟踪脚本就地更新 1 项（`r01_t08_gate_cli_regression.sh`，R2 引入）+ 新增 `STAGE_REPAIR_R8/` 证据 10 件。候选总计 **105 文件 = 18 修改 + 87 新增**（77 前轮 + 10 R8）。`git diff --check` 通过。

逐文件 SHA-256（R8 触碰 9+10=19 项；完整清单 `/tmp/r01-repair-r8/r8-file-hashes.txt`）：

```
M docs/rust-tauri/R01/r01_t08_gate_check.py            （G6d 连接形态扩展+失败关闭 + 自测 N69–N77/P5/P6 + 契约 1.8）
U docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh   （既存未跟踪，就地更新：+8 负向 +2 正向 → 6 正+54 负）
M docs/rust-tauri/R01/R01_REPORT.md                    （R8 进展叙述 + 独立审查段）
M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json       （description + A15 stage_repair_note 补 R8 链）
M docs/rust-tauri/R01/R01_HANDOFF.json                 （stage_review_r8/stage_repair_r8 坐标 + stage_gate_position + artifact_hashes 重算）
M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json           （R01 stage 块：round 8 / verdict FAIL / 复验与修复坐标）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json   （1.8 契约重生成，语义差异仅 contract.version，判定不变）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log     （1.8 契约重生成）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log  （1.8 契约重生成，83 用例）
N artifacts/rust-tauri/R01/STAGE_REPAIR_R8/（10 件，逐文件哈希见 /tmp/r01-repair-r8/r8-file-hashes.txt）
```

**候选工作树冻结指纹**（同 R5–R8 评审法：`git diff --binary` 字节直接串接按路径排序的新文件 SHA-256 清单后取 SHA-256；原像 352,485 B 留档 `/tmp/r01-repair-r8/r8-final-fingerprint-input.bin`）：

```
0c70bc5360375b7ea7982e33209a95666a6b75f561d6b4c2b5481b4d2b4e8845
```

HANDOFF `artifact_hashes` 20 项钉住哈希在最终候选状态逐项实算全部匹配（5 项按 R8 改动重算：R01_REPORT.md/R01_ACCEPTANCE_LEDGER.json/r01_t08_gate_check.py/r01_t08_gate_cli_regression.sh/gate-report.json；15 项未触碰逐字节复核，RISK_REGISTER.json 保持 `87e34d0e…`）；`working_tree_digest` 维持 T08 执行时点快照不改（同 R1–R7 惯例）。JSON 重写方法：HANDOFF / ORCHESTRATOR_PROGRESS / LEDGER 均为 `json.dumps(ensure_ascii=False, indent=2)+"\n"` 字节级可重现格式（本轮编辑后三文件逐一验证 raw==canonical）。gate-report.json 相对 R7 态语义 diff 仅 `contract.version` 一行（对照 /tmp/r01-repair-r7/iso 留档逐键比对）。

## 3. /tmp 隔离副本验证矩阵（iso=/tmp/r01-repair-r8/iso）

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 | gate_check --self-test（主仓+iso 双跑） | **83/83 OK** | f01-gate-selftest-r8.txt / f01-iso-recheck-r8.txt |
| F01 真实 CLI 电池 | r01_t08_gate_cli_regression.sh（双跑） | **6 正+54 负全过** | f01-cli-regression-r8.txt / f01-iso-recheck-r8.txt |
| F01 R8 评审原样反例+对照 | --register /tmp/r01-r8-*.json（10 例，双跑） | **10/10 按预期**（四种误放转 exit 1/text-mismatch，R7 三反例仍拒，正向不变） | f01-stage-repro-r8.txt / f01-iso-recheck-r8.txt |
| F01 R7 评审原样反例 | --register /tmp/r01-r7-*.json（13 例，双跑） | **13/13 exit/verdict/类别逐项不变** | f01-r1-r7-regression-r8.txt / f01-iso-recheck-r8.txt |
| F01 R1–R6 全套反例回归 | repro 目录（25 例，双跑） | **25/25 类别不变** | 同上 |
| F01 正向真实数据 | gate_check（1.8 契约，双跑） | exit 0 PASS_WITH_CONDITIONS，15 递延 TRACKED | gate-report.json / f01-iso-recheck-r8.txt |
| A15 覆盖自测+真实数据 | coverage_check --self-test / 真实数据（iso） | 6/6 OK + COVERAGE-CLOSED（736 F-ID 双向映射闭合） | a15-a16-recheck-r8.txt |
| A16 隔离检查 | isolation_check（iso） | ISOLATED（生产入口零原型引用） | 同上 |
| 闭包再生成零漂移 | iso: node scripts/compute-cli-closure.mjs | exit 0，`git status --porcelain -- build/` 零输出 | f02-closure-census-r8.txt |
| census 单文件 | iso: vitest run tests/cli-closure-census.test.ts | 23/23（含原位重写用例），build/ 零漂移 | 同上 |
| 四文件门禁（候选态） | iso: vitest run upstream-sync-matrix+audit-seal+round2+round3 | **68/74，6 红=预期封印前红集合**（audit-seal diff guard 1：旧坐标；round2 R10-03/R10-04/R10-09 3；round3 manifest/replay 2）——与 R5/R6/R7/R8 评审候选态同集；upstream-sync-matrix 7/7 绿 | f02-fourfile-iso-r8.txt |
| 全量 npm test（候选态） | iso: npm test | **14943 passed / 6 failed / 15 skipped**（总 14964，exit 1）——6 红与四文件门禁同集，与 R7/R8 评审候选态同数同集，R8 零新增红 | f02-npm-test-iso-r8.txt |
| typecheck | iso: npm run typecheck（tsc×3） | exit 0 | typecheck-lint-iso-r8.txt |
| lint | iso: npm run lint | 0 errors / 10896 warnings（与 R4–R7 持平） | typecheck-lint-iso-r8.txt（摘要） |
| 主仓守卫 | git diff --check | 通过（零空白错误） | — |
| artifact_hashes | HANDOFF 20 项逐项实算 | 全部匹配 | — |
| iso 零漂移 | 测试前后 `git status --porcelain --untracked-files=all` 比对 | 唯一增量为 STAGE_REPAIR_R8/ 内本轮证据文件；build/ 与生产目录零漂移 | — |

主仓只运行了不写盘/不重写交付的检查器与一次性 CLI（gate/coverage/isolation/--self-test/电池/反例复测，gate 证据三件按既有惯例在主仓重生成）；`compute-cli-closure.mjs` 再生成、census、四文件门禁、全量 npm test、lint、typecheck 全部只在 /tmp 隔离副本运行；**主仓全程未运行任何会重写交付 gzip 的脚本**（round2/round3 证据测试仅由 vitest 在 iso 内执行）。

**iso 组装事故与处置（如实记录）**：首次组装 iso 时 `cp -R` 回退链与随后的 `cp -cR` 先后对 node_modules 各拷贝一次，形成 `node_modules/node_modules/` 嵌套副本（约 6.5 万重复文件），干扰 Node 模块解析与 nft 运行时追踪，使首次闭包再生成出现伪漂移（`@anthropic-ai/sdk` ↔ `@earendil-works/pi-agent-core` vendor 集合漂移，18,048+/8,522-）。处置：删除嵌套副本（node_modules 文件清单与 R7 iso 逐项一致复核）、`git checkout -- build/cli-runtime-closure.json` 还原后全链重跑——闭包零漂移、census 23/23、typecheck/lint 复跑、全部 Python 类 iso 证据重生成；并在 R7 iso（/tmp/r01-repair-r7/iso）现时 A/B 复跑再生成确认其仍零漂移，证明漂移系副本事故而非环境/代码变化。所有最终证据均来自修复后的 iso；被污染期间生成的证据文件已整体覆盖作废。

## 4. 正式提交后复验矩阵（交接总控/复验代理）

1. 由另一全新 Codex 子代理对完整 R01 阶段做只读复验：核对本报告指纹 `0c70bc53…` 与开工/结束工作树一致；复验 F01——R8 原样四变体（`最迟 R09；最迟：R10`、`最迟：Ｒ１０`、`最迟于 R10`、单处 `最迟：R10`）exit 1/NO-GO/risk-stage-text-mismatch，半角冒号/连接词族/否定连接式 exit 1，触发词无阶段 ID（`最迟于第三阶段完成`）exit 1/risk-stage-latest-unresolved，R7 原样/顺序交换/全角混用 exit 1，多处一致（含连接形态）与否定连接式一致及上下文提及 R10 exit 0；R1–R7 全套正负电池保留（自测 83/83、CLI 6 正+54 负、R7 评审反例 13/13、R1–R6 全套 25/25 类别不变）；复核证据哈希/范围（R8 触碰 19 项逐文件 SHA、R7 候选 76 件未动证明）与 16 项 REQUIRED 证据链。
2. 阶段复验 PASS 后，总控按 PROGRESS.md seal 工作流冻结真实候选（本报告 §2 指纹对应工作树）提交，再在真实最终提交上执行：绿色电池 → round2/round3 分片补丁 VERIFIED → 坐标推进（纯审计提交，guard 仅 6 审计文件）→ 四文件门禁 **74/74** → **全量 npm test 0 失败** → 可达集对象审计 0 个 ≥100MiB → 推送后 ls-remote 核对 SHA → 工作树零漂移。不得用预演 SHA 虚报正式封印，不改 allowlist 或删除门禁。
3. 台账回填：ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r8.report_sha256 由后续账本提交补登本报告哈希。

## 5. 失败与限制（如实清单）

1. 候选未提交未推送（无授权）；`report_sha256` 字段为 null 待总控回填。
2. 封印未执行；候选态 6 红（四文件门禁 68/74、全量 npm test 6 failed）如实保留——它们是坐标未推进的预期封印前红（VERIFIED_SOURCE_SHA 仍指 R00 C3 `f2b8c687…`），不由本修复伪造转绿；预演/候选态 SHA 不作正式坐标。
3. iso 组装事故（§3）曾产生闭包伪漂移与一批被污染证据，已全部作废并以修复后 iso 重跑覆盖；最终证据零残留。事故根因（cp 两次拷贝 node_modules）已在本报告与证据文件头注明。
4. 本轮实测为 macOS 27.0 arm64 单机；Windows/Linux/macOS x64、真实供应商/凭证、正式打包/签名、T04/T05/T06 可见原型与 PDF 全量重渲染、授权态录音/屏幕、真实数据迁移未在本轮重跑，结论沿用各 T 独立报告与前轮评审声明。
5. Rust 工作区本轮零改动，未重跑 cargo test；沿用 R4 隔离副本 19 binary 45/45 证据。
6. lint 全量输出本轮只保留摘要日志（exit 0、0 errors/10896 warnings），未归档 1MB+ 全文；如需全文可在 iso 复跑。
7. 开工时 `git ls-remote` 因本机代理（127.0.0.1:7890）离线首次失败，经直连重试成功（开工/收尾两测均为 `363999378…`）；全程零推送。
8. 触发词族/有界连接段的文档化边界见 §1「设计决策与边界」：不含触发词的裸阶段时间后缀提及（「R10 前完成」）不构成期限标记；连接段 >16 字符或含 r/R/终结符的触发词按 unresolved 拒绝（严格契约方向）。
9. 全程未 commit/push/PR/tag/release、未 force push、未触真实用户数据/外发/付费操作；未扩大审计白名单、未退役门禁/测试、未改断言、未虚报验证坐标；未改任何风险登记正文（RISK_REGISTER.json 一字节未动）。

## 6. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 83/83
bash docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                     # 6 正+54 负
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# R8 评审原样反例与对照（10 例登记副本在 /tmp/r01-r8-*.json，仅改 RR-T05-X1 正文）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-r8-colon-conflict.json   # exit 1
# R7 评审反例（13 例在 /tmp/r01-r7-*.json）；R1–R6 全套（18 例 /tmp/r01-repair-r5/repro/，7 例 /tmp/r01-repair-r6/repro/）
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
  xargs shasum -a 256   # 串接后取 SHA-256（原像：/tmp/r01-repair-r8/r8-final-fingerprint-input.bin）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R8/`（10 件）；仓外 `/tmp/r01-repair-r8/`（iso 副本、指纹原像、逐文件哈希清单、检查器与电池 R7→R8 diff、diff 原像与未跟踪哈希清单、npm test 全文日志）。

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
