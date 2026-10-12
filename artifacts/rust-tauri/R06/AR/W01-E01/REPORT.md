# R06-AR-W01 / F01 执行报告（冻结交接）

状态：READY_FOR_INDEPENDENT_REVIEW（自检完成，等待全新独立审查）。这里只报告 F01 修复份额，整 Stage 仍 OPEN/FAIL；本执行者不是独立 Reviewer。

本次解决了新一轮对话不带旧记录的问题。模型现在读取同一用户获准查看的同一助手、同一会话有效分支的完整历史，再加入这次输入一次；工具只恢复原结果，不重新执行。展示正文、内部模型档案与原始工具收据各自保留，历史页和导出不多出回复或工具卡。

## 候选和工作边界

- 实际 HEAD/base：`bc2ce51fd005ea75b9949156974e15d2761eb734`。用户被审 base `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d` 不是回退要求。bc 的 F08 原独立 PASS 仍是历史事实，组合变化须新验。
- 只改隔离工作区 `/Users/study_superior/.codex/worktrees/r06-w01-execution/LingxiAgent`；主仓产品、用户 settings、控制文档未写。未 stage/commit/push/reset/clean/发布，未变权限或模型设置，未使用真实资料/真实模型/付费服务。
- 精确 owned 登记 26 路径，其中原 C09 夹具已撤销错误修改并恢复 base 字节；实际改动 25 文件（22 tracked + 3 新文件），详见 `owned-plan.json`、`candidate-source-draft.json`、`candidate.diff.patch`。
- Rust锁未改：`d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4`。无新依赖、无新迁移、无 v8/v9 登记；复用现有 messages 内部 entry_type。MigrationInspector 版本未改变，内部节点和公共驱动/角色契约变化仍纳 F14 组合审查。
- 已全文读原 ENTRY 1219 行、F01/C01—C04/§6/角色循环/最终范围，原 R06-T01—04 Steps/A01—08/R00叶/共同架构/原有效R05交接、SCOPE原语义及F08真实报告/结果/diff。登记不等于 PASS。

## 原正常对照、旧失败与根因

`old-c01-03` 使用未修真实 Rust binary、认证 HTTP、实际 SQL 和 loopback 模型。第一 Run 正常成功并生成运行时 nonce；第二 input 不复述 nonce，第二真实 wire 缺前轮 user/assistant（exit1）。`old-c01-03-observation.json` 保存请求、原 SQL 消息、二进制前后摘要。`new-c01-01` 及当前永久 C01 对应新绿；不是预拼历史、编译失败或0匹配冒充行为红。

|真实入口|原原因|最小配套|
|---|---|---|
|认证 execute → SessionStore → RunSupervisor|每个新 Run 空 exchange；UI final 不包含完整模型状态|新持久 ModelHistory，先读有效前史，再加当前输入一次|
|模型多轮/五供应商 renderer|没有历史用户类型；只还原正文会丢工具/签名与原 serving 身份|UserTurn/本次输入边界；各 family 保角色/顺序；精确模型档案|
|工具 raw 与 F08投影|raw-only UI卡无法还原完整请求；Failed/Cancelled/Unknown 的合法 null 被误当损坏|保存并只回放真实 F08 projection；按真实四态校验 null；raw原始档案不截断|
|fork/retry/rewind/重启|fork RunFK NULL；child final 混入父正文；未知来源不能时间猜|专档 RunFK NULL、自足来源/child排除；旧命名调用精确usage+Run+谱系，纯文本有界兼容|
|父子并发追加|合法真实 child append 会移动父head，不能一概当异writer|只精确新child lineage+原parent仍祖先有界重试；reset/异writer拒绝|
|取消/等待者丢弃|已明确Abandoned的原Run仍Running，合法新请求读到活工具|只当前registry明确Abandoned+真实源身份/journal/single-finalize；Unknown仅模型诊断|
|摘要/跨Run预算|版本与后缀未持久回放|真实ModelGateway摘要同一版本+后缀原子保存；失败原始历史不变|
|明确后台受理|新增读取放大原受理早于真实durable start窗口|同一driver commit真实started后通知；未start失败补偿；断连后台继续|
|原权限|曾错误要求当前reader主体等于session owner/原actor|恢复LocalUser原广域规则，当前reader、session owner、原actor各自核准；foreign Device/Web拒绝|
|分叉深预检|基本校验只查checksum和头，坏工具/opaque/actor可被复制|统一解析器只读指定保留前缀，验证后才事务复制；目标外坏后缀不影响|

深 fork 旧 `deep-fork-original-red-02`：checksum 有效但工具 provider配对坏、opaque来源缺/伪换、原actor伪换四种均真实200并创建目标；正常中段fork/源删除再fork正控绿。修后当前 C03 四种500/零目标/零wire，源messages/head+revision/runs不变。red01 opaque没有正文final、目标无效导致400，是探针准备边界，不算有效opaque行为红。live opaque 核原调用usage的 provider/model/protocol/sourceRun/session/modelCall，绝不拿当前reload路由替代原来源。已删除源用合法已校验自足专档恢复。

## 四个原验收场景及主动边界

|要求|当前真实检查（F01 8项内，原始请求/SQL/文件留在 probes）|
|---|---|
|C01|独立 Rust process+authHTTP第二 wire 前轮 user/assistant、本次user各一次；另一session nonce不见；五family角色顺序；真实观测system整文及逐段SHA与wire一致；历史/导出2user/2final无内部档案|
|C02|A/B共同祖先、互斥nonce、真实工具只执行一次；重启/retry/rewind旧后缀不见；forkID稳定、RunFK NULL、真实源归档删除后恢复及再次fork|
|C03|34k真实文件 raw保完整，恢复仅32768B受限投影；provider/host IDs/resource/process/digest/truncated精确；错误原文不截；签名工具多轮重启、改provider明确失败零wire；校验码/配对/opaque/actor/同owner别session/旧NULL来源/跨owner负控；projection存储失败保原raw+真实收据并停止；伪坏argsDigest Success/Unknown/伪拒绝/缺源拒绝|
|C04|10个40k用户Run到保守预算并真实摘要；同版本+后缀重启；摘要HTTP400保全部原始记录无假replace；真实取消/重启无迟到假回复；真实副作用一次、SIGKILL/Interrupted后Unknown诊断，不补造完成收据/不重放|
|child|正式SubagentRuntime、真实compiler/gateway；保留授权裁剪profile材料，不读父完整历史/其它Agent私史；真实合法不同Agent override可到wire，普通Full伪造override零Run/wire；普通Run不混child正文；精确child并发20次正控+reset/异writer负控|
|权限交替|同other-owner session LocalUser→正确Web owner→LocalUser→Web四轮，旧工具只执行一次，原actor与Session owner分别绑定；foreign Device/Web拒绝且原史不改；真实fork/source删除及actor伪换负控|

不同Agent child 的人物内容错误仍真实复现（`f01-child-final-12`、`ex03-other-agent-persona-diagnostic.json`）：原 ContextCompiler cache `(session,bool)` 缺Agent维度。root明确由F05/EX03处理；本候选只证明合法授权/history不会误拒绝且父私史不泄漏，不改compiler、不签EX03 PASS。

## 原断言配套与回归保留

- `r05_t07_usage_trace` C09 最终原 `LocalUser` 调用和全部断言字节等于base。此前Web替代绿属错误权限假设有限窗口，已撤销；真实原C09新红后精准生产权限修复再绿。
- 两个旧ExchangeItem消费者仅增穷举臂，原断言/预期不动。
- resources 合成provider取最后当前user；closed_loop取最后当前user及之后本次tool tail，仅合成provider路由，不过滤生产历史。原完整压力次数、取消/线程/FD/RSS/文件/重启、每轮4 wire/顺序/freshnonce/原文件/权限全部保留。原C02由固定messages[3]改本次tail，仍逐字节assert_eq(v1)。
- tool_matrix 耗grant后真正pending且零dispatch，明确Reject后结算；所有原grant/foreign/spentonce/parked断言保留，root/W03批准精确夹具配套。
- T03 GET/branch 是内部完整节点链，原expected扩user/archive/final（reset原位置），不空过滤。全部旧ID/顺序/叉点/独立写/NULL FK/oldRun Completed/append-only/reset保留；逐档新增schema/source/bound/Session owner/原actor/digest核准和fork重绑/另一后缀不可见。UI历史/导出另外实际查无内部档案/重复final或工具卡。
- closeout 原P2只等child/thread空闲不证明父Run退出；全部原中间断言后 await原_second并验真实父Completed，再做原P3/stale/duplicate/count断言。旧Busy红、purebc窄窗口绿和当前绿全保，无加sleep/弱断言/改productiongate。

## 当前命令、失败与限制

最新 `final-prebuild-14`、`fmt-final-04`、`clippy-final-07 --workspace --all-targets -D warnings`、`contracts-final-04`（56+626）、`boundaries-final-04`（736features/69stores/11facts/70service sources）全部exit0。`f01-finals-17` 43/43实际绿：usage9/F01 8/F08 7/T03 13/T04 6，实际guard armed。

`workspace-final-06`已完整结算794.069秒，1880实际用例=1875绿/5红/0忽略、exit101、完整input unchanged；`f01-finals-19`直接预编译测试exe最终8/8实际绿，服务与testexe前后SHA相同、6个独立binary探针路径直接绑定成功stdout/raw wire/SQL/hash。05整轮795.8秒exit101，输入unchanged：5失败4targets。以下原bc失败不改原测试/F08掩红，整阶段不放行：

- F08新增投影事件与旧有序事件向量：cancellation_tree两项、run_lifecycle一项。亲跑pure-bc cancellation8（6绿2红）、lifecycle12（11绿1红）；F14另处理固定契约同步，必须保全部原ordered/approval并更严格验新投影。
- 原quota parked等待夹具：cancel_link_inheritance一项（W03真正purebc/root亲读，外部证据不是我的亲跑）；subagent_closeout parent-cancel一项（我亲跑purebc8，7绿1同红）。本Task未改。

首workspace01全部39真实失败按根因逐名列 `failure-root-cause-matrix.json`：30个nullable错误、3个child head窗口、1个明确Abandoned、1个错误LocalUser权限、2个合成当前输入夹具、1个已证F08向量、1个LAN超时历史环境。原panic nullable问题修后仍有独立F08向量红；quota早期本次branch问题修后仍有原purebc等待红，不能混为F01 PASS。后来出现的后台start、grant pending、C02固定索引、T03内部节点、父真正结算、深fork新红均单列。所有原失败/编译准备失败/停止/超时都保留，0匹配不是业务红。

## 输入、二进制、监督与交接边界

- 固定绝对 cargo1.98.1，全部适用Cargo `--locked -j8`，独有target。每test binary guard600秒/8192MiB不关闭；workspace外层2430秒，父监督TERM→有界KILL→wait/reap，独立合成home。
- 早期whole01/03错误600秒外层截断，02/旧regression TERM停止分别保exit124/-15、未跑范围非PASS；4个真实输入漂移窗口单列，不用作冻结源证明。正式R05原7命令合计19800秒，未用600外层假称整pipeline完成；本执行者当前完成必要workspace+受影响门禁，不签整阶段。
- 早期元数据只列Rust，未单列compiler非Rust模板/脚本prehash；purebc初approval元数据缺实际test executable before。不得事后补before。`supplemental-unmodified-product-inputs.json`明确capture时点、HEAD gitblob/currentSHA/mtime（mtime不是不可变证明），只作为限定补充。
- 最新增强窗口列全Rust、1069 tracked非Rust输入及执行脚本 pre/post；实际 FileContextMaterialSource 用产品目录，en identity/yuan/agent templates及fallback均在绑定内。C01真实观测整文摘要与第二wire相符。当前二进制/测试exe before/after 与输入锁绑定；独立Reviewer仍需在冻结副本完整重建/重验，不继承旧缺口闭合结论。
- 证据只交必要业务日志/脱敏请求/SQL快照文本/hash/命令/可重建脚本。合成home内 physical devices/device-credentials/local-token/私钥/credentials/config、target/binaries/fullreadonlybaseline/archive、SQL db备份均为 excluded_sensitive_ephemeral 或 rebuildable_local_artifact，不列成已交远端。manifest列精确文件清单，不盲拷全树。
- W03/F03独立候选产品未复制。本候选不包含其真实同DB门、TaskSupervisor issued binding/真实scope/lease延续/混DB拒绝。后续共享 runs/session_tree/sessions/background/storage机械融合需执行者核准并全新源/真实全部受影响回归+独立Review；真实started通知只同一driver durable commit之后，不能复制启动/结算。
- 130原叶仅12有限修复贡献，其余F01 NOT_RUN；38原Steps/A/交付逐原文保留全部整条OPEN。A08大型历史I/O、原WS/slash/UI完整入口、真实供应商/其它平台/正式打包未在本地合成窗口全验。F02/F03/EX01/02/11等保持OPEN，F08历史PASS不抹，但当前组合STALE。

最终results/manifest/SOURCEFAMILY/candidate及精确diff已准备，通知root冻结，由root另建全新独立只读Reviewer副本。本执行者不自签独立PASS。

最终源集SHA256：`c496d9fa91acc50ac552ec4608f2d1f275d375b3280a5153dfe0dfb62cab9db9`；实际服务binary SHA256：`1fbf11908706428b6b133d5ad0cfa191bd498b7c351e9e0d1332f81a632fc7c5`。

二进制窗口边界：Cargo18 source/input未变，但输出路径before `80fe600c58f6c9066366ea89f26d5f85dd2ae06b6a6fe245457ccfc44e0392f3`、after `1fbf11908706428b6b133d5ad0cfa191bd498b7c351e9e0d1332f81a632fc7c5`不同；不假称同binary。实际18独立进程均用after。final19直接执行已构建testexe，service/testexe前后及所有实际starts SHA完全相同。完整workspace与最后专项的两份已测构建产物摘要分别保留，不混写。

Guard边界：历史Python直属进程探针未arm Rust testguard（metadata明确false、外层600秒），不事后补作已arm。当前正式Rust永久探针及workspace实际arm600/8192。旧失败Rust临时home可能本地保留、不可交付，不声称广域清场；最新Pythonhome、wrapper和F27清理均实证，冻结检查own target无剩余进程。
