# R06-AR-W08 / F08 执行交接

状态：**READY_FOR_INDEPENDENT_REVIEW**。执行者只负责本项，没有独立验收权。本候选尚待全新 Reviewer 验证；没有 Git 写入、推送或发布。

本次修复了“长工具结果一旦出现警告，完整原文又被送给模型”的问题。完整结果仍留在原始档案中，模型使用单独保存的受限版本。冻结副本的四项 C、对抗自检和关联原回归共 609 项通过，格式、代码、接口契约和模块边界检查通过。这里的通过均是执行者自检，不等于阶段完整验收。

## 问题、根因和最小改动

未修源码中，`tool_result_guard` 已返回 `guarded_content`，但 `record_tool_result!` 的 warning 非空分支继续拼接 `result.content`。真实 read 正对照的正常分支通过；加入既有扫描器测试文字后，同一合法临时文件的中段唯一标记重新出现在下一次 loopback provider 请求中，断言真实变红。旧 C02 raw 为 39079B，wire 工具文本为 39268B；这不是编译失败或零匹配证据。

生产宏共有 9 个工具结果闭合调用者，本次统一走同一个成功投影入口：

- `runs.rs` 警告始终组合到 `guarded_content`，不会恢复原文；失败结果保持原先完整错误合同。
- `hooks.rs` 单 Text/Reasoning 仍限 32KiB；每个成功工具结果的最终模型文本总量限 32768B，警告本身最多 2048B。实际生产 renderer 计入资源引用、进程状态、分隔符和截断说明。该上限是每个成功结果的 UTF8 渲染字节，不是整个请求、整个 Run 或 token 总量。
- 保留 resource/opaque/进程身份。固定身份材料本身超出预算时明确产生模型投影失败，raw 档案保留，避免破坏签名或身份。Reasoning 的 JSON 转义按最多六倍保守预留。
- kernel 的必要小配套修复：极小预算时原截断标记本身会超预算。现在标记和 UTF8 窗口也服从硬上限；普通 32KiB 窗口没有附带重构。
- 原 `tool_call_completed` 继续保存权威 raw。另经现有 StoragePort/SQLite 事件通道先提交、后发布 `tool_result_model_projection`，然后才把投影放进 exchange；归档失败明确终止，不让无档案投影进入下一模型请求。未修改 adapters、Cargo、共享 lib.rs 或原测试。

`tool_result_model_projection` 使用开放事件通道，schema 为 `lingxi.tool-result-model-projection.v1`。记录 `toolCallId`、`rawEventId`、`providerCallId`、target、raw/projection digest、result、结构化 processStatus、实际 renderedBytes、successBudgetBytes 和独立 errorBudgetPolicy。结果 digest 按真实投影内容重新计算。

raw 的 `truncated` 只说明工具自身返回的结果是否已省略；projection 的 `truncated` 说明模型上下文版本是否省略（也保留原工具的截断事实）。C04 中 raw=false、projection=true，不能把原始工具卡误当模型见过全文。另存投影不是对实际供应商是否处理请求的回执；真实下一 wire 由本项夹具独立捕获。

源码同族扫描与上下游见 [source-scan.json](source-scan.json)；实现差异见 [candidate.diff](candidate.diff)，新增测试单独按 SHA 绑定。

## 第一层：四项 C 的期待、实际和证据

| 项目 | 期待 | 冻结副本实际 | 状态及证据 |
| --- | --- | --- | --- |
| F08-C01 | 真工具返回大正常文本；下一请求有头尾/说明和真实截断标志 | 真 read raw39065B完整；下一wire26302B，中段标记无，头尾/说明有，raw=false/projection=true | PASS_SELF_CHECK，[c01](observations/attempt11-frozen-copy-final-tests/c01.json) |
| F08-C02 | 扫描器测试文字触发警告；中段唯一标记不泄回 | 真 read raw39079B；警告可见；下一wire26451B，中段无；旧分支有中段且39268B，行为red | PASS_SELF_CHECK，[c02](observations/attempt11-frozen-copy-final-tests/c02.json)、[旧红日志](logs/attempt05-old-warning-red.log) |
| F08-C03 | 循环警告、多文本和资源块全部服从预算，身份保留 | 外部合成应答经真实gateway；两大文本+存在的合法临时资源，3次重复，高风险及重复警告均有；raw文本78144B每次完整，各投影26363/26363/26398B，resource/refs/调用配对保留 | PASS_SELF_CHECK，[c03](observations/attempt11-frozen-copy-final-tests/c03.json) |
| F08-C04 | raw/projection/metadata/history 对账，错误语义保留 | 真实SQL重读及生产history导出：raw工具卡含中段且false，projection无中段且true，投影renderer与下一wire逐字一致；两次失败原error42032B全文保留，wire42055/42180B，truncated=false | PASS_SELF_CHECK，[c04](observations/attempt11-frozen-copy-final-tests/c04.json)、[history](observations/attempt11-frozen-copy-final-tests/c04-history.json)、[error](observations/attempt11-frozen-copy-final-tests/error.json) |

四 C 是从真实 ServiceState/bootstrap/SessionService.execute_for/RunSupervisor 业务入口运行，guard、消息组装、网关、状态机和 SQLite 存储均不 mock。C01/C02/C04 使用真实 read 执行器；模型应答只用 loopback。C03/error 只把外部工具应答作为合成响应，真实 gateway 仍执行其检查。新增四 C 没有冒称经过认证 HTTP 服务入口或独立 service 进程；原 T02 A03/A04 回归另启动真实 service 二进制验证该层。

每次 C 运行的候选、内容摘要、工具链、锁、夹具摘要、平台、命令、时间、退出码、测试数量、期待/实际、替身边界和证据摘要见 [case-runs.json](case-runs.json)。最终输入配置、实际有效 home 和 wire/event 摘要见 [final-fixture-observations.json](final-fixture-observations.json) 与 [wire-byte-summary.json](wire-byte-summary.json)。

## 原 Task 全份额与边界

[original-task-share.json](original-task-share.json) 逐条保留 R06-T02 全部 4 Steps、3 交付物、A03/A04，以及 R06-T06 全部 5 Steps、3 交付物、A11/A12 的期待、实际、证据和状态。最终冻结回归包括：kernel lib122、kernel压缩44、service lib387、新增7、原T02真实二进制2、原T02压缩26、原hooks4、原memory7、原skill_context4、原skills6，共609/0。

原 hooks 的 4 个名称、数量和断言没有改动，原文件 SHA 保持 `e4b2f8257c7f61d732da85830402c87404500accb01232700acd3a7ece803e2f`。原 A03/A04/A11/A12 是本机关联回归通过，不是各自整个迁移范围已经重新验收。

[original-leaf-share.json](original-leaf-share.json) 列出原 T06 全部 101 个到期叶的原验收期待，全部标 `NOT_RUN_FULL_LEAF`，仅明确关联回归可作部分支持；本轮没有逐叶从所有产品入口重证。T02 没有直接叶，但预算/压缩横切义务没有因此豁免。其它 F-ID/EX 份额仍 OPEN，不能改归 R07。

## 第二层：对抗自检和红绿对照

新增对抗检查实际覆盖 1/2/100/40000 文本块、超长普通警告、ASCII/CJK/控制字符、Reasoning 转义、真实 digest、0..149B 小预算、opaque 小身份不变和大身份明确失败。总成功文本始终受限，超量固定身份明确拒绝，raw 未被修改。错误失败序列警告经真实下一 wire 到达，原错误全文仍不截断。

等效旧版红→新版绿已经成立：未修生产正常正对照 attempt04 PASS；未修生产 C02 attempt05 有效行为 FAIL；冻结相同 C02 契约 attempt11 PASS。中途没有在候选源码复旧干扰其它执行者。

隔离 mutation 留给全新 Reviewer：应恢复完整旧 warning 拼原始结果的分支来验证旧根因。仅将 `project_tool_success` 的 guarded 参数改回 raw 仍会经过新增总预算第二道保护，可能继续绿；这表明第二道保护有效，不能把这次单参数变异误称已恢复完整旧分支。当前冻结副本只读，执行者没有在其中修改代码做 mutation。

未新增竞争条件/存储失败注入/真正重启后加载投影检查：`NOT_RUN`。源码保证先持久化后exchange；现有存储错误路径回归可作支持，不能代替新的完整注入矩阵。F01 的跨轮恢复接线不属于本项已验内容。

## 全部失败和重试

15 次命令记录及原日志均保留；没有删除失败：

- attempt01：夹具编译 API 错误，0test；修夹具，不算旧行为红。
- attempt02：夹具配置字段不符严格生产配置；修真实字段，不算旧行为红。
- attempt03：正确wire核心断言已过，夹具对home多加的runtime目录假设失败；改为验证真实config home生效，不算旧根因红。
- attempt04：正常旧分支有效正对照，1/0。
- attempt05：旧警告分支下一wire泄漏中段，有效行为红，0/1。
- attempt06：3/1；外部应答引用不存在的artifact，真实gateway拒绝。改为合法存在的临时文件，不 mock 掉 gateway。
- attempt07：初步新增7/0，之后补强断言及中文注释；最终证据以冻结结果为准。
- attempt08：主仓组合候选609/0；不能与固定副本的原锁混同。
- attempt09：clippy选项重复，准备失败，无产品测试。
- attempt10：主仓修正clippy通过。
- attempt11：固定副本自有target最终609/0。
- attempt12/13/14/15：固定副本fmt/clippy/contracts/boundaries均exit0；contracts 56生成文件和626条目无漂移，boundaries所有权/依赖/negative battery通过。

精确命令、起止时间、独立有效 LINGXI_HOME、平台和 SHA 见各 `attempt*.json`/`logs/*.log`。Cargo本人串行，使用绝对 `/Users/study_superior/.cargo/bin/cargo +1.98.1 --locked -j8`。10个最终测试进程均打印guard armed，8192MB/600s，父进程没有关闭。原回归夹具自己的显式临时home与新增夹具配置生效home分开解释，不能只凭环境变量声称原测试走了同一路径。元数据汇总脚本的两次准备问题也如实见 [metadata-preparation.json](metadata-preparation.json)，原wire/events从未改写。

## 冻结绑定

候选 `W08-CANDIDATE-01`，HEAD `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`，分支 `codex/rust-tauri-migration`。固定副本 `/Users/study_superior/.codex/worktrees/r06-w08-review/LingxiAgent`，自有 target 位于该副本 `rust/target`，执行者最终检查只读四份产品/测试文件。

| owned 文件 | SHA256 |
| --- | --- |
| runs.rs | eca52e6151299a43d7386fb2056908a3d86afeadc1085cd3aa55dd35b5646a6e |
| hooks.rs | 4736be444a0a8f73d8fbe9d9745290ee3779520931e91f149fe137685471968f |
| kernel memory.rs | 75ae4cbf1ffde042f2e64054379ff155eec0e11a04ea2932e5240a4c65725de3 |
| 新r06_ar_f08_projection.rs | c43d08523a20efa14dc1e6494dbfb15bde6b5ce05b9c10a5fcd9f99b86e4e0fe |

四份主仓与副本逐 SHA 相同。总控 `sorted path NUL sha LF` 摘要 `55c32488cc123116c9c91ec514a2daa0215a9dc1e575d9b7ef13d448f514df06`；执行者原 `sorted keys canonical compact JSON mapping` 摘要 `4ec449d7a73eba996aac0e0f967c71617a7c5eacb2ac46b15279bf8ae1974eae`。二者算法不同，不能混称同一摘要。

固定副本原2e锁 SHA `d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4`；主仓冻结组合锁 SHA `9e91ff754118fa274c95363e3a9f3d0c802667b179c2576bd10e45fc146b6506` 含未接受 W06 YAML 依赖。本项没有修改 Cargo/锁，独立候选不含该依赖。保护用户初始settings SHA仍为 `0f72eb0a2549c95973c2aa8abe54c9aa44491fb9ca2a1c6e437404655d36b3d3`。

实际运行的10份test executable及原T02启动的service binary绝对路径/SHA/大小见 [snapshot-binding-final.json](snapshot-binding-final.json)。attempt11早期命令元数据没有cwd/target字段，这两项由运行前binding及编译日志佐证，最终binding明确补充，未回写原日志。所有证据SHA由 [manifest.json](manifest.json) 收录，不复制源码、.git或缓存。

## 影响、STALE与交接

- **F01 / 历史恢复 STALE**：未来加载必须按 toolCallId/rawEventId/providerCallId 消费持久化的受限 projection.result，复原 resourceRefs、processStatus、digest、truncated。不能依赖当前内存exchange，不能从raw工具卡恢复大全文。旧raw-only记录缺projection，应显式兼容处理；本项没有实现或验收跨Run加载。
- **F04 / history**：工具卡保留raw供核验，模型实际投影来源独立；界面展示全文不能代表模型已见全文。
- **R02-R05事件消费者**：新开放事件写入/发布经现有通道，相关service回归与contracts通过；组合最终门禁及完整R05依赖闭包由总控重验，不声称本项已覆盖全部。
- 本机 Darwin arm64、合法临时材料、loopback验证。其它平台、真实供应商LIVE、正式打包均 `NOT_RUN`。没有读用户真实资料、付费调用或外发。
- 原其它 F/EX、101叶完整范围继续 OPEN；本项没有改变阶段台账或R07归属。

完整逐项结果见 [results.json](results.json)。第三层独立审查 `NOT_RUN`，等待全新空历史 Reviewer；执行者到此结束，不再复用接下一任务。
