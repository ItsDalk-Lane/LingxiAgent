# R01 阶段修复 R6 — 修复报告

- 修复者：ZCode:R01 阶段修复 R6（ZCode 一次性任务）；非 R1–R5 修复者、非 R1–R6 阶段验收者、非任一 T 执行/评审代理。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`；仓库 `ItsDalk-Lane/LingxiAgent`。
- 验收输入：`/tmp/r01-stage-review-r6.md`，SHA-256 `7c3cc019f7462bf971105dbf0683faf7f531b7d9d7cda740f76836f25a7865b5`（本机实算吻合）。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01 修复完成并通过主仓与 /tmp 隔离副本双跑回归；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。

## 0. 开工状态核对（与交接一致）

- 本地 HEAD 与开工时 `git ls-remote origin refs/heads/codex/rust-tauri-migration` 均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`。
- 开工未提交候选 = 18 个已跟踪修改 + 57 个新文件 = 75 文件；按 R5/R6 评审同法（`git diff --binary` 字节串接按路径排序的未跟踪文件 SHA-256 清单后取 SHA-256）复算开工冻结指纹 = `b14cc659712942cc4df910621fd6078e4bee98ff8d5f4fdb7d7984b91e892215`，与 R5 修复自报及 R6 评审冻结值逐字节一致。证据链：R5 留档指纹原像（`/tmp/r01-repair-r5/r5-final-fingerprint-input.bin`，284,639 B，SHA-256 == b14cc659…）+ R5 隔离副本中 R6 拟触碰文件的 R5 态哈希逐项匹配 R5 报告所列值 + 其余文件与 R5 隔离副本逐字节一致 + R5 证据 10 件匹配所列哈希，完整重建「开工状态 == R5 冻结候选」。
- R6 修复在 R1–R5 候选之上叠加，未覆盖他人改动，未执行 reset --hard / clean -fd；主仓 `git diff --check` 通过（零空白错误）。

## 1. F01（BLOCKING）全角同形阶段可隐藏在合法引用之前或「最迟」之后 → 已修复

### 根因

检查器 `r01_t08_gate_check.py` 的三条阶段正则只从 ASCII `R` 起始：`STAGE_TOKEN_RE`（合法引用提取）、`STAGE_DISGUISE_RE = re.compile(r"R\d+")`（伪装形态扫描）、`LATEST_MARKER_RE`（「最迟 X」捕获）。全角同形阶段（`Ｒ９９`/`Ｒ１０`/`Ｒ０９`，U+FF22+U+FF10 族）完全不参加提取、真实阶段索引存在性核验与「最迟」一致性核验——只要正文另有一个合法 ASCII `R09`，`tokens[0] == resolve_by_stage_id` 即成立。因此仅改临时登记副本 `RR-T05-X1.resolve_by_stage` 正文（结构化 `resolve_by_stage_id`/`resolve_latest_stage_id` 保持 R09/R09）的两条评审反例均 exit 0 / PASS_WITH_CONDITIONS / TRACKED(R09)：

- `Ｒ９９（不存在的截止阶段）；R09（仅用于宿主集成）`——虚构截止占据正文首位被完全忽略；
- `R09（宿主集成）最迟 Ｒ１０（虚构放宽关卡）`——虚构放宽占据检查器专设的「最迟」语法位置被完全忽略。

同族形态处理还不一致：`R０９`（ASCII R + 全角数字）反而被 `\d` 的 Unicode 十进制语义提取成「R０９」并非真实阶段拒绝——同一视觉形态家族，ASCII R 起始的被抓、全角 R 起始的隐身。混合宽度（`Ｒ99`）、全角复合（`ＸＲ０９`）、小写（`r99`/`ｒ９９`）同病。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.5-stage-repair-r5 → 1.6-stage-repair-r6，规则族 G6c）

1. **全角同形 1:1 等长折叠（先正规化再检验，失败关闭）**：新增 `_fold_stage_homoglyphs()`——全角数字 U+FF10–FF19 → ASCII 0-9、全角大写 U+FF21–FF3A → A-Z、全角小写 U+FF41–FF5A → a-z，逐字符 1:1 等长映射（其余字符不动，含已是 ASCII 者）。阶段引用提取、真实阶段索引核验、正文首坐标=结构化 deadline、「最迟」=结构化 latest 一致性核验**全部在折叠文本上进行**；折叠等长使 span 与原文 1:1 对齐，伪装形态报告切片仍取**原文**（审计者可看到真实伪装字形）。
2. **结构化坐标保持 ASCII 精确权威（不折叠）**：`resolve_by_stage_id`/`resolve_latest_stage_id` 不做折叠、不接受全角形态；它们是机器校验的权威坐标，正文折叠后须与之一致。冻结契约（deadline_stage/latest_stage 逐项）与 REAL_STAGE_INDEX（R00–R11，冻结自任务书 stage-index.json）一字节未动——R5→R6 检查器 diff 260 行中契约/索引数据零改动（diff 留档 `/tmp/r01-repair-r6/gate-check-r5-to-r6.diff`）。
3. **大小写契约明确化（评审第三行许可的契约选择）**：合法阶段引用仅认大写 `R`；小写 `r`+数字（含全角小写）一律按伪装形态拒绝（risk-stage-disguised）——`STAGE_DISGUISE_RE` 加 `re.IGNORECASE` 使折叠后的小写形态必被捕获，而合法提取仍只认大写。真实登记 15 个递延绑定条目正文扫描零命中全角/小写形态（纯 ASCII），正向判定不受影响。
4. **完整标识边界语法不变**：R5 的「标识延续字符」边界类（字母/数字/下划线+点号/连字符族，ASCII/全角/en–em dash/间隔号）原样保留，复合/拼接/虚构小阶段仍按完整复合形态拒绝；折叠在边界判定之前施加，两者正交组合。

### 修复后行为矩阵（真实 CLI 实测，iso 双跑）

| 正文变体（仅改临时登记副本 RR-T05-X1 一字段） | 修复前 | 修复后 |
|---|---|---|
| `Ｒ９９（不存在的截止阶段）；R09（仅用于宿主集成）`（评审原样 v1） | exit 0 / PASS_WITH_CONDITIONS | **exit 1 / NO-GO / risk-stage-unknown**（折叠为 R99，不在 R00–R11） |
| `R09（宿主集成）最迟 Ｒ１０（虚构放宽关卡）`（评审原样 v2） | exit 0 / PASS_WITH_CONDITIONS | **exit 1 / NO-GO / risk-stage-text-mismatch**（最迟 R10 ≠ 结构化 R09） |
| `Ｒ０９（看似合法的截止）；R09（校验）`（评审第三行） | exit 0（首坐标校验失真） | **exit 0 / PASS_WITH_CONDITIONS**——规范化后首坐标=R09 与结构化一致，满足评审「至少识别/规范化正文首个视觉阶段」期望，且 TRACKED 命中 |
| `R０９…`（ASCII R+全角数字） | exit 1（被 \d Unicode 语义提取后拒） | **exit 0**——折叠后与结构化一致，同族处理不一致消除 |
| `Ｒ99…`（全角 R+ASCII 数字混合宽度） | exit 0（隐身） | exit 1 / risk-stage-unknown（折叠为 R99） |
| `ＸＲ０９（…）；R09` | exit 0（隐身） | exit 1 / risk-stage-disguised（折叠为 XR09 复合形态） |
| `r99…；R09` / `ｒ９９…；R09` | exit 0（隐身） | exit 1 / risk-stage-disguised（大小写契约） |
| 真实登记原文（control） | exit 0 | exit 0（不变，15 递延 TRACKED） |

证据：`STAGE_REPAIR_R6/f01-stage-repro-r6.txt`（7 变体+对照，iso 真实 CLI）。

### 负向覆盖（进程内与真实 CLI 双层，只增不删）

- **进程内自测 67/67**：positive-control + N1–N58（R1–R5 全保留）+ **N59–N65 新增 7 个全角/大小写反例**（全角 Ｒ９９ 首位→unknown、最迟 Ｒ１０→text-mismatch、全角 Ｒ１０ 首位→text-mismatch、混合宽度 Ｒ99→unknown、全角复合 ＸＲ０９→disguised、小写 r99→disguised、全角小写 ｒ９９→disguised）+ **P2-positive-fullwidth-normalized**（全角 Ｒ０９ 规范化一致正向，TRACKED 命中）。证据：`f01-gate-selftest-r6.txt`（主仓）、`f01-iso-recheck-r6.txt`（iso）。
- **真实 CLI 电池 2 正向 + 43 负向**：`r01_t08_gate_cli_regression.sh` 在 1 正+36 负之上追加 8 个 R6 变体（7 负向全部 exit 1 且类别正确 + 1 个全角规范化正向 exit 0）；正向对照 exit 0 不变。证据：`f01-cli-regression-r6.txt`（主仓）、`f01-iso-recheck-r6.txt`（iso）。
- **R6 评审原样反例+变体 7/7**（与评审同法：仅改临时登记副本 RR-T05-X1 正文，结构化坐标不动）：两条原样误放修复后 exit 1/NO-GO（类别 unknown/text-mismatch），五个相近形态变体全部按上表预期，control exit 0。
- **R1–R5 全套原样反例回归 18/18**（登记副本 `/tmp/r01-repair-r5/repro/`）：control exit 0；17 负向全部 exit 1 且类别逐项不变（unknown/disguised/text-mismatch 分布与 R5 一致）。证据：`f01-r1-r5-regression-r6.txt`。
- 未删任何测试、未放宽任何关卡、未改任何既有断言（全部为追加）；未改任何风险登记正文/结构化坐标（`RISK_REGISTER.json` 本轮一字节未动，SHA-256 保持 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307`）。

### 设计决策的如实说明

- **选「先正规化再检验」而非「统一拒绝混用」**：评审第三行明确期望「至少识别/规范化正文首个视觉阶段」；全角 `Ｒ０９` 与 ASCII `R09` 语义等价时拒绝会造成同族不一致（`R０９` 已被 \d 语义部分识别）。折叠后全部既有核验（存在性/首坐标/最迟/契约一致）原样生效，失败关闭由既有规则族保证。
- **结构化坐标不折叠**：若登记 JSON 的结构化字段本身写入全角形态，会被「非合法阶段坐标」拒绝（结构化字段必须是 ASCII 精确坐标）——权威侧保持最严。
- **小写拒绝是显式契约选择**：评审指出 `r99` 混用同病且「可视作另一个大小写契约选择」；本轮选择从紧（大写 R 唯一合法），真实登记无小写形态，零正向影响。

## 2. 修改范围与指纹

R6 叠加改动（R5 候选 75 件中 66 件逐字节未动，逐文件证明见 `/tmp/r01-repair-r6/r6-file-hashes.txt` 尾部比对：10 项已跟踪修改与 R5 隔离副本逐字节一致、55 项未跟踪与 R5 隔离副本逐字节一致、1 项 `STAGE_REPAIR_R5/f02-count-correction-r5.txt` 与 R5 哈希清单所列 `fcbaf9d7…` 一致——差异仅为 R5 报告时点说明所载 rsync 后 §3 计数更新三行）：已跟踪修改 8 项 + 既存未跟踪脚本就地更新 1 项（`r01_t08_gate_cli_regression.sh`，R2 引入）+ 新增 `STAGE_REPAIR_R6/` 证据 10 件。候选总计 **85 文件 = 18 修改 + 67 新增**（57 前轮 + 10 R6）。`git diff --check` 通过。

逐文件 SHA-256（R6 触碰 19 项；完整清单 `/tmp/r01-repair-r6/r6-file-hashes.txt`）：

```
M docs/rust-tauri/R01/r01_t08_gate_check.py            db8932837ecc088fff7902524c7afdce7d4dab52b08e24fcd70436c4abe26bd4
M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json   dc87c816947c390c6a1d207ab6cea405157caf306a0521e7310fbe4912bdc6be
M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log     349e253a608756610ee87ef48b5cc869b7407c8b2fe4ab113489dc7b193b5f62
M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log  ef0ed5763de44bce0782eab9538acabb162dc67697b99c68a7aa61d00581f145
M docs/rust-tauri/R01/R01_REPORT.md                    484d7d1d16233ddf905ee819a2cbd7272dfeb396f4be54d5857a2502dbd09737
M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json       0e53d19d52e419dc0a2325f4aaba3259a6d9502bc7d1b36c306112a2a78aaca4
M docs/rust-tauri/R01/R01_HANDOFF.json                 fcadcc1579547176e930644129fd58f87d53fbc0b2e29966721c1121b016e75e
M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json           a314cfa208e5acd8279d83ccfeb2bfefa7c84cef68236d5eb7b8adf0daf32f9b
U docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh   d81df5581e0ecc09caa0798ef4455772362b91be38884de7e8ef842879e7152d（既存未跟踪，就地更新）
N artifacts/rust-tauri/R01/STAGE_REPAIR_R6/（10 件，逐文件哈希见 /tmp/r01-repair-r6/r6-file-hashes.txt）
```

未触碰确认：`RISK_REGISTER.json` 仍为 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307`（R4 所列值）；F01 为纯检查器侧修复，真实登记正文/结构化坐标一字节未动。

**候选工作树冻结指纹**（同 R5/R6 评审法：`git diff --binary` 字节直接串接按路径排序的新文件 SHA-256 清单后取 SHA-256；原像 307,095 B 留档 `/tmp/r01-repair-r6/r6-final-fingerprint-input.bin`）：

```
04dea823b8ff24703241ec2d6128efcb991586565d585e9249e5f61044b3999f
```

HANDOFF `artifact_hashes` 20 项钉住哈希在最终候选状态逐项实算全部匹配（5 项按 R6 改动重算：R01_REPORT.md/R01_ACCEPTANCE_LEDGER.json/r01_t08_gate_check.py/r01_t08_gate_cli_regression.sh/gate-report.json；15 项未触碰逐字节复核，RISK_REGISTER.json 保持 `87e34d0e…`）；`working_tree_digest` 维持 T08 执行时点快照不改（同 R1–R5 惯例，钉住对象为已入库的 T08 交付）。JSON 重写方法：HANDOFF / ORCHESTRATOR_PROGRESS / LEDGER 均为 `json.dumps(ensure_ascii=False, indent=2)+"\n"` 字节级可重现格式（本轮编辑后三文件逐一验证 raw==canonical）。

## 3. /tmp 隔离副本验证矩阵（iso=/tmp/r01-repair-r6/iso，rsync 自主仓候选工作区 + APFS 克隆 .git/node_modules）

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 | gate_check --self-test（主仓+iso 双跑） | **67/67 OK** | f01-gate-selftest-r6.txt / f01-iso-recheck-r6.txt |
| F01 真实 CLI 电池 | r01_t08_gate_cli_regression.sh（双跑） | **2 正+43 负全过** | f01-cli-regression-r6.txt / f01-iso-recheck-r6.txt |
| F01 R6 评审原样反例+变体 | --register /tmp/r01-repair-r6/repro/*.json（7 例+对照，iso） | **全部按预期**（两条原样误放 exit 1/NO-GO，五变体类别正确，control exit 0） | f01-stage-repro-r6.txt |
| F01 R1–R5 全套反例回归 | --register /tmp/r01-repair-r5/repro/*.json（18 例，iso） | **18/18 类别不变** | f01-r1-r5-regression-r6.txt |
| F01 正向真实数据 | gate_check（1.6 契约，双跑） | exit 0 PASS_WITH_CONDITIONS，15 递延 TRACKED | gate-report.json / f01-iso-recheck-r6.txt |
| A15 覆盖自测+真实数据 | coverage_check --self-test / 真实数据（iso） | 6/6 OK + COVERAGE-CLOSED（736 F-ID 双向映射闭合） | a15-a16-recheck-r6.txt |
| A16 隔离检查 | isolation_check（iso） | ISOLATED（原型 209 文件，生产目录零引用） | 同上 |
| 闭包再生成零漂移 | iso: node scripts/compute-cli-closure.mjs | exit 0，8949 文件，`git status --porcelain -- build/` 零输出 | f02-closure-census-r6.txt |
| census 单文件 | iso: vitest run tests/cli-closure-census.test.ts | 23/23（含原位重写用例），build/ 零漂移 | 同上 |
| 四文件门禁（候选态，与 R5 相同四文件） | iso: vitest run upstream-sync-matrix+audit-seal+round2+round3 | **68/74，6 红=预期封印前红集合**（audit-seal diff guard 1：旧坐标；round2 3：R10-03/R10-04/R10-09；round3 2：manifest/replay）——与 R5 候选态同集；upstream-sync-matrix 7/7 绿 | f02-fourfile-iso-r6.txt |
| 四文件门禁补充跑（含 census 的 90 用例选择） | iso: vitest run audit-seal+round2+round3+cli-closure-census | 84/90，同样 6 红同集 | /tmp/r01-repair-r6/logs/fourfile-iso-r6-full.txt |
| 全量 npm test（候选态） | iso: npm test | **14943 passed / 6 failed / 15 skipped**（总 14964，exit 1）——6 红与四文件门禁同集，与 R5 候选态同数同集，R6 零新增红 | f02-npm-test-iso-r6.txt |
| typecheck | iso: npm run typecheck（tsc×3） | exit 0 | typecheck-lint-iso-r6.txt |
| lint | iso: npm run lint | 0 errors / 10896 warnings（与 R4/R5 持平） | typecheck-lint-iso-r6.txt（摘要） |
| 主仓守卫 | git diff --check | 通过（零空白错误） | — |
| artifact_hashes | HANDOFF 20 项逐项实算 | 全部匹配 | — |

主仓只运行了不写盘/不重写交付的检查器与一次性 CLI（gate/coverage/isolation/--self-test/电池/反例复测）；`compute-cli-closure.mjs` 再生成、census、四文件门禁、全量 npm test、lint、typecheck 全部只在 /tmp 隔离副本运行；**主仓全程未运行任何会重写交付 gzip 的脚本**（round2/round3 create-delivery-patch 仅由 vitest 在 iso 内调用）。

时点说明（如实）：隔离副本 rsync 于全部代码改动之后、文档同步与 R6 证据落位之前；iso 全量 npm test（06:34）跑在该时点上。文档四件（R01_REPORT/LEDGER/HANDOFF/ORCHESTRATOR_PROGRESS 的 R6 叙述）于四文件门禁复跑（06:53）前逐字节同步进 iso（四件 sha256 双向核对 MATCH）；已 grep 核实 `tests/` 与 `scripts/` 无任何测试/脚本读取这四件文档，故 doc 增量不影响任何测试结果。最终候选相对 iso 树仅多 `STAGE_REPAIR_R6/` 下 10 个证据归档文件——全部为证据文本，无测试/门禁读取，代码字节零差异。iso 测试后工作树零漂移（git status 与主仓一致，24 项逐项比对）。

## 4. 正式提交后复验矩阵（交接总控/复验代理）

1. 由另一全新 Codex 子代理对完整 R01 阶段做只读复验：核对本报告指纹 `04dea823…` 与开工/结束工作树一致；复验 F01——两条 R6 原样反例（`Ｒ９９（不存在的截止阶段）；R09（仅用于宿主集成）`、`R09（宿主集成）最迟 Ｒ１０（虚构放宽关卡）`）exit 1/NO-GO，`Ｒ０９`+`R09` 规范化一致正向 exit 0，相近形态变体（`Ｒ99`/`ＸＲ０９`/`r99`/`ｒ９９`/`R０９`）按 §1 行为矩阵；R1–R5 全套正负电池保留（自测 67/67、CLI 2 正+43 负、原样反例 18/18 类别不变）；复核证据哈希/范围（R6 触碰 19 项逐文件 SHA、R5 候选 66 件未动证明）与 16 项 REQUIRED 证据链。
2. 阶段复验 PASS 后，总控按 PROGRESS.md seal 工作流冻结真实候选（本报告 §2 指纹对应工作树）提交，再在真实最终提交上执行：绿色电池 → round2/round3 分片补丁 VERIFIED → 坐标推进（纯审计提交，guard 仅 6 审计文件）→ 四文件门禁 **74/74** → **全量 npm test 0 失败**（本机漂移已由 R3 候选根除；其他机器如再现新环境性漂移，census 用例就地报错并恢复，按 RR-ENV-CLOSURE-DRIFT 治理而非记绿）→ 可达集对象审计 0 个 ≥100MiB → 推送后 ls-remote 核对 SHA → 工作树零漂移。
3. 台账回填：ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r6.report_sha256 由后续账本提交补登本报告哈希。

## 5. 失败与限制（如实清单）

1. 候选未提交未推送（无授权）；`report_sha256` 字段为 null 待总控回填。
2. 封印未执行；候选态 6 红（四文件门禁 68/74、全量 npm test 6 failed）如实保留——它们是坐标未推进的预期封印前红（VERIFIED_SOURCE_SHA 仍指 R00 C3 `f2b8c687…`），不由本修复伪造转绿；预演/候选态 SHA 不作正式坐标。
3. 本轮实测为 macOS 27.0 arm64 单机；Windows/Linux/macOS x64、真实供应商/凭证、正式打包/签名、T04/T05/T06 可见原型与 PDF 全量重渲染、授权态录音/屏幕、真实数据迁移未在本轮重跑，结论沿用各 T 独立报告与前轮评审声明。
4. Rust 工作区本轮零改动，未重跑 cargo test；沿用 R4 隔离副本 19 binary 45/45 证据。
5. 收尾时 `git ls-remote` 因本机代理（127.0.0.1:7890）离线未能复测远端 HEAD；本轮全程未执行任何 push（无授权），开工时与 R6 评审开工/收尾三处记录远端均为 `363999378…`，本地 HEAD 全程同一值。
6. lint 全量输出本轮只保留摘要日志（exit 0、0 errors/10896 warnings），未归档 1.2MB 全文；如需全文可在 iso 复跑（约 2 分钟）。
7. 全程未 commit/push/PR/tag/release、未 force push、未触真实用户数据/外发/付费操作；未扩大审计白名单、未退役门禁/测试、未改断言、未虚报验证坐标；未改任何风险登记正文（RISK_REGISTER.json 一字节未动）。

## 6. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 67/67
bash docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                     # 2 正+43 负
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# R6 评审原样反例与变体（7 例登记副本在 /tmp/r01-repair-r6/repro/，仅改 RR-T05-X1 正文）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-repair-r6/repro/fw-r99-first.json
# R1–R5 全套反例回归（18 例登记副本在 /tmp/r01-repair-r5/repro/）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-repair-r5/repro/r09-dot-5.json
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
  xargs shasum -a 256   # 串接后取 SHA-256（原像：/tmp/r01-repair-r6/r6-final-fingerprint-input.bin）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R6/`（10 件）；仓外 `/tmp/r01-repair-r6/`（iso 副本、指纹原像、7 例 R6 反例登记、逐文件哈希清单、检查器 R5→R6 diff、iso 原始日志含四文件门禁补充跑全文）。

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
