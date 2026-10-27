仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是LingxiAgent R06修复的一次性执行代理，只负责R06-AR-W13/R06-AR-F13。
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

## 本轮执行补充（尚未派发；工作区/角色未建立）

实际已独立验收基线为 bc2ce51fd005ea75b9949156974e15d2761eb734，只接受 F08 自身份额。不能导入主工作区未接受的 W01/W02/W03/W05/W06/W09/W10/W12 整文件。首次候选可在该基线的全新独立普通 clone 实现本 Task；公共文件后续必须全新集成执行者及 Review，不由总控语义合并。

只读准备见 artifacts/rust-tauri/R06/AR/W13-PREP-01/source-contract-readback.json；它绑定的是混有未接受候选的主工作区源码，只是事实准备，不是运行失败/修复或验收。先在自己的实际基线重新核对所有路径。正式提交的 input/requestId 与原重复请求身份、同主体同 Session 去重保持正确；显式来源选择必须进入语义身份，同 requestId 改范围不能误复用旧接受。合法无资料提问仍保持原合同。不要允许模型或 payload 伪造 principal/Agent/Run/attempt。

原 T01 与 T07 全 Steps/交付/A01A02A13A14、知识原叶/原 claims、EX07/EX08/EX09 关联未决都要逐条保留；本 Task 只实际关闭自己经证明的份额，资料解析、失效清理、结构化分块未验证就 OPEN，不声称全原叶完整。R00 281 unique/376 memberships/687 assertions 全阶段总量不能改成局部计数。

C01必须真实生产二进制、认证正式提问，替身从收到的请求找服务铸造的范围，再经真实工具调用而不是调用测试方法。A/B冲突来源、当前轮重选、另一 Session/Run、另一主体、模型伪造、撤权/删除、版本更新、共享来源、缺资料、程序引用及重启只读恢复都留 wire/SQL/文件的关键原始证据；移除正式 scope 接线的隔离变异应真正运行至少1匹配并失败。不能拿手写 scope、工具注册命中、echo/握手替代完整业务。实时授权与内容冻结分开说明并实际验证，资料中的指令不可提高权限。

当前其他 Task 里真实失败的 registry 9vs7、F08 新投影旧严格事件断言、停取消/额度等待、知识请求关联和旧任务退出都保留原断言，不在此改松、过滤或算 PASS。相关新 source 会使旧证据 STALE，必须重跑当前候选。

每个实际二进制 guard 600秒/8192MiB 开启；完整 workspace 外层2700秒含有界开销，正式R05原7顺序预算19800秒+有界开销，不能外层600秒提前截断后算FAIL行为或PASS。使用 cargo --locked -j8；有效 source/lock/scripts/nonRust/runner/实际执行bin before-after精确SHA、真实home/READY/argv/cwd/time/exit/替身/清理都保存；没有测量RSS写null。自己的 targets/合成home不得纳入Git或候选product，必要仅本 clone .git/info/exclude 精确自有缓存路径并留前后证据，不能扩大门禁排除。全真实 homes/credentials/编译bin是 LOCAL_ONLY，交付安全证据manifest与实现path SHA。完成所有本Task逐项自检再对抗自检、完整交接/freeze后结束一次性角色；不要自称独立PASS、不要commit/push、不要启动Reviewer。

原产品选择 schema 与授权继承补充：必须完整读取 core/engine.ts buildConversationKnowledgeContext、core/desktop-session-submit.ts、server/routes/chat.ts、shared/knowledge-refs.ts normalizeKnowledgeRefs、lib/tools/knowledge-scope.ts 及 core/session-manifest/knowledge-ancestry.ts。原 knowledgeRefs:{notebookIds,mode} 兼容形状和空选择行为可作为最小兼容入口依据，不能忽略既有严格校验。合法受托子代理如原合同要求可在不可伪造真实父 lineage/当前 root turn 授权内继承来源；另一无关Session/Run和模型自造 owner 不得获得范围，不能把全部跨Session继承简单封禁来让反例通过。此准备仅源码事实、不授予新范围、不代表已运行。

正式生产二进制资源目录补充：若实际 executable 在工作区外的 owned target，原 resolve_product_dir 无项目祖先会回落 synthetic HOME/product；先实际确认内置 yuan/identity 材料位置。使用受支持显式 LINGXI_PRODUCT_DIR=本候选 ROOT/lib、真实记录有效env及材料SHA，使原业务断言运行；保留故意测试缺材料/错误配置的负例，不静默fallback、不补写旧env。W03同actual bin/原C02 exact1matching仅此环境差异正控真实PASS，记录为 artifacts/rust-tauri/R06/AR/W03-PREP-01/external-target-product-dir-positive-readback.json；未来演员不继承该结果，自己实际证明。

后续新运行资源约定（不改变当前活动运行）：合成资料根指 LINGXI_HOME，不是覆盖 OS HOME；OS HOME/系统工具与用户设置继承原值，所有实际服务/测试的有效 LINGXI_HOME 必须是本轮独占合成目录并证READY/路径来源。TMPDIR使用工作区外独占目录。针对已实证并行增量缓存造成的磁盘压力，后续新独立构建可明确记录 CARGO_INCREMENTAL=0，减少可重建中间缓存；保持原toolchain/--locked/-j8、产品特性、正式命令、断言、每bin guard600/8192 ON和原预算不变。把实际环境/产物执行时前后SHA加入绑定，不能把这个编译缓存选择写成关闭guard或隐瞒编译差异。已有已启动链不改环境。

临时缓存清理如确需执行，先证本任务归属/退出/无句柄和真正执行程序的SHA；权限位X_OK会把零字节incremental .lock误算成程序，精确被授权cache子树内锁文件的预期删除不得冒称真实程序变更。完整before原记录保留，缓存子树外全部executables与所有实际执行路径逐SHA核对。原helper若exit1就如实保留；未知原lsof/raw/free-before不能事后回填。清理不是行为验收，不能冲销原FAIL。

## 独占副本准备完成，仍未派发

2026-10-12T00:10:11.794056+00:00：普通独立 clone `/private/tmp/lingxi-r06-w13-executor-01`，own.git目录、detached HEAD bc2ce51fd005ea75b9949156974e15d2761eb734，原始用户设置未复制。已精确复制原任务书/不可变规格/全部原义务/当前控制/本Task准备材料/限定F08交付线索与own Node依赖；产品仍为纯已提交基线，无主dirty产品或未接受候选整文件覆盖。实际材料与两npm锁/基线Rust锁摘要见 `artifacts/rust-tauri/R06/AR/W13-PREP-01/own-clone-prepared-not-dispatched.json`。尚无执行角色、没有行为测试、没有验收。

W04当前追加v10分支锚与托管删除闭包、W11 embedding native receipt及未知费用处理仍在变化，不能假定已经提交/已冻结或预编造新SQL名称和hash。已受平台阻止的六类Review不得在本Task换名重试。共享行为或最终组合 source 需要其后全新集成Executor与Reviewer；本准备不分配新范围、不继承局部绿、不代替StageReviewer。
