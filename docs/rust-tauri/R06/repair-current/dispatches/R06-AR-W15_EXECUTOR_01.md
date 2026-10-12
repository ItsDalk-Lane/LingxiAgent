仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是LingxiAgent R06修复的一次性执行代理，只负责R06-AR-W15/R06-AR-F15。
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

## 本轮补充：PREPARED_NOT_DISPATCHED
尚未创建执行者/绑定workspace。主工作区混有未接受候选不能当全验收源；最终workspace/HEAD与各候选来源须总控精确提供再开工。已验收只F08自身份额bc2ce51fd005ea75b9949156974e15d2761eb734。

首先完整读取 1234 原义务记录、原八Tasks全部37Steps/24交付/16A、281独立叶376成员687断言、90上游份额、12EX根因簇和所有旧Review实际74items与原14docs，不以中低标签豁免契约。逐叶原始行为/原到期阶段/当前份额/合法后续/实际生产入口/实际正反例/未决/owner截止一一对齐；原28avatar/persona叶R06-only与R07任务冲突不能由总控一句R07裁决。仅以原需求/本次用户规格与真实源码证据明确裁决供全新Reviewer；不能修改只读原任务书/R00来取绿。原规格3禁止恢复已撤回独立知识研究引擎、完整本地模型管理/强制子代理目录、提前重做UI；真实模型付费/目标OS原生/Electron Tauri迁移按原可延期范围保留未完成owner截止，不假PASS。

先真实重现BE04硬编码指标与其它missing/NULL误记0。当前r06_t08_behavior_eval.rs里BE04三实际loopbackprovider调用却tokens_json(0,0)；usage_stats对NULL合并且.sum口径全outcomes，tokens_json字符串却宣称succeeded rows。这些目前只源码记录，必须 actual world/wire/usage/eval record 同窗复算并建立旧red/新green；不能改成另一套常数或只字符串grep。实际token与known/missing/unknown行覆盖分开，不把没有token测量算0，不用modelstub演示冒原旧引擎真正对照或LIVE。真正0仅有可追溯测量，未测null/NOT_MEASURED；失败任务、成功模型调用、schema有效/语义参数正确/任务真实效果四维不得混算。

原BE01—BE10全部真实电池、BE10写错b但a没变的负控、BE04目录外工具始终拒绝/解释腿和注入腿、原A15A16保留。也检查其它C-ID错误映射：echo/握手/manifest函数命中不能绑完整业务；对隔离副本missing→deferred无原依据、删除negative、旧候选混用和指标缺测伪0应实际非零并点名，恢复正控。需要F07新catalog语义集成时不改松旧能力/权限断言，应按真实三阶段发现→描述→调用与服务效果强化，并由新Reviewer核验。

当前唯一状态须是实际候选/独立Review/未决集合/真实Git回执；旧R06 ACCEPTED被本轮推翻标历史、原旧失败和无效窗口保留，不删除“不好看”证据。R06_NOT_ACCEPTED及Stage NOT_RUN在未完成时是正确事实，不能为了C04改成accepted。每项已关闭至少真实正/反语义证据，source-only/未跑严格区分。F01/F02/F06/F12独立Review仍受平台Daybreak访问未确认阻塞，不替代/换称重试；本Task只自己的正常义务。Stage新Reviewer及最终10项公式仍需随后实际执行，不自签Stage。

每testbin600sec/8192MiB ON；cargo绝对1.98.1 --locked -j8，workspace2700秒，正式R05原7项目19800秒+有界开销。独立生效synthetic LINGXI_HOME/READY/current exact candidate/runner/input/source/lock/bin before-after/argv/cwd/time/exit/mock/cleanup/RSS未测null，safe manifest/实际source全路径SHA；homes credentials buildbins工作副本LOCAL_ONLY。所有错误窗口保存，自己的局部绿不得替最终正式R06或前序全链。逐项自检再对抗自检完成/freeze/end，下一新Reviewer，禁止自身commit/push或Reviewer角色。

正式生产二进制资源目录补充：若实际 executable 在工作区外的 owned target，原 resolve_product_dir 无项目祖先会回落 synthetic HOME/product；先实际确认内置 yuan/identity 材料位置。使用受支持显式 LINGXI_PRODUCT_DIR=本候选 ROOT/lib、真实记录有效env及材料SHA，使原业务断言运行；保留故意测试缺材料/错误配置的负例，不静默fallback、不补写旧env。W03同actual bin/原C02 exact1matching仅此环境差异正控真实PASS，记录为 artifacts/rust-tauri/R06/AR/W03-PREP-01/external-target-product-dir-positive-readback.json；未来演员不继承该结果，自己实际证明。

后续新运行资源约定（不改变当前活动运行）：合成资料根指 LINGXI_HOME，不是覆盖 OS HOME；OS HOME/系统工具与用户设置继承原值，所有实际服务/测试的有效 LINGXI_HOME 必须是本轮独占合成目录并证READY/路径来源。TMPDIR使用工作区外独占目录。针对已实证并行增量缓存造成的磁盘压力，后续新独立构建可明确记录 CARGO_INCREMENTAL=0，减少可重建中间缓存；保持原toolchain/--locked/-j8、产品特性、正式命令、断言、每bin guard600/8192 ON和原预算不变。把实际环境/产物执行时前后SHA加入绑定，不能把这个编译缓存选择写成关闭guard或隐瞒编译差异。已有已启动链不改环境。

临时缓存清理如确需执行，先证本任务归属/退出/无句柄和真正执行程序的SHA；权限位X_OK会把零字节incremental .lock误算成程序，精确被授权cache子树内锁文件的预期删除不得冒称真实程序变更。完整before原记录保留，缓存子树外全部executables与所有实际执行路径逐SHA核对。原helper若exit1就如实保留；未知原lsof/raw/free-before不能事后回填。清理不是行为验收，不能冲销原FAIL。
