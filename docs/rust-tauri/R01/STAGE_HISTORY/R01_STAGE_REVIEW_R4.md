# R01 阶段独立验收 R4

- 验收者：全新 Codex 子代理 `/root/r01_stage_review_r4`；只读审查，未参与 R01 任一 T 的执行、修复、独立验收，也不是 R1/R2/R3 阶段验收者。
- 基线：R00 正式封印 C4 `328cc8bb5a807bdaad520b459907fb1fa4e10dca`。分支 `codex/rust-tauri-migration`，本地及 `git ls-remote origin refs/heads/codex/rust-tauri-migration` 均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`。
- 审查对象：R1+R2+R3 未提交阶段修复候选，18 个已跟踪修改、36 个新增文件。`git diff --binary` 与按路径排序的新文件 SHA-256 清单串接后的 SHA-256，在验收开始和结束均为 `3430e659b0e91e32b26053dbfcff14769a6c874e4605d119e592568c4787465d`（254044 字节原像 `/tmp/r01-stage-r4-fingerprint-start.bin`）；与 R3 修复报告自报值一致，`git diff --check` 通过。主仓未提交、未推送，测试后指纹无漂移。
- 输入：任务书 R01、stage-index/task-catalog/acceptance-catalog，R01_REPORT/HANDOFF/ACCEPTANCE_LEDGER/RISK_REGISTER、ORCHESTRATOR_PROGRESS、T01–T08 最终独立验收报告，以及 R1/R2/R3 阶段评审/修复报告。R3 修复报告 SHA-256 实算 `56587d5718faa7bc4ff3032d52a77402e4715f202538c1d3fdd15d72526677cb`。

**STAGE VERDICT: FAIL。** F01 是可用真实 CLI 复现的高风险截止期绕过；F02 是当前阶段交接事实与 R3 修复后实测相矛盾。不得据此把 R01 标为阶段 PASS、推进正式封印/推送或进入 R02。

## F01（BLOCKING）：伪装阶段 R099 被识别成真实 R09 并放行

R01-T08 要求每个高风险递延缺口有可执行的截止阶段与失败处理。R3 修复文档与 G6 的明文承诺是正文**任何**不存在的阶段 token 都要拒绝，正文和结构化坐标必须一致。实际 `docs/rust-tauri/R01/r01_t08_gate_check.py` 使用 `STAGE_TOKEN_RE = re.compile(r"R\d{2}")`，它对更长或带前缀的标识只截取合法子串：`R099` → `R09`，`XR09` → `R09`。这让错误的风险正文伪装成有效截止阶段。

独立复现只改临时风险登记中 `RR-T05-X1.resolve_by_stage` 一项；`resolve_by_stage_id=R09`、`resolve_latest_stage_id=R09`、其余真实候选字节保持不变。执行：

```
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --register /tmp/r01-r4-stage-token-repro/v1.json \
  --out /tmp/r01-r4-stage-token-repro/v1-report.json
```

`v1.json` 的正文为 `R099（不存在的阶段）`，结果 **exit 0 / PASS_WITH_CONDITIONS**，该能力显示 `TRACKED(RR-T05-X1 -> R09 最迟 R09)`；`R09/R099（混合了伪造阶段）` 同样 exit 0 / PASS_WITH_CONDITIONS（`v2.json`）；`XR09（伪造阶段）` 也 exit 0（`v3.json`）。正向对照 `R99（不存在的阶段）` 为 exit 1 / NO-GO（`v0.json`）。完整输入与输出在 `/tmp/r01-r4-stage-token-repro/`。因此现有 44/44 进程内自测和 1 正+22 负真实 CLI 电池都未覆盖完整 token 边界；这些绿灯无法证明 G6 对伪装阶段失败关闭。

严重度：**阶段放行阻断**。风险仍有结构化 R09 坐标，但作为交给后续阶段的人工执行说明，正文可写不存在的截止阶段并得到关卡 `PASS_WITH_CONDITIONS`；同时直接违反检查器自身「正文任何 token 均真实存在」的保证。修复应在冻结的真实阶段索引下按完整标识识别并验证阶段引用，补真实 CLI 负向覆盖 `R099`、混合 `R09/R099` 和相邻前后缀形态；保留已有 R1/R2/R3 全套反例、真实数据正向及全阶段回归。由全新 ZCode 阶段修复任务处理，再由全新 Codex 阶段验收者完整复验。

## F02（交接记录失真）：当前风险登记与验收账本仍要求处理已由 R3 候选修复的闭包漂移

R3 候选的代码、`R01_REPORT.md`、`R01_HANDOFF.json`、R3 修复报告及隔离封印预演一致显示：闭包漂移经类级过滤/测试恢复修复，同机再生成不漂移，预演全量 `npm test` 14949 passed / 0 failed。然而当前 `RISK_REGISTER.json` 的 `RR-ENV-CLOSURE-DRIFT` 仍以现在时陈述“本机当前环境再生…漂移”“致 2 个现场重放用例红”，状态 `OPEN`，`resolve_by_stage` 仍要求总控在正式封印前裁决“再生并提交环境对齐的闭包，或在无漂移环境执行”；`RR-AUDIT-SEAL-PREEXISTING.resolve_by_stage` 也称正式执行前**还需处理**该环境漂移。`R01_ACCEPTANCE_LEDGER.json.common_environment.npm_test_note` 的末句同样称“正式封印前需治理”，其 `description` 只讲到 R2 阶段验收待复验。上述字段没有标记为“R2 历史状态”，与 R3 当前候选结果矛盾，会使接手总控以为仍须采用 R2 提出的环境对齐或换环境方案。正式封印确实仍待完成，不能因此把**已修复的漂移**和**尚未执行的正式验证**混写。

本项不否定 R3 修复代码的实测效果，但违反 R01 阶段交接要求中账本/风险/状态必须反映实际结果的约束。修复时应保留 R2 失败历史，补记 R3 修复候选与隔离预演事实，并明确该风险的剩余条件是正式提交后在真实候选上复验；随后重算 HANDOFF 对这些文件的哈希。不要提前将正式封印记为完成。

## 全阶段其余核对结果

| 范围 | 独立核对 |
|---|---|
| R00→R01 顺序与 T01–T08 身份、提交和远端链 | R00 状态 ACCEPTED。T01–T08 8 个任务的执行者与最终独立验收者身份不同；8 份最终报告 SHA-256 均与 ORCHESTRATOR_PROGRESS 相符，报告存在；8 笔任务提交均为本地/远端 HEAD 的祖先，账本各任务 `DONE` / `PASS` / `CONFIRMED`。T02、T04 后续修正和 T05–T07 修复及独立复验记录在 HANDOFF/报告中。 |
| 16 个 REQUIRED 场景、Goal/Gate/Handoff | R01-A01–A16 在 ACCEPTANCE_LEDGER 中各出现一次，任务级均 PASS；交付包含所有权/协议/锁定依赖、浏览器/PDF/Tauri 原型、旧版拒写与回滚、架构高风险关卡和覆盖检查。阶段状态仍 `READY_FOR_STAGE_REREVIEW` / FAIL，没有把 T 级 PASS 冒充阶段 PASS；实施平台以外与授权态缺口挂 R09/R10，生产入口仍隔离。HANDOFF 20 个受钉住文件 SHA-256 全部实算匹配。F01/F02 阻止阶段放行。 |
| A15 高风险关卡 | 本轮实跑自测 44/44（`/tmp/r01-r4-gate-selftest.log`）、真实 CLI 1 正+22 负（`/tmp/r01-r4-cli-regression.log`）均通过；原 R3 `R99` 反例 exit 1。新 `R099` 等反例放行，见 F01。15 个递延项的结构化 `deadline/latest` 与冻结契约和任务书 R00–R11 顺序相符；R02、R04、R08、R09/R10 归属已逐项查看，未发现纯结构化字段越界被放行。 |
| A16 覆盖、原型隔离 | 覆盖自测 6/6（`/tmp/r01-r4-coverage-selftest.log`）；隔离扫描 2209 个生产文件、零原型引用，结果 ISOLATED（`/tmp/r01-r4-isolation.log`）。既有 736 F-ID、69 存储项、14 Pi 替换项的覆盖账本与原报告一致。 |
| R3 F02 闭包实现与测试卫生 | `normalizeSourceGraphPath` 先把仓内及 node_modules 可映射路径归为相对路径，新增 `normalizeNftTraceFiles` 过滤宿主绝对/逃逸路径，符合已提交基线 8949 个逻辑路径的语义；`/bin/bash` 属系统外部可执行文件而非仓库交付字节。census 测试增加结构性断言、`finally` 恢复写入前快照。本轮单文件 `23/23` 绿（`/tmp/r01-r4-closure-census.log`），测试后候选指纹无漂移；未见以删测试或放松树净守卫取绿。其他平台实际运行未在本轮复测。 |
| R2 F03 分片交付 | 本轮两份交付测试的 6 个分片夹具用例 6/6 绿（`/tmp/r01-r4-shard-tests.log`）。R3 隔离预演日志列 round2 6 片、round3 13 片，单片 45,000,000 B 以下，最大可达 blob 97,003,232 B < 104,857,600 B；裸仓实收 SHA 与预演封印 SHA 一致。正式 GitHub 推送仍未发生。 |
| 审计封印、全量回归 | `/tmp` 隔离预演日志在纯审计坐标提交后显示四文件 74/74、全量 `npm test` 14949 passed / 0 failed / 15 skipped、lint 0 errors、typecheck 绿、工作树零漂移；预演提交 `d1cfc7f29` → `d6800d892` → `500ee91db` **不是正式坐标**。R3 报告披露主仓未封印候选仍为四文件 6 红、全量 6 红（审计坐标尚指 R00 C3）；本轮没有在主仓运行可能改写交付 gzip 的脚本，也未把预演绿写成正式 PASS。 |

## 正式提交后复验矩阵与限制

新 ZCode 修复 F01/F02 后，须由另一全新 Codex 子代理对**完整** R01 阶段做只读复验，核对新候选指纹与文档/风险/证据哈希，并证明 `R099`、`R09/R099`、`XR09` 等变体均 exit 1，同时保留 R1/R2/R3 正负电池及 16 项 REQUIRED 证据链。只有独立阶段 PASS 才可由总控按 PROGRESS.md 的 Seal 工作流冻结真实候选、生成绿色记录与两份分片交付、提交实际源/记录坐标，再以纯审计提交推进六处坐标；在真实最终提交上复验四文件门禁、全量 `npm test` 0 失败、lint/typecheck、交付哈希与分片重组、Git 可达对象 <100 MiB、工作树零漂移和远端 SHA，之后才可进入 R02。

本轮未重新运行 T04/T05/T06 可见原型、PDF 全量重渲染、Windows/Linux/macOS x64、授权态录音/屏幕、真实供应商、正式打包/签名、真实数据迁移或 GitHub 正式推送；相应结论限已有逐 T 独立报告与本轮静态/小范围复验。未修改仓库或远端，仅在 `/tmp` 写本报告及隔离反例材料。
