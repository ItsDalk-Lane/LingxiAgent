# RR3 E-REVIEW-01 独立验收

**结论：FAIL。mustFix 两项：现行状态过时；worker 回调消息说明与真实入口不符。**

审查者 `rr3_e_review_01`，未参与 E 实施、修复或前审。本轮实际为新 Codex CLI `exec --ephemeral` 空历史，thread_id `01a113f6-79d4-73b2-b2b0-713abd85f4ad`；核对现存 dispatch/request.json 与 events.jsonl 首行，未虚称 collaboration spawn。未派代理、外发消息、修改被审对象、Git、系统或其他包。所有新增文件仅在本 E-REVIEW-01，dispatch 未覆盖。失败交**另一全新 E 修复者**；修后须另一全新 E 验收者，本人不修对象、不连续复审。

阶段仍 `NOT_ACCEPTED / R06_READY=false`。最新完成正式终审仍 RR2/FINAL-01 FAIL；本审不代替新 FINAL，不生成最终 testedSha。

## 1. 必改项

### MF-E01 — 已独立通过的 A/C 仍被顶层 current 声称待验（mustFix=true）

真实输入：总控最新矩阵 F42、F27-RR3、F46 都是 CLOSED；A-REVIEW-02 与 C-F46-REVIEW-01 报告分别独立 PASS，无包级 mustFix。两个报告实际 SHA：

- A-REVIEW-02/REVIEW.md：`cc52fe061bb557a82ba44397277f0dd719b02c8a2fd1ce02ad1b16a7e736f9e9`。
- C-F46-REVIEW-01/REVIEW.md：`c59ac409a316f5a1affbd29afbb68784c9a1ad48a80bce12a9608a600866de1e`。

被审现行内容仍是：A `SELF_CHECKED / PENDING`、C_F46 `SELF_CHECKED / PENDING / RUNNING`。本人独立 `06-current-command.json` 实际 exit1，**15 个不一致**：七份 JSON 各有 A/C 两处，加 HANDOFF unresolved_items 一处。七份分别为 R05_HANDOFF、PROGRESS_LEDGER、R05_ACCEPTANCE_LEDGER、R05_TEST_MAP、R05_PERFORMANCE_RESULTS、R05_LIVE_VERIFICATION，以及 ORCHESTRATOR_PROGRESS 的 stages/R05/rr3_current。

具体消费问题还包括：

- HANDOFF `unresolved_items` 仍列 A、C_F46；`allowed_next_scope.allowed` 仍要求它们重新独立验收；`evidence_refs` / `artifact_hashes` 没有消费最新两个独立 PASS。
- R05_REPORT §11 表格、R05_BLOCKERS §8、R05_INDEPENDENT_REVIEW 末尾 RR3 注记、R05_NEGATIVE_GATE_REPORT 的 RR3 当前段、MODEL_USAGE_SEMANTICS §11，均还写相关 PENDING/只自检。性能 `rr3_resource_selfcheck.acceptance` 仍称联合独立 PENDING。
- ORCHESTRATOR 的 R05 blockers/current_task 也沿用 A2/C-F46 PENDING。同一错误复制到七份记录，七份彼此相等不能证明真实一致。
- 本审观察时 G-NEG-RR3 已在总控矩阵登记 RUNNING，D 新当前身份独立审查 RUNNING；E current 的 G NOT RUN/D 仅 PENDING 是派发截点。必须区分正在运行、已完成与历史对象，不能提前消费其 PASS；FINAL 仍未运行。

E-01 REPORT §1 称“用户指定状态截点”，HANDOFF `status_snapshot` 称不追写其他验收，**不能豁免 current 一致性**。E-01 作为已冻结历史记录可以原样保留，但现行入口明确说 `rr3_current 为现行状态`、REPORT §11“本轮现行结论”，却没有真正更新的 current。这违反 RR1 F28、§6.1(7)、RR3 E/review brief 与本次用户明确要求。

交新修复者的具体完成条件：保留旧截点和全部历史 FAIL，并明确其历史身份；在真正 current 中消费最新 A/C/F46 CLOSED+独立 PASS及报告摘要，移出包级待验/未解决项，更新相关当前文字、阻断与下一步。正式全链仍待 FINAL，不能把包级 PASS 写成阶段 accepted。G/D 按届时实际原始结果登记，正在运行不能预造结果；D-01 身份只能作其历史轮次证据，不能替新构建身份授权。E 本审 FAIL 也须真实登记在新轮。新 FINAL 没有结果时路径/最终 testedSha 保持空；`accepted_tasks=[]`、R06_READY=false 保留。

本审只写此必改要求，未修改任何 current 字段或总控矩阵。隔离正常对照仅在自己的 JSON 中构造已关闭状态，单改 A 为 PENDING 目标红，再精确还原绿；见 controls-results.json。它不代表被审文档已修好。

### MF-E02 — worker 回调消息类型写错（mustFix=true）

WORKER_MODEL_BOUNDARY §1，实际第14行：`行协议: kind=model.complete + cb_id + purpose + prompt + max_output_tokens`。

真实生产入口：

- workerrpc.rs:17 写 `{"kind":"callback", "cb_id":..., "op":"model.complete", ...}`；:529–539 的 WorkerCallbackLine 包含独立 `kind` 与必需 `op` 字段。
- :1285–1286 实际分流 `match parsed.get("kind")`、`Some("callback")`；:1515–1519 对其他类型记录 `unexpected line kind … (only callback/result are legal)`，后续返回 ProtocolViolation。
- 真实 worker 夹具 src/bin/r04_t07_fixture.rs:103 也发送 `kind=callback` 与 `op=model.complete`。

因此按现行图构造消息会在进入模型回调前被拒，属于真实可消费接口说明错误。该行虽沿用旧文档，E 本轮已将整份标为“RR3 当前源码核对”，继承错误不能作为历史豁免。

本人独立静态一致性命令 `13-worker-wire-command.json` exit1，实际输出 document_kind=model.complete、production_first_branch_kind=callback，正常 fixture 交叉核对成立；源字节和区段保存 worker-wire-source.json。只在自己的文档副本验证：真实接口说明正常0 → 改回错误消息类型1 → 字节还原0，见 worker-wire-controls.json。**这是文档/生产分流源码一致性验证，未声称亲跑 worker 进程。**

最小修复：现行图明确 `kind=callback + op=model.complete`，必要时附对应真实最小 JSON；保留 cb_id、purpose、prompt、max_output_tokens 与宿主权限/预算边界。仅文档纠错，不改变生产接口、不新增权限。另一新验收者须重新核真实 WorkerCallbackLine、生产分流和夹具，不能仅检查出现字符串 model.complete。

## 2. 本人亲跑的独立检查

全部为本人新写检查器，**没有执行 E-01 verify.py 或实施脚本**。命令/退出/UTC/cwd/原始 stdout/stderr 与 SHA 在各 `*-command.json`，集中 commands.json、loghash.json。下列数量是文档断言，不是 Cargo 测试数。

| 检查 | 本人真实结果 | 原证 |
|---|---|---|
| 严格 JSON，含嵌套重复键正反控 | exit0，25项；七份现行 JSON、E 顶层材料、总控矩阵完整解析 | 07-json；json-results.json |
| 新增 Markdown 相对链接/锚点及 §6.2 JSON 源码/证据引用 | exit0，86项；缺文件与错锚点反控确实拒绝 | 03-links；links-results.json |
| before 原文摘要、完整历史字段差异、历史 Markdown 逐行保留、权威表未改、ORCH 非R05保留 | exit0，139项；大型账本旧字段无差异 | 04-history；各 historical-diff.json |
| 真实锁/全部交接 artifact SHA、E manifest、三轮作者日志、source 摘录、423输入与摘要独立重算 | exit0，329项 | 05-hashes；hashes-results.json |
| 字段全序、实际签名、版本/epoch、生产接线、样例及未知语义 | exit0，48项（不含随后发现的错误 worker 线格式） | 10-fields；fields-results.json |
| 历史层结果/资源负载与逐相位/普通取消回执/预算408归属 | exit0，18项 | 09-artifacts；artifacts-results.json |
| 原始资源数值极值、逐PID求和、全部日志≤3、峰值/释放、owner45回零、raw npm与directed原证 | exit0，17项 | 12-supplemental；supplemental-results.json |
| 当前记录对真正外部审查事实 | **exit1，15不一致**，不是被审源码编译或前置认证失败 | 06-current；current-results.json |
| worker 消息类型对真实入口 | **exit1**，点名两个实际不同值 | 13-worker-wire |
| 独立检查器五类隔离正常→目标红→精确还原 | 每类 **0→1→0**；只修改本目录自己的夹具 | controls-results、worker-wire-controls |
| 工具链、HEAD/跟踪引用/分支、许可文档 diff --check | 只读查询全部0；diff --check 0 | 14-environment；environment.json |

四类 JSON/证据对照是：current PENDING、删除 ModelTurnInput deadline 字段、替换实际 artifact SHA、删除完整历史 acceptances；第五类为 worker 文档类型。字段对照实际读取当前生产结构，不只比文档自己。current 正常副本按两份独立报告构造，只作为检查器阳性对照。

准备失败如实保留：02-json 的结果标签对相对路径错误调用 relative_to，退出1，修的是本审显示代码；08-fields 的两处错误期望分别是 SQL 空格字面值和误认为正常样例逐数字断言。核实际 SQL 与样例后改为真实 nullable 声明、真实样例结算断言及另读生产 decoder，10-fields 48/48。原失败日志未覆盖、不算产品反例；样例不被扩称原测试已逐数字断言 input5/outputNone。收尾清单计数断言曾误写24而实际artifact_hashes为25，退出1；只纠正本审报告计数，原25项逐个摘要检查仍通过，见 packaging-failure.json。02/08 的旧检查器版本未另存快照，保留实际错误输出并只以最终有效命令为通过依据。

## 3. §6.2 的源码核对结论

除 MF-E01 状态消费、MF-E02 worker 线格式外，本审未发现所列交接字段与所读现行源码的另一个必改不一致。source-audit.json 保存23个本人选择的实现区段和完整文件 SHA；worker-wire-source.json另列6段。字段与真实接口核对见 fields-results.json。

- ModelRouteRequest 三字段、ModelGatewayPort::resolve_route、ProviderCredentialPort::resolve/report_unauthorized 与 HANDOFF 一致；路由显式错误，不以模型载荷选择权限。RunContext五字段来自宿主认证/lineage。
- ModelTurnInput八字段、ExchangeItem AssistantTurn四字段/ToolResult三字段、AuxiliaryRequest四字段、EmbeddingRequest四字段、OperationCallContext四字段、ModelUsageQuery七字段都与真实声明全序相等；embed/rerank 真实 async 签名含可选 deadline/context。不是按作者声称的字段数推断。
- 主运行实际将 call_deadline 交给 next_turn；类型旧注释“None today”不能遮盖实际预算接线。主模型 permit 在执行工具前释放，worker callback 共享 QuotaManager。
- TurnOrigin 实际 provider/model 双精确匹配；Anthropic、Google、Responses/Codex 共用渲染段中的有状态重放拒绝缺来源或不同来源。消息最终投影由 normalize_final_message，MOOD丢弃/推理分离；没有仅由“文件存在”推断真实规范化行为。
- 生产 lib.rs 真实注入 LedgerWorkerCallbackTrace；成功/失败先记账后回话，abandoned/drop尽力落账与进程死亡边界在实际实现中可见。run 完成先记录 usage，再记录/发布完成事件；取消专用函数只写账行，不造 completed。
- usage 查询真实 owner 经 sessions JOIN，session/run/purpose/model/date筛选；无 session 的独立根 owner范围不可见。transport_attempts 在v7表中可NULL，真实0与未知分开；不由 total 猜缺失output。无价格来源不造费用。
- 正常 embedding 调用片段确实来自 rr1_f21_operation_context_carries_session_run_and_cause，配置受控 loopback、dimensions2、向量[0.25,0.75]、prompt5/total9，真实session/run/cause及成功/时序断言；生产 decoder只读取 output_tokens，缺失不猜4。**本审未运行此 Cargo 样例，也未称它是独立可执行或 LIVE 程序。**
- wire最小/最大1、lingxi.wire、event1、legacy preload/server1、DATA_EPOCH1 与当前源码/共享版本一致；存储 MIGRATIONS 最大7，不能混成 data_epoch7。三锁实际 SHA 与 HANDOFF 相等。
- source_sha 是本次 HEAD观察值；working_tree_digest 明确限定423输入，不冒充 xtask 总树或新 FINAL冻结摘要。accepted_tasks 空、R05_ONLY、最终结果与tested_sha为空正确，A/C的包级关闭不应改成已接受全部任务。

## 4. 历史、范围和摘要

E-01 before 的14份所有权原文均与 before.json 摘要相等；13份修改、INTERFACE_EVOLUTION不变。PROGRESS/ACCEPTANCE/TEST_MAP/PERFORMANCE全部原历史字段无改；REPORT/INDEPENDENT/BLOCKERS/NEGATIVE历史逐行保留。WORKER/USAGE 的改动差异已人工核完整，只有版本/语义纠错及新增当前段。HANDOFF六个旧字段变更逐项保存在 R05_HANDOFF-historical-diff.json，元数据旧值另存历史，v5→v7与scope误称纠正有真实依据。ORCH非R05保留，旧/新根状态与八任务变更均在差异记录，不把历史 PASS当当前全阶段接受。

本人由各层真实 JSON 重算：R05 **5/7、overall FAIL、stable=true、7/7 checkpoint稳定**；R04 **6/8、FAIL、stable=false、0/8稳定**；R03 **14/15、FAIL、stable=false、0/15稳定**。runner三层PASS不能抹去后两层绑定失败。第4层实际是R03命令 r02_legacy_regression 的脚本结果，不伪称独立 R02 verify-stage JSON；summary真实 E0–E4.5 ALL GREEN、E5 SKIP BY SCOPE，合法 directed-no-seal-family 原范围保留。

raw npm原始 s5-full-5 counts确为candidate三失败文件/六失败测试、base零失败；summary明确 candidate raw1/base raw0，分类登记红未转正式全绿。patch-too-large分类仍原严格形态，生成器本体未修，不新增豁免。LIVE仍 BLOCKED_NOT_AUTHORIZED、最迟R10；原Windows/Linux义务保留，当前macOS部分证据不外推所有平台。

scope authority真实130=119 share+6 full+5 deferred；124是dual_stage_r05_r07（含5 deferred），**不是124shared**。现行交接纠正正确，历史REPORT §4旧“124 share”已明确历史，不修改权威表或pins取绿。原16A、100+3C和16负测义务未削弱。

资源原始160混合负载、54 binary点、61 owner点及阈值逐项相等；15worker峰值/释放、115资源点日志最多3、45owner稳态归零、RSS/FD极值与文档自检汇总相等。来源是F46作者自检，不能替最新 C/F46 独立证据消费，后者必须按 MF-E01进入current。C旧2/2漏最后4日志超3及 F46旧6轮红历史不改。普通取消两具名回执各1 passed/0 failed/0 ignored、7/4 filtered；预算408的60running重启消解单列，不能代普通同实例恢复。

HANDOFF全部25个 artifact_hashes、三历史层SHA、三锁、E manifest与所有三轮作者stdout/stderr摘要都亲自重算正确；E commands-03 的15条0仅是自检。本审另外验证13-command真实是source模式、13-stdout仅38断言，自检不证明当前外部 A/C结果新鲜，更未覆盖 MF-E02 的线格式。

## 5. 输入绑定、并行边界与交付

HEAD=origin跟踪引用=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`；分支 codex/rust-tauri-migration，工具rustc/cargo1.98.1、Node v24.16.0、npm11.13.0、Python3.14.3、Darwin arm64。不访问远端、未推送。

input-before/input-after观察655项：R05 docs、rust（剔target/.git）、scripts/rust-tauri、共享版本/锁、冻结E包及两份最新独立报告；本审前后655项相等。423限定输入逐项与E原inputhash相等，独立重算仍为 `fbe267c3d817aa3c55eff3f2a905bf6bded21103050825273b12543f934a2871`。不包括全部工作树、其他G/D证据或编译目标；**不假称总树freeze**。G/D只evidence并行，真实总控记录已读取并保存本审时24份阅读快照。input-after记录本审所保护输入无变化，不给并行工作签名。

未运行 cargo构建/测试、workspace、默认N01–N16、verify-stage、npm test、LIVE、其他平台或系统许可操作。本审属于 E 文档/源码消费的独立验收，不能用其检查数量替代生产亲跑。G运行尚无本审可签收新结果，D新身份审查运行，FINAL未运行；不预写 PASS、最终SHA或最终平台许可。

本轮交付 FAIL / `mustFix=[MF-E01,MF-E02]`。保存 commands、exit/UTC、input/source、loghash及manifest；只读核验与隔离反例全部完成。请总控按唯一所有权交另一全新 E 修复者，修后交另一全新独立 E 验收者；不得让本人补 current 或连续复审。本报告自身及E-01历史失败证据原样保留。
