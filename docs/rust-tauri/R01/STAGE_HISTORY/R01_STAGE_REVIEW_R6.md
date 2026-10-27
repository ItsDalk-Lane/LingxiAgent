# R01 完整阶段独立验收 R6

- 验收者：全新 Codex 子代理 `/root/r01_stage_review_r6`；未参与 R1–R5 阶段验收、R1–R5 修复或 T01–T08 执行/验收。
- 日期：2026-09-26。分支：`codex/rust-tauri-migration`。本地 HEAD 与 `git ls-remote origin refs/heads/codex/rust-tauri-migration` 在开工、收尾均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`。本次未修改仓库、提交或推送。
- 验收对象：HEAD 上 R1–R5 未提交叠加候选。`git diff --binary` 字节直接串接按路径排序的新文件 `sha256sum` 行后取 SHA-256：**开工和结束均为 `b14cc659712942cc4df910621fd6078e4bee98ff8d5f4fdb7d7984b91e892215`**（原像 284639 字节）；18 个跟踪修改 + 57 个新文件 = 75 件。与 R5 修复报告一致；`git diff --check` 通过。
- 输入：R01 任务书及 stage-index/task-catalog/acceptance-catalog；R01_REPORT/HANDOFF/ACCEPTANCE_LEDGER/RISK_REGISTER、ORCHESTRATOR_PROGRESS；T01–T08 最终独立验收报告和进度账本；R1–R5 阶段验收、修复报告及仓内证据。R5 修复报告 SHA-256 实算 `84456ef2588388dbcf6c7419854480526c28bfc4ccbc28fffda93a5ba06abf02`，与交接一致。

**STAGE VERDICT: FAIL。** R5 已拒绝 R09.5 等虚构小阶段，但 R01-A15 对全角同形阶段文字与合法 ASCII `R09` 混用仍会放行，导致正文写出的截止/最迟阶段与机器接受的结构化阶段不一致。此项有真实 CLI 正向误放复现，违反 R01-T08 给每个高风险缺口设置可执行截止阶段及失败处理、未验证项必须挂真实 R09/R10 强制关卡的要求；阶段收口、正式封印和 R02 入口均不得据本候选放行。

## F01（BLOCKING）：全角同形阶段可隐藏在合法引用之前或「最迟」之后

检查器 `r01_t08_gate_check.py` 的 `STAGE_DISGUISE_RE = re.compile(r"R\\d+")` 只从 ASCII `R` 开始寻找；`STAGE_TOKEN_RE` 和 `LATEST_MARKER_RE` 同样只认 ASCII `R`。因此正文 `Ｒ９９` 或 `Ｒ１０`（全角 R 和数字）完全不参加提取、真实阶段索引存在性检查及「最迟」一致性检查。只要正文另有一个合法 ASCII `R09`，`tokens[0] == resolve_by_stage_id == R09` 即成立。R5 报告 §1 已披露「与合法引用混合出现时，装饰性片段不单独识别」的限制；实测表明这不是纯显示问题：可以把伪造的截止放在正文首位，也可把被放宽的最迟阶段置于明确的“最迟”后，机器仍给真实未验证高风险项 `TRACKED(R09)`。

复现只修改 `/tmp` 临时 `RISK_REGISTER.json` 副本中 `RR-T05-X1.resolve_by_stage` 一字段；`resolve_by_stage_id=R09`、`resolve_latest_stage_id=R09`、冻结契约、其他登记及输入全部保持原样。真实命令：`python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/<variant>.json`。结果：

| 正文变体 | 期望 | 实际 |
|---|---|---|
| `Ｒ９９（不存在的截止阶段）；R09（仅用于宿主集成）` | 拒绝正文首位不存在的 R99 同形标识 | **exit 0 / PASS_WITH_CONDITIONS** |
| `R09（宿主集成）最迟 Ｒ１０（虚构放宽关卡）` | 拒绝正文“最迟 R10”与结构化最迟 R09 矛盾 | **exit 0 / PASS_WITH_CONDITIONS** |
| `Ｒ０９（看似合法的截止）；R09（校验）` | 至少识别/规范化正文首个视觉阶段，避免首坐标校验失真 | **exit 0 / PASS_WITH_CONDITIONS** |
| 对照 `R09.5（虚构小阶段）`、`R09 最迟 R09.5` | 拒绝 | exit 1 / NO-GO / `risk-stage-disguised` |

这两个误放正文均与真实登记的高风险缺口绑定，并非任意孤立附言：前者改变读者理解的首次截止，后者位于检查器专设的「最迟」语法位置。大小写 `r99` 与合法 `R09` 混用也误放，但此项可视作另一个大小写契约选择；本次阻断只需上述与 R5 已处理的全角标点同族、视觉上直接混淆的全角同形字实证。`R０９`（ASCII R + 全角数字）会被 `\\d+` 提取并因非真实阶段拒绝；这恰恰表明当前 Unicode 处理不一致。

修复验收要求：保留合法真实登记正向、R1–R5 全部负向；对含全角同形阶段的正文按明确的完整标识规则拒绝，或在不改变结构化权威的前提下规范化后再做真实阶段/首坐标/最迟一致性核验；至少上述两条真实 CLI 误放须 exit 1 / NO-GO，且不能以删用例、放松冻结契约或改风险正文绕过。

## 完整阶段复核

1. **任务与身份链。** ORCHESTRATOR_PROGRESS 的 T01–T08 均 `DONE`、最终独立验收均 PASS；每项 executor_id 与 reviewer_id 不同。八份最终 review_report 文件 SHA-256 与账本记录逐项 MATCH；八个 task_commit_sha 均为当前 HEAD 祖先，八个登记的 push_result.remote_head 亦均为当前 HEAD 祖先。当前远端 SHA 与本地一致。R00 为已接受前置；R02 仍 PENDING。未把 T 级 PASS 当作阶段 PASS。
2. **16 项 REQUIRED、Goal/Gate/Handoff。** acceptance-catalog 中 R01-A01–A16 16 个 REQUIRED 与 R01_ACCEPTANCE_LEDGER 的 16 条结果一一对应，账本均有任务级 PASS、命令/退出码/证据与独立 review。所有权、协议、依赖锁、浏览器/PDF/Tauri 隔离原型、旧版拒写和回滚、职责覆盖都有各 T 报告与证据；其他平台和授权态缺口在风险登记挂 R09/R10，原型未进入生产入口。本轮实际复跑 A15 自测 **59/59**、真实 CLI 电池 **1 正 + 36 负**、真实登记 exit 0 / PASS_WITH_CONDITIONS（15 个递延项 TRACKED）；A16 覆盖自测 **6/6**、隔离扫描 2209 个生产文件且 0 原型引用。F01 是这些既有负向未覆盖的独立绕过，使 A15/阶段 Gate 不能最终放行。未重新运行需要 GUI/实际设备的 T04–T06 完整实验，沿用有指纹的任务级独立证据。
3. **风险与交接哈希。** HANDOFF `artifact_hashes` 20/20 逐文件 SHA-256 实算 MATCH，状态仍 `READY_FOR_STAGE_REREVIEW`，未虚报阶段 PASS。R4 证据目录实际 **11 件**，`/tmp/r01-repair-r4/r4-file-hashes.txt` 列出的 11 件 SHA 全部 MATCH；旧候选确为 18+47=65。R5 证据目录实际 **10 件**，现候选 18+57=75；R5 在 R01_REPORT/HANDOFF/账本/ORCHESTRATOR_PROGRESS 中的范围更正与实际相符，R4 原报告未改写。风险登记的 R2 环境漂移历史及 R3 修复后事实已区分，正式提交后验证仍列为未完成。
4. **分片与封印。** R2/R3/R4 `/tmp` 隔离预演及仓内日志支持双补丁分片后可重组、逐片最大 45,000,000 B，预演本地 bare push 可达对象最大 97,003,232 B，低于 GitHub 100 MiB 单文件上限；R3 预演审计四文件 **74/74**、全量 `npm test` **14949 passed / 0 failed / 15 skipped**。R5 当前候选在隔离副本的四文件 **68/74**、全量 **14943 passed / 6 failed / 15 skipped**，6 红与旧审计坐标未推进同集；这是封印前候选状态，不可记正式绿。R5 隔离验证 typecheck exit 0、lint 0 errors、闭包再生成零漂移。以上预演 SHA 均不是真实最终提交坐标；本轮不跑会重写 gzip 的主仓脚本、不伪造封印完成。

## 正式提交后复验矩阵（待新的阶段独立 PASS 后由总控实施）

1. 新的 ZCode 阶段修复任务收口 F01，冻结实际新候选指纹、更新 A15 真实正负日志及交接哈希；另一全新 Codex 子代理只读复验完整 R01，至少重做本报告两条全角同形字误放反例和 R1–R5 旧反例，不复用本代理。
2. 阶段独立 PASS 后，按 PROGRESS.md 对真实候选提交绑定绿色电池、round2/round3 分片重组与 VERIFIED 证据；坐标推进必须是审计白名单六文件的纯审计提交，不能以预演 SHA 代替真实提交。
3. 在真实最终提交上复验四文件 74/74、全量 `npm test` 0 failed、lint/typecheck、交付哈希/分片重组、Git 可达对象 0 个 ≥100 MiB、工作树及交付字节零漂移；再按已有授权无强推地 push 并核对远端 SHA。完成前不进入 R02。

## 限制与操作记录

仅在当前 macOS arm64 检查机器可用证据；未验证 Windows/Linux/macOS x64、真实供应商、正式包/签名、授权态录音/屏幕、真实数据迁移或 GitHub 正式 push。报告仅表示此未提交候选的阶段独立判定。主仓零写入；复现变体为 `/tmp` 临时风险登记副本，测试后初末指纹相同；未修改任何验收资产或工作树。**结论仍为 STAGE VERDICT: FAIL。**
