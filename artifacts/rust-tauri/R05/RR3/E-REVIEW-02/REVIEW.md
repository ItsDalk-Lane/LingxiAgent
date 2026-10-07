# RR3 E-REVIEW-02 第二轮全新独立验收

**结论：PASS（仅 E 文档与交接包）。mustFix=[]。MF-E01、MF-E02 均通过本轮独立核验。**

已通过包的当前状态、worker 回调消息说明和真实证据消费已对齐。这个 PASS 不代替 FINAL 独立阶段验收，R05 仍为 **NOT_ACCEPTED / R06_READY=false**。D 必需环境检查仍 FAIL/BLOCKED，G 在本轮最后观察时仍 RUNNING，FINAL NOT RUN。

审查者 `rr3_e_review_02`，未参与 E 实施、修复或首轮审查。实际为新 Codex CLI `exec --ephemeral` 空历史入口，thread_id=`01a11413-e333-7102-88eb-4a31ca6c73c3`；已只读核对本目录现存 dispatch/request.json 和 events.jsonl 首行，见 [身份记录](session-identity.json)。本轮未派代理、外发消息、修改被审对象、协调台账、Git 或系统规则；所有新增内容只在 E-REVIEW-02，自有隔离副本也只在本目录，dispatch 未覆盖。

## 1. 两项必改的独立结论

### MF-E01：PASS

亲读 RR1/RR2 MASTER 全文、RR3 共同/审查/E/E第二轮/本轮审查简报、最新总控矩阵/进度/HANDOFF，以及 E-01、E-REVIEW-01、E-02 完整报告和关键原证。冻结阅读输入及摘要见 [reading](reading.json)，实际复制阅读的当前文档与报告在 read-input；原规格与放行公式不削减。

七份现行 JSON 的真正 rr3_current 全部一致；另读五份现行说明的当前段。A/F42、B/F45、C/F27-RR3/F46 为包级 CLOSED / 独立 PASS，HANDOFF unresolved_items 不再包含 A/C 或上述已关闭问题，下一步也未重开它们。A/C 的真实报告摘要如下，均从文件重新计算；不是只比较七份记录彼此相等：

| 最新独立输入 | 本轮核验的真实结论 | REVIEW SHA256 |
|---|---|---|
| A-REVIEW-02 | A1/A2 同包 PASS，无 mustFix；正式全链仍待 FINAL | `cc52fe061bb557a82ba44397277f0dd719b02c8a2fd1ce02ad1b16a7e736f9e9` |
| C-F46-REVIEW-01 | C/F27/I10 与 F46 联合 PASS，无 mustFix；正式阶段未放行 | `c59ac409a316f5a1affbd29afbb68784c9a1ad48a80bce12a9608a600866de1e` |
| B-REVIEW-01 | B/F45 与 N03 限定范围 PASS，不替新默认全16项 | `8e83638b0b24ed5d98a8bd9acc2d632d5eb07fcba0df431350f20108777c72a1` |
| D-REVIEW-01 | 定位/精确准备 PASS，required gate FAIL、环境 BLOCKED | `6e2db4cdc7dfb8eca20d478b33bf272fa3753ce21663d8c8b528971c036c6a36` |

D 的原始正式回执自然 exit101，0 passed / 1 failed / 0 ignored / 0 filtered，非监督超时。本轮还独立回读磁盘 SHA 和 codesign 的 CDHash：SHA=`9f7489029c91d1c232e3c204236854bd53c69cee2fef33d6fd778a27f44696d3`、CDHash=`d9676388f524872f1269c13f61f413458c0e9e52`，与 D 最新 CargoJSON/运行中身份一致。D-01 c597…/6eadd… 仍明确为旧对象；同路径 permitted 不等于当前程序入站通过。FINAL 若重链接，必须核新的实际对象，本准备不代表其身份、授权或通过。

G 最后独立观察的原日志已运行到 N16，仍有真实 default runner 与 negative_gate 进程，没有 REVIEW/RESULT/REPORT/MANIFEST 完整交付，见 [16-g-observe-command](16-g-observe-command.json)及原 stdout。部分 case 的目标红不能当成默认16全部通过，也不能提前定性未完成的组合审查。现行 RUNNING 是真实交接事实。FINAL 未运行，result_ref/tested_sha 均为 null，未伪造 tested SHA。

E 首轮独立 FAIL 和两项 mustFix 历史完整保留。E-02 当前 SELF_CHECKED / PENDING 是正确的修后交接，不因它尚未消费本报告未来 PASS 判循环失败。本报告发布后的状态消费交另一新文档回填者，不由本人修改现行文档。accepted_tasks=[]、只允许 R05 工作、不将必需项交 R06，均通过。

### MF-E02：PASS

本人独立读 workerrpc.rs 的完整 WorkerCallbackLine 声明、实际 kind 分流、身份主张拒绝、用途白名单、期限调用和错误出口，并读真实 ask_model_tagged fixture。

现行图正确为 `kind=callback + op=model.complete`；最小正常 JSON 六字段的完整顺序与真实结构相等：kind、cb_id、op、purpose、prompt、max_output_tokens。实际入口先按 kind 进入 callback 分支，其他 kind 产生 ProtocolViolation；正常 fixture 同时发送 callback 与 model.complete，并使用 summarize、64 tokens。文档也如实说明生产当前没有另行匹配 cb.op，未凭空声称新增操作字符串校验。

宿主身份、已授予用途、invocation、回调数/token/prompt 预算与 timeout_at 仍按实际源码说明，没有改变权限或预算。详见 [worker-source](worker-source.json)、[05-wire 原始命令](05-wire-command.json)。这是独立的文档/源码一致性验证，本轮没有运行 worker 进程。

## 2. 亲跑的文档、源码与证据检查

以下命令由本审查者执行，自写检查器，没有执行 E 作者的 verify.py、check.py 或实施脚本。全部 argv/cwd/exit/UTC/stdout/stderr/hash 在各 command.json，集中索引见 commands.json、loghash.json。**表中数量均为文档或证据断言，不是 Cargo 测试数。**

| 检查 | 最终真实结果 | 证据 |
|---|---|---|
| 严格 JSON 与嵌套重复键正反控 | exit0，10项 | 03-json、json-results |
| 对真实外部报告核七份 current、待办、D/G/FINAL 边界 | exit0，33项 | 10-current、current-results |
| worker 声明、实际分流与真实 fixture | exit0，9项 | 05-wire、wire-results |
| §6.2 必需字段、结构全序、签名、版本、正常调用 | exit0，25项 | 10-fields、fields-results |
| 六个冻结包 manifest 全部文件、交接 artifact/三锁/实际输入摘要 | exit0，3559项 | 07-hashes、hashes-results、manifest-audit |
| 历史完整字段与说明保留、非R05对象、当前标题改动边界 | exit0，15项 | 10-history、historical-diff |
| 本地 Markdown 链接/锚点与当前 JSON 源码/证据引用 | exit0，100项 | 09-links、links-results |
| 资源逐点/逐PID复算、I10、原始npm红、范围、来源重放和版本 | exit0，381项 | 12-supplementary、supplementary-results |
| 五类隔离正常→目标红→精确还原 | 每类 0→1→0 | controls-results、history-controls-results |
| 独立重新枚举生产观察范围，含新增/删除检测 | exit0，423项全部相等 | 17-reenumerate、source-reenumerated |
| 收尾保护输入对开工快照 | exit0，444项无变化 | 18-input-final、input-before/after |
| 工具链/HEAD/跟踪引用/分支/状态、许可文档 diff --check | 11条只读命令全部0 | 14-environment、environment |
| D当前签名、G真实运行观察 | exit0/0 | 15-d-codesign、16-g-observe |

独立逐文件读取并重算的 manifest 条目：A 2475项、C/F46 533项、D 42项、E-01 168项、E首审150项、E-02 132项，所有大小与 SHA 相等。其原日志和 JSON 字节被实际读取，不只是引用报告 PASS。B 的四个相关输入（负测主脚本、注入器、自检脚本、权威pin表）另逐项和其独立证据核对相等。

五类目标反例全部在自有副本：把 A 的 PASS 改成 PENDING；把 worker 图改回 kind=model.complete；删 ModelTurnInput deadline 字段；替换一个实际 artifact SHA；删除大型账本原 acceptances。正常前置全部通过，变异各准确点名目标，原字节还原后通过。不是源码编译失败、认证早退、零匹配或对主树注入后恢复。

准备错误如实保留：06-fields 初版误用 wire 常量名、context 参数名字和 decoder 所在文件；04-current 初版错误要求汇总 BLOCKERS 必须直接出现 A 报告名，实际它已正确汇总 CLOSED 并链接当前主报告；08-history 初版把允许纠正的当前标题/生成消费指针也要求旧字逐行不变。11-supplementary 初版误读六轮记录的 rows/count 形状和恢复腿 mode 标签。只修本审查检查器，亲读真实声明/原JSON后重验；初版完整代码及失败 stdout/stderr 保留，不算产品红证或有效反例。

## 3. §6.2 的真实消费与历史边界

ModelRouteRequest、RunContext、ModelTurnInput、AuxiliaryRequest、EmbeddingRequest、OperationCallContext、ModelUsageQuery 的文档字段都与实际源码**完整顺序相等**，ExchangeItem 的 AssistantTurn/ToolResult 字段也相等。ModelGatewayPort 和 ProviderCredentialPort 真实 resolve/report_unauthorized 入口、宿主权限来源、可选操作 context 与 usage owner 查询边界已核。ModelGateway/Credential交接按实际网关和凭证端口消费。

正常 embedding 样例确来自永久具名测试 rr1_f21_operation_context_carries_session_run_and_cause：受控 loopback 配置、隔离 ServiceState、真实 session/run/attempt/cause、dimensions2、向量[0.25,0.75]、prompt5/total9。实际 usage decoder 只读明确 output_tokens，缺失仍未知，不由9−5猜4。样例是完整调用片段及必要前置说明，本轮只审源码，没有声称重新执行 Cargo 样例或真实供应商调用。

wire最小/最大1、lingxi.wire、event schema1、shared legacy版本1、DATA_EPOCH1 与真实常量相符；MIGRATIONS最大7与存储schema7一致。三份锁的真实摘要一致，schema7不冒充data_epoch7。

另外实际读取来源保护实现：TurnOrigin 精确 provider/model匹配；Anthropic、Google、Responses/Codex 的 enforce_turn_origin 对不同/缺来源拒绝有状态重放。normalize_final_message 是最终规范化投影；runs 的主模型 deadline 已接线，工具执行前释放模型permit。生产 LedgerWorkerCallbackTrace 已注入，成功/失败写账后返回，取消专用臂只落账不造 completed，worker abandoned/drop 及进程死亡边界如实登记。usage 查询的 owner/session/run/purpose/model/date字段和 unknown/partial/invalid、真实0/未知NULL、错误/取消/恢复/预算说明与所读实现对应。本结论是交接消费审查，不重新签收这些生产行为的完整阶段义务。

大账本原始历史字段全部保留，ORCHESTRATOR非R05阶段和任务内容未改；R05_INTERFACE_EVOLUTION字节不变。历史说明正文按顺序保留。当前两标题和一条生成消费指针被明确更新，原文仍在冻结 before 证据中，详见 historical-metadata-exceptions；没有删除或改写历史 FAIL/测试成绩。

原始层 JSON 独立复算仍为 R05 5/7 FAIL且7/7 checkpoint稳定，R04 6/8 FAIL且0/8稳定，R03 14/15 FAIL且0/15稳定；runner PASS不能覆盖嵌套绑定不稳。第四层为真实 R02脚本 directed-no-seal-family历史结果，E0–E4.5 ALL GREEN、E5 SKIP BY SCOPE，不虚称独立 R02 verify-stage JSON。raw npm 原证明确 candidate exit1、3失败文件/6失败测试，base exit0；strict patch-too-large登记形态和原许可不扩大。原LIVE未授权/最迟R10和Windows/Linux原平台义务保留，不外推本机部分实测。

130适用叶的权威记录真实为119shared+6full+5deferred；124dualStage含5deferred。原16A、100+3C、I01–I11、N01–N16及§6.1全部放行条件仍有效。

## 4. 资源原始汇总与输入复用

本人从最新独立 raw 重新计算160轮=60预算408+60错误+15正常+10长响应+15worker，两会话；54正式binary点、61进程内owner点，115点全部日志≤3。服务RSS27408–49184KiB/FD15–19、树RSS27408–51760/FD15–22、owner树RSS30640–35792/FD15–23，与现行独立汇总相等。每个点每PID RSS/FD/TCP求和、原绝对/增长阈值、15存活/15释放worker、45owner稳态归零、无.tmp、最终清理记录全部复核。

正常测量器与假FD/TCP零的原始退出分别0/101/101/0；真实旧顺序六轮[1,2,3,4,4,4]红，正常和精确恢复[1,2,3,3,3,3]绿。C资源两个原具名测试真实2/0/0/0、337.41s，F46作者335.40s明确为历史自检，没有互换。

60预算408重启前仍running/cancelSettled0/cancelDanglingActive60，重启消解且零重执行；不作为普通同实例取消恢复。普通恢复另由最新独立 subagent 与 late-result 两个具名真实回执，各1 passed，filtered7/4。正式binary进程树观测与进程内owner补证明确分开。原C漏检4日志超3的 FAIL保留，没有把旧2/2改成完整资源通过。

实际复用输入逐项重新计算：A19/19相等；C/F46执行321/321相等；B相关4/4相等。C广义405快照及F46扩展322项中性能文档变化的原不相等记录保留，不伪称总树冻结。E的423观察范围独立重新枚举，路径集合及每项hash均相等，摘要仍为 `fbe267c3d817aa3c55eff3f2a905bf6bded21103050825273b12543f934a2871`。范围为rust除target/.git、scripts/rust-tauri除__pycache__、根工具链及shared版本，包含测试/构建及实际范围内附带文件，不含文档/artifact/其他主树目录；不是xtask candidateSourceBinding或FINAL被测SHA。

## 5. 交付与未执行边界

HEAD与origin本地跟踪引用均 `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支codex/rust-tauri-migration，已有RR3未提交改动；未查询远端、未提交或推送。rustup代理rustc/cargo1.98.1、Node v24.16.0/npm11.13.0、Python3.14.3、Darwin arm64，真实命令已保存。

本轮保护的444项输入前后相等，不包含并行G证据或协调材料。G持续运行写evidence，不能据本包输入相等声称总树静默或freeze。

本轮未执行Cargo构建/测试、workspace、verify-stage、默认N01–N16、npm test、LIVE、其他平台、系统许可操作或R06。只审不修，两个原mustFix独立关闭，本包PASS无新增mustFix；原首审FAIL与作者SELF_CHECKED记录不覆盖。后续由新文档回填者消费本报告及届时G真实完整结果，FINAL仍必须由全新阶段审查者亲跑原§5.3并满足§6.1。**E包PASS不等于FINAL PASS，不允许R06开始。**
