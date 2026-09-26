# R01 阶段修复 R1 — 修复报告（stage-repair R1）

- 任务：修复全新 Codex 阶段独立验收 R1 的两项 BLOCKING 发现（F01/F02），交付可供另一全新 Codex 子代理独立复验的候选。
- 执行者：全新 ZCode 一次性「R01 阶段修复 R1」代理（非 R01 总控/非任何 T 执行或评审代理）。
- 日期：2026-09-25/26。分支 codex/rust-tauri-migration。
- **结论：READY_FOR_INDEPENDENT_STAGE_REREVIEW**。本报告不宣布阶段 PASS；阶段判定留给另一全新 Codex 子代理。

## 0. 前置核验（修复起点）

| 项 | 值 | 核验 |
|---|---|---|
| 本地 HEAD | 363999378482dfed42733a3c4e8ad20ac82fd0fc | `git rev-parse HEAD` |
| 远程 HEAD | 同上 | `git ls-remote origin codex/rust-tauri-migration`（已排代理干扰） |
| 工作树 | 干净 | `git status --porcelain` 为空 |
| 阶段评审报告 | /tmp/r01-stage-review-r1.md | SHA-256 `0ace64a61fdcff4aa6d15b9eaa890b55ca91b30c3fed2d62ddc77653ec2dc020`，结论 FAIL，发现 F01/F02 |

修复候选 = 上述 HEAD 的工作树改动（9 个修改文件 + 1 个新增证据目录 16 文件），**未提交、未推送**（本任务无提交授权）。全部验证在 `/tmp/r01-repair-r1/iso`（HEAD 363999378 的本地 clone + candidate.patch + node_modules 软链）内完成；主仓未运行任何会重写交付 gzip 的脚本（仅有的例外：早期一次四文件门禁主仓运行，事后 `git status` 核实字节零漂移——MISMATCH 态不改写补丁，详见 §5.4）。

## 1. 候选文件范围与指纹（SHA-256）

### 1.1 修改（9 个已跟踪文件，diff 589+/163-）

| 文件 | SHA-256 |
|---|---|
| docs/rust-tauri/R01/r01_t08_gate_check.py | 76487885dab02df73c0661deb5b04444d31bb34b49779665b438a950bf23d5e9 |
| docs/rust-tauri/R01/RISK_REGISTER.json | 73478c07be2bea1def69df7678383e3553b47b3b0fc5a677e611719eb3016530 |
| docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json | 3904988e4e035a4a8ba9d376b7f29edbbb6de335b905c68786aa32ad1f5bbfa7 |
| docs/rust-tauri/R01/R01_HANDOFF.json | e8828bc3949710a97757db9d3a672b1584cc500394d67c07ca833bf79b320ab1 |
| docs/rust-tauri/R01/R01_REPORT.md | 01c5cb18719c1b2f2972833ad98ede6f99be77470779d03d6caa47ba18ea1dce |
| docs/rust-tauri/ORCHESTRATOR_PROGRESS.json | 034599cc44610e4b1f4e3173b11187a21788de8a9a71ba1c2f93c0bd8b5ad8e2 |
| artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log | 6325d0a7ab61262266a9bd7db1a065221ac113c5edeefce879fae4539b0471bc |
| artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json | cb897f50021aa2c79faffd05842c9b5f45b55e026d87a18bf71c9d87d2543010 |
| artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log | 8c34add9a5ac3964ba7bd8becd0438fafdada0fa25c269e973cdd66f39da2d9f |

### 1.2 新增（artifacts/rust-tauri/R01/STAGE_REPAIR_R1/，16 个未跟踪文件）

| 文件 | SHA-256 | 内容 |
|---|---|---|
| audit-seal-gate-rerun.txt | 4a87c7bdce0ef5b0eb5b83e778ab9613d55a977f6b97eb7062358e70e2bbbdf6 | 四文件审计门禁候选态独立复跑（62/68，6 FAIL 归因；RISK_REGISTER.json 引用此文件） |
| seal-rehearsal-r1.txt | c3db04184a3a6c37a50b5705749f6b2632a3f594222706c25536fd2ef3b81339 | 封印预演摘要（坐标/尺寸/结论/复验条件） |
| seal-rehearsal-ceremony.txt | 72f934ea6378cf0e575e16e2fe0187b5d7789aa097bd1ba681e8ee2fadc28c45 | 封印仪式完整日志（68/68 绿） |
| f01-bypass-variants-iso.txt | acf48b2016ce890ecbf5683c0dadd516d1f6284f9f3ec95b18dca32bfd775e5f | F01 五个绕过变体全部 NO-GO（exit=1） |
| a15-gate-iso.txt | cac6bfa8025b702c2de4f7c1cd179758ad29af92a52b1e39edf0ae39c9dee770 | A15 关卡 PASS_WITH_CONDITIONS（exit=0） |
| a16-coverage-iso.txt | a1d691ae754765dea6b0913c97fc44fe6939c0a7823b054edd2f222e599974ba | A16 覆盖自测 6/6 + 5 负向 |
| coverage-report-iso.json | d2f165d06b4bb3c41014581dbe7229e257f35ccd61e4d70e2b1057c8d7ca5589 | A16 覆盖报告（隔离复算） |
| t01-ownership-iso.txt | 2dad4306747a8042de6870913c4522f3417f1e0d56e0381d7723b610da01ae93 | T01 OWNERSHIP_TARGET 无漂移（features=736 stores=69） |
| t02-check-generated-iso.txt | 89b581e448f6d8933675a4f99247c615dfb51e71d7c5e1c4fbc96045110bc9ee | T02 生成物 56 文件 + API_COMPAT 624 条零漂移 |
| t02-handshake-iso.txt | 16a87fa7bfd4cc3982975bc26ab827802de3705a0b42b499ea555f8d28cad8fc | T02 handshake 关卡 PASS（4409 契约） |
| t02-roundtrip-iso.txt | 4375cfb629d0087ced00817bba396da81e70fcef0870207bf2a9021dad7aa249 | T02 Rust→TS→Rust 12 样本字节一致 |
| cargo-test-workspace-iso.txt | 45bd6c5cbf150500e239cc5d9dcb923bc54225d197d9aefbc126add97431ffd5 | Rust 工作空间 19 个 test binary 45/0 |
| isolation-report-iso.json | f13b4fbe37ee00d2b2028b06fe1fd4d893d190e3c9c9e791f7e1946de414ad4d | T08 隔离核查报告（隔离副本复算） |
| t08-isolation-iso.txt | c425f6f6211511fba7294d5d9325948a50c9da5efa06ad09a2b2b84c2fcabf0a | 生产目录原型文件=0，ISOLATED |
| rehearsal-round2-patch.txt | 07f965770e5000649e561a39b9591ebb6c3e5bbd7bd22e128003770844300c01 | round2 补丁 source 态 VERIFIED（直跑记录） |
| rehearsal-round3-patch.txt | 3e65f1a9c3492d3e2d1beca8d5bc41df5eabbee97a1ca5d2778325db2567fb4c | round3 补丁 source 态 VERIFIED（直跑记录） |

（候选.patch 本身留档于 /tmp/r01-repair-r1/candidate.patch；复验方也可直接取主仓工作树。）

## 2. F01 — T08 关卡检查器权威来源修复

### 2.1 根因（复现确认）

原 `r01_t08_gate_check.py` 的"必需能力/递延项/证据路径/哈希"实际由被验输入 `r01_t08_gate_inputs.json` 自报驱动：复现 5 个绕过变体（删 user_takeover、清空 areas、删 sha256 字段、删递延项、删整个域）在修复前检查器全部 exit=0 放行——证明输入自报被当成了必需集权威。

### 2.2 修法

检查器内新增 `CONTRACT_VERSION = "1.1-stage-repair-r1"` 与 `FROZEN_CONTRACT`：**冻结于检查器自身、独立于被验输入**的契约——五域（browser_host/pdf_renderer/shell_capabilities/storage_cutover/protocol_chain）、逐域必需能力 ID→冻结证据路径（含 browser_host.user_takeover）、递延项 ID→冻结 risk_id 绑定。新增 G0 契约闭包检查：缺域/缺能力/多余/重名/改名 → BLOCKED/NO-GO；证据路径≠契约路径 → evidence-path-contract-mismatch（重算哈希不能绕过）；sha256 缺失/非法/不符 → 拒绝；递延项状态非法、risk_id 重绑（如 RR-T06-MIC）、风险台账条目缺失/缺 resolve_by_stage/failure_handling → 拒绝。报告新增 `contract` 与 `contract_violations` 字段。

### 2.3 验证（隔离副本实测）

- 自测 **17/17 OK**：正向对照 + N1–N16 负向（删 user_takeover、删域、删哈希字段、删递延项、加多余域、改名能力、重复能力、递延状态非法、risk 重绑、证据改道+重算哈希、areas 置空、resolve_by_stage 置空等），全部 NO-GO/BLOCKED。
- 五个原始绕过变体对修复后检查器全部 exit=1 NO-GO（f01-bypass-variants-iso.txt）。
- 正向真实原型输入仍 PASS_WITH_CONDITIONS、exit=0、contract_violations=[]（gate-report.json 重生成，哈希 cb897f50…）。

## 3. F02 — 阶段文档/账本事实修复

依据真实 Git 证据（`git rev-parse <ledger>^` 父子核对六对全部吻合、reflog 时间线、远程 HEAD）与 T08 独立验收 R1 报告（PASS，报告 sha256 `4a0c50ade4b10d3bb18b8303c78fb0670a42fee86852d4faf380804f9d731fcf`）：

1. **ORCHESTRATOR_PROGRESS.json**：current_head → `363999378482dfed42733a3c4e8ad20ac82fd0fc`（附注说明"台账提交记录前一 HEAD、不自嵌 SHA"约定；current_head 指实际候选可验坐标，非任何文件内嵌值）。R01 → status=READY_FOR_STAGE_REREVIEW，补 stage_review_round=1 / stage_verdict=FAIL / stage_findings=[F01,F02] / stage_review_report+sha / stage_repair_report=/tmp/r01-stage-repair-r1.md（sha256=null 待总控回填）。T04–T07 push_result 由 null 补齐为真实推送头（T04 5a8a8e24…、T05 76bd42c4…、T06 82870879…、T07 2bbec6d0…，reflog 逐条佐证，注明"原 null 为记录缺口，非未推送"）；T08 push_result remote_head=363999378… + post_pass_repairs 登记本轮修复。
2. **R01_ACCEPTANCE_LEDGER.json**：description 明示"A01–A16 全任务级 PASS，但阶段独立验收 R1 FAIL"的层级区分；npm_test_note 更正；A15/A16 状态 READY_FOR_REVIEW→PASS（tested_sha=358299c1e9a768a75186f197686d25ee6636d6d7，reviewer=ZCode:R01-T08-review-r1 agent_2c68e944…，verdict PASS(R1)，report+sha 入档）；A15 observed 增补 FROZEN_CONTRACT 修复叙事与 stage_repair_note。
3. **R01_REPORT.md**：结论改 READY_FOR_STAGE_REREVIEW 并写入阶段评审 R1 FAIL 叙事；HEAD 引用 2bbec6d0→36399937；A15/A16 表行改 PASS；审计门禁条目按 C4 绿（68/68+14942/0）与 R01 新增 6-FAIL 的事实重写；独立审查与远程/发布两节按 reflog/远程证据重写。
4. **RISK_REGISTER.json**：RR-AUDIT-SEAL-PREEXISTING 由 CARRIED 重分类为 **OPEN** 并更正事实（C4=328cc8bb5a 上 68/68+14942/0 全绿；R01 候选 62/68 为新增红；根因=坐标停在 C3 f2b8c687 + R01 新增 ~2100 个非白名单文件）；证据链补 candidates 态复跑文件；加 reclassified_by 与顶层 updated_by。
5. **R01_HANDOFF.json**：status→READY_FOR_STAGE_REREVIEW；source_sha=2bbec6d07 保留但明确标注为历史候选快照；新增 accepted_stage_candidate 块（t08_task_commit=358299c1e、ledger_commits=[f783a8e8e,363999378]、remote_verified、t08_review PASS(R1)、stage_review_r1 FAIL、stage_repair_r1 READY_FOR_INDEPENDENT_STAGE_REREVIEW）；accepted_tasks.T08 与 review_evidence.T08 改 PASS；stage_gate_position 改 17/17 + FROZEN_CONTRACT 说明；unresolved_items 同步 RR-AUDIT-SEAL-PREEXISTING 更正；artifact_hashes 17 个文件项全部复算 MATCH（本次修复后再次复核仍全部 MATCH）。

### 3.1 审计门禁 6-FAIL 归因（独立复核结论）

四文件门禁构成：post-verification-audit-seal(3) + round2-delivery-evidence(32) + round3-delivery-evidence(26) + upstream-sync-matrix(7) = 68。候选态实测 **62/68，6 FAIL**（主仓与隔离副本同值）：post-verification diff guard（1：f2b8c687..HEAD 出现非白名单文件）+ round2 R10-03/R10-04/R10-09（3：guard 红连带）+ round3 manifest/现场重放（2：同因）。upstream-sync-matrix 7/7 绿（六处坐标一致）。**这是 R01 新增红，非 R00 遗留四红**：R00 正式 C4 提交 328cc8bb5a 上实测 68/68 + 14942/0（/tmp/r00-seal-c4-postcommit.md）；R01 在 C3 坐标之后新增 ~2100 个非白名单文件，保护器按设计转红（HEAD 越过已验坐标即红，等待封印）。未虚报红为绿，未扩大白名单/退役门禁/改断言。

## 4. 阶段 Gate 封印判定与分步方案

**判定：R01 阶段 Gate（按任务书）不要求审计封印在阶段复验前关闭。** 依据 PROGRESS.md 封印流程：封印须"先将适用验证绑定到实际候选源码提交，再同步既有坐标来源、生成投影并完整复验；纯审计提交后再次核对差异门禁"——即封印只能发生在（a）阶段复验 PASS 确认了实际候选、（b）总控获得提交授权之后。本任务无提交授权，故交付分步方案 + 完整 /tmp 隔离预演证据。

**封印分步方案**（与 R00 C3/C4 模式同构，已在隔离副本全流程实证）：
1. 阶段独立复验 PASS 后，总控提交真实 R1c（本候选 9 文件 + STAGE_REPAIR_R1 证据包 16 文件）。
2. 证据重冻结仪式（run-evidence.py 依次执行，记录绑定新清单哈希）：绿色电池（typecheck / typecheck:core-contracts / lint:boundary / build:renderer）→ round2 patch VERIFIED 记录 → round3 patch VERIFIED 记录 → vitest 三文件绿记录；提交真实 R1s（SOURCE_MANIFEST×2 + manifests + COMMAND_RESULTS×2 + logs + patch.gz×2）。
3. 六处坐标推进到 R1s（verified-source-sha.txt / build-sync-matrix.mjs 常量 / matrix JSON+MD 重生成 / UPSTREAM_SYNC_AUDIT.md / PROGRESS.md），提交纯审计 R1a；`verify-post-verification-diff.mjs` 绿；四文件门禁 68/68；门禁运行后工作树干净。

**预演证据**（/tmp/r01-repair-r1/iso，REHEARSAL 标记提交）：R1c'=d8e4192f0 → R1s''=45f287c679fcfd33f23816f1e550c32b0603fee8 → R1a''=a29f0b957c92def52d0fa24663c01eb185744307；guard 报"仅 6 个审计文件变化"；**四文件门禁 68/68 exit=0**；工作树终态干净。重冻结清单 8174 条目（round2 cc6cc094… / round3 2528b2e6…），含修复后检查器哈希 76487885…。预演保真度说明：R1c' 未含 STAGE_REPAIR_R1 证据包（16 文件），真实 R1s 清单将约 8190 条目，机制不变。

**两个如实报告的真实阻塞/预存问题**（均先于本修复存在，不属于本修复失败，需总控治理裁决，本任务未擅自改动合同）：
- **补丁尺寸超推送硬限**：封印须重生成 round2 patch.gz 267,221,850 B / round3 patch.gz 577,433,322 B（未压缩 967,668,154 B；现库内 29.6MB/89.5MB）。根因：artifacts/rust-tauri/R01 受控证据 253MB 进入 BASE→current 全树 diff（全树清单字节 521MB）。两文件均超 GitHub 单文件 100MB 推送硬限——封印提交在现行合同下**不可推送**。候选处置（供总控决策，均需独立评审）：补丁分片/交付机制改哈希引用/证据瘦身或排除合同修订。
- **`npm run lint` 候选态为红**：61 errors 全部位于 R01-T06（8994c67bc）引入的 spike/tauri-shell/{scripts/probe_webdriver.mjs(25), app/frontend/probe.js(25), scripts/evidence_server.mjs(11)}（no-undef 浏览器/WDIO 全局；主仓单文件 eslint 复核同值）。R00 C3 仪式电池含 lint，正式封印电池须先修复 spike lint 或调整 eslint 作用域；预演电池已剔除并在此备案。

**正式提交后复验条件**：见 seal-rehearsal-r1.txt §正式封印后复验条件（1-5）；关键是验证必须绑定真实候选提交而非本预演 SHA，且推送前必须解决 100MB 硬限。

## 5. 实测记录（全部真实运行，隔离副本 /tmp/r01-repair-r1/iso）

| 门禁 | 结果 | 证据 |
|---|---|---|
| F01 检查器自测 | 17/17 OK | gate-selftest.log（候选内重生成） |
| F01 正向（真实原型输入） | PASS_WITH_CONDITIONS exit=0 | gate-report.json cb897f50… |
| F01 五绕过变体 | 全部 NO-GO exit=1 | f01-bypass-variants-iso.txt |
| A15 关卡 | PASS_WITH_CONDITIONS exit=0（15 递延项挂账） | a15-gate-iso.txt |
| A16 覆盖 | SELF-TEST OK 6/6 + 5 负向检出 | a16-coverage-iso.txt / coverage-report-iso.json |
| T01 ownership | UP_TO_DATE features=736 stores=69 exit=0 | t01-ownership-iso.txt |
| T02 生成物漂移 | 56 文件 + API_COMPAT 624 条零漂移 | t02-check-generated-iso.txt |
| T02 handshake | PASS（4409 契约） | t02-handshake-iso.txt |
| T02 roundtrip | 12 golden 样本 Rust→TS→Rust 字节一致 | t02-roundtrip-iso.txt |
| Rust 工作空间单测 | 19 binary 45 passed / 0 failed（rustup run 1.98.1） | cargo-test-workspace-iso.txt |
| T08 隔离核查 | ISOLATED（生产目录原型文件=0） | isolation-report-iso.json / t08-isolation-iso.txt |
| 四文件审计门禁（候选态） | 62/68，6 FAIL（§3.1 归因，与主仓同值） | audit-seal-gate-rerun.txt |
| 四文件审计门禁（封印预演态 R1a''） | **68/68 exit=0** | seal-rehearsal-ceremony.txt |

环境限制如实说明：rustup 不在 PATH（以 `~/.cargo/bin/rustup run 1.98.1` 显式调用）；ls-remote 需排本地代理；未跑全量 npm test（本任务门禁清单未要求；候选改动不含生产运行面——desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json 零改动，可由复验方决定是否补跑）；Windows/真实打包/真实供应商未验（平台限制，沿用既有声明）。T08 隔离核查备注：rust/target 未被 gitignore（如实记录，不判负，建议 R02 增补）。

## 6. 复验矩阵（供另一全新 Codex 子代理独立执行）

前置：主仓 HEAD=363999378…、工作树候选 = §1 文件（指纹逐项核对）；评审报告 sha256=0ace64a6…。

1. **F01 负向复验**：`python3 docs/rust-tauri/R01/r01_t08_gate_check.py --self-test`（如入口名不同，读文件内 self-test 入口）→ 17/17；手工构造五变体（删 user_takeover/删域/删 sha256/删递延/areas 置空）→ 全部 exit=1 NO-GO；正向 `r01_t08_gate_inputs.json` → PASS_WITH_CONDITIONS，gate-report.json 哈希=cb897f50…。
2. **F02 台账复验**：`git rev-parse {f783a8e8e,363999378}^` 等六对父子核对；`git ls-remote origin`；ledger/HANDOFF/ORCHESTRATOR_PROGRESS 三处 T08=PASS(R1) + stage_verdict=FAIL 并存；HANDOFF artifact_hashes 17 文件项复算 MATCH；RISK_REGISTER RR-AUDIT-SEAL-PREEXISTING=OPEN 且证据文件存在。
3. **门禁复跑（隔离副本）**：clone + 应用候选 + node_modules 软链 → A15/A16/T01/T02/cargo/隔离核查/四文件门禁，结果应与 §5 逐项同值（四文件门禁候选态仍为 62/68——坐标未推进属预期，封印是总控授权步骤）。
4. **封印方案复核**：读 seal-rehearsal-r1.txt + seal-rehearsal-ceremony.txt；抽查 /tmp/r01-repair-r1/iso 的 R1a''（a29f0b95…）上 guard 与门禁可复跑（若副本已被清理，可按 §4 步骤自建副本重演，预计 20-30 分钟）。
5. **越界检查**：候选不含 commit/push/tag；不含密钥；不改 tests/ 断言、.sync-audit/ 门禁、白名单；主仓 STAGE_REPAIR_R1 之外无无关改动（`git status` 应只见 §1 清单）。

## 7. 剩余限制

- 候选未提交未推送（无授权）；ORCHESTRATOR_PROGRESS.stage_repair_report_sha256=null 待总控回填本报告哈希。
- 封印未执行（按 §4 判定不属于本任务）；封印推送被 100MB 硬限阻塞，待治理裁决。
- lint 红（spike/T06 遗留）未修（不在 F01/F02 范围，已如实备案）。
- 本报告与证据均为 Darwin arm64 本地结果，不代替其他平台/正式打包/真实供应商验证。
