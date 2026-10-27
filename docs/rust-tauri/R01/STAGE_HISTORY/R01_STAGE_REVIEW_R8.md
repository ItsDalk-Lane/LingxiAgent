# R01 完整阶段独立验收 R8

- 验收者：Codex `/root/r01_stage_review_r8`，全新阶段只读复验代理；未参加 R1–R7 阶段验收、ZCode 修复、T01–T08 执行或任务验收。
- 对象：`codex/rust-tauri-migration`；本地 HEAD 与实际 `git ls-remote origin refs/heads/codex/rust-tauri-migration` 均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`。R00 基线 `328cc8bb5a807bdaad520b459907fb1fa4e10dca`。
- 开工、测试后和收尾主仓候选均为 **18 个已跟踪修改 + 77 个未跟踪新增 = 95 件**；`git diff --binary` 原字节串接按路径排序的未跟踪文件 SHA-256 清单，再取 SHA-256，原像 325,821 B，指纹均为 **`1c2bd57d3f973cc49e428fc6476af61e4ff0bbc18706702bc87ca0a2be624d29`**。与 R7 修复交接完全吻合；`git diff --check` 通过。主仓未改、未提交、未推送。
- 输入核实：原始总控要求、R01 任务书、`stage-index.json` / `task-catalog.json` / `acceptance-catalog.json`、R01 报告/交接/16 项验收账本/44 项风险登记/总控进度、PROGRESS.md Seal 工作流、R1–R7 阶段验收与修复报告。R7 验收报告实算 SHA-256 `e2bf485c725806ab8c052fec2a32a887d26611eb14e5c3df0c1201f15b3ea1c1`；R7 修复报告实算 SHA-256 `813eed988e1424a5127b75b63e50638b90d0c2e0b01e23d4e626326ff927f995`，均与任务输入一致。

## 阶段判定：FAIL

R7 原样缺陷已修复，但 **F01（BLOCKING）** 仍能在风险正文写出明确而矛盾的最迟关卡并由真实 CLI 放行。当前 R01 不能宣布阶段 PASS、不能进入 R02，也没有正式封印。四文件门禁和全量测试另有相同六项封印前红项，必须在真实提交及审计坐标推进后重新实测，不可拿旧预演代替。

## F01（BLOCKING）：带常见连接词或标点的「最迟」声明绕过逐处核验

真实登记 `RR-T05-X1` 的结构化 `resolve_by_stage_id=R09`、`resolve_latest_stage_id=R09`，检查器内冻结契约对该递延项也钉住 `deadline_stage=R09`、`latest_stage=R09`。只在 `/tmp` 风险登记副本修改该条的 `resolve_by_stage` 正文，其他字段、真实登记和冻结契约均不动。真实 CLI 实测：

| 正文变体 | exit / verdict / 类别 | 判断 |
|---|---|---|
| 原文 `R09（browser worker 边界实现与门禁）` | `0 / PASS_WITH_CONDITIONS` | 正向对照。 |
| R7 原样 `R09（宿主集成）最迟 R09；最迟 R10（虚构放宽关卡）` | `1 / NO-GO / risk-stage-text-mismatch` | R7 的 `finditer()` 修复确实生效。 |
| 顺序交换 `R09（宿主集成）最迟 R10；最迟 R09` | `1 / NO-GO / risk-stage-text-mismatch` | 方向性对照。 |
| 全角 `R09（宿主集成）最迟 Ｒ０９；最迟 Ｒ１０` | `1 / NO-GO / risk-stage-text-mismatch` | R6 折叠与 R7 逐处核验生效。 |
| **`R09（宿主集成）最迟 R09；最迟：R10（虚构放宽关卡）`** | **`0 / PASS_WITH_CONDITIONS / 无阻断类别`** | 明确的第二处最迟期限被误放。 |
| **`R09（宿主集成）最迟 R09；最迟：Ｒ１０（虚构放宽关卡）`** | **`0 / PASS_WITH_CONDITIONS / 无阻断类别`** | 全角阶段与冒号组合仍误放。 |
| **`R09（宿主集成）最迟 R09；最迟于 R10（虚构放宽关卡）`** | **`0 / PASS_WITH_CONDITIONS / 无阻断类别`** | 常见“最迟于”书写仍误放。 |
| `R09（宿主集成）最迟：R10（虚构放宽关卡）` | `0 / PASS_WITH_CONDITIONS / 无阻断类别` | 单处冒号形式也会漏检。 |
| `R09（宿主集成）最迟 R09；最迟 R09（重复但一致）` | `0 / PASS_WITH_CONDITIONS` | 多处一致应正常放行。 |
| `R09（宿主集成）；R10（后续平台事项）最迟 R09` | `0 / PASS_WITH_CONDITIONS` | 非最迟上下文正常放行。 |

**最小复现：**`python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-r8-colon-conflict.json`；预期 `exit 1 / NO-GO / risk-stage-text-mismatch`，实际 `exit 0 / PASS_WITH_CONDITIONS`。全量独立复现器 `/tmp/r01-r8-repro.py`，原始结果 `/tmp/r01-r8-cli-adversarial.tsv`，全部夹具 `/tmp/r01-r8-*.json`。

**根因与边界：**`LATEST_MARKER_RE` 只识别“最迟”后接可选空白、紧接阶段 ID 的形态；R7 把 `.search()` 换成 `.finditer()`，却没有识别“最迟：R10”“最迟于 R10”这两种明确期限声明。通用阶段扫描虽看见真实存在的 `R10`，只检查其在 R00–R11 索引中存在，既不与结构化最迟 `R09` 比较，也不将其当作强制期限。该现象直接违背当前 G6d、R7 报告和 HANDOFF 所写“正文每一处「最迟」均为强制期限声明、任一相矛盾即拒”的契约，也使 T08/A15 的高风险递延截止挂账可在交接文字中放宽。需要修复可识别的自然书写并增加真实 CLI 负向；同时保留结构化 R09 权威和重复一致、正常上下文正向。不能仅改报告文字或放宽冻结最迟关卡。

## 完整阶段核验

1. **R00 依赖与 R01 规格：**总控 R00 为 `ACCEPTED/PASS`；R01 任务书及三个目录文件一致列出 T01–T08、A01–A16，16 项均 `REQUIRED`。R01 报告、交接、账本仍为 `READY_FOR_STAGE_REREVIEW` / 上轮 `FAIL`，未冒称阶段通过。范围是协议/所有权/依赖与浏览器、PDF、系统能力、存储切换隔离原型；生产入口未切原型。44 条风险登记中 15 条高风险递延绑定，真实数据关卡输出 `PASS_WITH_CONDITIONS`、15 项 `TRACKED`；此正向不能消除 F01。
2. **八任务独立链：**总控八项均 `DONE/PASS`、推送 `CONFIRMED`；每项最终执行者与验收者 ID 不同，八份最终验收报告 SHA-256 全部实算匹配总控账本；八个任务提交均为当前 HEAD 祖先。T01 `58e00b53d9fc`、T02 `9390ae01d9e8`、T03 `5b63323b9a81`、T04 `abe4d5452ed6`、T05 `4fa0b573ce8c`、T06 `8994c67bc48f`、T07 `73bde3b5a289`、T08 `358299c1e9a7`。远端当前 SHA 已现场 `ls-remote` 核实；未重推任何候选。
3. **16 REQUIRED：**账本 A01–A16 各恰一项、均任务级 PASS，所列证据入口存在，任务提交先于当前 HEAD；A01–A02/T01、A03–A04/T02、A05–A06/T03、A07–A08/T04、A09–A10/T05、A11–A12/T06、A13–A14/T07、A15–A16/T08 分配与任务书一致。此次现场复跑 T01 所有权负向（N1–N15/exit 0），T02 跨语言 12 golden round-trip（exit 0）与 5 场景 HTTP/WS 握手（exit 0），Rust 工作区测试 45/45；其他平台/设备原型以独立 T 报告及其钉住证据复核，未冒充本轮重跑真实浏览器、授权态媒体、生产打包或跨平台。账本 A15/A16 的任务级 PASS 不能代替本轮阶段 F01 判定。
4. **R1–R7 修复与回归：**R1 冻结必需域/能力/证据/递延集合，R2 严格风险字段与状态及分片，R3 机器可验阶段绑定和闭包零漂移，R4 完整阶段标识及账本时态，R5 复合阶段边界和证据计数，R6 全角折叠与大小写，R7 多处直接“最迟 Rxx”逐处检查；本轮自测 **72/72**、真实 CLI 电池 **4 正 + 46 负** 全部通过。R1–R6 留存原样登记反例 25/25 本轮用真实 CLI 独立复跑，退出码及类别均与预期一致；R7 原样、顺序交换、全角混用均 `exit 1/risk-stage-text-mismatch`，两项正向均 `exit 0`。F01 是上述电池遗漏的明确最迟声明形态。
5. **范围与哈希：**HANDOFF 的 20 项 `artifact_hashes` 逐项现场复算一致；R7 修复目录 10 件证据均与 `/tmp/r01-repair-r7/r7-file-hashes.txt` 逐项一致，R6 目录 10 件与 R6 清单逐项一致；R7 清单内其余 85 件候选路径哈希复算一致。风险登记 44 条，原文件 SHA-256 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307` 与 R7 修复报告一致。未跟踪候选最大文件 1,261,676 B；现有 Git 可达对象 ≥100,000,000 B 为 **0 个**，最大 blob **97,003,232 B**。这是当前仓库/候选审计，正式提交和推送后的对象审计仍需重做。
6. **本轮机器门禁（`/tmp/r01-review-r8-iso` 克隆副本）：**覆盖自测 **6/6**，真实覆盖 `COVERAGE-CLOSED`（736 F-ID、11 关键事实、14 Pi 能力）；隔离检查 `ISOLATED`（生产入口零原型引用）；`npm run typecheck` exit 0；`npm run lint` exit 0，0 errors/10,896 warnings；`cargo test --manifest-path rust/Cargo.toml --workspace --offline` exit 0，45 tests passed。四文件门禁 **68/74**、全量 `npm test` **14,943 passed / 6 failed / 15 skipped**，均 exit 1；六项名称完全同集：audit-seal diff guard 1，round2 R10-03/R10-04/R10-09 3，round3 manifest/replay 2。当前审计 `VERIFIED_SOURCE_SHA` 尚指 R00 C3 `f2b8c687…`，而 R01 95 件尚未提交；round2/round3 源清单与补丁重放因此不能以现行 seal 坐标通过。R3 封印预演 74/74、14,949/0 仅是隔离彩排 SHA，不能记为本候选正式绿。

## 限制与交接

- 阶段 **FAIL 的直接原因是 F01**；六项封印前测试红另列为未满足的正式收口条件，并非本轮额外判出的 R7 新回归。主仓没有运行会重写交付 gzip 的脚本；所有可能写盘的门禁均在 `/tmp` 克隆副本执行。主仓测试前后 95 件和冻结指纹不变。
- 本轮仅 macOS arm64 环境；Windows/Linux/macOS x64、真实供应商/凭证、正式签名打包、授权态录音/屏幕、实际迁移用户数据均未本轮重做，按 R01 任务书后续强制关卡保留，不写成已验通过。
- 独立修复 F01 后须由未参与修复的新验收者重做完整阶段复验，重点真实 CLI 覆盖冒号、全角冒号后的阶段、常见连接词、R7 原样及顺序/全角、重复一致与非最迟上下文；检查冻结契约及 95 件候选新指纹。阶段 PASS 后，按 PROGRESS.md Seal 工作流把验证绑定到**实际源码提交**，验证分片整体/逐片哈希及补丁重放，再以仅六份审计文件推进坐标；在真实提交和最终 seal 上复跑四文件 **74/74**、全量 `npm test` **0 failed**、typecheck/lint/Rust/适用原型门禁、可达及实际推送对象 **0 个 ≥100 MiB**，核对远端 SHA 和工作树零漂移。不得用预演 SHA 虚报正式封印，不改 allowlist 或删除门禁。

本报告仅写 `/tmp`；主仓、远端、R02 和审计坐标均未修改。
