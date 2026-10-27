# R01 阶段修复 R4 — 修复报告

- 修复者：ZCode:R01 阶段修复 R4（ZCode 一次性任务）；非 R1/R2/R3 修复者、非 R1–R4 阶段验收者、非任一 T 执行/评审代理。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`；仓库 `ItsDalk-Lane/LingxiAgent`。
- 验收输入：`/tmp/r01-stage-review-r4.md`，SHA-256 `0d6ce6834a0426bfcae1273f18f030ba38907f687cc27a7767b31c75966deab8`（本机实算吻合）。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01/F02 全部修复并完成真实 CLI 正负反例与 /tmp 隔离副本回归；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。

## 0. 开工状态核对（与交接一致）

- 本地 HEAD 与 `git rev-parse origin/codex/rust-tauri-migration` 均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`。
- 未提交候选 = 18 个已跟踪修改 + 36 个新增文件（STAGE_REPAIR_R1/ 16 + STAGE_REPAIR_R2/ 9 + STAGE_REPAIR_R3/ 10 + r01_t08_gate_cli_regression.sh），与交接一致；R4 修复在其上叠加，未覆盖他人改动，未执行 reset --hard/clean -fd。
- R4 验收冻结指纹 `3430e659b0e91e32b26053dbfcff14769a6c874e4605d119e592568c4787465d` 对应的开工工作树 = R3 终稿候选：本轮修改前的 RISK_REGISTER.json / r01_t08_gate_check.py / r01_t08_gate_cli_regression.sh 现场哈希与 R3 报告 §4 逐字节一致；R01_HANDOFF.json 的 R3 终稿（`11b8f0f13c73d36a807a22cd2125c11248a4742883268e2809fddeb199f0df36`）由验收指纹原像 `/tmp/r01-repair-r3/candidate-fingerprint-input.bin` 的单文件 diff 块对 HEAD 重建复核吻合。

## 1. F01（BLOCKING）伪装阶段标识被截断放行 → 已修复

### 根因
`r01_t08_gate_check.py` 的 `STAGE_TOKEN_RE = re.compile(r"R\d{2}")` 与 `LATEST_MARKER_RE = re.compile(r"最迟\s*(R\d{2})")` 均无边界检查：对更长或带前后缀的标识只截取合法子串——`R099`/`R0999` 截成 `R09`、`XR09`/`R09X`/`R09_stage` 截出 `R09`，`最迟 R099` 截成 `最迟 R09`。仅改临时登记 `RR-T05-X1.resolve_by_stage` 正文（结构化 `resolve_by_stage_id`/`resolve_latest_stage_id` 保持 R09/R09）的 R4 评审反例 v1（`R099（不存在的阶段）`）、v2（`R09/R099（混合了伪造阶段）`）、v3（`XR09（伪造阶段）`）全部 exit 0 / PASS_WITH_CONDITIONS，违反 G6 自身「正文任何 token 均真实存在」的承诺。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.3-stage-repair-r3 → 1.4-stage-repair-r4，新增 G6b）
1. **完整形态提取取代截断提取**：`STAGE_TOKEN_RE = (?<![A-Za-z0-9_])R\d+(?![A-Za-z0-9_])`——R+1 位以上数字、前后边界非字母数字/下划线；`_extract_stage_references()` 返回 (clean_tokens, disguised_hits)。`R099` 提取为完整串 `R099`（随后按完整串核验存在性 → 不在 R00–R11 → `risk-stage-unknown` + 正文首坐标与结构化 R09 不一致 → `risk-stage-text-mismatch` 双重拒绝），不再产生合法 `R09`。
2. **伪装/截断形态单独拒绝**：`STAGE_DISGUISE_RE = R\d+` 扫描全部形似出现；凡未被完整形态 span 覆盖者（即被嵌入更长标识或带相邻字母数字/下划线，如 `XR09`/`R09X`/`R09_stage`/`R099X`）报新类别 `risk-stage-disguised`，不得截断成合法阶段放行。
3. **「最迟 X」完整形态捕获核验**：`LATEST_MARKER_RE = 最迟\s*(?<![A-Za-z0-9_])(R\d+(?![A-Za-z0-9_]))`；捕获组先核验存在于真实阶段索引（`最迟 R099` → `risk-stage-unknown`），再与结构化 latest 一致性核验。
4. 正向不变：真实登记 15 个递延绑定条目正文（`R09（…）`、`R09 最迟 R10`、`R08/R09`、`R04（先于 R04/R08）` 等）全部按完整形态正常提取，`R09/R10/R08` 等合法引用无一误伤；正向真实数据判定不变（exit 0 / PASS_WITH_CONDITIONS / 15 递延 TRACKED）。

### 验证（主仓与 /tmp 隔离副本双跑一致）
- 进程内自测 **51/51**：positive-control + N1–N28（R1/R2 全部负向保留）+ N29–N43（R3 全部负向保留）+ **N44–N50 新增**（正文 R099 / 混合 R09/R099 / 前缀 XR09 / 后缀 R09X / 最迟标记 R099 / 四位 R0999 / 下划线 R09_stage）。证据：`artifacts/rust-tauri/R01/STAGE_REPAIR_R4/f01-gate-selftest-r4.txt`。
- **真实 CLI 负向电池 1 正向 + 28 负向**：r01_t08_gate_cli_regression.sh 在 15 个 R2 变体 + 7 个 R3 变体之上新增 6 个 R4 变体（stage-text-r099 / stage-text-mixed-r09-r099 / stage-text-prefixed-xr09 / stage-text-suffixed-r09x / stage-latest-marker-r099 / stage-text-r0999），全部 exit 1 且输出对应拒绝类别；正向对照 exit 0 / PASS_WITH_CONDITIONS。证据：`f01-cli-regression-r4.txt`。
- **R4 评审原样反例复测**（与评审完全相同的方法：仅改临时登记 RR-T05-X1 正文，结构化坐标与其余候选字节不动）：v0（R99）exit 1、v1（R099）exit 1 / NO-GO（`risk-stage-unknown` + `risk-stage-text-mismatch` 双理由）、v2（R09/R099）exit 1 / NO-GO、v3（XR09）exit 1 / NO-GO（`risk-stage-disguised`）。修复前三例 exit 0 / PASS_WITH_CONDITIONS（R4 验收实测）。证据：`f01-stage-token-repro-r4.txt`（主仓）+ `f01-iso-recheck-r4.txt`（隔离副本）。
- 关卡证据三件按 1.4 契约重生成（gate-report.json / gate-check.log / gate-selftest.log，51 用例），判定与 15 递延 TRACKED 行不变；无尾随空白。

## 2. F02（交接记录失真）风险登记与验收账本 R2 时态更新 → 已修复

### 根因
R3 候选已根除闭包漂移（normalizeNftTraceFiles 类级过滤 + census finally 恢复/结构性断言；同机再生成 8949 文件零漂移；R3 /tmp 封印预演坐标推进后全量 npm test 14949 passed / 0 failed / exit 0），但以下字段仍以现在时陈述 R2 状态，未标记为历史，会使接手总控误以为仍须执行 R2 提出的环境对齐/换环境方案：
- `RISK_REGISTER.json` `RR-ENV-CLOSURE-DRIFT`：summary「本机当前环境再生…漂移」「致 2 个现场重放用例红」、resolve_by_stage「R01 正式封印执行前由总控裁决：再生并提交环境对齐的闭包…或在无漂移环境执行」；
- `RR-AUDIT-SEAL-PREEXISTING`：summary 末段「预演同时暴露…使全量 npm test 2 红，正式封印前需治理」、resolve_by_stage「正式执行前还需处理本机 cli-runtime-closure 环境漂移（再生提交闭包或在无漂移环境执行）」；
- `R01_ACCEPTANCE_LEDGER.json` `common_environment.npm_test_note` 末句「正式封印前需治理（再生提交闭包或环境对齐，超出 R2 范围）」、`description` 只写到 R2 验收待复验。

### 修复（保留 R2 失败历史，更新当前事实与剩余条件；不提前称正式封印完成）
1. **RR-ENV-CLOSURE-DRIFT**：summary 改写为「（R2 发现，历史保留）…（R01 阶段修复 R3 已修复）…」，R2 三实验归因与 2 红现场历史逐句保留；resolve_by_stage 改为「已由 R01 阶段修复 R3 候选修复（同机再生成零漂移），不再需要再生提交环境对齐闭包或换环境裁决；剩余条件=正式提交后在真实候选上复验再生成零漂移与全量 npm test 0 失败；若其他机器出现新的环境性漂移，census 用例将就地报错并恢复工作树，按本条目重新登记治理而非记绿」；failure_handling 补「修复候选未正式提交并复验前本项保持 OPEN」；status 保持 OPEN；evidence 补 R3 证据指针；新增 `r3_update` 字段记录 R3 修复与 R4 时态更正。
2. **RR-AUDIT-SEAL-PREEXISTING**：summary 末段改为「R2 预演曾暴露…（R2 时态，历史保留）…该漂移已由 R01 阶段修复 R3 候选根除…R3 /tmp 封印预演坐标推进后全量 npm test 14949 passed/0 failed/exit 0（预演 SHA 不作正式坐标）」；resolve_by_stage 改为「…R01 阶段修复 R3 已根除本机 cli-runtime-closure 环境漂移（同机再生成零漂移，不再需要再生提交环境对齐闭包或换环境方案），剩余条件=正式提交后在真实候选上复验（四文件门禁 74/74、全量 npm test 0 失败、分片重组哈希一致、可达对象 0 个 ≥100MiB、工作树零漂移、远端 SHA 一致）」；status 保持 OPEN；新增 `r3_update` 字段。
3. **R01_ACCEPTANCE_LEDGER.json**：npm_test_note 末段改为 R2 历史时态 + R3 已根除事实（预演全量 14949 passed/0 failed/15 skipped、exit 0）+ 当前剩余条件（候选尚未正式提交，全量 0 失败须在正式提交后的真实候选上复验）；description 补齐 R3 评审/修复与 R4 评审/修复全链（含 R4 报告 SHA-256 `0d6ce683…`）。
4. **同步**：R01_HANDOFF.json `unresolved_items` 两条摘要同构更新（resolve_by_stage 同步）；R01_REPORT.md 追加 R4 阶段验收进展段与独立审查段；R01_HANDOFF `stage_gate_position` 追加 G6b 叙事；ORCHESTRATOR_PROGRESS.json R01 stage 字段推进到 R4（stage_review_round=4、评审/修复 agent/report/SHA-256、stage_repair_note 重写为 R4 叙述）；ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r4.report_sha256 按惯例留 null 待总控账本提交补登。
5. 未把正式封印记为完成：所有新增文本明确「预演 SHA 不作正式坐标」「正式提交后在真实候选上复验」「封印推进属总控 seal 工作流」。

## 3. 修改范围与指纹

R4 叠加改动（R1+R2+R3 候选 54 项全部原样保留）：已跟踪修改 10 项 + 新增 STAGE_REPAIR_R4/ 证据 12 件。候选总计 **66 文件 = 18 修改 + 48 新增**（36 前轮 + 12 R4）。`git diff --check` 通过。

逐文件 SHA-256（R4 修改/新增项；R1–R3 既有 54 项见 /tmp/r01-repair-r3/all-candidate-files.txt 等，本轮未动的 10 项已跟踪文件与 36 项前轮新增全部逐字节未变）：

```
M docs/rust-tauri/R01/r01_t08_gate_check.py            c7a7e02b9cede39e9bae5337a018ae19b1f339520102d5ad8d851b0aae218c89
M docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh   f4aff09318de6eec7431fa172fb332c62b45e5e735b68e26a8941210daffddc0
M docs/rust-tauri/R01/RISK_REGISTER.json               87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307
M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json       a01a4a4795e8098300c7a433de1b1e7335f953abe77170d86a6a4910d0f03f12
M docs/rust-tauri/R01/R01_REPORT.md                    ee00601420c627212c41d13ae949d6ce4ad626a7630bc8e23dd02accc0a51557
M docs/rust-tauri/R01/R01_HANDOFF.json                 67956b1c27f606b7ecd5769de89ba3a21e4b70342345894306c0b48046f4bd0b
M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json           eb52f34b042847065aa7df6e212e1e11a804bb419691d10b9d69080014b0dcb4
M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json   af5b4a82c8a7043b09096141e486be86f2ba4961ffbdd736d3034ff5c2f3f1c3
M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log     d2522b4cde02ecaabd78561b93000a7d4b4b3e1c87e2d1e88f419be25b8ef829
M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log  7ef1e4d94cf5d3356b2980d0eb946fcb5b28368dd66ad1a76cb9c7368c3abba9
N artifacts/rust-tauri/R01/STAGE_REPAIR_R4/（12 件，逐文件哈希见 /tmp/r01-repair-r4/r4-file-hashes.txt）
```

**候选工作树冻结指纹**（同 R2/R3/R4 评审法：`git diff --binary` 输出直接串接按路径排序的新文件 SHA-256 清单后取 SHA-256；原像 268,548 B 留档 `/tmp/r01-repair-r4/r4-fingerprint-input.bin`）：

```
2e3913b6d1f8f580cbd7c90e2526844bd8d68be71d5825c4fdcba6b8eb2e9309
```

JSON 重写方法说明（透明记录）：R01_HANDOFF.json 与 ORCHESTRATOR_PROGRESS.json 均为 `json.dumps(ensure_ascii=False, indent=2)+"\n"` 可字节级重现格式——以 R3 终稿（HANDOFF 由验收指纹原像单文件 diff 重建、SHA `11b8f0f1…` 复核）实测确认后在其上以同法更新；RISK_REGISTER/LEDGER/gate_check.py/CLI 脚本为字符串级编辑，格式未动（RISK_REGISTER 现场哈希与 R3 报告一致的开工复核可证）。R01_HANDOFF.artifact_hashes 20 项在最终候选状态逐项实算全部匹配。

## 4. /tmp 隔离副本验证矩阵（iso=/tmp/r01-repair-r4/iso，rsync 自主仓候选工作区 + APFS 克隆 .git/node_modules）

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 | gate_check --self-test（主仓+iso 双跑） | 51/51 OK | f01-gate-selftest-r4.txt / f01-iso-recheck-r4.txt |
| F01 真实 CLI 电池 | r01_t08_gate_cli_regression.sh（双跑） | 1 正+28 负全过 | f01-cli-regression-r4.txt |
| F01 R4 原样反例 | --register /tmp/r01-repair-r4/v{0..3}.json | v0–v3 全部 exit 1/NO-GO（双跑） | f01-stage-token-repro-r4.txt / f01-iso-recheck-r4.txt |
| F01 正向真实数据 | gate_check（1.4 契约） | exit 0 PASS_WITH_CONDITIONS，15 递延 TRACKED | gate-report.json |
| F02 再生成零漂移 | iso: node scripts/compute-cli-closure.mjs | exit 0，`git status --porcelain -- build/` 零输出 | a15-a16-f02-iso-r4.txt |
| F02 census 单文件 | iso: vitest run tests/cli-closure-census.test.ts | 23/23（含原位重写用例），build/ 零漂移 | 同上 |
| A15/A16 覆盖+隔离 | coverage_check --self-test / isolation_check | 6/6 OK / ISOLATED（生产入口零引用） | 同上 |
| R2 F03 分片夹具回归 | iso: vitest run round2+round3 -t 分片 | 6 passed / 58 skipped，patches/ 零漂移 | 同上 |
| 四文件门禁（候选态） | iso: vitest run 四文件（证据落位前后两次） | 68/74，6 红=预期封印前红集合（audit-seal 1/round2 3/round3 2） | f02-fourfile-candidate-state-r4.txt / f02-fourfile-final-r4.txt |
| 全量 npm test（候选态） | iso: npm test | 14943 passed / 6 failed / 15 skipped（总 14964，exit 1）——6 红与四文件门禁同集、全部为坐标未推进的预期封印前红 | f02-npm-test-candidate-state-r4.txt |
| typecheck | iso: npm run typecheck（tsc×3） | exit 0 | typecheck-iso-r4.txt |
| lint | iso: npm run lint | 0 errors / 10896 warnings（与 R3 持平） | lint-iso-r4.txt |
| Rust 工作区 | iso: cargo test --workspace --offline | 19 binary 45/45 passed（候选零 Rust 改动） | typecheck-lint-cargo-iso-r4.txt |
| 主仓守卫 | git diff --check | 通过（零空白错误） | — |
| artifact_hashes | HANDOFF 20 项逐项实算 | 全部匹配 | — |

主仓只运行了不写盘/不重写交付的检查器与一次性 CLI（gate/coverage/isolation/--self-test）；`compute-cli-closure.mjs` 再生成、census、全量 npm test、分片夹具全部只在 /tmp 隔离副本运行；**主仓全程未运行任何会重写交付 gzip 的脚本**（round2/round3 create-delivery-patch 未运行）。

## 5. 正式提交后复验矩阵（交接总控/复验代理）

1. 由另一全新 Codex 子代理对完整 R01 阶段做只读复验：核对本报告指纹 `2e3913b6…` 与开工/结束工作树一致，复验 F01（R099/R09-R099 混合/XR09/R09X/最迟 R099/R0999 等变体 exit 1，R1–R3 全套正负电池保留）与 F02（风险/账本/交接事实与 R3 修复后实测一致、R2 历史保留、无提前封印宣告），及 16 项 REQUIRED 证据链。
2. 阶段复验 PASS 后，总控按 PROGRESS.md seal 工作流冻结真实候选（本报告 §3 指纹对应工作树）提交，再在真实最终提交上执行：绿色电池 → round2/round3 分片补丁 VERIFIED → 坐标推进（纯审计提交，guard 仅 6 审计文件）→ 四文件门禁 74/74 → **全量 npm test 0 失败**（本机漂移已由 R3 候选根除；其他机器如再现新环境性漂移，census 用例就地报错并恢复，按 RR-ENV-CLOSURE-DRIFT 治理而非记绿）→ 可达集对象审计 0 个 ≥100MiB → 推送后 ls-remote 核对 SHA → 工作树零漂移。
3. 台账回填：ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r4.report_sha256 由后续账本提交补登本报告哈希。

## 6. 失败与限制（如实清单）

1. 候选未提交未推送（无授权）；report_sha256 字段为 null 待总控回填。
2. 封印未执行；R3 预演 SHA（d1cfc7f29/d6800d892/500ee91db）与本轮候选态 6 红（四文件门禁 68/74、全量 npm test 6 failed）如实保留——它们是坐标未推进的预期封印前红，不由本修复伪造转绿。
3. 本轮实测为 macOS 27.0 arm64 单机；Windows/Linux/macOS x64、真实供应商/凭证、正式打包/签名、T04/T05/T06 可见原型与 PDF 全量重渲染、授权态录音/屏幕、真实数据迁移未在本轮重跑，结论沿用各 T 独立报告与前轮评审声明。
4. 全量 npm test 候选态 passed 计数 14943（=14964−6 failed−15 skipped）：R3 报告 §3 写作「14949 passed/6 failed」，其 14949 实为 R3 预演态（坐标推进后）passed 值，候选态按总数减法应为 14943——本轮实测 14943/6/15 与 R3 候选态同集同数，6 红名单逐项核对一致（audit-seal 1、R10-03/R10-04/R10-09、round3 manifest/replay 2）。
5. iso 验证副本在跑 cargo test 后含 rust/target/ 等未跟踪副产物（隔离副本一次性使用，不入候选）；主仓候选未受影响。
6. 全程未 commit/push/PR/tag/release、未 force push、未触真实用户数据/外发/付费操作；未扩大审计白名单、未退役门禁/测试、未改断言、未虚报验证坐标。

## 7. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 51/51
bash docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                     # 1 正+28 负
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# R4 评审原样反例（v0–v3 登记在 /tmp/r01-repair-r4/，仅改 RR-T05-X1 正文）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-repair-r4/v1.json
# A15/A16
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --self-test        # 6/6
python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py                   # ISOLATED
# F02（仅限 /tmp 隔离副本；主仓勿运行）
node scripts/compute-cli-closure.mjs && git status --porcelain -- build/    # 零输出=零漂移
npx vitest run tests/cli-closure-census.test.ts                             # 23/23
# 指纹复算
git diff --binary | cat; git ls-files --others --exclude-standard | sort | \
  xargs shasum -a 256   # 串接后取 SHA-256（原像：/tmp/r01-repair-r4/r4-fingerprint-input.bin）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R4/`（12 件）；仓外 `/tmp/r01-repair-r4/`（iso 副本、指纹原像、反例登记 v0–v3、逐文件哈希清单）。

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
