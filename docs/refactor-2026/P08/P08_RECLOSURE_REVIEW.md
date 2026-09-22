# P08 收口复验收报告（P08-RECLOSURE，FIXR1 后）——P00–P08 九阶段收口轮

日期：2026-09-22｜验收者=独立复验收子代理（全新上下文，与执行者/验收者/修复者均无共享会话；只读验收，不改生产代码/测试/历史交付文档；唯一写入=本报告与 docs 清单注记刷新）｜候选 HEAD `674b0151f`（P08 全部轮次产物在未提交工作区）。

范围：FIXR1 三处更正闭合的独立核实（不采信任何一方自报）、分歧裁决、FIXR1 范围纪律、清单独立复算、定向抽查、反例构造，以及 P08 阶段终判与编排层统一提交条件结论。

## 1. 三处更正闭合的独立核实

### ① STRICT_FINAL_SCOPE.json 宽松区账本（第 4 处 any 归属）——闭合

全部实测（grep + git blame + git show 基线比对）：

- scan_method 原口径（`git diff 92c6646c5..HEAD` 生产目录 +行 grep `: any|@ts-ignore|@ts-nocheck|as any`）复算：**恰好 4 命中**——`lib/task-registry.ts` update/complete/fail 三行 `options: any = {}` + `server/hono-helpers.ts` 一行 `const error: any = new Error(message)`。
- `server/hono-helpers.ts:37` 实测为 `const error: any = new Error(message);`（httpJsonError 内，error.status/error.code 赋值形态）；`git blame -L 37,37` = **e2392f301 = refactor(P01)** ✓。
- `lib/task-registry.ts` :169/:197/:218 实测为三条 options 参数；`git blame` 三行均 = **9a19740ba = refactor(P02)** ✓；基线（92c6646c5）update 签名为 `update(taskId, patch: any = {})`——`patch: any` 为既有风格，本轮仅加 options，账本"与相邻既有 patch: any 同风格"表述属实。
- 初版账本误写的 `lib/task-registry.ts:788`：该文件实测 635 行且无 `const error: any` 行——原记录确系归属笔误，FIXR1 更正成立。
- **反例（宽口径找第 5 处漏项）**：`any[]`/`<any>`/无空格 `:any` 新增行 0 命中；新增 `.d.ts/.d.cts` 仅 `shared/hana-runtime-paths.d.cts`（纯函数签名，无 any）；更宽 `\bany\b` 额外 2 命中均为 JSDoc 注释文字（findModel 类型化改造附近的说明，方向是收紧）；`@ts-expect-error` 本轮生产新增 0（现存 2 处：i18n 测试文件 + `hub/agent-executor.ts:533`，后者 git blame 为 d5275e568 / 2026-08-05 基线既有）。**未发现第 5 处真漏项**；FIXR1 any-scan 日志另登记了基线存量同款行（session-coordinator 25 处等），归属区分正确。
- 账本拆分后总数（4）、宽松区定性（非为清错批量加 any）、scan_method 口径未变 ✓。

### ② 计数更正（33→34、P06 12→11）——闭合

- `command-log.jsonl` 实测 **57 行**，逐 command_id 枚举：执行轮（P08-T* / P08-P07FB）**34** + 验收轮（P08-ACCEPT-*）**19** + FIXR1（P08-FIXR1-*）**4**，与 FIXR1 自报一致。
- P08_REPORT 验证节算式复核：full-test 1 + product-regression 族 6（base/r2/r3/r4/r5/r6）+ pack 族 2 + lint 族 2 = 11 显式，11 + 23 = 34 ✓（「其余 23 条」吻合）。
- `docs/refactor-2026/P06/EVIDENCE_SHA256.txt` 实测 = 1 行头注 + **11 条哈希行** ✓。
- **反例（残留扫描）**：「33 条」「其余 22 条」「P06 12 条」在 P08 全部交付物中的残留出现均为 FIXR1 更正注记的组成部分（"原记 33 条为计数笔误"），**无活引用残留**。
- 观察项（非阻塞）：`P08_RESULT.json`/`FINAL_RESULT.json` commands 字段「6 条首败-重跑链」——实测非预期失败事件为 **7** 个（product-regression ×5 + pack ×1 + lint ×1），「6」的计数口径未明示（可能未计某个诊断步）。原始日志全留档、可自行复算，不影响任何结论。

### ③ 锚点措辞精确化——闭合

- `FINAL_CALLSITE_MATRIX.json` note 与 E-DESKTOP `p08_verification`：已从「锚点行号与 P00（完全/逐字）一致」精确化为「核心链文件零改动、行号未移位；**深 5 锚点**与 P00 原始证据逐字一致」+ 锚点选取差异登记（E-DESKTOP 段位差异 / E-CRON 触发位 vs 定义位）。
- 深 5 锚点独立实测（当前 HEAD sed 与 P00 `agent-callsite-raw.md` 原文比对）：InputArea.tsx:1814 submitEditorMessage / chat.ts:2672 prompt|interject 分支 / hub/index.ts:213 Hub.send / desktop-session-submit.ts:385 submitDesktopSessionMessage / session-coordinator.ts:5176 promptSession——**全部逐字命中**，保留精确表述正当。
- 其余「完全一致」出现处均为其他事实且属实：14797 绿数（P08 未新增测试文件）、lockfile sha256（实测 `a9735825` 与研究基线 8037fae7a6 的 package-lock.json 完全一致）。
- 附加抽查：验收轮声称的其余锚点 engine.ts:4119（new ToolInvocationGateway）/ gateway:469（executeCanonical）/ pi-sdk:78（createAgentSession）/ bridge-manager:1995/:2124（hub.send）均实测命中。

## 2. 分歧裁决（验收轮 §4-③ 子论据 vs FIXR1 反驳）——**FIXR1 成立**

全部独立实测，不采信任一方自报：

| 争点 | 验收轮说法 | FIXR1 说法 | 独立实测 | 裁决 |
|---|---|---|---|---|
| composer-send.ts 是否「旧文件名」 | P00 基线时该文件已名 composer-send-coordinator.ts | 两文件基线并存 | `git ls-tree 92c6646c5`：`components/input/composer-send.ts`（blob ee207078）与 `services/composer-send-coordinator.ts`（blob c9eecfe2）**基线即并存**，两 blob 与 HEAD 完全相同；P00 raw log 第 6–7 行**同时引用两文件**（coordinator :1000/:1010/:1036 + composer-send :198/:553/:577）；P00 `CALLSITE_MATRIX.json` callsites[0] 原文「composer-send.ts:577 ws.send」 | **验收轮不成立** |
| :577 是否陈旧锚点 | 陈旧（旧文件名推论） | 在位且零移位 | `git diff 92c6646c5..HEAD -- composer-send.ts` = **0 行**；基线与 HEAD `:577` 均为 `ws.send(JSON.stringify(prepared.wsMsg));` | **验收轮不成立** |
| scheduler 行号 | 陈旧锚点 | 选取差异 | :126/:163/:168/:194/:442 当前 HEAD 全部在位且指向宣称符号（实测 sed 六行）；P00 raw 同时含 :163/:168 与 :126/:194/:442 | **验收轮不成立**（两者皆真，属选取差异） |

处置合规性：FIXR1 仅更正自身措辞、**不回改 P00 历史文档**（实测 `git status` P00 docs+artifacts 目录零修改）与验收报告原文（P08_ACCEPTANCE_REVIEW.md §4-③ 保留原表述），分歧在 FIXR1 条目（P08_RESULT/ACCEPTANCE_MAP/FINAL_CALLSITE_MATRIX/P08_REPORT）如实登记并注明「该解释登记为验收轮误读」——符合通用约束 §9「不能修改历史日志、旧 PASS 证明来冒充」与「历史证据不改写 + 如实登记」原则。注意：验收轮瑕疵③的**主判断（措辞过强需精确化）依然正确并已执行**，仅其两个子论据（旧文件名/陈旧锚点）经实测不成立；FIXR1 对主判断与子论据分别处理的方式准确。

## 3. FIXR1 范围纪律——合规

- 12 个 tracked 修改（6×P07 bench 工具、P07 logs 清单、P04 清单+手接、P06 三件）mtime 实测 15:38–15:44（执行轮 T07 收口窗口），全部早于 FIXR1 文档窗口（16:27–16:31）；FIXR1 写入仅在未跟踪 P08 目录（docs 5 件更新 + logs 追加 command-log 4 条与 8 件 P08-FIXR1-* 证据）。**12 个 tracked 修改未被 FIXR1 触碰**。
- P00 历史交付零触碰（git status 实测 0 行）；P04/P06 的 tracked 修改为执行轮 T07 的带注记合法更正（验收轮 §1.1 已逐文件核，本轮复认 diff 全部位于 docs/artifacts 证据目录、无生产代码/测试/package.json/tsconfig 改动）。
- `git worktree list` 仅主树（演练 worktree 已清理）；skills2set pycache 删除为执行轮动作（验收轮 §1.2 已核，tracked 技能文件原样）。

## 4. 清单独立复算与 JSON 合法性——全绿

`shasum -a 256 -c`（复验收者独立执行，真实 exit code）：

| 清单 | 条目 | 结果 |
|---|---|---|
| docs/refactor-2026/P08/EVIDENCE_SHA256.txt | 12（1 头注 + 12 哈希） | 12/12 OK，exit 0 |
| artifacts/refactor-2026/P08/logs/EVIDENCE_SHA256.txt | 105 | 105/105 OK，exit 0 |
| docs/refactor-2026/P04/EVIDENCE_SHA256.txt | 70 | 70/70 OK，exit 0 |
| docs/refactor-2026/P06/EVIDENCE_SHA256.txt | 11 | 11/11 OK，exit 0 |
| artifacts/refactor-2026/P07/logs/EVIDENCE_SHA256.txt | 147 | 147/147 OK，exit 0 |

- logs 目录文件差集自洽：106 个非清单文件 = 105 条目 + 1 自指豁免（`P08-FIXR1-manifest-check.out`，清单头注已声明）。
- JSON 合法性：P08 docs 8 个 JSON + P06 被修改的 2 个 JSON 全部 `json.load` 通过。

## 5. 定向抽查

- **f1 patch**：`artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` sha256 实测 = `25fb315f6c5d…`，与 P05/P07/P08 各报告引用一致（未被 R10-09 重写残留污染）；patch 目标文件（workflows 等）git status 干净。
- LEGACY_EXIT_LEDGER 定向锚点：L1 `cli/local-server.ts:3-11` 仅 import 权威实现 ✓；四常驻工具 `shared/tool-categories.ts:103-110` RESIDENT_CORE_TOOL_NAMES 精确 read/write/edit/exec_command ✓；账本引用的 tests/task-identity-attempts.test.ts + p02-*.test.ts（6 文件）在位 ✓。
- 未跑全量 npm test 与 pack（按本轮约束；全量与产物证据以执行轮+验收轮双跑日志为准，本轮复算了其产物清单哈希）。

## 6. 本轮新增发现（非阻塞，均为证据文档级）

1. **悬空引用 ×2**：`STRICT_FINAL_SCOPE.json` test_merge_mapping 引用「FINAL_REPORT.md §功能→契约→代码→测试」、`INDEPENDENT_REVIEW.md` §5 引用「FINAL_REPORT.md / FINAL_RESULT.json」——`FINAL_REPORT.md` 不存在（任务书 T07 建议名，实际职责由 P08_REPORT.md 承担，92 模板自证「文件建议」措辞、通用约束 §2 文档名为建议；但 P08_REPORT.md 亦无「功能→契约→代码→测试」索引节，各阶段映射实际散见 ACCEPTANCE_MAP 系文件）。最小修复：两处引用改指 P08_REPORT.md/ACCEPTANCE_MAP.json，或在 P08_REPORT 补索引节。不改任何门禁结果。
2. **「6 条首败-重跑链」口径未明示**（实测 7 个非预期失败事件，见 §1-②观察项）。
3. **status_note「12/14 场景 PASS + 1 BLOCKED + 0 FAIL」分母口径**：ACCEPTANCE_MAP 14 案例中 PASS 系 13（含 4 个带如实受限限定语）、BLOCKED 系 1（A06），summary 计 pass=12/blocked=1——A07（「PASS（macOS arm64 --dir 形态；安装器形态与其它平台 BLOCKED 已单列）」）未计入任何桶，12+1=13≠14。A07 自身状态字段如实（本地形态 PASS、跨平台部分单列 blocked_items），无掩盖；仅汇总算术口径未交代悬挂的 1 例。

以上三项建议随编排层统一提交时最小更正或注记登记，不构成本轮否决项（与验收轮 3 瑕疵同级别或更轻，均为证据文档笔误级）。

## 7. 已知 BLOCKED（按任务书口径单列，均无授权/环境，非本轮新增）

1. P08-A06 四平台候选 SHA CI 证据（ci.yml 仅 PR 触发，无开 PR/push 授权）。
2. F1 审计封印推进（需编排层统一提交产生候选提交 + PROGRESS.md 流程）。
3. 真供应商冒烟 + 真模型行为评测（P04 起继承，无凭证/费用授权）。
4. 桌面 GUI 层验收（无 GUI 自动化授权；协议+套件层已覆盖）。
5. 他平台实机 / DMG/NSIS/AppImage 安装器形态 / 产物级 OTA（无实机、无生产凭据与发布授权）。

## 8. 终判

**P08 阶段（含 FIXR1 修复轮后整体）：PASS（实现与本地验收完成；上列 5 项 BLOCKED 均为真实授权/环境受限且逐项带原因与解锁条件，未写「全部完成」，正式发布准备不标完成）。**

- FIXR1 三处更正（账本归属拆条 / 计数 33→34 与 P06 12→11 / 锚点措辞精确化）经本轮全实测**全部闭合**；分歧裁决 FIXR1 反驳成立，处置符合「历史证据不改写 + 如实登记」。
- 除已知 BLOCKED 外，**无未闭合的阻塞项**；§6 三项新发现均为非阻塞文档级观察。
- 未验证边界（诚实声明）：本轮为证据级复验收，未重跑全量测试/打包/产品回归（执行轮+验收轮双跑日志与清单哈希复算代替）；未验证真供应商、GUI、他平台实机、生产签名链；不承诺绝对无缺陷。

## 9. 编排层统一提交条件结论

**具备。** 依据：①零生产代码改动声称经三轮独立核实（执行/验收/本轮）均属实，`git diff HEAD` 12 个 tracked 修改全在证据目录且带注记；②P00–P07 历史交付零改写（P00 零触碰；P04/P06/P07 修改为执行轮带注记合法更正，验收轮已逐文件核）；③证据链自洽可复算（5 份清单 345 条哈希全绿、command-log 57 条逐 ID 可枚举、f1 patch 哈希一致）；④14 场景 0 FAIL，受限项全部如实标注未冒充；⑤已知 BLOCKED 的解锁路径明确——其中 F1 封印推进明确以「编排层统一提交产生候选提交」为前置，即统一提交正是收口动作本身。§6 三项文档级观察可在提交前顺手最小更正（推荐：两处悬空引用），或随提交注记登记，不构成提交阻塞。

——本轮写入：本报告 + docs/refactor-2026/P08/EVIDENCE_SHA256.txt 补入本报告条目（头注加 RECLOSURE 注记；其余条目哈希不变）。未触碰其他任何文件；未 commit/push/PR/tag。
