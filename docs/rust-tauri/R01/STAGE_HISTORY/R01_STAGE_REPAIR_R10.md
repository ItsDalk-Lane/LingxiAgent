# R01 阶段修复 R10 — 修复报告

- 修复者：ZCode:R01 阶段修复 R10（ZCode 一次性任务）；非 R1–R9 修复者、非 R1–R10 阶段验收者、非任一 T 执行/评审代理。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`；仓库 `ItsDalk-Lane/LingxiAgent`。
- 验收输入：`/tmp/r01-stage-review-r10.md`，SHA-256 `c6433031f5e2c94bcba28a1d052dcd0f53f10eb9bc3193f3c2c61d8709052aa3`（本机实算吻合）。对抗矩阵 `/tmp/r01-r10-adversarial.json`（SHA-256 `1a5354f7baf2036a1cb9c30b83b041e362dcd199cb370a26265732f9faec54f3`）与四引号对照 `/tmp/r01-r10-quote-pairs.json`（SHA-256 `db8b391410fa0100d5e225b5a28e8c69b534ec1e868a91e71a09ad77f5bb3c34`）一并实算吻合。R9 修复报告 `/tmp/r01-stage-repair-r9.md` SHA-256 `5dd9db4bd4a33777ffd49a6615ce3cc9c0b4cd6e929eaaba2f3e57740d564f6a` 核对吻合（历史报告原文保留，不改写）。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01 修复完成并通过主仓与 /tmp 隔离副本双跑回归；评审「最晚于/R10 前」独立裁定已落实为拒绝，R9 修复报告「任务书明确禁止同义词枚举」的无依据声称已在现行文档更正（历史报告原文保留）；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。

## 0. 开工状态核对（与交接一致）

- 本地 HEAD `363999378482dfed42733a3c4e8ad20ac82fd0fc`；开工 `git fetch` 因本机代理（127.0.0.1:7890）不通失败，经 `-c http.proxy= -c https.proxy=` 直连 `git ls-remote origin refs/heads/codex/rust-tauri-migration` 实测为 `363999378…`，与 R10 评审记录一致；收尾再测仍为 `363999378…`；全程零推送。
- 开工未提交候选 = 18 个已跟踪修改 + 98 个未跟踪新增 = 116 件，与任务书/评审一致；按评审同法复算冻结指纹 = `39e9bdd7a4576e7b7437400b321302c5608c5e9cc06cfff62bf04bba73a853e3`（原像 380,578 B）。不符即停——本轮开工核对完全相符，未发生覆盖或重置。
- R10 叠加在 R1–R9 候选之上：以 R10 评审 iso 留档（`/tmp/r01-review-r10-iso`，116 件 R9 候选逐字节冻结副本）为基准复算，本轮触碰 9 件（见 §2）、其余 **107 件逐字节未动、0 缺失**；未执行 reset --hard / clean -fd；`git diff --check` 通过。

## 1. F01（BLOCKING）引号豁免跳过整个期限区域 + 括号内终结符截断完整期限声明 → 已修复

### 根因（两处同根，均在 `_stage_binding_problems` 触发词治理路径）

1. **引号豁免跳过整个触发词**（旧 r01_t08_gate_check.py:421–424）：R9 的 `_quoted_trigger_word` 判定触发词被成对引号紧包后直接 `continue`——同时移除了**解析义务**（unresolved 失败关闭）与**区域治理**（G6e 期限子句唯一一致）。「被引号包裹的几个字内无法携带阶段」只证明引号内部没有阶段，不能证明引号后没有对该字段赋期限。于是 `R09（宿主集成）“最迟”字段：原定 R09，现改 R10 完成`（四种支持引号 “”/「」/『』/ASCII " 同族）中：标记核验只绑定触发词后的首个 R09（恰好与结构化一致），其后的实际改期 R10 只由通用阶段扫描查存在性——机器输出冻结 R09 的 TRACKED，接手者正文读到 R10。`“最迟” R09/R10 完成`、`「最迟」 Ｒ０９／Ｒ１０ 完成` 的非唯一期限同理逃逸。
2. **区域终结符不看括号层次**（旧 :432–436）：G6e 区域从触发词扫到首个终结符即止，不考虑该终结符处于未闭合括号内——`最迟（原定 R09；现改 R10）完成` 的分号、括号内换行同形，把一个完整括号期限声明截断成只含首阶段的区域，改期 R10 落在区域外只查存在性。

评审 §「最晚于」等文档化边界的独立裁定另指出：`R09（宿主集成）；最晚于 R10 完成` 与 `R09（宿主集成）；R10 前必须完成` 是已披露的明确截止，不能凭「文档化边界」自动当普通上下文；且 R9 修复报告 §1/§5 称「任务书明确禁止」同义词枚举——回到原共同约束、R01 任务书与三个机器索引均无此原文。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.9-stage-repair-r9 → 1.10-stage-repair-r10，G6f 期限区域完整性与已披露边界期限拒绝）

1. **引号紧包豁免收窄为仅豁免该触发词自身的解析义务**：词名引用（“最迟”/「最迟」/『最迟』/"最迟" 紧包）无须解析出阶段 ID（R9 披露的 `R10 文档介绍“最迟”字段` 误拒限制的修复保持，P8/P9 正向钉住）；但其「触发词起到括号感知终结符止」的后续区域仍逐引用与结构化 `resolve_latest_stage_id` 一致——**词名/字段名引用不得遮蔽同子句内随后的实际赋值/改期/选择**。词名后不一致阶段按 `risk-stage-text-mismatch` 拒绝（消息明示「引号只包裹词名，包裹不住其后的期限书写」）；词名后一致提及（`“最迟”字段即 R09`）放行（P11 正向钉住）。
2. **区域边界括号感知**：`_region_clause_end` 扫描终结符时维护全/半角括号深度，处于未闭合括号内的终结符（分号/换行/句号等）不终止区域；触发词之前未闭合的括号计入初始深度；未闭合括号使区域延伸至文本末尾——**失败关闭方向（区域只增不减，多核验的引用只会多拒不会少拒）**。标记连接段约束不变（连接段仍不得跨任何终结符，`最迟（说明；R10）` 类首阶段被终结符隔开的书写仍按 unresolved 失败关闭）。
3. **「最晚」并入显式期限触发词族**：既有族已含 迟/晚 两系的 不迟于/不得迟于/不晚于/不得晚于，唯独缺「最晚」——本轮补全对称（**封闭形态族补全，非开放同义词枚举**；R9 修复报告称任务书明确禁止该方向经 R10 评审查无原文依据，现行文档已更正、历史报告原文保留）。`最晚于 R10 完成` 入标记语法逐处核验即拒；一致书写 `最晚于 R09 完成` 照常放行（P10 正向钉住）。
4. **阶段锚定「前」边界期限失败关闭**：新增 `PRE_BOUNDARY_RE` = 完整阶段引用（完整标识边界、全角折叠后）紧邻（仅空白）`之前|以前|前`——`R10 前必须完成`、`R10 之前完成`、`Ｒ１０以前…` 全族覆盖（封闭词素族，非报告字面量黑名单）。命中按新类别 `risk-stage-boundary-unsupported` 拒绝：机器契约的结构化坐标是 by-stage 语义，无法与 before-stage 语义机器判定一致，**不支持即拒，不得凭「文档化边界」当普通上下文放行**（R10 评审独立裁定落实）。该形态无一致写法——表达期限须改用触发词族并与结构化坐标一致。真实登记不受影响（44 条正文无 `R\d+` 紧邻「前/之前/以前」形态；`R02（CI 门禁固化之前必须硬化）`、`R04（先于 R04/R08…）` 等均有括号/语序间隔，实测保持放行）。
5. **机器书写/交接契约成文**（模块 docstring G6f 段）：期限表达一律用触发词族（最迟/最晚/不迟于/不得迟于/不晚于/不得晚于）+ 完整阶段 ID（或结构化坐标）；四种支持引号仅用于词名引用且紧包内容不得携带期限；其他自然语言期限形态机器不支持识别、也不得据此放宽结构化坐标权威——**凡机器已识别为明确期限的书写（含上述已披露边界形态）均须可核验一致或被拒绝，不得静默当上下文放行**。
6. **既有断言零改动**：FROZEN_CONTRACT、REAL_STAGE_INDEX（R00–R11）、CURRENT_STAGE、全角折叠、完整标识边界、G6d/G6e 标记与逐处核验语义全部保持（diff 留档 `/tmp/r01-repair-r10/gate-check-r9-to-r10.diff`，407 行均为 G6f 逻辑/注释/自测/文档，对 FROZEN_CONTRACT/REAL_STAGE_INDEX 零命中）；复用 `risk-stage-text-mismatch`，唯一新增类别 `risk-stage-boundary-unsupported`；自测新增 N85–N96 十二个负向 + P9/P10/P11 三个正向，CLI 电池新增 12 负 + 3 正，全部为追加。

### 修复后行为矩阵（真实 CLI 实测，主仓+iso 双跑；仅改 /tmp 登记副本 RR-T05-X1 正文，结构化坐标 R09/R09 不动）

| 正文变体 | 修复前（R10 评审实测） | 修复后 |
|---|---|---|
| `R09（browser worker 边界实现与门禁）`（原文正向对照） | exit 0 / PASS_WITH_CONDITIONS | exit 0 / 同（不变） |
| R9 原样四变体：`最迟（原定 R09，现改 R10）完成`／`最迟由原定的 R09 顺延至 R10 完成`／`最迟在 R09 或 R10 完成`／`最迟 R09/R10 完成` | exit 1 | exit 1 / 同（不变） |
| R9 全角改期/全角斜杠/全角选择/拉丁 or 四变体 | exit 1 | exit 1 / 同（不变） |
| `R09（宿主集成）“最迟”原定 R09，现改 R10 完成`（F01 原样 1） | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| `R09（宿主集成）“最迟”字段：原定 R09，现改 R10 完成`（F01 原样 2） | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| 上行引号换 `「」`、`『』`、ASCII `"`（quote-pairs 三例） | **三项均 exit 0（误放）** | **三项均 exit 1 / NO-GO / risk-stage-text-mismatch** |
| `R09（宿主集成）“最迟” R09/R10 完成`（词名后斜杠非唯一期限） | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| `R09（宿主集成）「最迟」 Ｒ０９／Ｒ１０ 完成`（词名后全角斜杠） | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| `R09（宿主集成）最迟（原定 R09；现改 R10）完成`（F01 原样 3，括号内分号） | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| 上行分号换为换行（括号内换行同形） | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| `R09（宿主集成）；最晚于 R10 完成`（评审裁定边界 1） | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-text-mismatch** |
| `R09（宿主集成）；R10 前必须完成`（评审裁定边界 2） | **exit 0（误放）** | **exit 1 / NO-GO / risk-stage-boundary-unsupported** |
| `R09（宿主集成）；R10 之前完成`（前边界泛化对照） | （未测） | exit 1 / NO-GO / risk-stage-boundary-unsupported |
| `R09（宿主集成）“最迟 R10”完成`（完整引号包声明） | exit 1 | exit 1 / 同（不变） |
| `R09 最迟 R09；最迟：R10`（R7/R8 原样） | exit 1 | exit 1 / 同（不变） |
| `R09 最迟 R09；最迟 R09`（多处一致正向） | exit 0 | exit 0（不变） |
| `R09 最迟 R09 完成（R09 复核）`（同子句一致正向） | exit 0 | exit 0（不变） |
| `R09（宿主集成）；R10（后续平台事项）最迟 R09`（上下文正向） | exit 0 | exit 0（不变） |
| `R09 最迟 R09 完成；R10 文档介绍“最迟”字段`（词名纯介绍正向） | exit 0 | exit 0（不变） |
| `R09 最迟 R09 完成；R10 文档介绍「最迟」字段`（直角引号词名，新增正向） | （未测） | exit 0 / PASS_WITH_CONDITIONS |
| `R09（宿主集成）最晚于 R09 完成`（同义词一致，新增正向） | （未测） | exit 0 / PASS_WITH_CONDITIONS |
| `R09（宿主集成）“最迟”字段即 R09`（词名后一致提及，新增正向） | （未测） | exit 0 / PASS_WITH_CONDITIONS |

证据：`STAGE_REPAIR_R10/f01-stage-repro-r10.txt`（主仓 91 例：R10 评审 23 变体 + 四引号 4 例 + R9 25 变体 + R1–R8 38 反例，复现器 `/tmp/r01-repair-r10/rerun-adversarial.py` 读评审留档 JSON 硬预期）、`f01-iso-recheck-r10.txt`（iso 双跑 91/91，复现器 `/tmp/r01-repair-r10/rerun-adversarial-iso.py`）、`f01-realdata-r10.txt`（真实登记正向）。

### 负向/正向覆盖（进程内与真实 CLI 双层，只增不删）

- **进程内自测 107/107**：positive-control + N1–N84（R1–R9 全保留）+ **N85–N96 新增十二个负向**（词名后改期/字段名四引号族/词名后斜杠/全角斜杠/括号内分号/换行/最晚于 R10/R10 前/R10 之前）+ **P9 直角引号词名/P10 最晚于 R09 一致/P11 词名后一致提及三个新正向**。证据：`f01-gate-selftest-r10.txt`（主仓）、`f01-iso-recheck-r10.txt`（iso）；关卡证据 `gate-selftest.log` 按 1.10 契约重生成（107 用例）。
- **真实 CLI 电池 11 正向 + 74 负向**：`r01_t08_gate_cli_regression.sh` 在 8 正+62 负之上追加 12 负（词名后改期/四引号字段族/词名后斜杠与全角斜杠/括号内分号与换行/最晚于 R10/R10 前必须完成/R10 之前）+ 3 正（直角引号词名/最晚于 R09 一致/词名后一致提及）。证据：`f01-cli-regression-r10.txt`（主仓）、`f01-iso-recheck-r10.txt`（iso）。
- **R9 两例「文档化边界」正向按 R10 评审独立裁定翻转为负向**（`最晚于 R10 完成`→exit 1、`R10 前必须完成`→exit 1）：这是**加强拒绝**，不是放宽旧断言；R9 修复报告/评审原文不改写，翻转依据（评审 §「最晚于」等文档化边界的独立裁定）记录于本轮全部现行文档与电池头注。
- **R1–R8 留存 CLI 反例 38/38**（基准 R9 评审留档 /tmp/r01-r9-old-repros.json）：退出码与首阻断类别逐项不变。证据：`f01-stage-repro-r10.txt` / `f01-iso-recheck-r10.txt`。
- 未删任何测试、未放宽任何关卡、未改任何既有拒绝断言（全部为追加或评审裁定翻转的收紧）；`RISK_REGISTER.json` 本轮一字节未动（SHA-256 保持 `87e34d0e92cd011d79eb4a9f39f410c64ffc42e2ecfcb1d37ab20d360d69f307`；真实登记仅有的两处触发词文本 `R09 最迟 R10`、`R09（宿主集成）最迟 R10（跨平台产物）` 的触发词区域只含 R10、与结构化 R10 一致，修复不改变真实数据判定——真实数据 gate 实测 exit 0/PASS_WITH_CONDITIONS/15 递延 TRACKED）。

### 设计决策与边界（如实说明）

- **词名豁免收窄而非取消**：`R10 文档介绍“最迟”字段` 的词名引用仍免解析义务（P8/P9 正向），否则重现 R9 披露的误拒；但词名之后的同子句区域不再豁免——机器无法区分「词名介绍后跟一致提及」与「词名介绍后跟实际改期」时，唯一可审计的规则是区域内不一致即拒（失败关闭），一致即放行。纯词名介绍（其后无阶段或仅一致阶段）零误伤。
- **前边界一律拒绝，无一致写法**：「STAGE 前/之前/以前」在 resolve_by_stage 中一律视为期限边界书写拒绝（含假想的 `R09 前` 与 deadline 同阶段形态——before-stage 与 by-stage 语义不同，机器不支持等价核验）。代价是字面上属上下文的 `R10 前置条件/前期准备` 邻接写法也会被拒（失败关闭方向）；上下文提及请用括注（`R10（前置条件说明）`）等非邻接写法。真实登记 44 条零命中，无误伤。
- **仍不承诺无限自然语言理解**：未含触发词族且非已披露边界形态的自然语言期限（如裸「R10 后完成」）机器仍不识别为期限——由三方约束（结构化坐标权威+首坐标+存在性）与机器书写/交接契约（期限表达必须用触发词族语法）共同保证权威；本轮义务是评审已具体披露的形态必须拒绝，已全部落实。此边界为如实声明，不以「文档化边界」名义把已披露明确截止当普通上下文。
- **「最晚」入族不等于开放枚举授权**：仅补全 迟/晚 对称的封闭形态族（六个触发词全部列名冻结于 LATEST_TRIGGER_RE/LATEST_MARKER_RE，修改=修改关卡须经独立验收）；未新增其他同义词（「不早于」系下界语义不同，未纳入）。

## 2. 修改范围与指纹

R10 叠加改动（R9 候选 116 件中 107 件逐字节未动，对照 R10 评审 iso 留档逐文件复算，证明见 /tmp/r01-repair-r10/r10-file-hashes.txt 与 §0）：已跟踪修改 9 项 + 新增 `STAGE_REPAIR_R10/` 证据 10 件。候选总计 **126 文件 = 18 修改 + 108 新增**（98 前轮 + 10 R10）。`git diff --check` 通过。

```
M docs/rust-tauri/R01/r01_t08_gate_check.py            （G6f 期限区域完整性+已披露边界期限拒绝 + 自测 N85–N96/P9–P11 + 契约 1.10）
M docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh   （+12 负向 +3 正向 → 11 正+74 负；两例边界正向按评审裁定翻转）
M docs/rust-tauri/R01/R01_REPORT.md                    （R10 进展叙述 + 同义词枚举声称更正）
M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json       （A15 stage_repair_note 补 R10 链）
M docs/rust-tauri/R01/R01_HANDOFF.json                 （stage_review_r10/stage_repair_r10 坐标 + stage_gate_position + updated_by + artifact_hashes 重算）
M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json           （R01 stage 块：round 10 / verdict FAIL / 复验与修复坐标）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json   （1.10 契约重生成；语义差异仅 contract.version 与新增类别空间，判定不变）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log     （1.10 契约重生成）
M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log  （1.10 契约重生成，107 用例）
N artifacts/rust-tauri/R01/STAGE_REPAIR_R10/（10 件：f01-gate-selftest / f01-cli-regression / f01-realdata / f01-stage-repro / f01-iso-recheck / a15-a16-recheck / f02-closure-census / f02-fourfile-iso / f02-npm-test-iso / typecheck-lint-iso，均 -r10.txt；逐文件哈希见 /tmp/r01-repair-r10/r10-file-hashes.txt）
```

**候选工作树冻结指纹**（同 R5–R10 评审法：`git diff --binary` 字节直接串接按路径排序的新文件 SHA-256 清单后取 SHA-256；原像 417,470 B 留档 `/tmp/r01-repair-r10/r10-final-fingerprint-input.bin`）：

```
1917800317b41362c2c330b3f5c096c3c2891d2356481bee0b3803bc780f6be7
```

HANDOFF `artifact_hashes` 20 项钉住哈希在最终候选状态逐项实算全部匹配（5 项按 R10 改动重算：R01_REPORT.md / R01_ACCEPTANCE_LEDGER.json / r01_t08_gate_check.py / r01_t08_gate_cli_regression.sh / gate-report.json；15 项未触碰逐字节复核，RISK_REGISTER.json 保持 `87e34d0e…`）；`working_tree_digest` 维持 T08 执行时点快照不改（同 R1–R9 惯例）。JSON 重写方法：HANDOFF / ORCHESTRATOR_PROGRESS / LEDGER 均为 `json.dumps(ensure_ascii=False, indent=2)+"\n"` 字节级可重现格式（本轮编辑后三文件逐一验证 raw==canonical）。

## 3. /tmp 隔离副本验证矩阵（iso=/tmp/r01-repair-r10/iso）

iso 由真实 HEAD `363999378` 克隆 + 126 候选路径逐字节复制组装；node_modules 单次 APFS `cp -cR`；收尾全量哈希比对 126 件与主仓零差异；主仓未运行任何会重写交付 gzip 的脚本（round2/round3 证据测试仅由 vitest 在 iso 内执行）。

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 | gate_check --self-test（主仓+iso 双跑） | **107/107 OK** | f01-gate-selftest-r10.txt / f01-iso-recheck-r10.txt |
| F01 真实 CLI 电池 | r01_t08_gate_cli_regression.sh（双跑） | **11 正+74 负全过** | f01-cli-regression-r10.txt / f01-iso-recheck-r10.txt |
| F01 评审矩阵复放 | 23+4+25+38=91 例（双跑，登记副本仅写 /tmp） | **91/91 按预期**（9 误放转 exit 1、5 正向保持、既有负向保持、R9 仅两例裁定翻转、R1–R8 逐项不变） | f01-stage-repro-r10.txt / f01-iso-recheck-r10.txt |
| F01 正向真实数据 | gate_check（1.10 契约，双跑） | exit 0 PASS_WITH_CONDITIONS，15 递延 TRACKED | f01-realdata-r10.txt / f01-iso-recheck-r10.txt |
| A15 覆盖自测+真实数据 | coverage_check --self-test / 真实数据（主仓+iso） | 6/6 OK + COVERAGE-CLOSED（736 F-ID 双向映射闭合） | a15-a16-recheck-r10.txt |
| A16 隔离检查 | isolation_check（主仓+iso） | ISOLATED（生产入口零原型引用） | 同上 |
| 闭包再生成零漂移 | iso: node scripts/compute-cli-closure.mjs | exit 0，`git status --porcelain -- build/` 零输出 | f02-closure-census-r10.txt |
| census 单文件 | iso: vitest run tests/cli-closure-census.test.ts | 23/23（含原位重写用例），build/ 零漂移 | 同上 |
| 四文件门禁（候选态） | iso: vitest run upstream-sync-matrix+audit-seal+round2+round3 | **68/74，6 红=预期封印前红集合**（audit-seal diff guard 1：旧坐标；round2 R10-03/R10-04/R10-09 3；round3 manifest/replay 2）——与 R5–R10 评审候选态同集，逐项列出；upstream-sync-matrix 7/7 绿 | f02-fourfile-iso-r10.txt |
| 全量 npm test（候选态） | iso: npm test | **14943 passed / 6 failed / 15 skipped**（总 14964，exit 1；1463 文件通过/3 失败/3 跳过）——6 红与四文件门禁同集，与 R7/R8/R9/R10 评审候选态同数同集，R10 修复零新增红 | f02-npm-test-iso-r10.txt（全文 /tmp/r01-repair-r10/iso-npm-test.log） |
| typecheck | iso: npm run typecheck（tsc×3） | exit 0 | typecheck-lint-iso-r10.txt |
| lint | iso: npm run lint | 0 errors / 10896 warnings（与 R4–R10 持平） | 同上（全文 /tmp/r01-repair-r10/iso-lint.log） |
| 主仓守卫 | git diff --check | 通过（零空白错误） | — |
| artifact_hashes | HANDOFF 20 项逐项实算 | 全部匹配 | — |
| iso 零漂移 | 收尾 126 候选文件哈希比对 | 与主仓逐项零差异；build/ 零漂移 | — |

主仓只运行了不写盘/不重写交付的检查器与一次性 CLI（gate/coverage/isolation/--self-test/电池/反例复测，gate 证据三件按既有惯例在主仓重生成）。

## 4. 正式提交后复验矩阵（交接总控/复验代理）

1. 由另一全新 Codex 子代理对完整 R01 阶段做只读复验：核对本报告指纹 `1917800317…` 与开工/结束工作树一致；复验 F01——R10 评审粗体反例（`“最迟”原定 R09，现改 R10 完成`、`“最迟”字段：原定 R09，现改 R10 完成` 及 「」『』ASCII " 三种引号同形、`“最迟” R09/R10 完成`、`「最迟」 Ｒ０９／Ｒ１０ 完成`、`最迟（原定 R09；现改 R10）完成` 及括号内换行同形）exit 1/NO-GO；评审裁定边界（`；最晚于 R10 完成` exit 1/risk-stage-text-mismatch、`；R10 前必须完成` 与 `；R10 之前完成` exit 1/risk-stage-boundary-unsupported）；正向保持（词名纯介绍含直角引号、多处一致、同子句一致、上下文提及、`最晚于 R09 一致`、`“最迟”字段即 R09` 均 exit 0）；R9 评审四变体与全角/拉丁 or 仍 exit 1；R7/R8 原样反例 exit 1；R1–R8 全套正负电池保留（自测 107/107、CLI 11 正+74 负、91 例复放、38 反例类别不变）；复核范围证明（R9 候选 107 件未动 + 本轮 9 件触碰 + STAGE_REPAIR_R10 10 件逐文件 SHA）与 16 项 REQUIRED 证据链；核实现行文档不再含「任务书明确禁止同义词枚举」无依据声称而历史报告原文未改写。
2. 阶段复验 PASS 后，总控按 PROGRESS.md seal 工作流冻结真实候选（本报告 §2 指纹对应工作树）提交，再在真实最终提交上执行：绿色电池 → round2/round3 分片补丁 VERIFIED → 坐标推进（纯审计提交，guard 仅 6 审计文件）→ 四文件门禁 **74/74** → **全量 npm test 0 失败** → 可达集对象审计 0 个 ≥100MiB → 推送后 ls-remote 核对 SHA → 工作树零漂移。不得用预演 SHA 虚报正式封印，不改 allowlist 或删除门禁。
3. 台账回填：ORCHESTRATOR_PROGRESS.stage_repair_report_sha256 与 HANDOFF.stage_repair_r10.report_sha256 由后续账本提交补登本报告哈希。

## 5. 失败与限制（如实清单）

1. 候选未提交未推送（无授权）；两处 `report_sha256` 字段为 null 待总控回填。
2. 封印未执行；候选态 6 红（四文件门禁 68/74、全量 npm test 6 failed）如实保留——它们是坐标未推进的预期封印前红（VERIFIED_SOURCE_SHA 仍指 R00 C3 `f2b8c687…`），不由本修复伪造转绿；预演/候选态 SHA 不作正式坐标。
3. 前边界检测的失败关闭代价（§1 设计决策）：`R10 前置条件/前期准备` 类邻接写法会被拒，上下文提及需用括注等非邻接写法；真实登记 44 条零命中，本轮无误伤。未含触发词族且非已披露边界的自然语言期限（如裸「R10 后完成」）机器仍不识别为期限，由结构化坐标权威+首坐标+存在性三方约束与机器书写契约保证权威——如实声明，不冒称完整自然语言理解。
4. iso 重型套件（typecheck/lint/闭包/census/四文件/全量 npm test）在候选功能态（代码+脚本+三件关卡证据+当时账本）运行，账本/报告/证据文件终态以哈希同步并逐项零差异复核（测试不读取这些文件，功能态与终态等价）；Python 类证据（自测/电池/91 例复放/真实数据/A15/A16）在主仓与 iso 终态双跑全绿。
5. 本轮实测为 macOS 27.0 arm64 单机；Windows/Linux/macOS x64、真实供应商/凭证、正式打包/签名、T04/T05/T06 可见原型与 PDF 全量重渲染、授权态录音/屏幕、真实数据迁移未在本轮重跑，结论沿用各 T 独立报告与 R10 评审声明（R10 评审已复核的 A01–A16 证据链、Rust 锁定工具链 45/45、任务链/推送链等本轮未重做，不冒称重验）。
6. Rust 工作区本轮零改动，未重跑 cargo test；沿用 R10 评审锁定工具链证据。
7. 开工 `git fetch` 因本机代理离线失败，直连 ls-remote 实测远端 `363999378…`（开工/收尾两测一致）；全程零推送。
8. 全程未 commit/push/PR/tag/release、未 force push、未触真实用户数据/外发/付费操作；未扩大审计白名单、未退役门禁/测试、未改任何既有拒绝断言（两例翻转系评审裁定的收紧）、未虚报验证坐标；未改任何风险登记正文（RISK_REGISTER.json 一字节未动，SHA-256 `87e34d0e…` 保持）。

## 6. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 107/107
bash    docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                  # 11 正+74 负
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# R10 评审矩阵复放（23+4+25+38=91 例，读评审留档 JSON，登记副本仅写 /tmp）
python3 /tmp/r01-repair-r10/rerun-adversarial.py                            # 91/91
python3 /tmp/r01-repair-r10/rerun-adversarial-iso.py                        # iso 91/91
# A15/A16
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --self-test        # 6/6
python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py                   # ISOLATED
# 闭包（仅限 /tmp 隔离副本；主仓勿运行）
node scripts/compute-cli-closure.mjs && git status --porcelain -- build/    # 零输出=零漂移
npx vitest run tests/cli-closure-census.test.ts                             # 23/23
# 四文件门禁（仅限 /tmp 隔离副本）
npx vitest run tests/upstream-sync-matrix.test.ts tests/post-verification-audit-seal.test.ts \
  tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts   # 68/74（6 红=预期封印前红）
# 指纹复算
git diff --binary | cat; git ls-files --others --exclude-standard | sort | \
  xargs shasum -a 256   # 串接后取 SHA-256（原像：/tmp/r01-repair-r10/r10-final-fingerprint-input.bin）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R10/`（10 件）；仓外 `/tmp/r01-repair-r10/`（iso 副本、指纹原像 417,470 B、逐文件哈希清单、检查器/电池 R9→R10 diff、91 例复放器与逐例输入/输出 JSON、iso 重型套件全文日志）。

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
