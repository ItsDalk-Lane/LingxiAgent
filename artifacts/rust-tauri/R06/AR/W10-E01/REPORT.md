# R06-AR-W10 / F10 执行候选

状态：**READY_FOR_INDEPENDENT_REVIEW**；F10 整项 **NOT_COMPLETED / BLOCKED_DEPENDENCY_F09**。本报告是执行者证据，不是独立验收。F10-C04 的 running 中断续跑仍 **PARTIAL / BLOCKED_DEPENDENCY F09**，不能关闭 F10 全部必需行为，更不能关闭 R06。

实际工作区为 `/Users/study_superior/.codex/worktrees/r06-w10-execution/LingxiAgent`，基线 `bc2ce51fd005ea75b9949156974e15d2761eb734`（已接受 F08）。没有主仓产品修改、提交、推送、R07 或真实用户数据/付费调用。旧用户 `.vscode/settings.json` 未动。W12 缓存候选未复制进来；将来重叠源码整合必须重新冻结并复验。

## 原因、修改与用户影响

旧代码先在 GatewayKnowledgeEmbedder 读一次模型，再在 OperationService 内重新读取实际发送路由，返回身份却贴第一次标签；后续批次还忽略实际身份。同样长度的 B 向量因此可能写入 A 的索引，表现为看似正常的错检索。

OperationService 新增 `embed_served`，一次解析得到的拥有式路由贯穿实际协议请求、解析、usage 与返回身份。原 `embed(request,deadline,context)` 保留，使用同一实现并返回原结果。Gateway 不再额外解析给返回向量贴标签。每批与首批完整身份比较，并校验数量、实测维度、有限浮点；变化时明确停止旧构建、标 failed，已写 A 向量保留。

生产身份 v2 包含 provider/model/protocol、实际维度、参数指纹。指纹包括 endpoint、请求 dimensions、Ollama 声明 contextWindow、MiniMax groupId 与算法版本；文本、凭据、配置 generation 不入 key。document/query 为同一空间的成对用途。端点变化保守隔离，不影响向量含义的 reload 保持兼容。Ollama 原按 UTF-16 长度派生并 clamp num_ctx 的规则未改。旧四字段 key 未被自动认作兼容；查询显式 FTS 并提示重建，正式重建产生独立新 key，旧付费字节保留。

无需发送的 `identity()` 能返回当前可比较字段及指纹，但 dimensions=0 **UNKNOWN_NOT_SENT**。这是为 F09 后续真实复用提供兼容接口，未宣称已测维度或 ready 复用完成。Knowledge 仍把 deadline/context 传 None；F11 的归属、取消和预算是独立必修，未关闭。

精确 owned 为四个产品文件与一个新专属测试，见 `candidate-manifest.json`：

- `rust/crates/lingxi-service/src/knowledge.rs`
- `rust/crates/lingxi-service/src/operations.rs`
- `rust/crates/lingxi-kernel/src/knowledge.rs`
- `rust/crates/lingxi-adapters/src/models/operations/embedding.rs`
- `rust/crates/lingxi-service/tests/r06_ar_w10_embedding_identity.rs`

无锁文件、依赖、迁移、registry、门禁或其它 consumer 修改。`family-scan.json` 列 bootstrap、真实网关、工具检索、索引存储和原 API 消费路径。

## 实际业务证据

先运行未修产品正对照（`commands/positive-baseline-03`），再得到旧产品业务红（`commands/c02-old-red-01`）：实际 A→同 2 维 B reload 后任务错误地 done，B 字节进入 A key。编译/脚本/夹具准备失败不计旧红，均保留。

另做 89 个真实分块的等效 mutation：只撤后批身份比较，保留完整 Gateway/OperationService/KnowledgeService/SQLite/shape guard，正式 reload 后错误 done；`commands/c02-mutation-red-02` 真实 1 匹配红。按 SHA 恢复（`mutation-restored.json`），最终专属九用例实跑绿（`commands/final-nine-cases-13`）。最后一次 owned 源码前后完全一致；测试与实际 shipping binary 的 SHA 均在收据中。

| 必需项 | 期待与实测 | 状态 |
| --- | --- | --- |
| C01 | 真实配额等待屏障确认 route/凭据已解析而 wire 尚未发送；正式 reload B 后首次真实 A 请求、A usage、A variant 同源，后续 B 拒绝 | SELF_CHECKED，待独立审查 |
| C02 | 首批 A64、后批同维 B64；旧任务 failed，A variant 无 B 字节；mutation 确实重新污染 | SELF_CHECKED，待独立审查 |
| C03 | A 索引；in-flight A 查询期间 reload B 仍只读 A；新 B 同维及 C 不同维查询均显式 FTS、searchedVectorVariants 空；不混查 | SELF_CHECKED，待独立审查 |
| C04 failed 分支 | A64 已写后实际 HTTP400→failed→停止并重启 B→正式 reingest；B 新 variant 身份准确，旧 A 逐行字节保留 | 分支自检通过，不能代表完整 C04 |
| C04 running 分支 | 真 shipping binary READY 与 LINGXI_HOME 匹配；SIGKILL→同 home 重启 B，正式 reingest 返回409，旧 A 字节完整；当前无 running 重入入口 | **BLOCKED_DEPENDENCY F09** |

追加反例覆盖同 ID 改 endpoint、原 endpoint 不变 reload、未发送维度 unknown、MiniMax group/Ollama 声明 context、旧 key 的拒绝与正式重建并存。只有外部 loopback 供应商应答是替身，其它主体与权威存储均真实；C01 用真实 QuotaManager 入场，不用固定 route mock。`self-check.json` 分列期待/实际/证据和未完成。

五种协议的原 build/parse/validation goldens 已实际运行（原 adapters operations 27/27，知识存储9/9）；新算法用例逐协议调用真实 build_embedding 记录 document/query 计划及兼容正负控，见 `protocol-compatibility-matrix.json`。该层没有 dispatch。真正完整多批/query 链只做 OpenAI loopback；**五供应商 LIVE 全部 NOT_RUN**，不互相替代。

正对照一块实际发送两次，各 ledger 行 input_tokens=5、attempts=1、output_tokens=null，实测 usage_state=partial。89块 B 新建目前实际64+64+25，F09 重复首批/进度和计数仍开放，不用九用例绿掩盖。SIGKILL 时第二物理请求已发而未结算，实际只留首批一行 usage；`crash-usage-readback.json` 保留此边界，未说零成本或完整 crash 记账。

## 原任务份额与未决影响

`original-scope-matrix.json` 保留 T07 全部5 Steps、3交付、A13/A14、24独立叶、52条 assertion 索引及关联原/追加问题共124行。research-ledger 叶没有被漏掉，也没有拿“无研究表”测试冒充旧研究事实的完整兼容读取。

原 T07 服务11例、工具6例、旧 OperationService7例已在最终冻结源码实际重跑全绿（`commands/affected-old-service-final-02`）；原 adapter 知识存储9例及 operations27例也已最终同源重跑（`commands/affected-old-adapters-final-02`）。早先01回归有专属测试输入变动，旧service还在最终JSON排版前，均保留并标 STALE；原 A13 显式 scope、引用、越权和材料不提权断言保留。它不代替 F13 正式聊天提交自动冻结 scope。Step3/A14 身份隔离份额已有上述直接证据；Step4 恢复份额因 F09 running 缺口仍阻断。其它导入/删除/源文件/配置/引用边界仅按具名原测试记录回归，不写全叶独立验收。完整原 R06 仍为281独立叶、687断言，不迁走截止、不用局部比例制造阶段绿。

F09 的真实缺口续嵌/统计/ready复用、F11 deadline/Run/causal锚点、F12缓存保护、F13正式聊天scope、F14原门禁与固定断言同步均未关闭。W12及其它冻结候选没有被本Task接受。F09实现真实running恢复后，F10 C04及原Step4必须重新绑定当前候选实跑；禁止永久延期到R07。

## 门禁、失败与绑定

实际命令用绝对 cargo 1.98.1、--locked、-j8；独有非symlink `rust/target`；守卫8192MiB/600s开启。没有本任务进程峰值测量，memory_measured_mb=null；不能把守卫阈值当实测峰值。

fmt、workspace/all-target clippy、contracts、boundaries 实际通过。registry 前后都实际 FAIL：migrations9、登记7，v8/v9缺失（F14），未改门禁。第一次 clippy 指出新测试搬来的旧 doc 注释空行，已正确修订并全量重跑绿；所有中间记录保留。

完整 workspace 使用 --no-fail-fast。首轮途中补测试说明/五协议算法记录及一个 JSON 排版，源变化明确标 STALE，不能当最终候选全绿。取消流程出现旧 expected event list 缺 tool_result_model_projection 与 quota fixture 入场超时。`upstream-red-classification.json` 将同SHA原测试/源与父代理提供的纯bc实际红对应；只是归因佐证，不继承任何测试PASS。没有精确纯bc跑到的 panic 子案例仍 NOT_CLOSED。

正式 `verify-stage R05` 已按原 stage map 自然执行结束（外层 exit1），R05/R04/R03 三个结果均 overall FAIL，三层输入逐命令及结束绑定均 stable=true，没有 timedOut。外层没有额外600秒截断；测试 guard 原8192MiB/600s保持。R05 suites27套+64 lib pins及F27两例（334.85秒）实际PASS；R04工具10例（301.04秒）及RR1 repair实际PASS。三层workspace均实际在原quota fixture exit101停止，后续target在该workspace腿 NOT_RUN。R03 storage在S0实际9vs7登记红，后续业务腿 NOT_RUN；legacy物化准备因worktree只有.git引用文件而无独立.git目录受阻，业务未运行；R03 repair真实cancel_link_inheritance7匹配、subagent_closeout8匹配各101，不能把失败summary解析缺失说成0匹配。其它command逐项结果、时刻、缺证据与场景状态见 `formal-gates.json` 原收据。没跑到的后段不能PASS；阶段失败不由专属九用例替代。R06 gate 尚无原注册，F14仍必修。本候选只是可供独立审查的最小F10修复。

`candidate-manifest.json` 绑定最终 owned/HEAD/lock/实际执行二进制；`evidence-manifest.json` 列安全报告/原始业务wire/usage/variant/失败日志的精确哈希与收取名单。生成的合成 home 凭据、local-token/devicecredentials/private keys，以及 target/临时源均是本地敏感或可重建材料，**不进Git**。只捕获业务 body/endpoint，不捕获实际认证 header；日志显示的 `[token]` 是既有脱敏标记。源输入与执行记录不覆盖旧失败；只对本次可重建临时物做限定清理。

完整 no-fail-fast 首轮的四个 failed target / 五个用例均保留：cancel_link_inheritance 的 subagent_timeout；cancellation_tree 的 approval roundtrip 与 panicking child；run_lifecycle 的三调用终态；subagent_closeout 的父取消零调用。源在首轮有变化故全轮 STALE，不冒称最终候选全仓验收。最终 formal repair 又实际确认 quota/closeout 红；purebc实际红只做同SHA归因。panic精确purebc未跑仍 NOT_CLOSED。

交接优先顺序：全新空历史 W10 Reviewer 必须亲验当前修复与C04缺口，保留首次失败窗口；之后新F09执行者修真正续嵌/计数/ready复用，再整合冻结输入并重做独立验收。未自签验收。`command-history.json` 保留全部失败、准备失败、重试、源前后/二进制绑定限制；`formal-binaries.json` 是日志证实路径后的散列采样，未伪称每个旧脚本都具备启动前二进制采样。合成home保留为本地受保护证据、逐文件散列仅入排除索引；临时mutation源码备份已删除，target与全文源码副本不收取。

清单准备首次把所有.json当单记录解析，遇到原7行JSONL及负向用例故意空stdout而失败；`manifest-preparation-01.json` 与两份准备快照保留，最终清单按真实格式登记。未改写原始业务/阶段记录，不能将此准备失败计为产品红。
