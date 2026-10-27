# R01 完整阶段独立验收 R7

- 验收者：Codex `/root/r01_stage_review_r7`，全新只读阶段复验代理；未参与 R1–R6 阶段验收、ZCode 修复或 T01–T08 执行与验收。
- 范围：`codex/rust-tauri-migration` 上 HEAD `363999378482dfed42733a3c4e8ad20ac82fd0fc` 与 R1–R6 未提交叠加候选。开工与收尾均核对 `git ls-remote origin refs/heads/codex/rust-tauri-migration` 同 SHA；仓库未改动、未提交、未推送。
- 冻结指纹：开工与收尾均为 **`04dea823b8ff24703241ec2d6128efcb991586565d585e9249e5f61044b3999f`**。算法：`git diff --binary` 原字节串接按路径排序的未跟踪文件 SHA-256 清单，再 SHA-256；原像 307,095 B。范围为 **18 个已跟踪修改 + 67 个新文件 = 85 件**，与 R6 修复报告一致；`git diff --check` 通过。
- 输入：原始总控要求、R01 任务书及 `stage-index.json` / `task-catalog.json` / `acceptance-catalog.json`，R01 报告、交接、16 项验收账本、44 条风险登记、总控进度，T01–T08 最终独立验收与提交记录，R1–R6 阶段验收/修复报告和仓内证据。R6 修复报告 `/tmp/r01-stage-repair-r6.md` 实算 SHA-256 `d33659ba0798822a310c2ee1467272e9e4b98c49a439b9efe182dc37539458e8`，与交接一致。

**STAGE VERDICT: FAIL。** R6 修复关闭了全角同形阶段的原样反例；但高风险递延项的正文可出现两个互相矛盾的「最迟」关卡，检查器只校验第一处。第二处写出更晚的真实阶段时仍给出 `PASS_WITH_CONDITIONS`，风险交接正文与机器接受的最迟期限不一致。阶段不得据此收口或进入 R02；正式 seal 未执行、未获本轮 PASS。

## F01（BLOCKING）：重复「最迟」中第二个相矛盾的期限被忽略

任务书 R01-T08 要求给每个高风险缺口设置可执行截止阶段与失败处理；R01 Gate 要求跨平台等未验证项挂 R09/R10 强制关卡。当前登记的 `RR-T05-X1` 冻结截止与最迟均为 `R09`。只在 `/tmp` 的登记副本中修改这条风险的 `resolve_by_stage` 正文，结构化 `resolve_by_stage_id=R09`、`resolve_latest_stage_id=R09` 和其他字段保持原样，真实 CLI 结果如下：

| RR-T05-X1 正文 | 真实 CLI | 解释 |
|---|---|---|
| 原文 `R09（browser worker 边界实现与门禁）` | exit 0 / PASS_WITH_CONDITIONS | 正向对照。 |
| `R09（宿主集成）最迟 R10（虚构放宽关卡）` | exit 1 / NO-GO / `risk-stage-text-mismatch` | 单处矛盾正确拒绝。 |
| **`R09（宿主集成）最迟 R09；最迟 R10（虚构放宽关卡）`** | **exit 0 / PASS_WITH_CONDITIONS** | 第二个「最迟 R10」与权威最迟 R09 冲突却误放。 |
| `R09（宿主集成）最迟 R10；最迟 R09` | exit 1 / NO-GO / `risk-stage-text-mismatch` | 只检查首处的方向性对照。 |
| `R09（宿主集成）最迟 Ｒ０９；最迟 Ｒ１０` | exit 0 / PASS_WITH_CONDITIONS | R6 全角折叠后同样误放，不是漏折叠。 |
| `R09（宿主集成）；R10（后续平台事项）最迟 R09` | exit 0 / PASS_WITH_CONDITIONS | 单纯提及 R10 的正常正文不应因本问题被拒。 |
| `R09（宿主集成）最迟 R09；最迟 R09（重复但一致）` | exit 0 / PASS_WITH_CONDITIONS | 两处期限一致的正向对照。 |

复现登记在 `/tmp/r01-r7-R7-double-latest-conflict.json` 等 `/tmp/r01-r7-*.json`。复现命令：

```bash
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-r7-R7-double-latest-conflict.json
```

根因：`r01_t08_gate_check.py` 第 290–303 行对折叠后的正文使用 `LATEST_MARKER_RE.search(folded)`，只校验第一个匹配；`_extract_stage_references` 虽提取第二处的 `R10`，但它真实存在于 R00–R11，只有未知阶段核验，不会核对第二个「最迟」与结构化 `R09`。这是正文明确给出相互冲突的强制期限，不属于普通上下文提及 R10。应由全新的 ZCode 修复任务决定实现方式，并用真实 CLI 覆盖首处一致/后处冲突、顺序交换、全角混用、正常上下文提及及多处一致的对照；不得把结构化权威期限放宽至 R10。

## 完整阶段复核

1. **顺序、代理与提交链**：R00 在总控账本为 `ACCEPTED/PASS`。R01 依赖 R00，有 T01–T08、A01–A16（16 项均 `REQUIRED`）。八项任务均 `DONE`、最终任务独立验收 `PASS`、推送账本 `CONFIRMED`；每项执行代理与最终验收代理身份不同。八份最终验收报告 SHA-256 与总控账本逐项匹配，八个任务提交均为当前本地/远端 HEAD 的祖先。R1–R6 阶段验收及修复报告均存在；本轮只对候选做阶段只读验收，不继承先前阶段 PASS。
2. **Goal/Gate/Handoff 和范围**：现行报告与交接载明协议源、职责、依赖锁、浏览器/PDF/Tauri 隔离原型、存储回滚规则，以及 R09/R10 未验证项强制关卡；生产入口未切至原型。16 项验收账本各出现一次，均为任务级 `PASS`，其报告路径存在。`R01_HANDOFF.artifact_hashes` 的 20 项文件哈希逐项复算一致；R6 证据目录有 10 个 `.txt` 文件，候选总数 85。交接和进度仍写 `READY_FOR_STAGE_REREVIEW`、`stage_verdict=FAIL`，未冒称阶段通过。F01 说明 A15 高风险挂账期限核验还不闭合。
3. **R6 原问题及既有反例**：本轮实际运行 `--self-test` **67/67**、真实 CLI 电池 **2 正+43 负**；R6 原样 `Ｒ９９…；R09` 被 `risk-stage-unknown` 拒绝，`R09…最迟 Ｒ１０` 被 `risk-stage-text-mismatch` 拒绝，`Ｒ０９…；R09` 规范化正向通过；R5 `R09.5` 和 R4 `R099` 原样回归仍分别被 disguised/unknown 拒绝。代码中全角 1:1 折叠保留原文 span，结构化字段仍只接受 ASCII 精确阶段，大小写/复合标识的既定边界未被本轮发现问题。F01 是新的明确期限冲突，既有电池未覆盖。
4. **覆盖、隔离与风险**：本轮覆盖自测 **6/6**、真实覆盖 `COVERAGE-CLOSED`（736 个 F-ID、11 个关键事实、14 项 Pi 能力）；隔离检查 `ISOLATED`（扫描 2,209 个生产文件，209 个原型文件，生产目录零原型文件/生产入口零原型引用）。风险登记实际 44 条；15 个递延绑定项的真实正向关卡为 `PASS_WITH_CONDITIONS/TRACKED`。Windows/Linux/macOS x64、授权态媒体/真实供应商及生产替壳均未由本轮验证，不把它们记为通过。
5. **分片与封印**：已读 R2/R3 分片实现、夹具及隔离预演记录。两个超 100 MiB 的 gzip 交付物在预演中改为 45,000,000 B 以下分片，预演 round2 6 片、round3 13 片可重组并 VERIFIED；本地裸仓推送/可达对象最大 97,003,232 B。R3 `/tmp` seal 预演在暂存提交上四文件门禁 **74/74**、全量 `npm test` **14949 passed / 0 failed / 15 skipped**。R6 候选隔离副本日志则是封印前四文件 **68/74**、全量 **14943 passed / 6 failed / 15 skipped**；六红与旧审计坐标未推进同集，不能当正式 0 失败。R6 隔离日志另示闭包再生成 8,949 文件零漂移、census 23/23、typecheck 通过、lint 0 errors。预演 SHA 不是可供正式交接的实际最终提交 SHA。未在主仓运行重写 gzip 的测试或生成器。

## 修复交接与正式收口条件

1. 由全新的 ZCode R01 阶段修复任务处理 F01，保留已有候选、风险与审计边界；产出报告、范围和候选指纹。再派与 R1–R7 不同的全新 Codex 子代理重新独立审查完整 R01，包括 F01 的真实 CLI 正负对照及所有 16 REQUIRED、证据与候选指纹。
2. 独立阶段 PASS 后，总控才可按 `PROGRESS.md` 的 Seal 工作流提交真实候选、把验证绑定到实际源码提交、生成并核验 round2/round3 分片及矩阵，再以仅六份审计文件推进坐标。正式提交后重跑四文件门禁 **74/74**、全量 `npm test` **0 failed**、typecheck/lint 与适用 Rust/原型门禁，核对每片与整体哈希、Git 可达及实际推送对象 **0 个 ≥100 MiB**、工作树零漂移；最后按既有授权推送并用远端 SHA 核对，方可进入 R02。环境或封印红项必须如实列出，不用预演替代正式验证。

本轮仅写本 `/tmp` 报告和 `/tmp` 风险登记副本；仓库文件、远端和 seal 坐标均未改变。
