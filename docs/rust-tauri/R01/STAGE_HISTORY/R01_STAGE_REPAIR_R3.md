# R01 阶段修复 R3 — 修复报告

- 修复者：ZCode:R01 阶段修复 R3（ZCode 一次性任务）；非 R1/R2 修复者、非 R1/R2/R3 阶段验收者、非任一 T 执行/评审代理。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`；仓库 `ItsDalk-Lane/LingxiAgent`。
- 验收输入：`/tmp/r01-stage-review-r3.md`，SHA-256 `df460deb5453bb27757f47f9c4f7eb8c76c548eeafe4fac94c06976a795bf64f`（本机实算吻合）；R2 评审/修复报告 SHA-256 `727e82f0…`/`5cddb8c7…` 与交接一致。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01/F02 全部修复并完成隔离复现与预演回证；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。

## 0. 开工状态核对（与交接一致）

- 本地/远端（remote-tracking）HEAD = `363999378482dfed42733a3c4e8ad20ac82fd0fc`（fetch 因本机代理不可达，以 origin 缓存引用核对；R1/R2 同样限制）；全程未 commit/push/tag。
- 未提交候选 = 16 个已跟踪修改 + 26 个新文件（STAGE_REPAIR_R1/ 16 + STAGE_REPAIR_R2/ 9 + r01_t08_gate_cli_regression.sh），与交接一致；R3 修复在其上叠加，未覆盖他人改动，未执行 reset --hard/clean -fd。
- R3 评审冻结指纹 `beee5117ed3bd0382efa3ac6ac43d353cc37befa56c12ecb6e29e60169808038` 以同法（§4）在开工时工作树复算吻合——证明 R1+R2 候选原样保留。

## 1. 跨轮根因归并与同类路径覆盖矩阵

R01 已连续三轮 FAIL。先归并三轮根因、再修本轮发现，避免再漏新字段形态：

| 轮次 | 发现 | 字段/机制类别 | 处置 |
|---|---|---|---|
| R1 | 检查器把必需能力全集交给被验输入自报（删域/删项/删 sha256/删递延均放行） | 契约闭合 | R1 已修 FROZEN_CONTRACT，本轮 N1–N16 全保留复测 |
| R1 | resolve_by_stage 空串 | 字段空值 | R1 已修，保留 |
| R2 | null/{}/[]/数字/布尔/占位经 str() 冒充非空 | 字段类型 | R2 已修 `_meaningful_str`，本轮 N17–N22 全保留复测 |
| R2 | 输入递延状态与登记状态矛盾（CLOSED 仍挂账） | 状态一致性 | R2 已修 DEFERRED_RISK_STATUS_MAP，本轮 N23–N25 全保留复测 |
| R2 | 登记本体（缺 id/重复 id/risks 非列表） | 登记完整性 | R2 已修，本轮 N26–N28 全保留复测 |
| **R3** | **截止阶段指向不存在的 R99 仍放行** | **阶段存在性** | 本轮：REAL_STAGE_INDEX 冻结绑定（G6） |
| **R3** | **含糊措辞（如「以后」）无阶段坐标** | **阶段可解析性** | 本轮：正文 token 解析 |
| **R3** | **过晚（越过适用最迟关卡）** | **阶段上界** | 本轮：契约 latest_stage |
| **R3** | **错误归属（存在但不归属的阶段）** | **阶段归属** | 本轮：契约 deadline_stage |
| **R3** | **「只搜正文含 R09 字样」不能构成有意义截止期** | **结构化绑定** | 本轮：结构化字段为判定权威+正文一致性核验 |

15 个递延绑定项的同类字段（截止/失败处理/风险 ID/status）逐一扫查：新 G6 检查作用于全部 15 项（非仅 RR-T05-X1）；`resolve_by_stage`/`resolve_by_stage_id`/`resolve_latest_stage_id`/`failure_handling`/`risk_id`/`status` 六字段全部有负向覆盖（§2）。消费方扫查：`resolve_by_stage`/RISK_REGISTER 全仓仅 `r01_t08_gate_check.py` 一个消费方，改动面收敛。

F02 同类扫查：闭包生成物消费方仅 census 测试（再生+原位写）与 build-server-open（只读白名单），无其他原位写；RR-R00-ROUND2-SIDEEFFECT（round2/round3 失败时重写 patch.gz）绿态不触发，MISMATCH 不改写不变式保持。

## 2. F01（BLOCKING）截止阶段可机器校验绑定 → 已修复

### 根因
`evaluate()` 对 `resolve_by_stage` 只要求 `_meaningful_str`（非空、非占位），不校验它指向的阶段是否存在于任务书 stage-index、是否晚于当前阶段、是否为该项适用关卡——R99、纯「以后」、把 R04 事项推到 R10 均放行；正文含「R09」字样与真实截止期无必然联系。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.2-stage-repair-r2 → 1.3-stage-repair-r3）
1. **冻结真实阶段索引**：`REAL_STAGE_INDEX = R00..R11`（冻结自任务书 stage-index.json 的 12 阶段；该任务书目录未入 git 跟踪——`git ls-files` 实测 0——冻结进检查器保证任何检出可复现可审计，与 FROZEN_CONTRACT 同一哲学；修改=改关卡须独立验收）。
2. **契约升级**：FROZEN_CONTRACT 每个递延绑定从 `"cap": "risk_id"` 升级为 `{risk_id, deadline_stage, latest_stage}`。deadline/latest 冻结自风险登记各条目 resolve_by_stage 正文**已声明并经 R01 关卡放行**的坐标（如 RR-T06-PROXY 正文「R09 最迟 R10」→ deadline R09 / latest R10；跨平台/授权态缺口保持 R09/R10 强制关卡；RR-T07-F2「R08/R09」→ R08/R09）。
3. **登记结构化坐标**：RISK_REGISTER.json 15 个递延绑定条目补 `resolve_by_stage_id`/`resolve_latest_stage_id`（值=正文声明坐标）；正文与其余字段一律未改。
4. **G6 校验**（`_stage_binding_problems`，作用于全部 15 项）：
   - 结构化字段必须为非空字符串（缺失/非字符串 → `risk-stage-field-missing`）；
   - 正文须含阶段 token（无 → `risk-stage-vague`）；正文**任何** token 必须存在于阶段索引（含非首个 → `risk-stage-unknown`）；
   - 正文首个 token = 结构化 deadline；正文「最迟 X」token = 结构化 latest（违 → `risk-stage-text-mismatch`）；
   - deadline 必须严格晚于当前阶段 R01（违 → `risk-stage-current-or-past`）；latest ≥ deadline（违 → `risk-stage-order-inverted`）；
   - deadline == 契约 deadline（违 → `risk-stage-deadline-mismatch`，覆盖错误归属与过晚）；latest == 契约 latest（违 → `risk-stage-latest-mismatch`）。
   - TRACKED 行改为机器可校验格式 `TRACKED(<risk_id> -> <deadline> 最迟 <latest>)`，正文不再作为判定依据。

### 验证（主仓与 /tmp 隔离副本双跑一致）
- 进程内自测 **44/44**：positive-control + N1–N28（R1/R2 全部负向保留）+ N29–N43（15 个新负向：正文 R99/双字段 R99/含糊「以后再说」/过晚拖延/错误归属/越过契约最迟/越过登记声明最迟/文本字段背离/当前阶段/缺两个字段/倒挂/null/最迟标记背离/正文内嵌 R99）。证据：`artifacts/rust-tauri/R01/STAGE_REPAIR_R3/f01-gate-selftest-r3.txt`。
- **真实 CLI 负向电池 1 正向 + 22 负向**：r01_t08_gate_cli_regression.sh 在 15 个 R2 变体上新增 7 个阶段变体（stage-text-nonexistent/stage-both-nonexistent/stage-vague-text/stage-deadline-too-late/stage-latest-past-gate/stage-field-missing/stage-order-inverted），全部 exit 1 且输出对应类别；正向对照 exit 0。证据：`f01-cli-regression-main.txt`。
- **R3 评审原样反例复测**：与评审完全相同的方法（仅改 RR-T05-X1 正文为 `R99（不存在的阶段）`）——修复前 exit 0/PASS_WITH_CONDITIONS，修复后 **exit 1/NO-GO**，双理由拒绝（正文引用不存在阶段 + 正文/结构化坐标不一致）。证据：`f01-r99-repro-r3.txt`。
- 正向真实数据：exit 0 / PASS_WITH_CONDITIONS / 15 递延 TRACKED 新格式 / `contract_violations=[]`；关卡证据三件按 1.3 契约重生成（gate-report.json/gate-check.log/gate-selftest.log）。

## 3. F02（正式封印/推进阻断）全量测试受控绿色 → 已修复

### 根因（独立复现确认，与 R2 三实验归因一致）
nft 静态分析解析 `lib/sandbox/*` 等源码中 `/bin/bash` 硬编码 spawn 目标，把**本机该路径文件**计入追踪；该宿主绝对路径经 `normalizeSourceGraphPath` 落空（fallthrough 保留绝对路径）进入闭包，使本机再生成与已提交基线漂移（+14/−3，8949→8950）。census「原位重写」用例把漂移写进跟踪文件后，同套件并行的 round2/round3 生成器 tree==HEAD 守卫按设计拒绝 → 2 红。四树实验（本轮裸 HEAD 复现 + R2 的 A/B/C）证实漂移与候选内容无关、纯环境性。

### 修复（不改基线、不删测试、不放松守卫/断言、不伪造坐标）
1. **`scripts/compute-cli-closure.mjs`**：`normalizeNftTraceFiles` 新增**类级过滤**——丢弃宿主绝对（posix `/` 前缀或 Windows 盘符）与越界（`../`）条目。依据：committed 基线 8949 项经实测**零绝对路径、零越界路径**——「闭包=仓内相对路径+node_modules 逻辑路径」是既有语义；过滤该类使所有机器收敛于同一基线（不产生 /bin/bash 的机器输出不变，产生的机器被归一到基线），**不需要提交任何机器特定数据**。基线未动、生成语义注释明确。
2. **`tests/cli-closure-census.test.ts`** 三处（加强非放松）：
   - 「matches committed」用例新增**结构性断言**：逐 file 断言无 `/` 前缀、无 `../` 前缀、无盘符前缀（未来任何宿主路径泄漏会在根因处报错而非写脏基线）；
   - 「writeCliRuntimeClosure/writeOpenBoundaryBaseline … in place」用例加 `finally` 恢复快照：未来若再现漂移，该用例只报自身失败并还原跟踪文件，不再级联污染工作树激活交付守卫（RR-R00-ROUND2-SIDEEFFECT 同类卫生）；
   - 新增归一化单测：宿主绝对/越界条目被丢弃、仓内相对条目保留。
3. 交付生成器 tree==HEAD 守卫、MISMATCH 不改写、审计白名单、census 用例本体：零改动。

### 验证
- **修复前复现**：裸 HEAD 363999378 隔离树再生成 → +14/−3 `/bin/bash`（与 R2 归因逐字节同型；实验后已还原）。
- **修复后同机零漂移**：同一台机器同一环境，候选树再生成 8949 文件，`build/cli-runtime-closure.json` 与 `build/open-boundary-baseline.json` 均不再出现于 git status——**再生成与已提交基线逐字节一致**。证据：`f02-closure-regen-zero-drift-r3.txt`。
- **census 单文件**（干净候选树）：23/23 全绿（22 既有 + 1 新增），含三个全量生成用例；运行后 build/ 零漂移。
- **候选态全量 npm test**：14949 passed / 6 failed / 15 skipped（exit 1）——6 红与四文件门禁同集、全部为预期封印前红（VERIFIED_SOURCE_SHA 仍指 R00 C3，坐标推进属总控封印步骤）；**R2 的 2 个环境性红消失**；总数 14964 = 14963 + 本轮新增 1 单测。
- **/tmp 封印预演全链**（ceremony-r3.sh，预演提交均为 REHEARSAL 彩排产物不作正式坐标）：R3c'=d1cfc7f29（候选源冻结）→ 绿色电池 5 步（typecheck/core-contracts/lint:boundary/**lint 0 errors**/build:renderer）+ round2 分片 VERIFIED（6 片，patchBytes=267,408,980）+ round3 分片 VERIFIED（13 片）+ 三文件记录全绿 → R3s''=d6800d892 → 六处坐标推进（guard：仅 6 个审计文件变化）→ R3a''=500ee91db → **四文件门禁 74/74 exit 0** → **全量 npm test 14949 passed / 0 failed / 15 skipped，exit 0** → 可达集对象审计最大 blob 97,003,232 B（0 个 ≥100MiB）→ 本地裸仓推送预演 REMOTE_SHA==R3a''、裸仓实收最大 97,003,232 B → **终态工作树零漂移**。证据：`seal-rehearsal-r3-ceremony.txt`。
- 配套复测：typecheck tsc×3 exit 0；`npm run lint` exit 0（0 errors/10896 warnings，与 R2 持平）；cargo test --workspace 19 binary **45/45**（候选补丁零 .rs/Cargo 改动，结果不变性成立）；A16 COVERAGE-CLOSED（6/6）；隔离核查 ISOLATED。

## 4. 修改范围与指纹

R3 叠加改动（R1+R2 候选 42 项全部原样保留）：已跟踪文件 +2 修改（`scripts/compute-cli-closure.mjs`、`tests/cli-closure-census.test.ts`）、10 项内容更新（gate_check.py/RISK_REGISTER/LEDGER/HANDOFF/REPORT/ORCHESTRATOR_PROGRESS/gate-report.json/gate-check.log/gate-selftest.log/r01_t08_gate_cli_regression.sh）、新增 STAGE_REPAIR_R3/ 证据 10 件。候选总计 **54 文件 = 18 修改 + 36 新增**。

**候选工作树冻结指纹**（同 R2/R3 评审法：`git diff --binary` 输出直接串接按路径排序的新文件 SHA-256 清单后取 SHA-256；输入留档 `/tmp/r01-repair-r3/candidate-fingerprint-input.bin`，254,044 B）：

```
3430e659b0e91e32b26053dbfcff14769a6c874e4605d119e592568c4787465d
```

`git diff --check` 通过。逐文件 SHA-256（R3 新增/更新项；R1/R2 既有 42 项哈希见 `/tmp/r01-repair-r3/all-candidate-files.txt` 与 /tmp/r01-repair-r2/candidate-file-sha256.txt，其中两个补丁生成器与 eslint/probe/fixture/交付测试等 10 项与 R2 报告逐字节同值未动）：

```
20a13e688d248f678150c993be4ca7e0cc8feb2b2e7db980ab7faa01ed41719b  M docs/rust-tauri/R01/r01_t08_gate_check.py
95897967978263df922613b04d82de4f9a2b8905aa2e6ca47fb2236b3395d4ae  M docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh
8b2451d310240e837817ac40b81d4d2ad92b027357151be69075213d8985cb12  M docs/rust-tauri/R01/RISK_REGISTER.json
083bf8bc498506b9f76895004ca0d14529303de0785cb132b87ab4e83ef12c6b  M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json
11b8f0f13c73d36a807a22cd2125c11248a4742883268e2809fddeb199f0df36  M docs/rust-tauri/R01/R01_HANDOFF.json
2b32aeecb27021ff39eb39598030773e78c677ca0d26a8e5dceff019abffad9e  M docs/rust-tauri/R01/R01_REPORT.md
6c7490ee44b0c91d5734e8d852e4f2498cfed953e71bd3d5801e1921bc1bff06  M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json
b4fbf2e66942a7bf5bf6c1244108b6035b9c66ac4275d035108fc7542e81a063  M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json
55a735065a41f66e5dc86fc06abcd559a3d16169848927ce49ad7a153eb8ed76  M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log
97b31b03f643c2b5c95ad6bcb2f0beeb62ce98f7ee877594cc05e460885f9729  M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log
5f0604ab6aa169e6c7926763d07278c0dc59dd26caf3936fd9fec10169513466  M scripts/compute-cli-closure.mjs
cddb32aedcf184aa9df36d07e044392df69996ae081fb20bf0d249ed3c62c27c  M tests/cli-closure-census.test.ts
（新增 N）artifacts/rust-tauri/R01/STAGE_REPAIR_R3/ 10 件证据，逐文件哈希在 /tmp/r01-repair-r3/all-candidate-files.txt
```

## 5. 阶段复验矩阵（本修复后状态）

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 | gate_check --self-test | 44/44 OK（主仓+iso 双跑） | f01-gate-selftest-r3.txt |
| F01 真实 CLI 电池 | r01_t08_gate_cli_regression.sh | 1 正+22 负全过（双跑） | f01-cli-regression-main.txt |
| F01 R99 原样反例 | --register /tmp/r01-repair-r3/risk-invalid-deadline.json | exit 1 NO-GO 双理由 | f01-r99-repro-r3.txt |
| F01 正向真实数据 | gate_check（1.3 契约） | exit 0 PASS_WITH_CONDITIONS，15 递延 TRACKED 新格式 | T08/gate-checker/gate-report.json |
| F02 修复前复现 | 裸 HEAD 树 node compute-cli-closure | +14/−3 /bin/bash（与 R2 归因一致） | f02-closure-regen-zero-drift-r3.txt |
| F02 修复后零漂移 | 候选树再生成 | 8949 文件，build/ 两文件零漂移 | 同上 |
| F02 census 单文件 | vitest run tests/cli-closure-census.test.ts | 23/23（干净树含原位重写） | 同上 |
| F02 候选态全量 npm test | npm test（候选树） | 14949/6 failed（全为预期封印前红，2 环境性红消失） | f02-npm-test-candidate-state-r3.txt |
| F02 预演态全量 npm test | ceremony-r3.sh @ R3a'' | **14949 passed / 0 failed / 15 skipped，exit 0** | seal-rehearsal-r3-ceremony.txt |
| 四文件门禁（候选态） | vitest 四文件 | 68/74，6 红=预期封印前红集合 | f02-fourfile-candidate-state-r3.txt |
| 四文件门禁（预演态） | ceremony-r3.sh @ R3a'' | **74/74 exit 0**；guard 仅 6 审计文件 | seal-rehearsal-r3-ceremony.txt |
| 对象尺寸/推送预演 | ceremony-r3.sh | 可达集与裸仓 0 个 ≥100MiB（最大 97,003,232 B）；推送 SHA 一致 | 同上 |
| A16 覆盖 / 隔离 | coverage_check / isolation_check | COVERAGE-CLOSED 6/6 / ISOLATED | a15-a16-isolation-iso.txt |
| lint / typecheck | npm run lint / typecheck | 0 errors / tsc×3 exit 0 | typecheck-lint-cargo-iso.txt |
| Rust 工作区 | cargo test --workspace（rust/） | 19 binary 45/0；候选零 Rust 改动 | 同上 |

## 6. 正式提交后复验条件（交接总控/复验代理）

1. 阶段复验 PASS 后，总控提交真实 R3c（候选=§4 指纹对应工作树）。
2. 按 PROGRESS.md seal 工作流（与 ceremony-r3.sh 同构，绑定真实 SHA）：绿色电池 → 双补丁分片 VERIFIED → vitest 三件套 → R3s → 六处坐标推进+矩阵再生 → 纯审计 R3a → post-verification diff guard 仅 6 审计文件。
3. 四文件门禁 74/74；**全量 npm test 应得受控 0 失败**——本机环境漂移已由 F02 修复根除（无漂移环境下 census 原位写写回恒等字节，不再触发交付守卫），若在其他机器出现新的环境性漂移，census 用例将就地报错并自行恢复工作树，按 RR-ENV-CLOSURE-DRIFT 治理而非记绿。
4. 推送前可达集对象审计（`git rev-list --objects <候选> | git cat-file --batch-check`）无 ≥100MiB blob；推送后 ls-remote 核对 SHA。
5. 台账回填：ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r3.report_sha256 由后续账本提交补登本报告哈希。

## 7. 失败与限制（如实清单）

1. 候选未提交未推送（无授权）；ORCHESTRATOR_PROGRESS/HANDOFF 的本报告 SHA-256 字段为 null 待总控回填。
2. 封印未执行（须阶段复验 PASS 后由总控执行）；预演 SHA（d1cfc7f29/d6800d892/500ee91db）全部为 REHEARSAL 彩排产物，不作正式坐标。
3. 预演树与最终候选的差异：ceremony 在 STAGE_REPAIR_R3 证据落位与台账/日志微调（含 gate-check.log 一处尾随空白清除）之前启动，故 R3c' 树不含最终 10 件证据与台账终稿——与 R2 同先例（代码与测试逐字节为最终版，差异仅为文档/证据补记）；正式封印在真实候选上重新生成全部证据，不依赖预演树。
4. 真实 GitHub 推送未执行（无推送授权且代理不可达）；以本地裸仓推送预演代替。
5. 候选态四文件门禁 6 红与全量 npm test 6 红**如实保留**——它们是坐标未推进的预期封印前红，不由本修复伪造转绿；预演态已证明坐标推进后即 74/74 与全量 exit 0。
6. 平台限制沿用既有声明：Windows/Linux/macOS x64、真实供应商/凭证、正式打包/签名未验，按 R09/R10 关卡挂账；本报告全部结论为 macOS 27.0 arm64 本机实测。

## 8. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 44/44
bash docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                     # 1 正+22 负
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# R3 原样反例（改 RR-T05-X1 正文为 R99 后）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-repair-r3/risk-invalid-deadline.json
# F02
node scripts/compute-cli-closure.mjs && git status --porcelain -- build/    # 零输出=零漂移
npx vitest run tests/cli-closure-census.test.ts                             # 23/23
# A15/A16/隔离
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py [--self-test]
python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py
# 封印预演全链（仅限 /tmp 隔离副本，勿在主仓运行）
bash /tmp/r01-repair-r3/ceremony-r3.sh
# 指纹复算
git diff --binary | cat; git ls-files --others --exclude-standard | sort | \
  xargs shasum -a 256   # 串接后取 SHA-256（原像：/tmp/r01-repair-r3/candidate-fingerprint-input.bin）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R3/`（10 件）；仓外 `/tmp/r01-repair-r3/`（ceremony-r3.sh、logs/、全量 npm test 输出、指纹原像）。

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
