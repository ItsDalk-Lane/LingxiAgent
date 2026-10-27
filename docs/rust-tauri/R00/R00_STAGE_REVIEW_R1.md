# R00 阶段级独立验收 R1

**STAGE VERDICT: FAIL**

验收对象：`codex/rust-tauri-migration`，stage base `7d1a0c6bc28062ff455adcf8f68c3a80b117e90d` → 当前已提交且本地远端引用一致的 HEAD `8b153b1031bbb01204375b08e9caaa891397d7a5`。验收者是未参与 R00 执行、修复及逐任务验收的新阶段复查者。本报告只给出阶段组合结论，不重新宣布任何逐任务 PASS。当前原工作区原有未提交项仅 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`；本轮未读取真实用户数据、未调用真实供应商或付费接口、未进行远程写入、未修改产品代码或已提交文件。独立复跑日志放在 `/tmp/r00-stage-review-r1-d2_qggba/`。

## 1. 判定摘要

| 阶段要求 | 复查结论 |
|---|---|
| Goal：建立不改变生产运行行为的功能、入口、数据、依赖与验收基线 | **实质材料基本成立**。stage diff 共 641 文件：证据 466、文档 115、迁移测试 28、`scripts/rust-tauri` 13、旧任务书移出跟踪 17、`.gitignore`/`eslint.config.js` 各 1；`desktop/server/core/lib/shared/cli/hub/plugins/skills2set/package*.json` 零变更。依赖锁 SHA-256 `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b` 未变。832 个生产登记入口、736 个现役叶子、37 条入口面记录、69 个 store、14 类 Pi 职责已相互连到 952 个场景；抽样及守卫见下。 |
| Gate：无漏账、隔离、失败分类、账本与阈值可执行 | **未全部成立**。盘点、隔离、账本与阈值有可复查证据；失败分类在“相对 T08”与“相对 R00 stage base”之间混用，见 F2。`LEDGER_VALID checks=14945` 真实通过，但不能证明已提交 T08 的交接已绑定，见 F1。 |
| Handoff：R01 仅消费已验证输入 | **不成立**。`source_sha`、T08 提交记录、A15/A16 的 `committed_in` 和账本基准仍停在 T07；两个已知 T03 后续项未进入 unresolved_items，见 F1/F3。R01 任务书 §2 要求经审阅的 R00 报告与 HANDOFF 为前置，当前不得把该 HANDOFF 当作已封存终态输入。 |

没有发现两名现役组件同时写同一权威 store 的新增情形。T04 的 `provider-state` 等 store 记录现状写进程，T03 的 Rust owner 是未来目标责任，R01-T01 才正式决定唯一目标 owner；不能把这些目标名称冒充已实现接口。R00 也未新增 Rust/Tauri 生产入口。

## 2. MUST_FIX findings

### F1｜已提交 T08 仍被交接和账本标为待提交，绿灯没有覆盖这个偏差

**复现：**`git rev-parse HEAD` 得 `8b153b1031bbb01204375b08e9caaa891397d7a5`；`R00_HANDOFF.json.source_sha`、`ACCEPTANCE_MAP.json.basis.head`、`BLOCKERS.md` 的基准头仍是 `e0b7be6108c4d5bc873061dee279b7163ca78a41`。HANDOFF 的 `accepted_tasks[R00-T08]` 仍为 `commit:null`、`READY_FOR_REVIEW`，而 T08 独立 R2 已判 PASS 且 T08 已提交。账本 `RES-R00-A15/A16.committed_in` 仍为 `null`。机器对照和摘要在 `/tmp/r00-stage-review-r1-d2_qggba/coordinate-audit.json`（SHA-256 `e231997b387d1fd3014ef56bfe4dc530261746d728d4469db80941b22e954d0d`）。

**为何 `LEDGER_VALID` 仍报绿：**`r00_t07_validate_ledger.py` 约第 352–359 行仅在 `committed_in=null` 时核对 `tested_sha == basis.head`，没有核对该“待提交头”是否仍是当前 Git HEAD。两者同为旧 T07 SHA，所以当前实跑仍 `LEDGER_VALID checks=14945 scenarios=952 results=16 entries=832`、exit 0（日志 SHA-256 `983b4e2124c3fad71af769b8303133c4d4fdcb5fe665885d6a094c2cadb160b3`）。账本的哈希校验和 18/18 HANDOFF 固定文件哈希、T08 清单 173/173 哈希均匹配；问题是**坐标语义**，不是文件损坏。

**跨任务后果：**A13/A14 校验器放过 A15/A16 的已提交后空提交字段；T08 的交接将 T07 状态交给 R01，R01 无法仅凭 HANDOFF 判断 T08 产物已被哪个提交封存。`R00_REPORT.md` §2/§4 和 `R00_EVIDENCE_SUMMARY.md` 首行同样仍以 T07 为当前 HEAD/Task base，须区分历史 task base 与现时 source SHA。

**修复归属：**R00 阶段交接收口（T08 交接与 T07 账本生成/校验链）；不改产品运行代码。至少将 HANDOFF `source_sha` 与 T08 `accepted_tasks.commit` 绑定实际 T08 SHA，T08 verdict 引用其既有 R2 独立验收；将 A15/A16 的 `committed_in` 绑定实际提交并按既定生成器更新 `basis.head`、`BLOCKERS.md` 头、报告/摘要的“当前 HEAD”字样及相关摘要。可给校验器加一个窄负例：`committed_in=null` 且 `basis.head` 已不是当前 Git HEAD 时拒绝，避免下次历史基准伪装为待提交态。

**历史字段保留：**`stage_base_sha=7d1a0c6…`、`task_base_sha=e0b7be610…`、A15/A16 实际运行的 `tested_sha=e0b7be610…`、A01–A14 各自原 tested SHA/提交/时间、T02/T03 原冻结 tested SHA、T06 原测量提交和阈值冻结值、原始失败日志、T08 的 173 文件候选清单都应保留原来所指的历史事实；不要把它们统改成 8b153b103，也不要重写测试结果来假造“在提交后运行过”。HANDOFF 字节变化必使 A16 `source_digests` 过期，需用合法重建路径重绑并真实复跑。原 T08 候选清单在交接修订后应当按其原提交 `8b153b103` 的文件字节核验；它不再代表新的阶段收口候选，新的候选另算摘要。

### F2｜四项审计封印失败的“预存”表述未说明阶段基点

**复现：**在 `/tmp` 的 stage-base 独立副本运行 `node .sync-audit/verify-post-verification-diff.mjs`，`7d1a0c6…` 上 exit 0，输出“仅 6 个审计文件变化”（日志 SHA-256 `fbb9fd1e0bef5079f9d1b0d27369e6ce1414e166038db7be09db681a5c515d0d`）；当前 HEAD 同命令 exit 1，非白名单清单从 `.gitignore` 和 R00 增量开始（日志 SHA-256 `da6eeaf5d9b12a93d7f6fe5dc589bf005393d3cfa394f0d0341d31ef1f43f0d2`）。T08 两轮原始日志中四项失败相同，A15 复现校验器本轮 exit 0；因此“四项相对 T08 开始前已存在”成立，**“相对 R00 stage base 预存”不成立**。当前 `R00_REPORT.md` §7、`R00_EVIDENCE_SUMMARY.md` §4 以及 HANDOFF `AUDIT-SEAL-PREEXISTING.kind=preexisting_failure` 易让 R01 按后一种含义误读。

**跨任务后果：**T08 的 A15 任务级复现不能替代阶段级的新增失败分类。全量 `npm test` 不能标 PASS；不能以这四项只是“旧基线原有失败”为由跳过 R00 范围的封印处置。它们属于 R00 提交触发的审计坐标冲突，未显示生产行为回归。按 AGENTS.md 的既定封印规则，不能靠扩白名单、退役门禁或虚报已验证坐标求绿；已授权的封印流程若在阶段收口推进，应由总控以真实候选及验证证据处理。

**修复归属：**R00-T08 阶段报告/HANDOFF 的分类文案与总控封印收口。明确写“两种基点”：相对 T08 为先前失败，相对 R00 stage base 为阶段提交触发；保留四个 FAIL 原始日志、两次复现及 exit 1。单纯改文案不声称测试已绿；封印流程若仍待执行，应在 R00 收口项列出真实状态和责任，不推给 R01 当作它的实现缺陷。

### F3｜T03 独立验收的两条已知后续项未进入阶段 unresolved_items

**复现：**`R00-T03_REVIEW_R3.md` 的 F01 指出 `lib/desk/heartbeat.ts` 的巡检状态写入/指纹去重锚点未在 worker 行级账本显式登记；F02 指出 `lib/autolearn/autolearn-service.ts` 的后台模型作业、技能写入及通知形态没有按相邻 worker 先例登记。该审查将其定为 LOW、没有推翻 A05/A06，并明确要求 T07/R07 承接。HANDOFF 的 12 个 `unresolved_items` 含 T05/T06 后续项，却无这两项；`R00_REPORT.md` 风险表同样未列。对照记录：`/tmp/r00-stage-review-r1-d2_qggba/handoff-followup-audit.json`，SHA-256 `8cd7474a9122aeb434e5b8677f1011b265c6d6612ae806634a77219042ce54e6`。

**跨任务后果：**R01 的 owner 设计和 R07 的外围 worker 账本若只消费阶段 HANDOFF，将漏掉 T03 审查要求的两个行级补充。现有 PI-08 已覆盖 `callText` 网关方向、W1-3 已覆盖巡检执行链，故这是交接可追踪性缺口，不是凭此判定 T03 原 PASS 错误或要求现在改产品代码。

**修复归属：**R00 阶段交接。在 `unresolved_items` 与阶段风险表补两个独立条目，给出源码、T03 R3 引用、唯一承接任务/最晚阶段及复验方式。若仅补交接引用，无需改变冻结的 T03 矩阵；若选择现在修改矩阵，则按影响图重建、复跑 T03 校验及账本哈希链。

## 3. A01–A16 与跨任务组合复查

下表中的“既有独立验收”只标明审过哪份历史材料；本轮实测在 §4。**场景记录为 PASS 不等于本阶段 PASS。**

| 场景 | 阶段层观察 |
|---|---|
| A01/A02（T01，R4） | R4 的真实安静窗口、两个真实目录哨兵/完整摘要、真实服务隔离目录和脏工作树快照可复查；本轮遵守零真实数据接触限制，未再对用户目录做探针。T08 后无生产启动链源码改动。 |
| A03/A04（T02，R10） | 832 入口/736 叶子/24 域及四组差集为零；Ollama、现役子代理保留，研究/强制目录/本地模型管理排除。当前 `--negative-checks` exit 1 为 `tested_sha` STALE；本轮在内存重算三份输出，三份各仅 `/tested_sha` 变化，业务字段无差（`t02-semantic-diff.json`）。历史 PASS 的内容未被后续任务破坏，但当前工具不可直接报绿。 |
| A05/A06（T03，R3） | Pi 14 职责、19 worker、51 依赖由冻结图交接。当前 `r00_t03_validate.py` exit 1：独立源集 1677 vs 冻结图 1664；差出的生产扫描范围文件为后续新增 13 个 `scripts/rust-tauri/r00-*` 测量/回放脚本，不是现役核心入口。T03 F01/F02 为已知非阻塞但交接遗漏（F3）。 |
| A07/A08（T04，R2） | `r00_t04_scan.py --validate` 本轮重跑：101 锚点、392 路由、69↔69 store、0 未分类/0 过期、4/4 负例；六组存储/认证测试 74/74。CLI/Bridge/cron/开发四链身份来源、MCP app-tools 直调例外及 Bridge 外部账号信任风险均在 `OWNERSHIP_CURRENT.md`，没有把未证实路径写成已安全。独立抽查 provider save 叶→账本→`provider-state` 权威 store 写进程 server 闭合。 |
| A09/A10（T05，R1） | 本轮 9 组夹具三次回放全部 exit 0，规范化 diff 相同；强化网络守卫后的 21 项迁移测试全过。损坏尾记录使用修正后不变量，没有把旧 bug 当新标准。替身边界是本地协议/临时文件，不代表真实供应商。 |
| A11/A12（T06，R2） | 本轮重算阈值 SHA-256 `bf40009e347df014dba1d2a2ecb7505bf4bdccfdef1992fc99b144749a992331` 与 HANDOFF 相符；G1 server/PTY/desktop 各 32 样本，八项门槛含数值/比较口径，Rust 结果尚不存在。仅 darwin-arm64 标 MEASURED；其余平台 PENDING_MEASUREMENT，长时 G1 `NOT_RUN_THIS_ROUND`，未冒充通过。 |
| A13/A14（T07，R3） | 本轮账本 952 场景、16 结果、832 入口、14945 checks exit 0；隔离自测 18/18 + 正面对照，0 临时副本残留。F1 表明校验器还漏了“已提交后仍空提交”的一类跨任务状态。 |
| A15/A16（T08，R2） | A15 两份 T08 失败日志复现校验 exit 0，四个封印用例仍 FAIL；阶段基点分类见 F2。A16 交接原有三点 spotcheck exit 0，本轮另选供应商保存叶 `F-D06-…9E3981`→`server/routes/agents.ts:700`→补充场景 `R00-T02-LA-9E3981A9BFCA`→R06-T01/T06，并核对 `provider-state` 路径及 server 唯一写进程、T04 存储测试 74/74。A16 可定位性成立，但 HANDOFF 终态未闭合（F1/F3）。 |

跨 T 影响复核：T08 对 T05 网络守卫的改动已经由本轮回放和负向测试覆盖；T07 账本将 T02 的叶/入口、T04 的入口/store 以及 T08 的 A15/A16 结果放进同一映射，交叉计数与摘要有效；T02/T03 的当前 checker 非零是冻结坐标差，不可静默改报 PASS，也没有据此发现新生产漏账。未发现后续任务修改原生产调用路径或产生第二内核。未执行真实数据迁移、真实账号、长时 2h/1000 任务、macOS x64/Windows/Linux 安装包或发布，分别保持原阻塞状态。

## 4. 本轮 Stage Gate 实跑记录

环境：macOS 27.0 (26A428) / Darwin 27.0.0 arm64；Node v24.16.0、npm 11.13.0、Python 3.14.3；环境记录 SHA-256 `030f8bdd2b01644b10cea59c03c455ee7ed3ac6f3d8d4f77bc48d2c30dab0ccd`。测试用 HOME/TMPDIR/LINGXI_HOME 均指向 `/tmp/r00-stage-review-r1-d2_qggba/` 的隔离子目录，外部协议只用本地替身。下表日志路径均以该目录为前缀；SHA-256 是原始合并 stdout/stderr 文件，不是测试结果推断。原仓库无输出路径的只读命令在原仓库执行；可能改写已提交证据的全量测试和 renderer 构建在同一 HEAD 的 `/tmp/.../clone` 副本执行。

| 命令/范围（日志） | 预期 | 实际/退出码 | 日志 SHA-256 |
|---|---|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t07_validate_ledger.py` (`ledger.log`) | 当前账本结构/证据通过 | `LEDGER_VALID checks=14945`，0；F1 语义另查 | `983b4e2124c3fad71af769b8303133c4d4fdcb5fe665885d6a094c2cadb160b3` |
| `r00_t07_selftest.py --suite all --art-dir /tmp/.../ledger-selftest` (`ledger-selftest.log`) | 18 负/正变体有效 | 18/18 + positive，0 | `804636862a133f58503e5db5b54be69ada4d04caba6f144ce72325dfdcad3396` |
| `r00_t02_inventory.py --negative-checks` (`t02-current.log`) | 当前输出应与盘上吻合 | STALE，1；内存差异仅三份 `/tested_sha` | `2c5287695361fd6deb7eab73465c54bfa9a78175e4f32a48c89b3ede4ca21325` |
| T02 内存重算差异 (`t02-semantic-diff.json`) | 判定 STALE 实质 | 三份各仅旧→当前 SHA，一致性成立，脚本 0 | `33dde05dced8986b51ac5c07cb598100bc85fdd79bff91df6e5f9dfcf7fcd5eb` |
| `r00_t03_validate.py` (`t03-current.log`) | 冻结图校验 | 1664↔1677 不一致，1；后增 13 个测量/回放脚本 | `4c77efab860a2553fd00ae2196218ff72037d7093b50c9702eb5a591842a5375` |
| `r00_t04_scan.py --validate`（仅把输出重定向 `/tmp`，`t04-current.log`） | 入口/store 无差及负例触发 | 101/392/69，0 未分类，4/4 负例，0 | `ee651b0c535e30bcb23265ecd54640ebd2e5a01cdde9c3d9b8b4247d3aeabf31` |
| 六组存储/认证 `vitest` (`store-security-tests.log`) | 存储点、认证与设备拒绝路径通过 | 6 文件/74 用例，0 | `60046093cae7ed67a719fc6119ec4ff6716e4c7850578efa9f4637d594efa6e3` |
| `check-persistence-schema-fingerprint.mjs` (`schema-fingerprint.log`) | 170 守卫源无改变 | OK，0 | `cb2aef5ab10674689d5dfb5ba0d28ef2fe7977f9621384fa2edf8e74c42a6e4b` |
| `r00-t05-replay.mjs --out /tmp/.../replay` (`replay.log`) | 9 夹具三轮一致 | 3× exit 0，两个 normalized diff identical，0 | `b13e7402bdfd4f77cb38ab3436d979a4b634e296a6743c96c7d1787a71693eb8` |
| 三组 migration `vitest` (`migration-tests.log`) | 21/21 | 3 文件/21 用例，0 | `0bc98ab471475dcab693f1e9833da41995f1e2203a390a199d9409e51af936b6` |
| `npm run typecheck` / `typecheck:core-contracts` | 全类型/28 契约文件通过 | 0 / 0 | `e678e3a57ec1050015581c10510d0428b2b6d4153ba6165a6d78a5b69dcbb99e` / `9a3a2b6ce40e47627287266176c6142f859d948935aa73c756c0a6e541a2ced7` |
| `npm run check:dependency-boundaries` / `check:tool-invocation-boundaries` | 边界通过 | 2031/2249 文件，0 / 0 | `d79ce6684377c37d822cfa13de8ec146eb78478be4e7580a697475a77bbd2306` / `a577d5cb67f55477b516eae074e5e902ec0260eac98bf5bf927b5ae42d9a248c` |
| `npm run lint` (`lint.log`) | 0 错误 | 0 错误、10887 warning（33 可自动修），exit 0 | `149a9a875d237a7d39ea0004e47f9c1cb64d6eb2439d238f1b0fbc30abd9670d` |
| `npm run test:knowledge-platform-smoke` (`knowledge-smoke.log`) | 15 文件/125 用例 | 同预期，0 | `9bd40d57a7803c778eb46c78689d0cb174eb3d7172cd9aad810d4b4346d6fad7` |
| `npm run build:renderer`（隔离副本，`renderer-build.log`） | renderer 编译 | 成功，0；未生成安装包 | `191c34492595c4768e7e9e6827ecb07d47d2033326facb9bb5a20a3f7324cb3e` |
| `r00_t08_a15_verify_repro.py` (`a15.log`) / `r00_t08_a16_spotcheck.py` (`a16.log`) | 四失败同组、三点可定位 | 两者 exit 0；阶段缺口见 F1/F2 | `145be9f94d7b634c48997844c448aa900f55a9d7cfd4db897ee39299e335e991` / `ee9dc14ef4cea7afd654ee93f6daa3aa909ec9dd1d2ca74298b8d817b10c9e64` |
| 审计封印脚本：stage base / 当前 HEAD (`audit-seal-stage-base.log` / `audit-seal-head.log`) | 确定阶段前后分类 | 0 / 1，R00 新增非白名单项触发 | `fbb9fd1e0bef5079f9d1b0d27369e6ce1414e166038db7be09db681a5c515d0d` / `da6eeaf5d9b12a93d7f6fe5dc589bf005393d3cfa394f0d0341d31ef1f43f0d2` |
| `npm test`（隔离副本，依赖软链接首轮，`npm-test-isolated.log`） | 全量已知四封印失败，不能报 PASS | exit 1；1462 文件/14893 用例通过，6 失败：四封印 + CLI closure 两项。原仓库单独重跑 CLI closure 定点 1/1 通过，表明这两项属于隔离副本解析差异，不据此宣告产品回归 | `0cba7fb08892a4815d666abe1a269e8e71dbf65e8cc2704cb22b98db3fb30dae` |
| `npm test`（隔离副本，依赖物理 APFS 副本复轮，`npm-test-isolated-physical.log`） | 排除软链接差异并保留失败全集 | exit 1；1462 文件/14894 用例通过，5 失败：四封印 + `artifact-core-ustar` 临时目录清理 ENOTEMPTY。该用例在原仓库单独 10/10 通过，隔离并发偶发失败未冒充绿灯 | `c795ef2c425dff812a5cdc35f5f9b90cf78cded8aa6de9e57c055894a27a0a92` |
| CLI closure 单项 / ustar 全文件原仓定点复核 (`cli-closure-source.log` / `ustar-targeted.log`) | 区分副本环境失败 | 1/1（其余 21 skipped，不计 PASS）/10/10，均 exit 0 | `ec7a1925ad58346682b0c91e2784bc88c093638e41bc50af18c0b972a6add94d` / `4e59941931b1c3fe563b2f51c4e2367640e13b810638e346a8c9f3e68d4938e1` |

`npm test` 两次副本运行都触发已知 patch.gz 重写，仅发生于 `/tmp/.../clone`。原工作区未执行此有副作用的全量命令；T08 在原仓候选上已有全量日志（4 FAIL / 14895 PASS）及 A15 同条件双复现。**本轮没有在当前原工作区 HEAD 得到一份“全量恰四失败”的直接运行记录**；两次隔离复跑的额外项已完整列出，不能抹掉。源码与数据隔离检查未触碰用户目录或真实凭证。审后 `git status --short` 除原有总控账本外无本轮修改；本报告新增后应只再多本文件。

## 5. 修复后重跑矩阵与 R01 边界

1. **F1 坐标和账本：**只在获准 R00 交接路径改 HANDOFF、T07 结果源及生成物；先在隔离副本做重建/负例（旧 `basis.head` 配空 `committed_in` 必须非零），再运行 `r00_t07_selftest.py --suite all --art-dir /tmp/…`、真实仓库 `r00_t07_validate_ledger.py`、`r00_t08_a16_spotcheck.py`、HANDOFF 固定摘要与新阶段候选摘要复算。原 T08 候选清单只按 `8b153b103` 提交内的原字节核验。重跑 A15/A16 引用的具体 migration 三文件、A15 双日志比对；输出绑定修复后的实际候选 SHA/工作树摘要。若校验器源码变动，重跑其 A13/A14 全部负例与正例，不能只见 LEDGER_VALID 即关单。
2. **F2 分类：**独立复核 stage-base 封印 exit 0、当前候选封印 exit 1 和四个失败用例原始日志；报告、摘要、HANDOFF、阻塞表的相对基点及责任一致。若推进正式封印，严格走 `PROGRESS.md` 现行流程，把证据绑定真实候选提交；审计门禁仍红时继续如实记 FAIL，不扩大白名单或退役测试。
3. **F3 交接：**核对 T03 R3 两项原文与 HANDOFF 的 ID、源码、owner、最晚解除阶段、下游任务；不修改冻结矩阵时做文档链接/摘要一致性检查及 A16 spotcheck。若修改 PI 矩阵/生成器，则重做 T03 两差集与 worker 负例、T07 source-digest/账本、相关 cron/autolearn 测试，并重新独立审阅变动。
4. **共同 Stage Gate：**在最终候选上复跑 T02 语义差异、T04 扫描/六组存储认证测试、T05 三轮回放与负向守卫、typecheck/core contracts/边界/lint/knowledge smoke、renderer build，以及隔离副本全量 `npm test`；记录所有失败与 clone 环境差异。性能阈值文件和 G1 raw 摘要复算，未实测平台/长时/真实供应商/正式包保持未验证。候选若只改文档/账本且输入未变，可沿已绑定 HEAD 的运行日志，明确逐项影响分析，不机械伪称重跑。

当前可供 R01 **准备但不能宣称前置 Gate 已通过**的输入：736 叶子及 832 入口映射、37 入口身份与 69 store 现状、Pi/Node 14 职责与依赖矩阵、9 组脱敏协议夹具、八项冻结阈值及原始 G1 数据、952 场景账本结构、明确的未来平台/LIVE/长时限制。R01 只应把这些当现状与目标设计输入；`server-http-ws` v1、`preload-bridge` v1、`DATA_EPOCH` v1 是现役接口，Rust/Tauri 目标协议及目标 owner 尚未实现，须在 R01 决定。F1–F3 修复并重新阶段独立验收之前，不启动 R01 实施任务。

远程/发布：本轮未提交、推送、发布或变更权限；仅只读核对本地远端引用已指向 T08 HEAD。真实供应商、真实用户数据迁移、四目标平台真机、2h/1000 次长时测量和正式安装包均未运行，不能由本机结果外推。
