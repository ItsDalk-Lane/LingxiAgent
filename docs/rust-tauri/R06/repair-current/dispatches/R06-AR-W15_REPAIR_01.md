仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是LingxiAgent R06修复的一次性全新 Repair 代理，只负责R06-AR-W15/R06-AR-F15。
你不是总控、不是独立Reviewer，完成后不接下一个任务。

必须读取本完整修复提示词、该F-ID全部源码事实/修复要求/四项C-ID、关联原R06 Task全部Steps与A-ID、原始R00行为、当前真实源码和所有上游/消费者，以及先前依赖Task有效交接。
研究SHA不是强制回退点。先记录实际HEAD、candidate范围、原始用户修改和隔离home；不reset/clean，不改真实数据。

执行：
1. 从真正生产入口追踪该问题，区分本提示词的源码事实和你本机实际复现；先保存一个正常正对照。
2. 对源码确认的错误建立正确契约断言，在未修代码上取得行为失败证据。安全反例只用合成临时文件和loopback服务。编译失败、脚本未运行、测试0匹配不算“旧代码红”。
3. 若初始反例不成立，继续核对前置与调用链；发现事实已由新提交修复，应给源码与运行证据，不凭主观意见改需求或删案例。真正证伪交全新Reviewer裁决。
4. 找根因并扫描同族全部路径，完成最小完整修复，接通生产调用，处理错误、权限、取消、存储和旧数据边界。
5. 第一层逐项自检：对当前F-ID全部C-ID、原Task Steps/交付物/A-ID和影响范围逐项填“期待/实际/证据/状态”，不得留一个“总体通过”代替。
6. 第二层对抗性自检：你自己作为攻击者构造权限伪造、竞态、错误注入、边界输入、重启等本Task相关反例；核对测试没有mock掉被测对象。至少做一个能重新暴露旧根因的隔离变异或等效旧版本红/新版本绿对照。
7. 跑关联回归，保留全部失败和重试记录。不要只保存最后一次绿，不删除/改松原断言，不改变原始到期范围，不虚报平台/LIVE。
8. 检查真正发给模型的数据、真实文件/数据库/进程结果；源码存在和mock回包不是行为证明。
9. 输出实现差异、所有自检结果、同族扫描、未验证、影响/STALE清单及候选摘要。状态只可READY_FOR_INDEPENDENT_REVIEW。

禁止commit/push/发布；Git由总控在独立PASS后处理。不能自签独立验收。完成后结束该子代理。

## 本轮补充：全新 Repair，必须等待完整独立 FAIL 及精确交接

这是准备中的具体派发书，尚未创建 Repair 角色。独占普通 clone 为 /private/tmp/lingxi-r06-w15-repair-01，own.git、实际基线 bc2ce51fd005ea75b9949156974e15d2761eb734。总控随后提供第一次独立 Reviewer 的完整终结裁决、安全清单及精确传递回执；未提供完整裁决不能凭部分消息启动 Repair。主工作区混有未接受候选，不能作为本 Task 已验收源。只继承精确候选字节和失败资料，不继承作者或 Reviewer 角色，不继承任何 PASS。你不接替其它被平台阻断的 Reviewer。

首先完整读取 1234 原义务记录、原八Tasks全部37Steps/24交付/16A、281独立叶376成员687断言、90上游份额、12EX根因簇和所有旧Review实际74items与原14docs，不以中低标签豁免契约。逐叶原始行为/原到期阶段/当前份额/合法后续/实际生产入口/实际正反例/未决/owner截止一一对齐；原28avatar/persona叶R06-only与R07任务冲突不能由总控一句R07裁决。仅以原需求/本次用户规格与真实源码证据明确裁决供全新Reviewer；不能修改只读原任务书/R00来取绿。原规格3禁止恢复已撤回独立知识研究引擎、完整本地模型管理/强制子代理目录、提前重做UI；真实模型付费/目标OS原生/Electron Tauri迁移按原可延期范围保留未完成owner截止，不假PASS。

先真实重现BE04硬编码指标与其它missing/NULL误记0。当前r06_t08_behavior_eval.rs里BE04三实际loopbackprovider调用却tokens_json(0,0)；usage_stats对NULL合并且.sum口径全outcomes，tokens_json字符串却宣称succeeded rows。这些目前只源码记录，必须 actual world/wire/usage/eval record 同窗复算并建立旧red/新green；不能改成另一套常数或只字符串grep。实际token与known/missing/unknown行覆盖分开，不把没有token测量算0，不用modelstub演示冒原旧引擎真正对照或LIVE。真正0仅有可追溯测量，未测null/NOT_MEASURED；失败任务、成功模型调用、schema有效/语义参数正确/任务真实效果四维不得混算。

原BE01—BE10全部真实电池、BE10写错b但a没变的负控、BE04目录外工具始终拒绝/解释腿和注入腿、原A15A16保留。也检查其它C-ID错误映射：echo/握手/manifest函数命中不能绑完整业务；对隔离副本missing→deferred无原依据、删除negative、旧候选混用和指标缺测伪0应实际非零并点名，恢复正控。需要F07新catalog语义集成时不改松旧能力/权限断言，应按真实三阶段发现→描述→调用与服务效果强化，并由新Reviewer核验。

当前唯一状态须是实际候选/独立Review/未决集合/真实Git回执；旧R06 ACCEPTED被本轮推翻标历史、原旧失败和无效窗口保留，不删除“不好看”证据。R06_NOT_ACCEPTED及Stage NOT_RUN在未完成时是正确事实，不能为了C04改成accepted。每项已关闭至少真实正/反语义证据，source-only/未跑严格区分。F01/F02/F06/F12独立Review仍受平台Daybreak访问未确认阻塞，不替代/换称重试；本Task只自己的正常义务。Stage新Reviewer及最终10项公式仍需随后实际执行，不自签Stage。

每testbin600sec/8192MiB ON；cargo绝对1.98.1 --locked -j8，workspace2700秒，正式R05原7项目19800秒+有界开销。独立生效synthetic LINGXI_HOME/READY/current exact candidate/runner/input/source/lock/bin before-after/argv/cwd/time/exit/mock/cleanup/RSS未测null，safe manifest/实际source全路径SHA；homes credentials buildbins工作副本LOCAL_ONLY。所有错误窗口保存，自己的局部绿不得替最终正式R06或前序全链。逐项自检再对抗自检完成/freeze/end，下一新Reviewer，禁止自身commit/push或Reviewer角色。

正式生产二进制资源目录补充：若实际 executable 在工作区外的 owned target，原 resolve_product_dir 无项目祖先会回落 synthetic HOME/product；先实际确认内置 yuan/identity 材料位置。使用受支持显式 LINGXI_PRODUCT_DIR=本候选 ROOT/lib、真实记录有效env及材料SHA，使原业务断言运行；保留故意测试缺材料/错误配置的负例，不静默fallback、不补写旧env。W03同actual bin/原C02 exact1matching仅此环境差异正控真实PASS，记录为 artifacts/rust-tauri/R06/AR/W03-PREP-01/external-target-product-dir-positive-readback.json；未来演员不继承该结果，自己实际证明。

后续新运行资源约定（不改变当前活动运行）：合成资料根指 LINGXI_HOME，不是覆盖 OS HOME；OS HOME/系统工具与用户设置继承原值，所有实际服务/测试的有效 LINGXI_HOME 必须是本轮独占合成目录并证READY/路径来源。TMPDIR使用工作区外独占目录。针对已实证并行增量缓存造成的磁盘压力，后续新独立构建可明确记录 CARGO_INCREMENTAL=0，减少可重建中间缓存；保持原toolchain/--locked/-j8、产品特性、正式命令、断言、每bin guard600/8192 ON和原预算不变。把实际环境/产物执行时前后SHA加入绑定，不能把这个编译缓存选择写成关闭guard或隐瞒编译差异。已有已启动链不改环境。

临时缓存清理如确需执行，先证本任务归属/退出/无句柄和真正执行程序的SHA；权限位X_OK会把零字节incremental .lock误算成程序，精确被授权cache子树内锁文件的预期删除不得冒称真实程序变更。完整before原记录保留，缓存子树外全部executables与所有实际执行路径逐SHA核对。原helper若exit1就如实保留；未知原lsof/raw/free-before不能事后回填。清理不是行为验收，不能冲销原FAIL。


## 新一轮必须处理的实际独立反例（目前部分结果，等待终结绑定）

独立窗口 independent-sql-wire-mutations-03 的真实 17 项中，七个指定反例实际点名拒绝，十个错误报告被 exit 0 接受：own-be01-file-false-with-pass、own-be03-rejection-effect-false、own-be01-expected-calls-999、own-coverage-status-false、own-outcome-coverage-false、own-be07-full-chat-claim、own-be04-session-forgery、own-candidate-bindings-deleted、own-comparison-mode-live-with-false-flags、own-coverage-invented-outcome。逐个建立旧 red/新 green，扫描同族所有其他 BE/身份/集合/空集/模式/语义和实际任务结果关系，不能只按这十个名字加特判。完整实际文件结果、生产 SQL、物理 loopback 请求与测量状态必须支撑报告，缺原实证或未知字段不能靠报告自身断言“已知/通过”。保留原 BE-10 的任务失败负控、BE-04 三腿，以及真正 0、UNKNOWN、PARTIAL 的实测区别。两个真实独立 synthetic home 可出现同一 runId，必须绑定真实 home、run、session、attempt、wire 与源；不能以 runId 全局唯一或 endpoint 计数代替。删光候选绑定仍通过尤其要修复为缺必要精确候选证据就拒绝，不能只循环检查提交者自选空集合。原四维契约由真实任务效果支持，固定模型替身不得冒旧引擎或真实模型，原期待调用次数不能随报告改 999 来掩盖多调用。

独立 Reviewer 本次完整原 workspace actual exit 101 / 1061.842 秒，初步逐 stdout 是 1864 passed / 8 failed / 146 summaries / 6 原 targets；不是作者的 1865/7。本次 subagent_closeout 两个原断言都失败，原六个 targets 其余取消/队列/完整事件/实际临时边界失败也保留。终结清单到达后以其真实 SHA/原窗口为准。不得改松原测试，不得把它们改称 R07。确由别的 Task 拥有的业务根因需精确 owner/STALE/最终合源重跑，但未修复不能写本 Task 全原义务 PASS。

首次独立 Reviewer 已逐原条款裁决 28 avatar/persona 叶：原 R06 到期服务义务不能由 R07 管理条款整体豁免；完整 UI 的合法延期仅按原 R08/R09 等自身明确条款和截止保留，不把 R07 管理条款当整体服务豁免。终结逐叶依据需亲读，并继续保留原 1234 条记录/281 叶/376 成员/687 断言/37Steps/16A 与十二 EX 的未完成状态，不把全部硬写 OPEN 的报告等同阶段关闭，也不自行关闭业务叶。其业务实现需要相关独立新 Task 实际验收及最终组合源；本 Repair 不能代替这些未发生的验收。

必须修正源/有效运行/实际 binaries 的绑定不足，清楚区分 PID 库存快照与启动前实际 SHA、真实 terminal/wait/physical cleanup 与采样进程消失；测不到就 null/NOT_MEASURED。补充记录不能倒填旧窗口未知数据，不以“最终目录里有程序”冒充启动前已绑定。所有实际原失败、并行 JSONL 无效、环境准备错误、旧缺 PID 测量继续保留。完整源和输入档案留 LOCAL_ONLY，安全文件只包含明确必要候选单文件/摘要/去敏证据，不把全库归档写进 safe。最终终结清单按有效最终源、实际有效环境/锁及 source/test/service before-after 绑定；freeze 后不再改候选。

该 Repair 必须第一层逐项自检，再第二层对抗自检，完整交接并结束；总控再创建全新 Reviewer 实际验收。自己的审查不算独立 PASS，已知未完也不能跳过第一次新的实际 Reviewer。绝不提交或推送，Stage 尚不可启动，R07 不开始。


## 完整第一次独立 FAIL 已终结并精确送达，允许本新 Repair 开工

完整 Reviewer 已结束不可复用；独立报告 `artifacts/rust-tauri/R06/AR/W15-REVIEWER-01/REPORT.md` SHA bf9030efba18fc3e58ca63d733f8dc9e4a71a173639e0dddbbe6c0e2e4712e20，全部11发现/23影响含12EX及28逐叶原条款裁决都在本副本。安全清单389份205271529B逐字节已核，safe manifest SHA69ddf42c7d3482220f3b2d86824dc8138b22f9157d94c22d8ea1ab7360c25d7a；END SHAec69de7628051c95d8ec9858abcdc27d8a7bc6486ea1a3e3cad765b05ca806f9。根传递回执 `artifacts/rust-tauri/R06/AR/W15-REPAIR-PREP-01/first-review-final-integrity-and-transfer.json`，初始4059源码与作者及独立Reviewer冻结全族逐SHA相同，只有两候选源覆于纯bc独占普通clone；无主产品覆盖，无用户设置复制。首次完整正式FAIL计1，先前部分发现不另计轮次。

按 FINDINGS.json 全11项逐条根因/同族/原义务/真实证据推进，不只修十个具体字符串。其它业务Task或EX owns 的缺口也属于完整原义务，当前没有通过，不可一句“归别人/R07”关闭；新真实候选/所需精确依赖由总控随后提供，活动W04/W11源不能偷抄或假冻结。若必须语义整合共享候选，明确完整依赖及同族回归，在本独占源实际执行；root不会语义merge代替你。可以先完成自己核验/评测同族的最小完整修复并保留所有业务OPEN，但最终完整Task PASS必须实证原到期业务满足。不得以诚实OPEN清单单独充当本Task完整成功。

第一次Reviewer已是实际FAIL，新的交接/current report必须正确反映历史事实，不能继续硬写“独立Review从未运行”。旧作者历史scope/frozen报告保留；为本轮新候选另建真实报告/冻结输入，区分历史FAIL、本轮尚未Review及未来Stage尚未执行。不要把当前root controls复制改写旧 frozen 输入，修复后必须全新的独立Reviewer；不会复用本角色。
