# R01 完整阶段独立验收 R5（Codex）

- 身份：全新 Codex `/root/r01_stage_review_r5`，未参与 R01 T 执行/验收、R1–R4 阶段验收或修复；只读审查 R1+R2+R3+R4 未提交候选。日期 2026-09-26。
- 仓库/分支：`/Users/study_superior/Desktop/Code/LingxiAgent` / `codex/rust-tauri-migration`。开始及结束时本地 HEAD 与 `git ls-remote origin refs/heads/codex/rust-tauri-migration` 同为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`，未 commit/push。
- 候选冻结：开始及结束均为 `2e3913b6d1f8f580cbd7c90e2526844bd8d68be71d5825c4fdcba6b8eb2e9309`；方法是 `git diff --binary` 字节串接按路径排序的 `git ls-files --others --exclude-standard` 文件 SHA-256 清单后计算 SHA-256，原像 268,548 字节。与 R4 修复报告自报指纹吻合。实际范围 **18 个已跟踪修改 + 47 个新文件 = 65 文件**；`git diff --check` 通过，主仓审查中无漂移。
- R4 修复报告 `/tmp/r01-stage-repair-r4.md` SHA-256 `2a5b91c02acd961d0908d8c8d1304dd77bf914b661e2247d9e270580e6a031d7`，已核对。

**STAGE VERDICT: FAIL。** R4 修复关闭了 R4 原样 `R099`/`XR09` 等反例，但同一风险正文仍可用 `R09.5`、`R09-5` 等不存在于阶段索引的伪造小阶段获得放行。R01-A15 的延期关卡仍不具备完整失败关闭；不得提交推送 R01 阶段收口、推进正式封印坐标或进入 R02。交由另一全新 ZCode 阶段修复任务处理，再由另一全新 Codex 子代理对完整 R01 独立复验。

## F01（BLOCKING）非真实小阶段 `R09.5` / `R09-5` 仍被截作 `R09` 并放行

任务书 `stage-index.json` 只定义 R00–R11；R01-T08 要求每个高风险缺口有可执行的截止阶段。真实登记 `RR-T05-X1` 当前正文为 `R09（browser worker 边界实现与门禁）`，结构化截止和最迟关卡均为 `R09`。本轮只将临时副本中该项的 `resolve_by_stage` 正文改为 `R09.5（虚构小阶段）`，其余 JSON、结构化字段及仓库文件一字节不动；调用实际 CLI：

```text
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-stage-r5/r09-dot-5.json
exit 0；VERDICT: PASS_WITH_CONDITIONS；fetch_interception_layer: UNVERIFIED -> TRACKED(RR-T05-X1 -> R09 最迟 R09)
```

`R09-5（虚构小阶段）`、`R09．5（虚构小阶段）` 同样 exit 0 / `PASS_WITH_CONDITIONS`。对照真实原始登记 exit 0；`R99`、`R099`、`R09/R099`、`XR09`、`R09X`、`R0999`、`R09_stage` 和 `R09 最迟 R099` 均 exit 1 / NO-GO。完整逐例结果见 `/tmp/r01-stage-r5/f01-real-cli-results.txt`，临时登记副本同目录。

根因：`STAGE_TOKEN_RE = (?<![A-Za-z0-9_])R\d+(?![A-Za-z0-9_])` 把点号/连字符当作合法 token 边界；`STAGE_DISGUISE_RE = R\d+` 对 `R09.5` 也只看见 `R09`。因此不存在的整体截止标识仍能在人工执行说明里冒充已机器校验的 R09。修复应在保持真实 `R09/R10`、中文标点等合法正文及 R1–R4 全套负向的前提下，明确阶段引用的完整语法，拒绝这类伪造后缀；新增真实 CLI 回归和进程内用例，检查正文、结构化 ID、冻结契约及真实 stage-index 一致。不能仅改风险文本或隐藏负向用例。

## F02（非独立阻断，证据范围账数不符）R4 修复报告多计一件证据

R4 报告在 §3、§7 写 `STAGE_REPAIR_R4/` 有 12 件、候选 18+48=66；实际该目录仅 **11 件**，`/tmp/r01-repair-r4/r4-file-hashes.txt` 的“12 件”标题下也只列 11 条 SHA，`git ls-files --others --exclude-standard` 总数 47。11 件都存在且与所列 SHA 逐项匹配，R4 修改的另 10 项文件 SHA 也逐项匹配；冻结指纹与自报值一致，未发现被点名却缺失的关键日志。该项是报告/清单计数错误，修复交接时须把实际文件范围统一更正，并说明是否原计划另有第 12 件；单独不足以否定 F01 之外的实测结论。

## 完整阶段复核摘要

1. **顺序、任务与身份**：R00 进度状态 `ACCEPTED`。已读 R01 任务书及 `stage-index.json`、`task-catalog.json`、`acceptance-catalog.json`：R01 依赖 R00，有 T01–T08 和 A01–A16，16 项均 `REQUIRED`。总控账本八项均 `DONE` / 最终 `PASS` / `CONFIRMED`；八组执行者与最终独立验收者 ID 不同；八份最终独立报告 SHA-256 与账本相符；八项任务提交均为本地/远端 HEAD 祖先。T01/T05/T06/T07 的修复轮与复验、T02 生成坐标补修复与复验、T04 文档补修复与复验均有记录。本轮复核了 R1–R4 阶段报告与修复报告的实际 SHA，全部与交接相符。
2. **16 项 REQUIRED、Goal/Gate/Handoff**：验收账本 A01–A16 各一次，均任务级 PASS、有命令、结果、日志和独立复核记录；A06 故意缺依赖的 exit 101 属预期拒绝。职责、协议、锁定依赖、浏览器/PDF/Tauri 原型、旧版拒写与回滚、A15 风险、A16 覆盖均有现行报告。R01_HANDOFF `artifact_hashes` 有 20 个真实文件哈希，逐项实算均匹配（另一个 `note` 非哈希）；阶段仍 `READY_FOR_STAGE_REREVIEW`，未虚称阶段 PASS。R09/R10 跨平台和授权态缺口继续挂账，原型仍与生产入口隔离。
3. **A15/A16 与 R4 修复**：实际运行 gate 自测 **51/51**、真实 CLI 电池 **1 正 + 28 负**；R1–R4 原样负向均被拒，真实登记为 `PASS_WITH_CONDITIONS`、15 个递延项 TRACKED。覆盖自测 **6/6**；隔离检查扫描 2209 个生产文件，生产入口零原型引用。F01 是额外独立真实 CLI 反例，证明现有 51/51 与 28 负尚未覆盖全部截止标识形态。
4. **R4 F02 文档状态**：`RR-ENV-CLOSURE-DRIFT` 和 `RR-AUDIT-SEAL-PREEXISTING` 现保留 R2 两红历史并追加 R3 零漂移/隔离预演 14949/0 事实，明确正式提交后复验；状态仍 OPEN。账本 `npm_test_note` 与 HANDOFF 未决项同样区分历史/当前，未提前宣称正式封印。R4 证据范围计数见 F02。
5. **分片与封印**：R2/R3 机制、分片夹具及隔离预演日志表明双补丁改为 45,000,000 B 以下分片，预演 6+13 片重组 VERIFIED；预演纯审计坐标后四文件门禁 74/74、全量 `npm test` 14949 passed / 0 failed / 15 skipped、lint 0 errors、typecheck 与 Rust 45/45、可达/裸仓最大 blob 97,003,232 B，裸仓实收 SHA 一致。这些是 `/tmp` 预演证据，**不是正式已提交验证**。本轮 R4 候选态日志为四文件 68/74、全量 14943 passed / 6 failed / 15 skipped；六红与旧坐标未推进相符，未误记为通过。未在主仓运行重写 gzip 的脚本或全量测试。

## 复验与收口条件

全新 ZCode 修复 F01 并更正 F02 计数、冻结完整候选，随后由全新 Codex 子代理只读审查整个 R01，重点以只改风险正文的真实 CLI 复测 `R09.5`、`R09-5`、`R09．5`、R4 原样反例以及全部既有正负用例，复核证据哈希/范围和 16 REQUIRED。阶段独立 PASS 后，才可由总控按 `PROGRESS.md` Seal 工作流绑定真实候选提交，生成/核验双补丁分片与矩阵投影，纯审计坐标提交后实跑四文件 **74/74** 与全量 `npm test` **0 failed**，复核 lint/typecheck、分片重组哈希、可达 Git 对象 **0 个 ≥100 MiB**、工作树零漂移，最后按已有授权推送并用远端 SHA 核对；预演 SHA 不能充作正式坐标。

未重做 T04 浏览器、T05 PDF、T06 桌面原型的整套现场操作或其他目标平台；这些范围沿用 T 级独立报告，本轮针对跨任务关卡及现行候选实测。未触真实用户数据、付费/真实外发或发布操作。
