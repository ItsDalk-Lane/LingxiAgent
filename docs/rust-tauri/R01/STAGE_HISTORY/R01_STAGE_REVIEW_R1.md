# R01 阶段独立验收 R1

- 审查者：Codex `/root/r01_stage_review_r1`（全新阶段验收子代理；只读主仓）
- 基线：R00 正式 C4 `328cc8bb5a807bdaad520b459907fb1fa4e10dca`
- 候选：本地及远端 `363999378482dfed42733a3c4e8ad20ac82fd0fc`
- 分支：`codex/rust-tauri-migration`；审查前后主仓工作区干净
- **STAGE VERDICT: FAIL**

## 核对范围与已证实部分

读取用户总控授权、共同任务书、R01 阶段规格、16 个 REQUIRED 场景、R00 交接、R01 八项执行及各轮独立验收报告、阶段报告/交接/风险/验收账本，以及 `328cc8bb..36399937` 的实际提交与源码。差异共 2103 个文件，集中于隔离原型、生成协议、文档和证据；`desktop/`、`server/`、`core/`、`lib/`、`shared/`、`package.json`、`package-lock.json` 无变化，生产入口隔离检查扫描 2209 个文件，零原型引用。R00 的功能清单 736 项、存储 69 项、Pi 替换项 14 项均在 R01 所有权表双向闭合。四份 ADR、协议版本与 Rust 工具链、宿主原型及数据切换设计均有真实交付；跨平台与授权态能力明确列为后续关卡，未伪称本阶段通过。

T01–T08 均有不同的 ZCode 执行/独立验收代理 ID、执行报告、最终 PASS 报告与独立可核的 SHA-256；T01/T05/T06/T07 的修复轮亦有新的代理和复验报告。八项主提交及 T02 补修复均为当前 HEAD 祖先；远端 SHA 等于本地 HEAD。Git 远端跟踪 reflog 记录了 T04、T05、T06、T07 对应各阶段的独立 push（分别到 `5a8a8e24a`、`76bd42c43`、`82870879d`、`2bbec6d07`），因此进度账本中这四项 `push_result: null` 是记录缺口，**并不表示未推送**。T08 独立报告 SHA 与账本相符，主提交 `358299c1e` 已由后续远端提交包含。

## 本代理实际重跑

| 检查 | 实际结果 |
|---|---|
| R01-A15 真实输入及自测 | 真实输入 `PASS_WITH_CONDITIONS`、五域 COMPLETE、15 递延项已挂账；自测 5/5（exit 0） |
| R01-A16 覆盖及自测 | `COVERAGE-CLOSED`，736 F-ID/69 store/14 Pi 项闭合；自测 6/6（exit 0） |
| T01 所有权检查和生成物 | exit 0；无双 owner / 未登记模块；736/69 up-to-date |
| T02 生成物检查 | 全新隔离 `CARGO_TARGET_DIR`，56 文件与 624 条兼容接口零漂移（exit 0） |
| T02 round-trip | 12 golden Rust→TS→Rust 字节一致、TypeScript 类型检查通过（exit 0） |
| T02 HTTP/WS 握手 | loopback 5 场景，版本过新/过旧明确拒绝（exit 0） |
| Rust workspace 单测 | 锁定 1.98.1、离线、全新隔离 target，45 passed / 0 failed（exit 0） |
| T08 原型隔离 | 2209 生产文件扫描，0 违规（exit 0） |
| 当前四文件审计证据门禁 | **62 passed / 6 failed**，exit 1；原始输出在 `/tmp/r01-stage-review-r1-seal-gate.log` |

未在本轮重建 T04 浏览器、T05 PDF、T06 Tauri 桌面原型的完整本机产物，也未使用真实账号/用户数据或验证其他平台；各任务已有独立原型报告，当前阶段的新增关卡和跨任务契约由上述实测覆盖。完整 `npm test` 未在主仓重跑，因为四文件证据门禁已明确失败，且全量测试有生成证据的副作用；不能因此将全量测试记作 PASS。

## F01（BLOCKING）高风险关卡允许删掉必需能力后通过

- 位置：`docs/rust-tauri/R01/r01_t08_gate_check.py` 的 `evaluate()`，遍历 `inputs.get("areas", {})` 和各域 `required_capabilities`，但没有独立的必需域/能力闭合集合；`sha256` 校验也只有字段存在才执行。
- 独立复现：在 `/tmp` 建立 R01 真实输入副本，只删除 `browser_host.required_capabilities` 中的 `user_takeover`，以 `--inputs` 调用实际 CLI，返回 **exit 0 / browser_host COMPLETE / 总体 PASS_WITH_CONDITIONS**；将 `areas` 整体改为 `{}`，返回 **exit 0 / PASS**；删除一项必需能力的 `sha256` 仍返回 exit 0；删除已挂账递延项仍返回 exit 0。以上只改内存/临时文件，主仓未改。
- 为什么违反目标：R01-A15 明确要求“截图成功但用户接管失败”不可宣告浏览器完整；T08 §4 要求高风险缺口不可被演示遮蔽。当前实现能拒绝 `user_takeover: FAILED`，却可以通过**删掉该项**绕过同一门禁。报告中“逐项验证五域必需能力与证据哈希”的承诺不成立。
- 根因与同类面：检查器把需要验证的能力全集也交给被检查输入自己声明，且未验证证据哈希必填；五域、各必需项、递延项均受同一缺口影响。
- 修复要求：由独立于被检查输入的冻结契约定义必需域和能力 ID；集合必须精确闭合，不得缺项/重复/改名；必需项证据路径及合法 SHA-256 必填并核验；递延项的闭合与合法状态同样校验。保留现有正向证据，增加删域、删 `user_takeover`、删 hash、删递延项的负向回归，再以新 ZCode 修复代理和新 Codex 独立复验完整阶段。

## F02（BLOCKING）阶段证据仍停留在 T08 执行前，审计失败分类与真实基线相反

- 位置：`docs/rust-tauri/R01/R01_REPORT.md`、`R01_HANDOFF.json`、`R01_ACCEPTANCE_LEDGER.json`、`RISK_REGISTER.json`、`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`。
- T08 独立验收已 PASS、提交并推送，实际 HEAD `36399937`；但阶段报告仍称 T08 “未提交/待总控另派”、A15/A16 为 `READY_FOR_REVIEW`；交接的 T08 `commit=null`、`review_evidence=PENDING`；验收账本两项为 `READY_FOR_REVIEW`、`review=null`。交接 `source_sha=2bbec6d07` 和 `working_tree_digest=239acceb...` 是历史候选快照（8686 行清单哈希可复算），却没有新增“已独立接受并推送的阶段候选 SHA”。当前 `ORCHESTRATOR_PROGRESS.current_head=f783a8e8e` 与本地/远端 `36399937` 不同；T04–T07 `push_result=null` 与实际推送历史不符。
- 审计分类更严重：R01 风险登记声称 `RR-AUDIT-SEAL-PREEXISTING` 原样承继 R00，R01 报告称“四项预存 FAIL”。但 R00 **正式 C4 基线** 的提交后报告 `/tmp/r00-seal-c4-postcommit.md` 记录同一四文件门禁 **68/68**、全量 `npm test` **14942 passed / 0 failed**。本代理在 R01 HEAD 实跑四文件门禁为 **62/68、6 failed**（post-verification 1、round2 3、round3 2），失败随 R01 增量出现；不能继续称为 R00 正式基线的“预存四红”。`VERIFIED_SOURCE_SHA` 仍是 C3 `f2b8c687...`，R01 2103 个新增/修改文件触发封印检查。R01 阶段是否需要独立封印收口，应依据现行项目封印流程裁定；无论如何必须记录**正式 R00 基线绿、R01 候选六红**的真实事实，不能把绿灯误报。
- 后果：R02 消费的交接不是当前已验收版本；两个 REQUIRED 场景虽有 T08 独立 PASS 报告，权威账本却仍未 PASS；阶段报告的失败数和归因错误，无法据此批准阶段关卡。
- 修复要求：新的 ZCode 阶段修复任务核对真实 T08 提交/远端和验收报告，统一更新阶段报告、交接、16 场景账本、风险登记及进度坐标；保留 T08 执行前 `source_sha/working_tree_digest` 作为明确的历史快照，另增加当前被验收的提交/证据坐标，不尝试把 Git 提交 SHA 自嵌同一提交造成虚假“当前 HEAD”。T04–T07 的 push 记录可按 Git reflog 与远端祖先关系补全。审计六红应按封印流程及用户已授权范围处理，任何“旧失败”分类须以正式 R00 C4 基线复证；不得扩白名单、退役测试或虚报通过。
- 必须重跑：F01 负向电池 + A15/A16 正向、T01/T02 相关关卡、Rust workspace 测试、四文件审计证据门禁；如调整审计封印，按正式候选 SHA 做双补丁重放及完整 `npm test`，检查工作区与交付字节无漂移。修复后由**全新** Codex 阶段验收代理复验全部 R01，不沿用本代理。

## 结论

当前 R01 原型和主要契约有真实通过的实施平台证据，但高风险门禁可被删项绕过，阶段交接和审计状态与实际提交不一致。按任务书的“全部 REQUIRED 场景及阶段 Gate 真实可信”标准，**STAGE VERDICT: FAIL**。主仓未改动、未提交、未推送。
