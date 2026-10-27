# R01 完整阶段独立验收 R9

- 验收者：Codex `/root/r01_stage_review_r9`，全新只读阶段复验代理；未参加 R1–R8 阶段验收、ZCode 修复或 T01–T08 执行/验收。
- 日期：2026-09-26；实施环境 macOS 27.0 arm64，Node v24.16.0、Python 3.14.3。正式 Rust 复测显式使用 rustup 1.98.1（cargo 1.98.1 / rustc 1.98.1）；本机裸 cargo/rustc 实为 Homebrew 1.93.0，初次额外试跑亦通过，但不拿它代替锁定工具链结果。
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。R00 阶段基线 `328cc8bb5a807bdaad520b459907fb1fa4e10dca`；本地 HEAD、开工与收尾实际 `git -c http.proxy= -c https.proxy= ls-remote origin refs/heads/codex/rust-tauri-migration` 均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`。
- 候选开工、测试后、收尾一致：**18 个跟踪修改 + 87 个未跟踪新增 = 105 件**。算法为 `git diff --binary` 原字节串接按路径排序的未跟踪文件 `SHA-256 + 两空格 + 路径 + 换行` 清单，再取 SHA-256。原像 352,485 B；指纹始终 **`0c70bc5360375b7ea7982e33209a95666a6b75f561d6b4c2b5481b4d2b4e8845`**。原像 `/tmp/r01-r9-start-fingerprint.bin`；`git diff --check` 通过。
- R8 输入实算：`/tmp/r01-stage-review-r8.md` SHA-256 `8e926451df0ef4b9a3f03c9a50d07710493c48e795b3318950b8d130c160ebba`；`/tmp/r01-stage-repair-r8.md` SHA-256 `a0daa60cb0c98b6e323b95368f250083129fb5bf539cc4a6b180651217a4c653`，均与交接一致。
- 范围：原始总控、共同任务书、完整 R01 规格与三个机器目录，现行 REPORT/HANDOFF/ACCEPTANCE_LEDGER/RISK_REGISTER/ORCHESTRATOR_PROGRESS，PROGRESS.md 现行 Seal 流程，R1–R8 阶段评审与修复证据、各 T 最终独立报告及历史修复/交接链；R00→HEAD 已提交差异 2103 路径与本次 105 件候选。全部机器门禁及写盘测试仅在 `/tmp/r01-review-r9-iso` 克隆副本运行；独立输入/输出/报告均仅写 `/tmp`。主仓未改、未提交、未推送、未推进审计坐标或 R02。

## STAGE VERDICT: FAIL

R8 点名的冒号/连接词/全角反例均已修复，R7 重复最迟反例及既有回归也保持。但 **F01（BLOCKING）** 仍让一条明确的「原定 R09，现改 R10」最迟期限通过真实 CLI。机器只核验触发词后的首个阶段，未核验同一期限声明内的改期结果；人工交接期限与机器接受的冻结期限仍相矛盾。不得据此把 R01 标阶段 PASS 或进入 R02。

六项旧封印坐标红项独立列为未完成的正式收口条件；本轮没有把它们算正式通过，也没有把它们冒充 R8 新回归。

## F01 — 单处期限声明中的「原定→改期」仍只校验首个阶段

- 严重度：**BLOCKING（R01 阶段放行）**。
- 位置：`docs/rust-tauri/R01/r01_t08_gate_check.py:178–181`（LATEST_MARKER_RE 在首个 R/r 处停止）、`:354–377`（只检查捕获的阶段；触发词只要落在已匹配 span 内就算已解析）、`:327–334`（其余真实阶段仅检查存在性）。
- 前提：真实登记 `RR-T05-X1` 的结构化 deadline/latest 与冻结契约均保持 **R09/R09**。独立夹具只修改该条 `resolve_by_stage` 正文，其余字节、输入与契约不动。

### 真实 CLI 最小复现

```bash
python3 -B /tmp/r01-review-r9-iso/docs/rust-tauri/R01/r01_t08_gate_check.py \
  --register /tmp/r01-r9-one-trigger-correction.json \
  --out /tmp/r01-r9-one-trigger-correction.out.json
```

正文：`R09（宿主集成）最迟（原定 R09，现改 R10）完成`。

**预期：exit 1 / NO-GO**，矛盾期限或无法确定唯一一致期限时拒绝。

**实际：exit 0 / PASS_WITH_CONDITIONS**；`blocking_reasons=[]`；`browser_host=COMPLETE`、`problems=[]`；该递延项仍显示 `TRACKED(RR-T05-X1 -> R09 最迟 R09)`。这不是只调用内部函数的反例，而是完整 Python CLI 的实际输入、输出及退出码。

### 同根因矩阵

| 只改正文的变体 | 实测 exit / verdict | 判断 |
|---|---|---|
| 原文 `R09（browser worker 边界实现与门禁）` | 0 / PASS_WITH_CONDITIONS | 正向。 |
| `R09（宿主集成）最迟（原定 R09，现改 R10）完成` | **0 / PASS_WITH_CONDITIONS** | 同一最迟期限明确改至 R10，误放。 |
| `R09（宿主集成）最迟由原定的 R09 顺延至 R10 完成` | **0 / PASS_WITH_CONDITIONS** | 明确顺延，误放。 |
| `R09（宿主集成）最迟在 R09 或 R10 完成` | **0 / PASS_WITH_CONDITIONS** | 可选择更晚期限，未失败关闭。 |
| `R09（宿主集成）最迟 R09/R10 完成` | **0 / PASS_WITH_CONDITIONS** | 非唯一期限被首阶段代表，未失败关闭。 |
| `R09（宿主集成）最迟 R09；最迟 R10` | 1 / NO-GO / risk-stage-text-mismatch | R7 修复仍有效。 |
| `R09（宿主集成）最迟 R09；最迟：R10` | 1 / NO-GO / risk-stage-text-mismatch | R8 修复仍有效。 |
| `R09（宿主集成）最迟 R09；最迟 R09` | 0 / PASS_WITH_CONDITIONS | 多处一致正向。 |
| `R09（宿主集成）；R10（后续平台事项）最迟 R09` | 0 / PASS_WITH_CONDITIONS | 普通上下文正向。 |

**根因：**连接段扩展与 unresolved 只覆盖「触发词→首个阶段」之前的形态。找到首个合法 R09 后，标记结束、触发词被视为已解析；后面的“现改 R10”“顺延至 R10”不再属于任何被核验标记。通用扫描看到 R10，却因它存在于阶段索引而通过。因而 `finditer()` 可以覆盖多个触发词，仍无法保证一个真实期限声明内部完整一致。

**违反要求：**R01-T08 要给每个高风险缺口设置后续可执行截止阶段；阶段 Gate 依赖未验证项的强制关卡。当前 G6d 及 HANDOFF 又承诺每一处显式期限必须与结构化最迟一致、不能以无法核验的期限放行。上述两条改期正文含实现明确认可的“最迟”触发词、合法真实阶段、同一子句、短连接段；R10 明确属于期限变更，不能当普通后续平台事项。这也不属于 R8 文档所列“不含触发词的裸阶段后缀”边界。修复者书面声明机器语法只看首阶段，并不能豁免实际交接中的矛盾期限。

**后果：**人工接手者读到的强制期限为 R10，检查器却给高风险浏览器边界 `TRACKED(R09)` 并允许下一阶段；A15 的风险截止期一致性仍不能保证。

**修复要求及同类面：**由全新修复代理检查单个期限子句中的多阶段/改期/选择表达，明确机器可接受的完整期限书写；无法确定唯一一致期限时拒绝。无需无限枚举所有自然语言同义词，也不能仅把本报告字面量列黑名单。保留正常上下文提及其他阶段、多处一致、R08/R09 等原本合法非期限引用；冻结 deadline/latest 不放宽，不改真实风险正文掩盖问题。至少上述两条明确改期须真实 CLI exit 1，并覆盖“或”/斜杠及全角变体、保持 R1–R8 电池和全阶段回归。

原始复现器 `/tmp/r01-r9-adversarial.py`，25 种扩展正文的逐条结果 `/tmp/r01-r9-adversarial.tsv`，输入/输出 `/tmp/r01-r9-*.json`。夹具只位于 `/tmp`。

## 其余边界复核与非阻断观察

1. **R8 原样与扩展：**全角冒号、冒号后全角阶段、“最迟于”、单处冒号均 exit 1/risk-stage-text-mismatch；R7 原样、顺序交换、全角混用均同类拒绝。自测 83/83；真实 CLI 电池 6 正+54 负通过。独立测试连接段 16 字符后 R10 被 mismatch 拒绝，17 字符、连接段含 r/R、跨换行均由 risk-stage-latest-unresolved 拒绝，失败关闭机制没有在这些边界失效。连接形态一致、否定连接式一致和普通 R10 上下文均通过。
2. **触发词误拒的实际限制：**`R09 最迟 R09 完成；R10 文档介绍“最迟”字段` 实测 exit 1/risk-stage-latest-unresolved。这是引用词名而非第二个期限声明，说明当前触发词扫描也没有区分普通说明语境。该限制没有改变真实登记本轮结果，不单独列为阶段阻断；修复 F01 时应避免把所有阶段提及或所有文字出现一概当期限，至少保留现有普通上下文正向。没有因为“边界已文档化”而把 F01 的明确改期视作无问题。
3. **范围数字更正（非 material）：**R8 修复报告 §0 说“R7 候选 85 件清单中 76 件未触碰”指的是 r7-file-hashes.txt 主表 85 项；其 §2 又写“R7 候选 95 件中 76 件未动”，范围措辞不准确。该文件末尾另列 R7 新增证据 10 项。独立解析主表与末尾两部分并逐文件复算：**完整 R7 候选 95 项 = 85 主表 + 10 R7 证据；R8 改 9 项，保留 86 项 = 76 主表 + 10 R7 证据**。95 项无缺失、R7 十件新增全部哈希一致；R8 19 件触碰清单全部哈希一致，当前总候选 105 件正确。无漏交/隐藏修改，不把这处措辞当额外 BLOCKING。逐文件证据 `/tmp/r01-r9-r7-preservation.json`；原 R7/R8 报告未改写。

## 完整 R01 组合阶段覆盖

### 前置、Task 身份/报告/提交/远端

- R00 在现行总控为 ACCEPTED/PASS；R01 为 READY_FOR_STAGE_REREVIEW、stage_verdict=FAIL、review_round=8；R02 及后续仍 PENDING。
- 三个原始规格目录一致给出 T01–T08、A01–A16，16 项全部 REQUIRED，原始要求未被改写。Task 依赖顺序及成对场景映射一致。
- 八任务均 DONE/PASS、push CONFIRMED；八个 executor 与各自最终 reviewer 身份不同；最终八份独立报告的 SHA-256 全部实算匹配进度账本，登记的历史 review/repair/post-pass report hash 同样匹配。八个主 Task 提交、记录的 push head、T02 后续修复提交均为当前实际本地/远端 HEAD 祖先。材料能证明登记的独立身份链及提交/远端链，本轮不把材料中的主观“完成”当额外 PASS。
- T01 `58e00b53d9fc`、T02 `9390ae01d9e8`、T03 `5b63323b9a81`、T04 `abe4d5452ed6`、T05 `4fa0b573ce8c`、T06 `8994c67bc48f`、T07 `73bde3b5a289`、T08 `358299c1e9a7`。机器记录 `/tmp/r01-r9-task-chain.json`。

### 16 REQUIRED 的逐项判定依据

账本中 A01–A16 各恰一条、均任务级 PASS，有命令、预期、观察、退出码、证据和独立复核。全部证据指针存在，tested_sha 的实际提交坐标均为 HEAD 祖先；A15/A16 的 tested_sha 字符串附带历史说明，按其明确 40 位提交前缀核对，未拿它当本次新候选 SHA。

| 场景 | 本轮检查及实际证据适用范围 |
|---|---|
| A01 核心不依赖桌面 | T01 独立 R3 的 metadata/无 UI 构建证据；本轮锁定工具链 workspace 45/45、所有权正负检查通过，原型隔离未把桌面加入核心。 |
| A02 双负责人检出 | 本轮实际跑 T01 --self-test，N1–N15 逐条拒绝、RESULT OK；关键事实集合/写者闭合保持。 |
| A03 跨语言 round-trip | 本轮 rustup 1.98.1、全新隔离 target 实跑 12 golden Rust→TS→Rust 字节一致及 TS 类型检查，exit 0。 |
| A04 版本不兼容诊断 | 本轮真实 loopback HTTP/WS 五场景，400/4409 显式 version_incompatible，exit 0；transcript 独立落 /tmp。 |
| A05 锁可复现 | 当前三份锁 hash 与 HANDOFF 精确相同；本轮 --locked --offline workspace 成功，锁未改变；T03 独立干净 CARGO_HOME 证据保留。 |
| A06 缺依赖不掩盖 | T03 独立移除 libsqlite3-sys 的 exit 101/恢复 exit 0 证据及源码分支核对；本轮未重新移除依赖，不把未重做写成实跑。 |
| A07 浏览器等价 | T04 原型/旧侧/用户接管独立实测及 ADR-002 对照；T04 原始 51 项 + docfix 12 项证据全量哈希通过。未本轮重开 GUI。 |
| A08 会话/不可信页隔离 | T04 B1–B6 与 ADV-1..8 独立报告及当前证据一致，浏览器 core 源码未被阶段修复改动；未本轮重跑真实浏览器。 |
| A09 中文长文档完整 | T05 compare/matrix 的真实旧/新双链证据及独立 R2 报告；根 632 项、repair 653 项全量证据 hash 一致。未本轮重渲染。 |
| A10 危险资源/失败清理 | T05 独立 WS/CSP/canary/无限脚本超时与 cleanup 证据；与 ADR-003/R09 未决 WebRTC 条目一致。未本轮重启 renderer。 |
| A11 自定义命令受限 | T06 独立 R2 的三产物 main/untrusted/remote ACL 表及源码；本轮 lint 修复为真实环境声明和等价 if 改写，未删权限断言。未本轮重建 GUI。 |
| A12 测试能力不进 release | T06 独立 release 无监听/无 wdio 入口、cfg(feature) 证据；六个历史构建二进制已按 T08 disposition 删除，哈希和运行记录保留。本轮没有实物重新探测。 |
| A13 旧程序拒写 | T07 独立 epoch 高位/半途/逃生口/分离根探针及 ADR-004 实测边界，生产 corrupt fail-open 缺陷已明确挂 R02。本轮仅复核历史证据，不宣称下载旧安装包重跑。 |
| A14 回滚保留新增数据 | T07 独立 WAL 热态第五消息/Online Backup/归档/幂等再导入演练；90+89 项证据 hash 全过，未用真实用户数据。 |
| A15 高风险不被演示遮蔽 | 本轮真实数据五域 COMPLETE、15 递延 TRACKED；83/83、CLI 6正54负及旧反例通过。**新的 F01 使阶段级期限一致性 FAIL，历史任务级 PASS 不能覆盖它。** |
| A16 目标职责闭合 | 本轮覆盖 6/6 自测和真实 COVERAGE-CLOSED：736 F-ID、69 store、11 核心事实、14 Pi 能力、19 worker 反例闭合，无第二 Agent loop 保留。 |

### 交接、证据真实性和范围

- HANDOFF 的 20 个 artifact_hashes 逐项实算一致；三份 dependency_locks 精确匹配。RISK_REGISTER 44 条，15 个关卡递延绑定的结构化截止/最迟与冻结契约及原任务阶段索引一致；问题在 F01 的正文语义核验，并非结构化坐标被改。
- T03 SHASUMS 48 项中当前 43 项一致，5 项由后续 T04/T06 合法更新；这五项在 T03 实际提交 `5b63323b…` 逐项匹配原 hash，属历史快照非当前损坏。T06 57+114 项清单中 6 个二进制缺失均有 T08 disposition 与原 hash 记录，其他 55+110 项当前一致；未冒称六二进制仍存在。T04 51+12、T05 632+653、T07 90+89 全部当前一致。审计结果 `/tmp/r01-r9-evidence-manifests.json`。
- 生产路径 desktop/server/core/lib/shared/cli/hub/plugins/skills2set/package*.json 相对 R00 基线无变化；隔离检查扫描 2209 生产文件，209 原型跟踪文件，生产目录与生产入口零原型引用。原型仍只证明选型可行，未被说成 Rust 内核或 Tauri 正式迁移已完成。
- R1 冻结能力/证据/递延全集，R2 严格类型/状态/登记本体与分片，R3 结构化阶段绑定及闭包卫生，R4 token 与账本时态，R5 复合标识及计数更正，R6 全角/大小写，R7 逐触发词，R8 连接段/触发词族/unresolved：源码与报告关系一致，没有删旧负向或降低冻结 deadline/latest。R1–R6 的 25 个留存 CLI 夹具及 R7 的 13 个留存 CLI 夹具本轮重新实际调用，38/38 退出码和首阻断类别按历史预期保持（含三项与四项正向），结果 `/tmp/r01-r9-old-repros.json`。

## 本轮隔离机器门禁

所有日志在 `/tmp`；副本创建自当前真实 HEAD，再逐字节复制 105 候选路径，node_modules 仅 APFS 复制一次。测试结束后 105 个候选文件与主仓逐项 hash 零差异，build/ 无差异。主仓没有运行会重写交付 gzip 的脚本。

| 检查 | 实际退出码/结果 | 原始证据 |
|---|---|---|
| A15 self-test | 0，83/83 | /tmp/r01-r9-gate-selftest.log |
| A15 真实 CLI 电池 | 0，6 正+54 负 | /tmp/r01-r9-gate-cli.log |
| A16 自测/真实覆盖 | 0，6/6 与 COVERAGE-CLOSED | /tmp/r01-r9-coverage-selftest.log / coverage.log |
| 原型隔离 | 0，ISOLATED | /tmp/r01-r9-isolation.log |
| T01 自测 | 0，N1–N15 | /tmp/r01-r9-ownership.log |
| T02 锁定 round-trip/handshake/generated | 均 0，12 golden/5 场景/56 文件+624 兼容项 | /tmp/r01-r9-roundtrip-locked.log / handshake-locked.log / generated.log |
| Rust workspace 锁定工具链 --locked --offline | 0，19 test-result、45 passed / 0 failed | /tmp/r01-r9-rust-locked.log |
| npm run typecheck | 0，tsc×3 | /tmp/r01-r9-typecheck.log |
| npm run lint | 0，0 errors / 10896 warnings | /tmp/r01-r9-lint.log（完整输出） |
| 闭包真实再生成 | 0，8949 文件，build/ 零漂移 | /tmp/r01-r9-closure.log |
| 四文件门禁 | **1，68 passed / 6 failed，总 74** | /tmp/r01-r9-fourfile.log |
| 全量 npm test | **1，14943 passed / 6 failed / 15 skipped，总 14964** | /tmp/r01-r9-npm.log |

四文件和全量的六个失败完全同集：post-verification audit-seal diff guard 1；round2 R10-03/R10-04/R10-09 3；round3 source manifest/现场 replay 2。upstream-sync-matrix 7/7；分片相关新增用例未失败。当前 verified-source-sha 仍指 R00 C3 `f2b8c687…`，HEAD 已包含 R01，且 105 件修复候选尚未提交；source manifest/tree==HEAD/replay 保护器按设计拒绝。它们是实际红灯，未通过正式 seal。R3 隔离预演 74/74、14949/0 是另一棵彩排树，不能移用成本候选正式证据。

候选新文件最大 **1,261,676 B**；当前 `git rev-list --objects --all` 可达 blob ≥100,000,000 B **0 个**，最大 **97,003,232 B**。R2/R3 分片方案与测试仍保留逐片/重组 hash、现场 replay 和恢复约束，未扩白名单或退役门禁。本轮没有重生成正式巨大交付物，也没有正式 GitHub push，因此对象结果仅适用于当前候选/现有可达集；真实收口后的对象与远端必须重审。

## 适用的真实提交后封印复验矩阵

本轮 **FAIL**，以下是必要收口条件，未执行或宣称完成：

1. 由全新修复代理关闭 F01；新增真实 CLI 单条期限内改期/选择表达负向，保持 R1–R8 原样负向、多处一致及普通上下文正向，给新候选指纹和逐文件证据。另一全新阶段验收代理重新验完整 R01，不能只复验本 finding。
2. 阶段独立 PASS 后，总控按既有授权及 PROGRESS.md Seal 工作流提交实际冻结源码候选；把适用绿色电池绑定实际提交、lockfile、工具链、配置、fixture、平台和产物 hash。
3. 在该实际源码提交上重冻结 round2/round3 清单/绿色记录/分片交付；逐片及整体 hash 相符、单片尺寸受控、现场重放 source/seal 两状态 VERIFIED，交付测试保持树净守卫及失败时恢复。不得照搬 R3 预演 SHA 或旧未提交指纹为正式坐标。
4. 以既有六份审计文件的纯审计提交推进实际已验证坐标，生成矩阵投影，审计 guard 只允许当前白名单；不扩白名单、不改需求、不删除正确测试。
5. 在真实最终 source/evidence 与 seal 提交上复跑四文件 **74/74**、全量 npm test **0 failed**、typecheck/lint、Rust 锁定工具链和适用协议/所有权/风险/覆盖/隔离门禁；闭包再生成和工作树/交付字节零漂移。新增失败或平台差异单独调查，不以“预期红”免验。
6. 正式收口提交后重新审查候选新增文件、可达及实际推送对象，**0 个 ≥100 MiB**；按授权非强推推送，现场 ls-remote 校验真正远端 SHA；记录工作树没有本次未提交遗留。最终实际远端/封印及验收证据齐全前不进入 R02。

## 本轮限制与结论

仅 macOS arm64 本轮实测；Windows/Linux/macOS x64、真实供应商/凭证、正式签名/安装产物、授权态录音/屏幕、真实用户数据迁移未重做。T04/T05/T06 可见原型和 PDF 全量渲染未本轮重做，采信未被本次改动影响的已钉住 T 级独立报告/证据，并明确其原平台/时点限制；T06 历史二进制已按既定处置删除，不能声称现场实物复查通过。没有对当前无法访问的真实环境伪造 PASS。

本轮直接阶段阻断为 F01；范围计数措辞与触发词引用误拒按上文实证披露，不掩盖、不重复放大。六项正式封印红另列未满足收口条件。开工及收尾主仓候选 105 件和指纹完全相同，实际远端未变；本报告及所有新测试材料仅位于 `/tmp`。

**STAGE VERDICT: FAIL。**
