# R01 阶段修复 R5 — 修复报告

- 修复者：ZCode:R01 阶段修复 R5（ZCode 一次性任务）；非 R1/R2/R3/R4 修复者、非 R1–R5 阶段验收者、非任一 T 执行/评审代理。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`；仓库 `ItsDalk-Lane/LingxiAgent`。
- 验收输入：`/tmp/r01-stage-review-r5.md`，SHA-256 `3107c695916f600d4e00b2be2eb5cb0fe3d49aa5135efb764247ee498f519b21`（本机实算吻合）。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01 修复、F02 更正全部完成并通过主仓与 /tmp 隔离副本双跑回归；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。

## 0. 开工状态核对（与交接一致）

- 本地 HEAD 与 `git rev-parse origin/codex/rust-tauri-migration` 均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`。
- 开工未提交候选 = 18 个已跟踪修改 + 47 个新文件 = 65 文件；按 R4/R5 评审同法（`git diff --binary` 输出串接按路径排序的未跟踪文件 SHA-256 清单后取 SHA-256）复算开工冻结指纹 = `2e3913b6d1f8f580cbd7c90e2526844bd8d68be71d5825c4fdcba6b8eb2e9309`，与 R4 修复自报及 R5 评审冻结值逐字节一致（原像 268,548 B 留档 `/tmp/r01-repair-r5/r5-fingerprint-input.bin`）。
- R5 修复在 R1–R4 候选之上叠加，未覆盖他人改动，未执行 reset --hard / clean -fd；主仓 `git diff --check` 通过（零空白错误）。

## 1. F01（BLOCKING）非真实小阶段 `R09.5`/`R09-5`/`R09．5` 被截作 `R09` 放行 → 已修复

### 根因

R4 修复引入的完整标识边界类只含 `[A-Za-z0-9_]`，把点号/连字符当作合法 token 边界：`STAGE_TOKEN_RE = (?<![A-Za-z0-9_])R\d+(?![A-Za-z0-9_])` 对正文 `R09.5` 提取出 `R09`（`.` 被视为边界）、`R09-5` 提取出 `R09`（`-` 同理）、`R09．5`（全角点）同理；`STAGE_DISGUISE_RE = R\d+` 对这些出现也只看见 `R09`，不报伪装；`LATEST_MARKER_RE` 的捕获边界同病。因此仅改临时登记 `RR-T05-X1.resolve_by_stage` 正文（结构化 `resolve_by_stage_id`/`resolve_latest_stage_id` 保持 R09/R09）的三个虚构小阶段字面量全部 exit 0 / PASS_WITH_CONDITIONS / TRACKED(R09)——不存在的整体截止标识在人工执行说明里冒充了已机器校验的 R09，而任务书 stage-index.json 只有 R00–R11。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.4-stage-repair-r4 → 1.5-stage-repair-r5）

1. **合法阶段引用完整语法明确化（G6b 扩展）**：正文阶段引用合法形态 = `R` + 至少一位 ASCII 数字组成的**完整 token**，且紧邻前后字符均不属「标识延续字符」。延续字符 = 字母/数字/下划线（构成更长标识：R0999/XR09/R09X/R09_stage）**+ 点号/连字符族连接符**（构成复合/虚构小阶段/区间/拼接标识：ASCII 与全角点 `.` `．`、连字符 `-` `－` 及 en/em dash 族 `–` `—` `―` `‐` `‑` `‒` `−`、下划线 `_` `＿`、小数点 `﹒`、间隔号 `·` `・`）。空白、斜杠 `/`、中文标点（（）／，。、：等）是**合法分隔符**，分隔两个独立完整引用——真实登记的 `R09（…）`、`R09 最迟 R10`、`R08/R09`、`R04（先于 R04/R08）`、`R09（宿主集成）最迟 R10（跨平台产物）` 等全部形态不受影响，CJK 邻接（如「在R09阶段」）也不受影响。这是按「标识延续」原则定义的边界类，不是对三个字面量打补丁。
2. **复合/拼接形态按完整复合形状拒绝**：`_extract_stage_references()` 返回 (clean_tokens, disguised_shapes)；凡形似引用（R+数字）但不构成完整引用的出现，向两侧扩展至标识延续字符边界后按**完整复合形态**报告（如 `R09.5`/`R09-5`/`R09．5`/`R09.5.1`/`R09.R10`），类别仍为 `risk-stage-disguised`，不得截断成合法阶段放行。`R09.R10` 这类点号拼接两个真实阶段的歧义写法同样整体拒绝（不产生合法 R09/R10 引用）。
3. **「最迟 X」标记同边界规则**：`LATEST_MARKER_RE = 最迟\s*(?<!edge)(R\d+(?!edge))`——`最迟 R09.5`/`最迟 R09-5` 的标记位复合形态不再捕获出合法 R09；该出现由伪装规则整体拒绝。
4. **结构化坐标仍是判定权威**：正文逐 token 核验真实阶段索引、正文首坐标=结构化 deadline、「最迟」=结构化 latest、结构化坐标×冻结契约（deadline_stage/latest_stage）×REAL_STAGE_INDEX（R00–R11）三方一致核验全部保持不变。
5. 正向不变：真实登记 15 个递延绑定条目正文全部按完整形态正常提取，正向真实数据判定不变（exit 0 / PASS_WITH_CONDITIONS / 15 递延 TRACKED）。

### 负向覆盖（进程内与真实 CLI 双层，只增不删）

- **进程内自测 59/59**：positive-control + N1–N28（R1/R2 全保留）+ N29–N43（R3 全保留）+ N44–N50（R4 全保留）+ **N51–N58 新增 8 个复合形态反例**（R09.5 / R09-5 / R09．5 / R09－5 / R09.5.1 / R09.R10 / 最迟 R09.5 / 混合 R09/R09.5）。证据：`artifacts/rust-tauri/R01/STAGE_REPAIR_R5/f01-gate-selftest-r5.txt`。
- **真实 CLI 负向电池 1 正向 + 36 负向**：`r01_t08_gate_cli_regression.sh` 在 1 正+28 负（15 基础 + 7 R3 + 6 R4）之上新增 8 个 R5 变体，全部 exit 1 且输出 `risk-stage-disguised`；正向对照 exit 0 / PASS_WITH_CONDITIONS。证据：`f01-cli-regression-r5.txt`。
- **R5 评审原样反例复测 18/18**（与评审完全相同的方法：仅改临时登记副本 RR-T05-X1 正文，结构化坐标与其余字段不动；登记副本 `/tmp/r01-repair-r5/repro/`）：修复前三字面量 exit 0/PASS_WITH_CONDITIONS（R5 评审实测）→ 修复后 `r09-dot-5`/`r09-hyphen-5`/`r09-fullwidthdot-5` 全部 exit 1 / NO-GO / `BLOCKED(risk-stage-disguised)`；合理变体 `r09-fullwidthhyphen-5`（R09－5）/`r09-multi-dot`（R09.5.1）/`r09-dot-r10`（R09.R10）/`latest-r09-dot-5`（最迟 R09.5）/`latest-r09-hyphen-5`（最迟 R09-5）/`r09-slash-dot5`（R09/R09.5）同样 exit 1；R4 原样反例 v0(R99)/r099/r09-r099/xr09/r09x/r0999/r09-stage/latest-r099 保持 exit 1 且类别不变；`control-original` 保持 exit 0。证据：`f01-stage-token-repro-r5.txt`（主仓 18 例逐条）+ `f01-iso-recheck-r5.txt`（隔离副本复跑）。
- 未删任何测试、未放宽任何关卡、未改任何既有断言（全部为追加）。

### 边界决策的如实说明（fail-closed 取舍）

- ASCII 句点/连字符**紧贴**阶段引用（如英文句尾 `R09.`、行尾 `R09-`）按复合形态拒绝：这类写法与 `R09.5`/`R09-5` 在机器语法上不可区分，从紧拒绝。中文句号 `。`、全角逗号等是合法分隔符不受影响；真实登记正文无此形态。
- 全角同形字 `Ｒ０９`（全角 R+全角数字）不属 ASCII 阶段引用语法：纯正文仅含它时由 vague 规则拒绝（无任何合法阶段坐标）；与合法引用混合出现时，其装饰性片段不单独识别为伪装（结构化坐标仍为权威、正文首个真实引用仍须与之一致）。真实登记无此形态，已知限制如实记录。

## 2. F02（非独立阻断）R4 修复报告证据范围计数失真 → 已更正

### 事实核实

- 实际 `artifacts/rust-tauri/R01/STAGE_REPAIR_R4/` = **11 件**（`.mimosa/` 为 git 忽略的本地扫描缓存，不计入候选）；候选 = 18 修改 + 47 新增 = **65 文件**。R4 报告 §3/§7 写「12 件」「候选 18+48=66」为多计一件。
- 11 件哈希与 `/tmp/r01-repair-r4/r4-file-hashes.txt` 所列 SHA **逐项 MATCH**（11/11，见 `STAGE_REPAIR_R5/f02-count-correction-r5.txt` §2）。
- **无第 12 件计划内证据**：R4 报告 §3（改动清单）/§4（验证矩阵证据列）/§7（可复现命令与原始日志）点名的 STAGE_REPAIR_R4 日志恰为 11 件、全部在列；`r4-file-hashes.txt` 在「12 件」标题下也只列 11 条 SHA；除计数措辞外无任何第 12 件文件名或哈希。判定：**R4 报告计数错误，非证据缺失**——与 R5 评审结论一致（「11 件都存在且与所列 SHA 逐项匹配…未发现被点名却缺失的关键日志」）。

### 处置（历史可追溯）

- **不改写** R4 原报告 `/tmp/r01-stage-repair-r4.md` 及其已记录 SHA（评审侧记录 `0d6ce683…` 不变）；`/tmp/r01-repair-r4/` 全部原样保留。
- 实际范围更正写入现行交接/范围链四处：`R01_HANDOFF.json`（新增 `stage_review_r5.f02_note` 与 `stage_repair_r5.note`、`artifact_hashes.note` 补更正句）、`ORCHESTRATOR_PROGRESS.json`（`stage_repair_note` 重写为 R5 叙述并写明 11 件/65）、`R01_REPORT.md`（R5 阶段验收进展两处）、`R01_ACCEPTANCE_LEDGER.json`（description 补 R5 链含 F02 更正）。
- 可还原性证明：HANDOFF / ORCHESTRATOR_PROGRESS / LEDGER 三份 JSON 剥离 R5 追加后 SHA-256 逐一还原为 R4 所列值（`67956b1c…` / `eb52f34b…` / `a01a4a47…`），即 R5 对它们的改动 = 纯追加文本，无历史改写。

## 3. 修改范围与指纹

R5 叠加改动（R1+R2+R3+R4 候选 65 项中 46 项逐字节未动，证明见 `/tmp/r01-repair-r5/r5-file-hashes.txt` 头注）：已跟踪修改 8 项 + 既存未跟踪脚本就地更新 1 项（`r01_t08_gate_cli_regression.sh`，R2 引入）+ 新增 `STAGE_REPAIR_R5/` 证据 10 件。候选总计 **75 文件 = 18 修改 + 57 新增**（47 前轮 + 10 R5）。`git diff --check` 通过。

逐文件 SHA-256（R5 触碰项；完整清单 `/tmp/r01-repair-r5/r5-file-hashes.txt`）：

```
M docs/rust-tauri/R01/r01_t08_gate_check.py            678d7ebcd81ac453ef042dd648cbe7bc60e29f8e42f7c0392100f8e8ee13893e
M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json   9e4bfab8753454124f6a0a2694b24367870f4196ad935ad305c383a250d3bc32
M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log     2fca0e0da481046899a13d5d5a4ac538c5ef2ea258363131e7ca87520f2dbf61
M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log  1da490bb130121ddd0942e154a9a13d2e735e02d92c18400f876efa8a8a38288
M docs/rust-tauri/R01/R01_REPORT.md                    4759707ced74c9a14bf62f66d291ff6442e2b9e667dfef8c22d2fe4949e25315
M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json       c34d4a7667f5c471a2ee02afa14a40bcdd4cf76f36fe7de4203b83dfb8046d13
M docs/rust-tauri/R01/R01_HANDOFF.json                 18f4bbbf314b28c7b50d8ee4500e9776057d4b7f0ec6bc35e1adcdf0164be346
M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json           66c220b3b38d060f8ed4618b699dcc37ea7371892b9b80c43184d7cca37b7fb4
U docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh   9c75819d97b01a712db5d229b5c01ac7acc0fa3c7c174218bd25c89c0fbc8cd6（既存未跟踪，就地更新）
N artifacts/rust-tauri/R01/STAGE_REPAIR_R5/（10 件，逐文件哈希见 /tmp/r01-repair-r5/r5-file-hashes.txt）
```

未触碰确认：`RISK_REGISTER.json` 仍为 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307`（R4 所列值）；F01 为纯检查器侧修复，真实登记正文/结构化坐标一字节未动。

**候选工作树冻结指纹**（同 R4/R5 评审法：`git diff --binary` 输出直接串接按路径排序的新文件 SHA-256 清单后取 SHA-256；原像 284,639 B 留档 `/tmp/r01-repair-r5/r5-final-fingerprint-input.bin`）：

```
b14cc659712942cc4df910621fd6078e4bee98ff8d5f4fdb7d7984b91e892215
```

HANDOFF `artifact_hashes` 20 项钉住哈希在最终候选状态逐项实算全部匹配（5 项按 R5 改动重算 + 15 项未触碰逐字节复核）；`working_tree_digest` 维持 T08 执行时点快照不改（同 R1–R4 惯例，钉住对象为已入库的 T08 交付）。JSON 重写方法：HANDOFF / ORCHESTRATOR_PROGRESS / LEDGER 均为 `json.dumps(ensure_ascii=False, indent=2)+"\n"` 字节级可重现格式，且本轮对三者的改动已通过「剥离 R5 追加→还原 R4 哈希」逐字节验证。

## 4. /tmp 隔离副本验证矩阵（iso=/tmp/r01-repair-r5/iso，rsync 自主仓候选工作区 + APFS 克隆 .git/node_modules）

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 | gate_check --self-test（主仓+iso 双跑） | **59/59 OK** | f01-gate-selftest-r5.txt / f01-iso-recheck-r5.txt |
| F01 真实 CLI 电池 | r01_t08_gate_cli_regression.sh（双跑） | **1 正+36 负全过** | f01-cli-regression-r5.txt |
| F01 R5 评审原样反例+变体 | --register /tmp/r01-repair-r5/repro/*.json（18 例，双跑） | **18/18 全部按预期**（三字面量与全部变体 exit 1/NO-GO/risk-stage-disguised，R1–R4 反例类别不变，control exit 0） | f01-stage-token-repro-r5.txt / f01-iso-recheck-r5.txt |
| F01 正向真实数据 | gate_check（1.5 契约） | exit 0 PASS_WITH_CONDITIONS，15 递延 TRACKED | gate-report.json |
| A15 覆盖自测 | coverage_check --self-test（双跑） | 6/6 OK | a15-a16-recheck-r5.txt / f01-iso-recheck-r5.txt |
| A16 隔离检查 | isolation_check（双跑） | ISOLATED（扫描 2209 个生产文件，零原型引用） | 同上 |
| F02 闭包再生成零漂移 | iso: node scripts/compute-cli-closure.mjs | exit 0，`git status --porcelain -- build/` 零输出 | f01-iso-recheck-r5.txt |
| F02 census 单文件 | iso: vitest run tests/cli-closure-census.test.ts | 23/23（含原位重写用例），build/ 零漂移 | 同上 |
| 四文件门禁（候选态） | iso: vitest run 审计四文件 | **68/74，6 红=预期封印前红集合**（audit-seal 1：diff guard 旧坐标；round2 3：R10-03/R10-04/R10-09；round3 2：manifest/replay）——与 R4 候选态同集 | f02-fourfile-iso-r5.txt |
| 全量 npm test（候选态） | iso: npm test | **14943 passed / 6 failed / 15 skipped**（总 14964，exit 1）——6 红与四文件门禁同集，与 R4 候选态同数同集，R5 零新增红 | f02-npm-test-iso-r5.txt |
| typecheck | iso: npm run typecheck（tsc×3） | exit 0 | typecheck-iso-r5.txt |
| lint | iso: npm run lint | 0 errors / 10896 warnings（与 R4 持平） | lint-iso-r5.txt |
| 主仓守卫 | git diff --check | 通过（零空白错误） | — |
| artifact_hashes | HANDOFF 20 项逐项实算 | 全部匹配 | — |

主仓只运行了不写盘/不重写交付的检查器与一次性 CLI（gate/coverage/isolation/--self-test/电池）；`compute-cli-closure.mjs` 再生成、census、四文件门禁、全量 npm test、lint、typecheck 全部只在 /tmp 隔离副本运行；**主仓全程未运行任何会重写交付 gzip 的脚本**（round2/round3 create-delivery-patch 未运行）。

时点说明（如实）：隔离副本 rsync 于全部代码与文档改动之后、R5 证据落位之前；最终候选相对 iso 树仅多 `STAGE_REPAIR_R5/` 下 5 个证据归档文件（iso 复检记录自身与日志归档）及 f02-count-correction §3 的冻结计数更新——全部为证据文本，无测试/门禁读取，代码字节零差异。

## 5. 正式提交后复验矩阵（交接总控/复验代理）

1. 由另一全新 Codex 子代理对完整 R01 阶段做只读复验：核对本报告指纹 `b14cc659…` 与开工/结束工作树一致，复验 F01（`R09.5`/`R09-5`/`R09．5` 及 `R09－5`/`R09.5.1`/`R09.R10`/`最迟 R09.5`/`R09/R09.5` 变体 exit 1/NO-GO；R1–R4 全套正负电池保留：自测 59/59、CLI 1 正+36 负、原样反例 18/18）与 F02（STAGE_REPAIR_R4 11 件/候选 65 的更正事实、11 件哈希匹配、R4 原报告未改写），及 16 项 REQUIRED 证据链。
2. 阶段复验 PASS 后，总控按 PROGRESS.md seal 工作流冻结真实候选（本报告 §3 指纹对应工作树）提交，再在真实最终提交上执行：绿色电池 → round2/round3 分片补丁 VERIFIED → 坐标推进（纯审计提交，guard 仅 6 审计文件）→ 四文件门禁 **74/74** → **全量 npm test 0 失败**（本机漂移已由 R3 候选根除；其他机器如再现新环境性漂移，census 用例就地报错并恢复，按 RR-ENV-CLOSURE-DRIFT 治理而非记绿）→ 可达集对象审计 0 个 ≥100MiB → 推送后 ls-remote 核对 SHA → 工作树零漂移。
3. 台账回填：ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r5.report_sha256 由后续账本提交补登本报告哈希。

## 6. 失败与限制（如实清单）

1. 候选未提交未推送（无授权）；`report_sha256` 字段为 null 待总控回填。
2. 封印未执行；候选态 6 红（四文件门禁 68/74、全量 npm test 6 failed）如实保留——它们是坐标未推进的预期封印前红（VERIFIED_SOURCE_SHA 仍指 R00 C3 `f2b8c687…`），不由本修复伪造转绿；预演/候选态 SHA 不作正式坐标。
3. 本轮实测为 macOS 27.0 arm64 单机；Windows/Linux/macOS x64、真实供应商/凭证、正式打包/签名、T04/T05/T06 可见原型与 PDF 全量重渲染、授权态录音/屏幕、真实数据迁移未在本轮重跑，结论沿用各 T 独立报告与前轮评审声明。
4. Rust 工作区本轮零改动，未重跑 cargo test；沿用 R4 隔离副本 19 binary 45/45 证据（`typecheck-lint-cargo-iso-r4.txt`）。
5. F01 边界决策的 fail-closed 取舍与全角同形字已知限制见 §1「边界决策的如实说明」。
6. 全程未 commit/push/PR/tag/release、未 force push、未触真实用户数据/外发/付费操作；未扩大审计白名单、未退役门禁/测试、未改断言、未虚报验证坐标。

## 7. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 59/59
bash docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                     # 1 正+36 负
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# R5 评审原样反例（18 例登记副本在 /tmp/r01-repair-r5/repro/，仅改 RR-T05-X1 正文）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-repair-r5/repro/r09-dot-5.json
# A15/A16
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --self-test        # 6/6
python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py                   # ISOLATED
# F02 证据范围核实（只读；11 件哈希比对明细见 f02-count-correction-r5.txt）
# F02 闭包（仅限 /tmp 隔离副本；主仓勿运行）
node scripts/compute-cli-closure.mjs && git status --porcelain -- build/    # 零输出=零漂移
npx vitest run tests/cli-closure-census.test.ts                             # 23/23
# 指纹复算
git diff --binary | cat; git ls-files --others --exclude-standard | sort | \
  xargs shasum -a 256   # 串接后取 SHA-256（原像：/tmp/r01-repair-r5/r5-final-fingerprint-input.bin）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R5/`（10 件）；仓外 `/tmp/r01-repair-r5/`（iso 副本、开工/冻结指纹原像、18 例反例登记、逐文件哈希清单）。

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
