# R08 收尾独立上下文验收报告（R09 准入独立审查）

- 审查者：`independent-reviewer`（独立上下文，未参与 R08 收尾任何实施；结论与实施者分离）
- 审查日期：2026-10-26 20:14–20:30（Asia/Shanghai）
- 候选：HEAD `aa99d2ec1dae7f0aef5971bab049734f22fcf7d8`（分支 `codex/rust-tauri-migration`）+ **未提交工作树**
  （门回执 `worktreeDirty=true`；本轮审查全程未 commit/push，审查结束 `git status --porcelain` 仍为 79 项、HEAD 未变）
- 主证据：`artifacts/rust-tauri/R08/verify-r08-closeout-08/verify-stage-result.json` 及其 `SUITES/`、`R08_INPUTS/`；
  对照：`verify-r08-closeout-06/`（97/29/FAIL）与 `verify-r08-closeout-07/`（21/22，A15 V4 红，保留）
- 边界：只读候选源码/证据；只写本目录（`artifacts/rust-tauri/R08/closeout-proofs-02/independent-review/`）与
  `/private/tmp/review-*`；**未重跑正式门 `verify-stage`**；未修改源码/文档/阶段图/门禁；未触碰真实 `~/.lingxi`；未启动 ZCode。

---

## 结论：**ADMIT_R09**

正式门 closeout-08 的 **22/22 命令、16/16 场景、126/126 叶 PASS（0 BLOCKED / 0 FAIL）、candidateSourceBinding.stable=true、
in-gate A/B 71 任务 = 68 parity + 3 recorded + 0 differs** 全部经独立反查成立，且独立脚本对 126 叶逐断言 case 覆盖
（281 条原断言 / 487 个断言→case 实例 / 449 个不同 case 名）复核 **0 缺口、0 为假**；未发现门禁放水、未批准 recorded 或
A13 合同缩水；两条关键证据链（A/B 原始观测反算、UI 87 case）均为本审查独立复跑/独立重算可重现。

**附保留项 6 条**（1 条子句级未证 + 1 条已披露但应与 R00 原文对账 + 3 条文档/门禁设计小项 + 1 条无法证边界），
见 §九。保留项均不改变门级判定，但 **F1/F4 建议作为 R09 交接记录项/文档修订项**，不应在交接中被省略。

---

## 一、正式门收据独立核对（已证）

| 项目 | 门-08 实测值（独立读取） | 核对方式 | 判定 |
|---|---|---|---|
| overall | `PASS` | 读 `verify-stage-result.json` | 已证 |
| 命令 | `22/22 PASS`，逐条 `exitCode=0`、`missingEvidence=[]`、无 `timedOut` | 逐条枚举 | 已证 |
| 场景 | `16/16 PASS`（A01–A16） | 逐条枚举 | 已证 |
| 补充叶 | `126 PASS / 0 BLOCKED / 0 FAIL`；`declaredInStageMap=126`、`deferredToLaterStage=0` | 读 `supplementalLeafCoverage` + 逐叶 `status` | 已证 |
| 候选绑定 | `stable=true`；before/after `digestSha256=0f07f2f7…`、`fileCount=21212`、`manifestSha256=6c92f702…`；22 条逐命令 checkpoint 全部同一 digest、`changedPathBytesHex=[]`；`testedShaAtEnd=aa99d2ec…` | 读收据 + 复算 before/after manifest 文件 sha256（两者字节相同） | 已证 |
| runner 绑定 | `runnerSourceBinding.status=PASS`，before/after digest 一致 | 读收据 | 已证 |
| 测试坐标 | `testedSha=aa99d2ec1dae7f0aef5971bab049734f22fcf7d8`、`worktreeDirty=true`、`toolchainChannel=1.98.1` | 读收据 vs `git rev-parse HEAD` | 已证 |
| 门内耗时自洽 | 命令 `durationMs` 合计 882,687ms，门窗口 894,254ms（19:57:32.907→20:12:27.161，比值 0.987）；`SUITES/` 各 `result.json` mtime 全部落在窗口内且顺序与命令耗时一致 | 复算 | 已证（非伪造时间线） |
| A13/A14 | A13 `assertionResults=68` 全 PASS；A14 `=7` 全 PASS | 读 `scenarios[]` | 已证 |
| A16 基线 | `gitHead=aa99d2ec…`、`rustBinary=b855e5d638aea660…`、`stageMapInputDigest=70b75054…`；当前 `rust/target/debug/lingxi-service` sha256 **逐位相同** | 复算 sha256 | 已证 |

### 与 06 / 07 对比（红应保留、修复不放水）

| 门 | overall | 命令 | 场景 | 叶 | 说明 |
|---|---|---|---|---|---|
| closeout-06 | FAIL | 21/21 | 16/16 | 97 PASS / 29 BLOCKED / 0 FAIL | 唯一失败原因=29 pending |
| closeout-07 | FAIL | **21/22**（红=`r08_a15_isolation`） | **14/16**（A15/A16 连带） | 126/0/0 | 真实红，证据保留 |
| closeout-08 | PASS | 22/22 | 16/16 | 126/0/0 | 修 A15 后全绿 |

- 07 真实红原因独立核实：`verify-r08-closeout-07/SUITES/r08_a15_isolation/output.log` =
  `A15_ISOLATION verdict=ISOLATION_VIOLATED violations=1 defaultOff=true` +
  `VIOLATION V4 r08_ui_pages: argv references real home or repo worktree:
  /Users/…/verify-r08-closeout-07/SUITES/r08_ui_pages`。判定正确（argv 指向仓库工作树）。
- 08 修复方式**不是放宽门禁**：`scripts/rust-tauri/r08_stage_inputs.py:1169-1186` 把 UI 生产者输出改为先落
  `/private/tmp` 合成 scratch（`r08_ui_pages-…/ui-pages-out`）再 `shutil.copy2/copytree` 拷入证据目录；
  08 的 `r08_ui_pages/result.json` argv 确为
  `--out /private/tmp/lingxi-r08-verify/r08_ui_pages-61448-1793016086838/ui-pages-out`。
  A15 的 V4 规则本身未改（`r08_t08_isolation_check.mjs:102-130`，仅把仓库/真实 home 路径判 violating），
  08 的 `suiteArgvSweep` 19 个已落盘套件 `suspicious=[]`。

---

## 二、126 叶逐断言覆盖独立复核（已证）

方法：写一次性只读脚本 `tooling/verify_coverage.py`，**不导入生成器**，直接从 R00 冻结账本
（`docs/rust-tauri/R00/ACCEPTANCE_MAP.json` + `FEATURE_STAGE_ACCEPTANCE.json`）、阶段图 `R08.json`、
门-08 收据、清单 `CLOSEOUT_CHECKLIST.json` 与 3 份 suite 原始案例文件交叉核对。结果见
`results/coverage-result.json`：**findings = []**（0 项）。

| 检查项 | 结果 |
|---|---|
| 叶集合一致（R00 两本账 = 阶段图 = 门收据 = 清单） | 均 **126**，集合相等 |
| 每叶 `original.assertions` 与 R00 冻结断言逐字相同（顺序/数量/文本）、`then`/`taskIds`/`due` 相同 | 126/126 通过（门收据 `r00Mirror`、阶段图 `r00*`、清单 `original` 三处一致） |
| 每叶 `originalAssertionCases` 组数 = 原断言数、每组非空 | 126/126 通过（共 281 组） |
| 叶内 case 不复用（同一 case 不得出现两次） | 0 复用（`verify.rs:1018-1035` 也会硬拒，本轮实测无触发） |
| 清单 `assertionCases` flatten = `cases[]` = 阶段图 `assertionContract.cases` = 门收据 `assertionResults` | 126/126 通过 |
| 每个 case 在**本轮门原始证据文件**中 `ok=true / actual=expect=1` 且整份证据文件全部为真 | 350（AB）+87（UI）+12（CLI）全部通过 |
| `leaf::find.bridge-redaction` 门内 cargo 旁证 | `SUITES/r08_c3_find_bridge_redaction/output.log` 含 `test c3_find_redacts_bridge_time_tags_only_for_bridge_bound_sessions ... ok`；套件 exit 0 |
| 生产者分布 | AB 118 叶 / UI 5 叶 / CLI 3 叶；域 D01 23 / D02 66 / D03 19 / D04 12 / D13 3 / D20 3 = 126 |
| 规模 | 281 条原断言 → 487 个 case 实例 → 449 个不同 case 名（跨叶共用 21 个，叶内 0 复用，符合门规） |

> 说明：`CLOSEOUT_CHECKLIST.json` 的 `summary={PASS:126}`、`gateSummary`（overall=PASS、22/22、16/16、
> stable=true、testedSha=aa99d2ec…）与门-08 收据一致；`generatedFrom.gateResult` 指向门-08。

---

## 三、A/B 原始观测反查（已证）

门-08 的 `r08_t07_ab` 原始 scratch（`/private/tmp/lingxi-r08-verify/r08_t07_ab-61788-1793016143174/out/`）
仍在，本审查写独立脚本 `tooling/derive_check_full.py` 从 `ab-node.json`/`ab-rust.json` 的 `observed` **重算全部叶图钉**
（泛化 `cases:{name:bool}` 规则 + 手写派生表 + ADJ-4/5 Rust-authoritative 口径 + cargo 旁证分开核）：

- 门内 `cases.json` 共 **350** 个案例；独立重算可覆盖 **349** 个（余 1 个 `leaf::find.bridge-redaction` 由 cargo 日志单证），
  **逐条 actual 与门内记录完全一致，mismatch=0**（`results/ab-derivation-full.json`）。
- `ab-compare.json` 的 `summary`：`tasks=71, parity=68, differs=0, recorded=3, failedA=0, failedB=0`；
  recorded 任务恰为 `leaf.slash`（ADJ-5）、`leaf.slash.new`（ADJ-4）、`leaf.ws.terminal`（D13 传输面旧任务），
  三者均在 `RECORDED_APPROVALS` 登记；`differs=[]`。**
- ADJ-4/5 案例按裁决以 Rust 腿判真，A 腿 false 值如实保留：如
  `leaf.slash.new.slash.new.rotates-and-archives` A=false/B=true、`slash.reset.history-cleared` A=false/B=true；
  与 `CLOSEOUT_ADJUDICATIONS.md` ADJ-4/5 一致。
- 3 个扩展任务（`leaf.terminal.positive` / `leaf.tool.ask-user` / `leaf.chat.task-abort`）由
  `EXPECTED_EXTRA_TASKS`（`r08_stage_inputs.py:898,1334-1337`）强制要求双腿观测存在，缺失即 guard fail；
  本门均存在且 leaf case 全真。

---

## 四、关键断言抽样语义复核（已证为主，含边界）

按任务书要求从 ask_user 5 断言、task abort 2 断言、terminal 三叶 6 断言、UI 五叶抽样，实际抽读 **18 组**，逐组读
真实测试/证据（非只看名字）：

| # | 断言组 | 证据实现（实际读取） | 语义判定 |
|---|---|---|---|
| 1 | ask_user A0 卡片可见 | `r08-t07-interactive-ab.test.ts:374-396`：真实工具回合产生 card（旧栈 WS content_block / Rust interactions 轮询面），用真实 `readAskUserForm` 解析问题/选项/推荐项/待答面 | 已证（强） |
| 2 | ask_user A1 提交 key/label 映射 | 同文件 `397-433`：真实 answer 路由提交，读旧栈转录/Rust history 的 details，逐字段断言 answered/keys/values/labels/未知值丢弃/可选未答/无自动选中/待答清除/回合完成 | 已证（强） |
| 3 | ask_user A2 超时只填推荐项 | 同文件 `449-480`：真实 `timeout_seconds=30`（两内核 clamp）真实等待；断言 timedOut/autoSelected、推荐值映射、非推荐不填、待答清除 | 已证（强） |
| 4 | ask_user A3 拒答无答案 | 同文件 `482-511`：真实 answer 路由 `dismissed/rejected`，断言 answered=false、dismissed=true、answers=[]、待答清除 | 已证 |
| 5 | ask_user A3 中止 + A4 不编造 | 同文件 `513-545`：真实 WS abort，待答问题离场；工具结果 null 或 answers 空、回显无被选答案；README 明确“中止后旧栈未落 details=null” | 已证（否定式；已披露） |
| 6 | ask_user A4 无效/缺会话显式错误 | 同文件 `434-447,547-562`：真实工具回合（重复 key）→ 错误回显且不产卡；settled/ghost/missing 路由 4xx；A4「ConfirmStore 不可用」无活体开关，**已核**旧栈 `tests/ask-user-tool.test.ts:130-135` 与 Rust `rust/crates/lingxi-service/tests/r06_interactive_completion.rs:355-370` 覆盖（披露诚实） | 已证（子句经既有专属测试） |
| 7 | task abort A0 终止本会话任务 | `r08-t07-interactive-ab.test.ts:608-627`：用 `R08-TOOL-PLAN` 真实派出后台子代理任务，拿真实 taskId 后调真实路由；断言 200 + aborted/already_stopped、响应键白名单不泄露内容 | 已证（强） |
| 8 | task abort A1 身份/所有权/运行态 | 同文件 `628-656`：未知 404、缺 body 400、坏 token 401/403、二次 abort 显式终态、跨会话 403 不泄露且任务仍由 owner 正常停止 | 已证（强；A/B 差异已披露） |
| 9 | terminal snapshot A0/A1 | `r08-t07-terminal-ab.test.ts:299-364`：真实 PTY（exec_command tty=true）+ 真实 WS 帧；快照含真实 terminalId/status/seq/cwd/command；身份不符显式 error 且窗口内无终端内容；他人会话快照不含本会话终端 | 已证（强） |
| 10 | terminal tail A0/A1 | 同文件 `366-463`：真实 write_stdin 写入 marker 后按 sinceSeq 收到新输出且 lastSeq 增长；身份不符/他人 terminalId 拒绝且无 terminal_tail 泄漏 | 已证（强） |
| 11 | terminal close A0/A1 | 同文件 `465-552`：关闭返回 killed；心跳文件（每 0.2s 追加）在关闭后 1.2s/1.0s 两次采样大小不变=真实进程被终止；未知/他人/身份不符一律 rejected 且他人终端仍 running。manager 不可用分支披露由 Rust 单测覆盖 | 已证（强；子句披露诚实） |
| 12 | UI chat a2 恢复流控件态 | `r08-ui-chat-page.test.tsx:343-379`：真实 `handleServerMessage` 分发 status → store → 真实 ChatPage 控件 send/stop 切换 | 已证 |
| 13 | UI map a1 卡片切会话/缓存 | `session-map-live/main.cjs`（真 Electron+真 lingxi-service，Network debugger 抓真实请求）：同一卡片零 switch/history、changed revision 真读 history、archive 拒绝保留当前会话等，check 在断言后 push | 已证（强） |
| 14 | UI MCP a3 批量混合/a5 权限保存失败 | `r08-ui-settings-mcp.test.tsx:680-771`：真实 `McpTab`+settings store，仅 fetch 传输 mock；断言逐行成功/runtimeApplied=false、无全成功 toast、失败重读后仍未授权 | 已证 |
| 15 | UI security a1 沙箱开关 | `r08-ui-settings-security.test.tsx:139-192`：真实 `SecurityTab`；断言 PUT body、重读 GET、失败恢复、未初始化 loading 不发请求、PUT 成功但快照失败如实报错 | 已证 |
| 16 | UI quick-chat a1 失败不伪称成功 | `quickchat-live/main.cjs:33-38`：真实双 Electron 窗口；销毁主窗/不存在会话/晚到 ACK 均断言失败且浮窗保留（含 `QUICK_CHAT_MAIN_UNAVAILABLE`） | 已证（强） |
| 17 | D20 CLI checkpoints/restore/diagnose | `tests/cli-data.test.ts:227-260`（真实 checkpoint 列目录/跳过损坏）、`324-364`（真实 restore 事务+隔离+日志）、`199-214`（corrupt 显式报告且不改字节） | 已证（强） |
| 18 | find.bridge-redaction | `rust/crates/lingxi-service/tests/r08_c3_find_bridge_redaction.rs:77-190`：真实 TCP/HTTP/SQLite，bridge 会话脱 `<t>…</t>`、普通会话保留、无效会话 403/404 | 已证（强） |

> 上述 18 组均能在门内 `cases.json` 找到对应 case 名且值为 1；第 18 组的 cargo 日志行已单独核对（§二）。

---

## 五、门禁 / 白名单放水核查（已证：未发现放水）

- **未提交 diff 只增不减关键规则**：
  - `scripts/rust-tauri/r08_t07_ab_drill.mjs`：`RULES` 由 HEAD 的 **35** 项 → 现在 **68** 项，**removed=[]**、added 33 项；
    `sessions.switch`/`sessions.create` 由 recorded 收紧为真实对拍；未新增任何 `true` 字面量 case。
  - `rust/crates/xtask/src/`：**未修改任何 xtask 源码**（`git status` 仅 `stage_maps/R08.json` 变化）；
    阶段图 18→22 命令（新增 `r08_a04_ui_vitest`/`r08_c3_find_bridge_redaction`/`r08_cli_data_vitest`/`r08_ui_pages`），
    叶 106 pending → 0 pending，新增门合同 A04=3 / A13=68 / A14=7（HEAD 无合同）；`--check` 整图逐字节比对，无手改空间。
  - `r08_t08_isolation_check.mjs`：由“只产摘要、从不失败”重写为 V1–V5 硬校验（真实 home 前后摘要、默认关、
    21 套件 argv 实查、进程 env），**只收紧未放宽**；07→08 的修复只改生产者输出位置，V4 规则未改（§一）。
  - 生成器新增“只注册不引用/引用未注册”即 SystemExit、`--check` 整图字节自检、vitest 关键套件 skipped 即失败
    （`r08_stage_inputs.py:1229-1232,1606-1618,1630-1658`）。
- **recorded 无未批准**：3 个 recorded 任务（`leaf.slash`/`leaf.slash.new`/`leaf.ws.terminal`）全部在
  `RECORDED_APPROVALS` 登记；A13 合同 68 行全 1。`leaf.sessions.admin` 的 recorded 回退分支若触发将因未登记
  直接 guard fail（fail-closed），本门实际 verdict=parity。
- **skip 容忍**：门-08 全部输出日志中仅 `r08_ui_pages/output.log:404` 有 `Tests 2 passed | 21 skipped (23)`
  （composer 文件按 `-t` 精确选取本叶 2 条，生产者与覆盖矩阵均如实披露“其余用例被过滤未运行，不冒充整文件绿”）；
  其余套件 skipped=0。AB 三套件无 `it.skip/todo/only`，`describe.skipIf` 依赖 env，env 缺失会导致观测缺失→A13 合同 0→门红（fail-closed）。
- **A13 合同**：68 = 演习 RULES 全集（65 parity + 3 已批准 recorded），另有 3 个扩展任务由
  `EXPECTED_EXTRA_TASKS` 存在性 guard + 叶 case 双重把关；A14 负路径任务 7 个。均非缩水。

**设计注记（不构成本轮问题，见 §九 F5）**：A13 合同由同一 `RULES` 表派生，且门内 `--suite-files` 使
`subsetMode=true`（`drill:42`），任务集合取“实际观测并集”（`drill:548`）——若未来有人同时从 RULES 与套件删除
一个任务，A13 合同会同步缩小而不报错。本轮的独立反证是：RULES 35→68 无删除、68 个 RULES 任务全部出现在
`ab-compare`、输出 `71 = 68 + 3`，故该潜在缺陷未被触发。

---

## 六、隔离与诚实性（已证）

- `SUITES/r08_a15_isolation/a15-isolation.json`：`verdict=ISOLATED violations=0 defaultOff=true`；
  19 个已落盘套件 argv 全部 `suspicious=[]`（另 r08_inputs_check/r08_a15/a16 自身不在被扫目录）；notes 明确 V1（真实 home 前后一致）与 V2（预览默认关）。
- 真实 `~/.lingxi`：门首命令 `R08_INPUTS/real-home-before.json` 与 a15 事后快照 **topLevel 逐项相同、
  `data-epoch.json` sha256 相同**（`88ebeb5c…`）；本审查复跑后再次比对当前 `~/.lingxi/data-epoch.json` 与
  门内快照仍逐位相同、顶层条目数仍 57。证据中仅 a15 自身记录出现真实 home 路径（预期）。
- `worktreeDirty=true` 与“未提交工作树是唯一副本”一致：HEAD 未变、无 commit；候选 before/after manifest
  字节相同（sha256 `6c92f702…`）；**门后至今被改动的文件仅 6 个收尾文档**
  （`CLOSEOUT_CHECKLIST.json`/`CLOSEOUT_PROGRESS.md`/`R08_DECISION_BRIEF.md`/`R08_HANDOFF.json`/
  `R08_REPORT.md`/`T07_UI_DIFFERENCE_REPORT.md`），全部候选源码/脚本/阶段图/测试与门内 manifest **逐文件 sha256 相同**
  （`results/manifest-compare.json`：`changed` 仅上述 6 文档、`missing=[]`；manifest 未收录的 gitignored 文件（.DS_Store、历史 artifacts 子目录等）按候选绑定策略不属于受测候选，不参与比对）。与 HANDOFF `gitHeadNote`
  “收尾文档与受测候选分离、不冒称已测”一致。

---

## 七、可复跑抽检（独立复跑，未触碰正式门）

| 抽检 | 命令 | 结果 |
|---|---|---|
| UI 生产者全套重跑 | `python3 -B scripts/rust-tauri/r08_stage_inputs.py --run-suite r08_ui_pages --evidence /private/tmp/review-ui-pages-indep-01` | **exit 0，guardFailures=0，51.93s**；`87 cases / 87 ok`；与门内 `ui-cases.json` **case 集合相等、逐 case 值 diff=0**；真 Electron live `quickchat 9 checks PASS`、`session-map 19 checks PASS` |
| 终端 A/B 单任务重跑 | `LINGXI_R08_AB_TEST_FILTER='leaf.terminal.positive' node scripts/rust-tauri/r08_t07_ab_drill.mjs --bin rust/target/debug/lingxi-service --work /private/tmp/review-terminal-ab-01 --suite-files desktop/src/react/__tests__/r08/r08-t07-terminal-ab.test.ts` | **exit 0**，`AB_RESULT tasks=1 parity=1 differs=0 recorded=0 failedA=0 failedB=0`；14/14 case 双腿全 true；关闭后心跳文件大小 before=after1=after2（A 185/B 195 字节），他人终端 `terminalStillRunning=true` |
| 生成器自检 | `python3 -B scripts/rust-tauri/r08_stage_inputs.py --check` | **exit 0**，`inputs digest match`（另 `inputs-check.json`：`digestMatch=true`、`graphByteMatch=true`、digest `70b75054…`） |
| 二进制基线 | `shasum -a 256 rust/target/debug/lingxi-service` | `b855e5d6…`，与门-08 `a16-versions.json` 的 `rustBinary` **一致**（复跑与门同二进制） |
| 真实 home 复核 | 复跑后 `~/.lingxi/data-epoch.json` sha256 | 仍为 `88ebeb5c…`，与门内 before/after 快照相同 |

复跑证据已存本目录：`results/rerun-ui-pages-*.json`、`results/rerun-terminal-ab-summary.json`、
`results/rerun-*.log`。复跑结束后确认无遗留 `lingxi-service`/Electron/vitest 进程。

---

## 八、文档一致性（已证，附 F2/F3 小瑕疵）

| 文档 | 关键数字 | 与门-08 一致性 |
|---|---|---|
| `CLOSEOUT_CHECKLIST.json` | summary PASS=126；gateSummary overall=PASS/22=22/16=16/stable/testedSha | 一致 |
| `R08_HANDOFF.json` | stageOutcome 含 22/22、16/16、126 PASS、0 BLOCKED、0 FAIL、stable=true、overall=PASS、71=68+3+0、0 differs；gitHead=aa99d2ec…；按域 23/66/19/12/3/3=126；pending=0；07 21/22 红回执保留；08 PASS 回执在列 | 一致 |
| `R08_REPORT.md §十` | 22/22、16/16、126/0/0、stable、A15 violations=0、A16 head/rust、71=68+3+0、overall=PASS；§八/§九 标注为历史快照 | 一致 |
| `R08_DECISION_BRIEF.md` | 22/22、16/16、stable、126/126（0/0）、71=68+3（0 differs）、07 红保留 | 一致 |
| `CLOSEOUT_PROGRESS.md` 末段 | 07 红原因（A15 V4 argv 指仓库）、08 修复与结果、域分布 | 一致 |
| `CLOSEOUT_ADJUDICATIONS.md` | ADJ-1..5 执行方式；R00 未改（`git status` 无 R00 改动） | 一致；门收据逐叶带 ADJ 注记 |

---

## 九、保留项（未证 / 已披露偏差 / 文档与设计小项 / 无法证）

### F1（**未证·子句级**，建议 R09 记录；不阻断门级判定）——slash 族的“接管态/无历史”子句未直接取证且未披露

R00 冻结断言中有 4–5 叶的“拒绝/边界”断言含子句，但门内图钉案例只覆盖其中另一子句：

| 叶 | 原断言（R00 原文） | 图钉 case | 实际证据缺口 |
|---|---|---|---|
| `R00-T02-LA-485B2D0A8A02` /new | A1「接管态拒绝，未找到当前会话明确提示」；A0 末句「无历史时仍创建新会话」 | `slash.new.invalid-session-rejected`（+A0 三 case） | 仅测“不存在会话拒绝”；**无 bridge 接管态场景**，也**未测“目标无历史”分支**。Rust 实现存在（`slash.rs:641-702`），但**无任何测试引用该分支**（全仓 grep 无 `/new` + attachment 测试） |
| `R00-T02-LA-3FCB259FF69C` /reset | A1「接管态拒绝；未找到时不删除其他会话」 | `slash.reset.invalid-session-rejected` | 接管态拒绝由**仓库既有 Rust 集成测试** `tests/r06_slash_completion.rs:708-745` 覆盖（attached `/reset`+`/fresh-compact` 拒绝并保持三会话快照），但该测试**不在门-08 命令内**，非本轮证据；“他话不受扰”仅以 overview 200 弱验证 |
| `R00-T02-LA-5A6E0D526903` /fresh-compact | A1「接管态拒绝；失败明确回报且原始历史保留」 | `invalid-session-explicit-error`、`original-history-retained` | 接管态拒绝同上由 r06 测试覆盖（非门内）；AB 案例覆盖失败回报+历史保留 |
| `R00-T02-LA-4EA88B5073E4` /stop | A1「接管态仅中止被接管桌面会话，不误停 Bridge 自身其他会话」 | `slash.stop.idle-reports-already-stopped`、`other-session-unaffected`（`kernel-ab.test.ts:3100-3110`） | 覆盖“空闲已停止”与“另一会话不受扰”，但**非接管态场景** |
| `R00-T02-LA-0FE8BED686E2` /compact | A1「失败回传压缩失败；接管态目标为被接管桌面会话」 | `slash.compact.invalid-session-explicit-error` | 后句更像上下文/边界说明；失败回报已覆盖 |

- 性质：这些叶仍满足**门规**（每条原断言 ≥1 个专属且为真的 case；`FULL_BINDINGS` 中逐组登记），因此不是门级 FAIL；
  但“126/126 full_original_behavior / 逐断言专属案例全覆盖”的**语义外延**在子句层面存在缺口，且
  `CLOSEOUT_BLOCKED.md`/`CLOSEOUT_PROGRESS.md`/`RECORDED_APPROVALS`/UI 矩阵**均未披露**这几处子句。
- 建议：R09 记录为已知残差（或在 bridge-takeover 场景下补 1 个 AB 用例使 /new、/stop 子句闭合；
  /reset、/fresh-compact 已可由既有 r06 测试补跑）。

### F2（文档·小）——`CLOSEOUT_CHECKLIST.json` 未携带 ADJ 裁决注记

- 门-08 的 `supplementalLeafScenarios[].evidenceRequired` 对 `485B2D0A8A02`/`3FCB259FF69C`/`25C4FF66FEE5`
  带有 ADJ-4/5/1 的权威注记（`r08_stage_inputs.py:1560-1566`），但 `CLOSEOUT_CHECKLIST.json` 的 126 个 item 均**不含** `evidenceRequired` 字段（清单生成器 `r08_closeout_checklist.py` 未携带该字段）。不影响断言/门判，但清单作为
  交付摘要丢失了“改说明书”这一关键语义订正记录。

### F3（文档·小）——`RECORDED_APPROVALS` 中 `leaf.prompt-snapshot` 的 basis 已过时

- 该登记文本仍写“叶 FF4F06011B48 保持 pending，待聊天线束…”，实际该叶已按 `leaf.prompt-snapshot.real`
  （真实模型回合后的快照）绑定为 full_original_behavior；本门 `leaf.prompt-snapshot` 任务 verdict=parity
  （旧 recorded 分支未触发），登记项成为未使用条目。属文档陈旧，不影响门。

### F4（已披露偏差，需 R09 文档对账）——UI 两处“负向子句”按现役实现验收

- R00 原文：MCP 叶 A1「连接器或权限读取失败**仅 console.error**，保留旧状态，**无专门错误态或 toast**」；
  安全叶 A2「检查点列表 GET 失败**直接设为空数组且无警告**；页面可能呈假空态，**不能要求显示读取错误**」。
- 现役实现（HEAD 未改文件 `McpTab.tsx:310`、`SecurityTab.tsx:208` 起）会显示 `role="alert"` 错误条 + 重试；
  `R08_UI_COVERAGE_MATRIX.md` §未覆盖/差异 1、4 已明确披露“按现役行为记录，不把旧描述当验收”。
- 判定：披露诚实、非冒充；但这两条负向子句在字面上**未按 R00 原文满足**，属“现役行为已改进/漂移”的文档对账问题。
  建议 R09 按 ADJ-1 同机制（改说明书）修订语义，避免“验旧文/测新行为”的口径分叉。

### F5（门禁设计注记·潜在，本轮未触发）

- A13 合同任务集由同一 drill 的 `RULES` 派生（`r08_stage_inputs.py:900-913` 解析 RULES；`1497-1527` 生成合同），
  且门内 `--suite-files` 使 `subsetMode=true`、比较任务取“观测并集”（`r08_t07_ab_drill.mjs:42,548`）。
  若未来版本同时删除一个 RULES 任务与其套件测试，A13 合同会同步缩小而不报错（当前由“缺失 verdict→0”覆盖
  RULES 内任务，但不覆盖“任务被整体移除”）。本轮反证充分：RULES 35→68 无删除、68 任务全部观测、71=68+3。
  建议后续门把“预期任务集”独立于 drill RULES 固定登记（如生成器侧常量），或对 RULES 数量/键做跨轮快照比对。

### F6（**无法证**边界）

- 未逐条人工复读全部 281 条原断言的**每个子句**（按任务书抽读 18 组 + 机械全量核对）；其余断言的子句级
  语义强度本报告不能担保，仅能以机械覆盖（每组非空、case 全真、无复用）与抽样为限。
- `CLOSEOUT_ADJUDICATIONS.md` 所引“业主裁定原文”的真实性无法独立核验（以仓库文档为准）；ADJ-4/5 的 recorded
  合法性依赖该登记。
- 正式门未重跑（遵守任务书）；本审查以收据自洽性（时间线、manifest、命令耗时、证据时间戳）+ 两条生产者独立复跑
  替代，不能排除“门级证据与完整重跑之间的未覆盖差异”。

---

## 十、判定理由与后续条件

1. **门级硬指标全部独立成立**：overall=PASS、命令 22/22、场景 16/16、叶 126/0/0、候选绑定 stable、
   in-gate A/B 68 parity+3 recorded+0 differs、隔离 violations=0；06/07 的真实红历史保留且 08 修复未放宽门禁。
2. **逐叶断言覆盖与真值经独立脚本重算 0 缺口**（不依赖生成器/收据派生物）；A/B 350 case 中 349 可由原始观测复算、
   1 由 cargo 日志单证；UI 87 case 由独立复跑逐 case 复现。
3. **未发现门禁/白名单放水**：RULES 只增不减、记录审批齐全、A13/A14 合同扩充、`--check` 整图字节一致、
   xtask 源码未改、skip 防护加强。
4. **保留项 F1/F4 是子句级/文档级**，在“门规=每条原断言≥1 专属 case”的标准下不阻断 R08 收尾；
   但应在 R09 交接中**如实记录**（F1 补证或披露；F4 按 ADJ-1 机制修订 R00 语义或明确现役行为权威），
   不应在交接与后续审计中把它们说成“已逐子句证明”。
5. **ADMIT_R09 的前置/限定**（沿用 HANDOFF 口径）：本会话未 commit/push；提交/推送/发布仍需业主另行指令；
   R09 不得重改模型协议或数据架构；R09 以门-08 `a16-versions.json` 为对照基线。

> 结论：**ADMIT_R09（附记录项 F1–F6）**。本判定不替代业主的提交/发布授权。

---

## 附录 A：本审查实际执行的命令与退出码

| # | 命令 | 退出码 / 结果 |
|---|---|---|
| 1 | `git rev-parse HEAD` / `git status --porcelain` | `aa99d2ec…` / 79 项（与审查开始相同；无 commit） |
| 2 | `python3 -B scripts/rust-tauri/r08_stage_inputs.py --check` | 0；`inputs digest match` |
| 3 | `python3 -B /private/tmp/review-independent/verify_coverage.py`（副本见 `tooling/`） | 0；`findings=[]`（126 叶/281 断言/487 实例/449 case） |
| 4 | `python3 -B …/derive_check_full.py` | 0；349/349 可复算 case 一致、mismatch=0 |
| 5 | `python3 -B …/manifest_compare.py` | 0；门后仅 6 个收尾文档变化、无源码/证据文件缺失 |
| 6 | `python3 -B …/r08_stage_inputs.py --run-suite r08_ui_pages --evidence /private/tmp/review-ui-pages-indep-01` | **0**，51.93s，87/87，与门内 diff=0 |
| 7 | `LINGXI_R08_AB_TEST_FILTER='leaf.terminal.positive' node …r08_t07_ab_drill.mjs --bin … --work /private/tmp/review-terminal-ab-01 --suite-files …r08-t07-terminal-ab.test.ts` | **0**，tasks=1 parity=1，14/14 case 真 |
| 8 | `shasum -a 256 rust/target/debug/lingxi-service` | `b855e5d6…` = 门-08 A16 baseline |
| 9 | `cat/read` 门-06/07/08 收据、A15 日志、SUITES 原始案例、A/B scratch 观测、源码与测试（抽样清单见 §四） | 只读 |

## 附录 B：本目录证据清单

- `INDEPENDENT_REVIEW_R08_CLOSEOUT.md` / `.json`：本报告（md + 机器可读）
- `tooling/verify_coverage.py`：126 叶覆盖独立复核脚本（不导入生成器）
- `tooling/derive_check_full.py`：从门-08 A/B 原始观测独立复算叶图钉
- `tooling/manifest_compare.py`：当前工作树 vs 门候选 manifest 对比
- `results/coverage-result.json`：覆盖复核结果（findings=[]）
- `results/ab-derivation-full.json`：A/B 复算结果（349 verified / 0 mismatch）
- `results/manifest-compare.json`：门后变更清单（仅 6 文档）
- `results/rerun-ui-pages-summary.json` / `results/rerun-ui-pages-suite-receipt.json` / `results/rerun-ui-pages.log`
- `results/rerun-terminal-ab-summary.json` / `results/rerun-terminal-ab.log`

复跑原始 scratch 仍在：`/private/tmp/review-ui-pages-indep-01/`、`/private/tmp/review-terminal-ab-01/`，
门-08 A/B 原始观测：`/private/tmp/lingxi-r08-verify/r08_t07_ab-61788-1793016143174/out/`。

## 附录 C：本次审查未做的事（边界复述）

未重跑正式门 `verify-stage R08`；未修改任何候选源码/文档/阶段图/测试/门禁；未 commit/push；
未读取或写入真实 `~/.lingxi`（仅只读 sha256 与顶层条目对比）；未启动 ZCode；未对 R09 内容作任何实施或预判。
