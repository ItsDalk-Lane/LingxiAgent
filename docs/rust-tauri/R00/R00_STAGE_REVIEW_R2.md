# R00 阶段级独立对抗性验收 R2

**STAGE VERDICT: PASS**

验收者：全新 STAGE-REVIEWER-R00-R2；未参与 R00 任一 Task 执行/验收、阶段 R1 验收或 ZCode 阶段修复。结论仅针对 **R00 Stage Goal/Gate/Handoff 在当前候选上的阶段复验**，不重新宣布逐 Task PASS，不代表审计封印已经完成、全量 `npm test` 已绿或 R01 已获实施授权。

## 1. 候选、范围与判定

分支 `codex/rust-tauri-migration`；stage base `7d1a0c6bc28062ff455adcf8f68c3a80b117e90d`；本地 HEAD 与本地 `origin/codex/rust-tauri-migration` 均为 `8b153b1031bbb01204375b08e9caaa891397d7a5`。当前候选是该提交 **加** 8 个 R00 已跟踪修复文件、`STAGE_REPAIR_R1/` 证据与修复报告的未提交内容。总控 `ORCHESTRATOR_PROGRESS.json` 的既有未提交改动不属此候选；R1 独立报告保留原字节。本报告自身为本轮唯一新增仓库文件。未提交、推送、建分支/worktree/PR/tag/release，未读真实用户数据或调用真实供应商、付费接口、真实外发。

读取了本地 AGENTS、任务书 00/01/02/03/04/05/06/90/91、R00 阶段书与三个 JSON 目录、T01–T08 交付和独立验收、阶段报告/交接/账本/阻塞、生成及校验脚本、原始证据、阶段 base→候选差异和关联现役源码。任务书 R00 的 16 个基础场景均为 REQUIRED；`ACCEPTANCE_MAP` 中 A01–A16 各有 PASS 结果，另有 16 个 T07 补充已验场景，736 个迁移叶子场景明确待其实施阶段执行。现役 832 个入口、736 个生产叶子、37 个入口面、69 个 store、14 类 Pi 职责进入映射。任务书清洁副本校验为 `DOCUMENT_SPEC_VALID`（12 阶段/100 任务/200 基础场景）。

**Goal 满足：**阶段 base→HEAD 共 641 个提交文件变化；`desktop/server/core/lib/shared/cli/hub/plugins/skills2set/package.json/package-lock.json` 没有差异。未提交修复仅 R00 文档/账本/校验脚本及证据，不含生产路径；依赖锁 SHA-256 仍为 `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`。因此本阶段没有生产行为改动或第二个内核；清单所记目标 Rust owner 仍是未来设计，不是现有运行接口。

**Gate 满足当前 R00 范围：**隔离与脏工作区证据、入口/数据/权限盘点、旧行为夹具、冻结性能阈值、可执行账本及失败分类均可复核。四个审计封印用例依然 FAIL；它们的两基点来源和总控收口责任已明确，且不被记成 PASS。它们属于候选提交后的封印坐标门禁，须在总控实际提交并绑定验证证据之后按 `PROGRESS.md` 推进；当前阶段 PASS 不替代该后置门禁。

**Handoff 满足：**交接绑定真实 T08 提交，历史测试时点保留，T03 两项 LOW 后续问题明确交给 R07-T12。R01 可消费经证实的现状清单、夹具、阈值及风险；目标协议版本和 Rust/Tauri 实现仍由后续阶段决定。未实测平台、真实账号、长时测试和发布授权是有最晚关卡的条件项，不是 R00 当前 REQUIRED 场景的假 PASS。

## 2. 阶段 R1 的 F1–F3 独立复核

### F1｜坐标与账本：关闭

- Git HEAD=`8b153b103…`；HANDOFF `source_sha`、T08 `accepted_tasks.commit`、`ACCEPTANCE_MAP.basis.head`、`BLOCKERS.md` 基准头及报告/证据摘要的“当前 HEAD”均指向它。T08 任务级 verdict 只引用既有 `R00-T08_REVIEW_R2.md`，没有冒称本轮重判。A15/A16 的 `committed_in=8b153b103…`，`tested_sha=e0b7be610…` 保留原执行时点；A01–A14 的历史 tested SHA 逐项是 HEAD 祖先，未统改为提交后测试。A16 的 HANDOFF `source_digests` 等于当前 HANDOFF SHA `c925e839c088502ae4e4d2798b9a284b26e30f26fb41ec1f39ced9a440669938`。
- 独立递归比较已提交账本与当前账本，恰 **19** 个字段差异：`basis.head`、生成时间、RESULTS 输入摘要、A13/A14 各 3 个工具/结果摘要及 working tree 摘要、A15/A16 的提交字段与时间注记、A16 HANDOFF 摘要及 working tree 摘要、2 个测试脚本摘要。`scenarios/tasks/features_index/entrypoint_index/surfaces/spec_digests/counts` 全部不变，A01–A12 结果零漂移。当前 `LEDGER_VALID checks=14945 scenarios=952 results=16 entries=832`，exit 0。
- 三份独立清单复算：阶段修复清单 **109/109** 对盘匹配，清单 SHA `4f8a64e585dc35cd68a4242fade90a2aaf3c4ad83aa0d49311db628641964b70`；HANDOFF 固定文件 **18/18** 匹配；T08 历史候选清单 **173/173** 与 **提交 `8b153b103` 的文件字节**匹配，其历史聚合 SHA `d83ae896ce19d1ee5a8c0c8084244c44a32516cee5d589a35ead9fff2a448645`。T08 清单不被误当成新增阶段修复候选清单。R1 报告 SHA `7284b8faeead270b40ca8753d91dcac2211d0d7a4757c67b8e45a6e69d3ef7aa`，修复报告 SHA `6c71dd6c7a3d64547d3b4990563c60f854cfb7fed8252631b57e5dc669451b73`，均与指定值一致。
- 独立在 `/tmp` 假根测试新增 `COMMIT-PENDING-BASIS`：无待提交结果 exit 0、当前 HEAD 的合法待提交结果 exit 0、旧 `basis.head=e0b7be610…` 加空 `committed_in` exit 1，且恰命中该规则一个错误。正式自测 19/19 变体加正面对照 exit 0，20/20 临时根清除。该检查仅在存在待提交结果时启用；未发现对无待提交结果或合法待提交窗口的误拒绝，也未发现靠改 `tested_sha` 绕过旧 HEAD 拒绝的路径。校验器依赖 Git HEAD，离线副本必须传其对应 `--git-repo`，此行为与脚本现有契约一致。

### F2｜封印失败分类：关闭；封印自身仍待总控

在 `/tmp` 两个隔离 Git 根中使用同一历史对象库、分别固定 HEAD 为 stage base 与当前提交，独立执行 `.sync-audit/verify-post-verification-diff.mjs`：**stage base exit 0**（仅 6 个白名单审计文件），**T08 HEAD exit 1**（R00 提交新增非白名单文件）。两份原始日志 SHA 分别为 `fbb9fd1e0bef5079f9d1b0d27369e6ce1414e166038db7be09db681a5c515d0d` 和 `da6eeaf5d9b12a93d7f6fe5dc589bf005393d3cfa394f0d0341d31ef1f43f0d2`。所以相对 T08 任务基点是已存在失败，相对 R00 stage base 是本阶段提交触发；修复后的 HANDOFF、报告、证据摘要均作此区分。

在 `/tmp` 隔离候选副本重跑 3 个封印测试文件：**exit 1，4 failed / 14 passed**，四项分别为 post-verification diff allowlist、round2 R10-03/R10-04、round3 manifest，与 T08 两次原始失败及阶段修复日志同组。A15 双日志比对器本轮 exit 0，但它证明失败集合稳定，**不表示四项测试通过**。没有扩白名单、退役门禁、改预期或虚报已验证 SHA。`PROGRESS.md` 的现行顺序为：先将适用验证绑定实际候选源码提交，再同步既有坐标来源/生成投影并完整复验，纯审计提交后再次查差异门禁；这需要由有相应提交授权的总控完成，不归 R01 业务实现。

### F3｜两条 LOW 交接项：关闭

对照 T03 R3 原文和源码：`lib/desk/heartbeat.ts` 的 `jian.md` exec-log 状态与指纹去重未在 W 表行级登记，执行链由 `hub/scheduler.ts` 的 `onBeat/onJianBeat` 到 `executeIsolated`，既有 W1-3 分类覆盖；`lib/autolearn/autolearn-service.ts` 通过 `callText` 进行后台提炼/审查，确认后安装技能并通知，PI-08 模型边界和 D21 技能写入责任已覆盖，但相邻 worker 形态未单列。HANDOFF `T03-R3-F01/F02` 逐项给出源码、R3 审查引用、唯一承接 **R07-T12**、最晚 **R07**、补登记动作与矩阵生成器/校验器复验；报告 §11 同步。R07-T12 任务书明确要求逐 worker 能力与退出策略及遗留清账，归属合适。两项是未来登记完整性条件，不改变 T03 原 A05/A06 的静态职责分类，也无需本阶段篡改冻结矩阵。

## 3. A01–A16 与跨任务接口

| 场景 | 当前阶段级复核结果与边界 |
|---|---|
| A01–A02 / T01 | R4 独立验收记录真实目录哨兵、安静窗口和脏树前后摘要；本轮不触碰真实用户目录。后续零生产启动链变更，清单哈希有效。 |
| A03–A04 / T02 | R10 验收后的 832 入口/736 叶子双向映射与现役 Ollama、subagent、知识库及撤回项分类留存。当前 T02 检查器的旧 `tested_sha` STALE 是工具坐标，不应冒充新运行 PASS；后续没有生产入口源码变更。 |
| A05–A06 / T03 | 14 Pi 职责、19 worker、51 依赖冻结。当前 T03 受控源集 1677 vs 冻结图 1664 的检查 exit 1，新增 13 个 R00 测量/回放脚本，未据此伪称图在当前树重新通过。F3 两 LOW 已有交接。 |
| A07–A08 / T04 | 本轮扫描 exit 0：101 锚点、392 路由、69 store、零未分类/过期、4/4 负例。新抽查 `查看定时任务`：F-ID `F-D18-…-1FB3D8` → `route:desk:GET:/desk/cron` → `server/routes/desk.ts:1045` → 补充场景 `R00-T02-LA-1FB3D8BC916D` → R03-T06/R07-T01；`bindCronRequestScope` 在读取前核对可信 principal 与 runtime studio，相异返回 403，`cron-automation` 是 server 单写的权威 store。该抽查是源码/映射证据，不代替真实账号跨平台验收。 |
| A09–A10 / T05 | 修复后同一 HEAD 的 9 夹具三轮回放证据保留；本轮在隔离副本重跑 migration 三文件，连同 cron 测试合计 4 文件/31 用例全过。损坏尾记录按修正不变量，不复刻旧 bug；网络为本地确定性替身。 |
| A11–A12 / T06 | 冻结阈值 SHA `bf40009e347df014dba1d2a2ecb7505bf4bdccfdef1992fc99b144749a992331`、G1 raw 和进程树测量可复核。仅 macOS arm64 有实测；Rust 性能结果尚不存在，长时与其他平台不报 PASS。 |
| A13–A14 / T07 | 账本校验、自测及 F1 三态独立反例真实通过；16 结果/952 场景/832 入口，A01–A12 历史内容未漂移。 |
| A15–A16 / T08 | 四封印 FAIL 稳定且分两基点分类；A16 原三点 spotcheck exit 0，本轮另抽 cron 功能→身份→权威存储→后续场景链；当前 HANDOFF 与账本哈希一致。 |

跨 T 影响：本轮修复只改变交接、账本结果及其校验/自测，不改变 T02/T03/T04 的冻结盘点数据、T05 夹具、T06 阈值或生产调用链。T07 账本 19 项差异已归因，A16 新 HANDOFF 摘要被重绑，T08 历史 173 项仍按其交付提交核验。现役 `server-http-ws`、`preload-bridge`、`DATA_EPOCH` 均仍是 v1 现状；R01 再定义目标契约。未发现新的当前 R00 必须关闭的阻塞。

## 4. 本轮实跑与未运行

环境：macOS 27.0 / Darwin arm64；Node v24.16.0、npm 11.13.0、Python 3.14.3、vitest 4.1.10。隔离根 `/tmp/r00-stage-r2-yuvfsak4/`；有写副作用的 vitest/扫描在由 HEAD 归档、叠加 8 个修复文件的 `/tmp/.../candidate` 中运行，HOME/TMPDIR/LINGXI_HOME 全指向 `/tmp`。原仓只运行只读校验/哈希；自测变体和日志也写 `/tmp`。以下 SHA-256 为本轮 `/tmp` 原始合并输出日志；`exit 1` 与 `exit 2` 均如实保留。

| 项目 | 实际结果 / 退出码 | 本轮日志 SHA-256 |
|---|---|---|
| 账本校验 | `LEDGER_VALID checks=14945` / 0 | `983b4e2124c3fad71af769b8303133c4d4fdcb5fe665885d6a094c2cadb160b3` |
| A13/A14 自测 | 19/19 + 正面对照，20/20 临时根清除 / 0 | `ead2224362dd20d24746bc79bdf0748c391d8721b757751dc7e39d0d575dfb4b` |
| F1 三态：无待提交 / 当前 HEAD 待提交 / 旧 HEAD 待提交 | 0 / 0 / 1（旧态恰 1 个 `COMMIT-PENDING-BASIS`） | `983b4e21…60b3` / `d221a03b5f8f279f4e089c9f1e9cbcc27acbdc5c9dcef50ade9dd5ca62020672` / `5382edcaabee0da9c8c89cad61d1af9708003cf7b6c0e6539a23738bd4e18ac4` |
| A15 双日志复现校验 | 四失败同组 / 0；首次漏传必需参数得到 usage / 2，随后按接口重跑 / 0 | `145be9f94d7b634c48997844c448aa900f55a9d7cfd4db897ee39299e335e991`（正确调用） |
| A16 HANDOFF 三点抽查器 | 3/3 / 0 | `17ea7a97b709b5beee044ef88828eca54e847e86829b320b9d8319d057741aae` |
| T04 源码/存储扫描 | 101/392/69，4/4 负例，零差集 / 0 | `ee651b0c535e30bcb23265ecd54640ebd2e5a01cdde9c3d9b8b4247d3aeabf31` |
| 隔离副本 cron + migration 定点回归 | 4 文件/31 用例通过 / 0 | `bd4fe0b621735eabda279b9f0857135039b731f23af3d2a7c76c474eb193b2de` |
| 封印独立脚本 @ stage base / HEAD | 0 / 1（两基点） | `fbb9fd1e0bef5079f9d1b0d27369e6ce1414e166038db7be09db681a5c515d0d` / `da6eeaf5d9b12a93d7f6fe5dc589bf005393d3cfa394f0d0341d31ef1f43f0d2` |
| 隔离副本封印三文件 | 4 failed / 14 passed，**exit 1** | `4bc986e0dc35cd3f757cb5f74e60297773122bd1b38678b4834fca1928b676f3` |
| 任务书 `validate_bundle.py` 原目录 / 按 MANIFEST 建的清洁副本 | 原目录因被忽略的 `.mimosa/hook-state` 额外文件而 exit 1；清洁副本 `DOCUMENT_SPEC_VALID` / 0，不把原目录失败隐去 | `a63580105945e630e04a8b9d08fee2c04f1045b15e12a044ea70da3ed0c5ad1b` / `da69c040fabfb8bc872bc1925f5f6da390e58256a228f3a8cf52863f2bf0b8b9` |

本轮**未重跑**全量 `npm test`、typecheck、lint、依赖/工具边界、knowledge smoke、renderer build、T05 三轮回放及六组存储认证测试。原因是相应产品源码、测试、夹具、依赖锁和构建输入与已提交 HEAD 相同，阶段修复只触及账本文档及校验工具；T08 和阶段 R1 的同 HEAD 历史实跑可作未变输入的辅助证据，本轮受影响的账本/交接工具与关键 migration/cron 回归已实跑。历史全量 `npm test` **exit 1，4 个封印 FAIL**，本轮封印定点仍 exit 1；不得写成全量 PASS。T02 当前检查器 STALE / exit 1、T03 当前冻结图校验 exit 1 亦保留其范围/坐标说明，不以其历史 PASS 冒称当前检查器绿。其他平台、2h/1000 任务、真实供应商、正式安装包、真实用户数据迁移均未运行。

## 5. 总控后置要求与边界

本 PASS 表示 R00 的基线建立、当前 REQUIRED 场景及交接复验可接受；**不是**提交后封印完成声明。总控若按已获授权流程收口，应把这份未提交修复及本报告固定为真实候选提交，重算候选摘要，确认总控账本与实际提交一致，将适用验证绑定该提交，再按 `PROGRESS.md` 更新审计坐标/生成投影/完整复验，纯审计提交后再次执行差异门禁。封印仍红时不得称全量测试 PASS，不扩大白名单、退役门禁或改写已验证历史坐标。之后是否推送及启动 R01 依既有授权与阶段顺序办理；本验收未代为实施。

未来条件项：T03 F01/F02 最晚 R07-T12 补登记并重跑矩阵；真实供应商/平台项最晚按 R10 任务书完成，长时 G1 按冻结阈值执行，远程发布 R11-A14 仅获授权时激活。现阶段没有凭这些未来未运行项推导出真实平台或发布已通过。
