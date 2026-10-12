仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是LingxiAgent R06修复的一次性执行代理，只负责R06-AR-W14/R06-AR-F14。
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

## 本轮执行补充（仅准备，未创建执行角色）

首轮必须在新隔离候选上实际重读最终 source，不能从这个准备记录继承任何 PASS。只读源码事实见 artifacts/rust-tauri/R06/AR/W14-PREP-01/source-contract-readback.json：目前只有 R02/R03/R04/R05 图，原 R05 七命令预算合计 19800 秒。完整修复后正式 R06/受影响上游链必须绑定最终候选；此前 Task 的有限绿色不等于阶段通过。

原 T08 全步骤/交付物/A15A16、原 R02 T04 全存储登记义务和本阶段全部范围保留。原范围依据是 SCOPE-01 的 281 unique leaves / 376 memberships / 687 assertions、37 Steps、24 deliverables、16 A、60 C、12 EX，以及后续真实新增必需问题。统计局部已验证用例不能减小总集合。281/376 的区别不得造成丢份额；合法后续阶段仅按原依据裁决，本阶段必需行为不能用伪造后移期限关闭。尚缺 10/08 原始人类提示词的 SPEC_GAP 不可编造补全。

生产者必须逐项由真正执行的具体案例及原始行为断言产生结果；grep 测试名字、手填 PASS、读取旧 Task 汇总或只核对整个套件数量都不能证明某条原断言。图→required 项→当前输入→实际命令→非空匹配→新原始结果→候选绑定必须双向核对，新增发现 R06-AR-F07-OBS-LIVE-CATALOG-HISTORY 等也属于当前必需缺口。旧证据不可签成新 source 的结果。

原样运行 r02_registry_consistency.py，保留真实缺 v8/v9（以及实际新候选 v10 等）的旧失败；用真正 inspector 新库和合成 v7/v8 升级库，严格追加真实版本、名称和 SQL hash，不改 v1-v7、不把 equality 改 subset、不让空库手填 receipt 代替实际升级。所有新增版本需按最终源码登记，重复启动真实幂等。

现有 original cancellation_tree/run_lifecycle/subagent_closeout 的失败不得删除或过滤。若既有精确事件期望因已独立接受 F08 新投影新增而过时，应保留原取消/批准/物理退出与事件顺序/数量语义，增加对真正 projection 内容、关联、位置、完整 raw 结果的严格检查，不能仅放宽为 contains 或跳过新事件。此配套改动使 F08 旧证据 STALE，必须由全新对应 Review 复验当前 source，不自动继承已验 Task 结果。原取消前未真正入队测试要用可观测真实队列/响应屏障固定前置，不能加睡眠猜时序或改成无论进入何状态都绿。

负向电池必须实际运行删除 required scenario、删除迁移行、错绑定案例、零匹配、篡改摘要、重放旧候选证据、伪造期限后移等，每项真正命中且非零指出缺口；0 matching/编译失败不是行为反例。还原的当前正对照真实运行并通过；仍有产品必需红项时整门禁 FAIL，不能令 wrapper 总返回成功。未执行/宿主终止/来源漂移/guard触发如实 NOT_RUN/BLOCKED/FAIL，不算 PASS。

每实际 bin guard 600秒/8192MiB，workspace 外层2700秒，R05 七命令原样顺序合计19800秒+有界开销；嵌套门禁继续保留自己的限制，不外层600秒截断。cargo --locked -j8，用实际 source/输入/lock/runner/有效 env、真实合成 home READY 和实际执行 bin before-after SHA 绑定。必要构建目标放在工作区外的本任务合成目录并记 argv/env，不借此排除产品 source；不改宽 ignore 或关闭 guard。原始 stdout/stderr、退出、SQL/文件/wire 关键证据保存，完整 homes/credentials/targets/bins LOCAL_ONLY。有效源码发生任何变化后重跑受影响结果，不补写未知运行前 hash。

全 Task 自检→对抗自检→完整 freeze/handoff 后结束一次性演员，仅 READY_FOR_INDEPENDENT_REVIEW。不要 commit/push，不兼任 Task Reviewer；此 Task 的 Reviewer 也不等于最终全新阶段 Reviewer。没有全十五 Task 独立有效 PASS、原始所有义务、最终 R06 与 R05→R04→R03→R02 和放行公式全部满足不得 R06_ACCEPTED，也不得开始 R07。

正式生产二进制资源目录补充：若实际 executable 在工作区外的 owned target，原 resolve_product_dir 无项目祖先会回落 synthetic HOME/product；先实际确认内置 yuan/identity 材料位置。使用受支持显式 LINGXI_PRODUCT_DIR=本候选 ROOT/lib、真实记录有效env及材料SHA，使原业务断言运行；保留故意测试缺材料/错误配置的负例，不静默fallback、不补写旧env。W03同actual bin/原C02 exact1matching仅此环境差异正控真实PASS，记录为 artifacts/rust-tauri/R06/AR/W03-PREP-01/external-target-product-dir-positive-readback.json；未来演员不继承该结果，自己实际证明。

补充监督取消检查：W09正式56的总控实际取消收据 W09-PREP-01/formal56-cancelled-partial-actual-readback.json 记录外层SIGTERM/wait真实-15，已启动原workspace50实际bin而非纯准备；原xtask注册命令独立PG短暂继续，最终自然退出并PS消失，adopted Cargo exit UNKNOWN，不能把外层reap等同所有后代wait。source verify.rs既有 setsid/session归属与timeout cleanup防误杀约束必须保留；这条记录尚未证明具体生产根因。F14执行时补亲跑外层取消/timeout实际登记子层的监督边界：不能隐藏孤儿、不能扩大按名字杀进程、不能把未知退出码填0；若仍活动必须如实BLOCKED并保留安全归属。分析正确关闭/记录全部注册链的原合同，不因新R06接图破坏原R02安全清理约束。

后续新运行资源约定（不改变当前活动运行）：合成资料根指 LINGXI_HOME，不是覆盖 OS HOME；OS HOME/系统工具与用户设置继承原值，所有实际服务/测试的有效 LINGXI_HOME 必须是本轮独占合成目录并证READY/路径来源。TMPDIR使用工作区外独占目录。针对已实证并行增量缓存造成的磁盘压力，后续新独立构建可明确记录 CARGO_INCREMENTAL=0，减少可重建中间缓存；保持原toolchain/--locked/-j8、产品特性、正式命令、断言、每bin guard600/8192 ON和原预算不变。把实际环境/产物执行时前后SHA加入绑定，不能把这个编译缓存选择写成关闭guard或隐瞒编译差异。已有已启动链不改环境。

临时缓存清理如确需执行，先证本任务归属/退出/无句柄和真正执行程序的SHA；权限位X_OK会把零字节incremental .lock误算成程序，精确被授权cache子树内锁文件的预期删除不得冒称真实程序变更。完整before原记录保留，缓存子树外全部executables与所有实际执行路径逐SHA核对。原helper若exit1就如实保留；未知原lsof/raw/free-before不能事后回填。清理不是行为验收，不能冲销原FAIL。

补充实际运行缺口（W09正式59，不继承PASS）：W09-PREP-01/formal59-full-chain-actual-readback.json 已核对30原命令、R05/R04/R03均FAIL、source稳定；原managed worktree合法.git为文件导致legacy基础物化检查真实失败，完整原E0-E5合同仍保留。修复时分别验证own.git目录和合法.git文件checkout、原baseSHA资料物化、源与目的真正互不包含及用户文件保护；不得以新clone正控冒作原窗口已通过，不得删掉原旧入口份额。外层index仅xtask1，不证明292嵌套运行；当前作者最终复核报告其中289窗口有内置guard、3个A16原scratch mutation_probe未链接guard，作者正按同原seed/argv/保留binary做外置逐PID600/8192补证。读取最终真实索引再下结论，旧guard缺口保留；正式门禁监督需覆盖scratch和直接shipping，以及原复杂子层归属，不仅依赖Cargo runner或环境变量。任何新补充执行不能改写旧FAIL或UNKNOWN。

## 补充当前已提交F08的原R03消费者失败线索 2026-10-11T23:43:07.634844+00:00

两名新执行者当前scope实际报告原cancellation_tree的严格事件序列与tool_result_model_projection新增事实不一致；W04在自己的纯bc普通clone另复跑6/8（两原strict reds）、配额夹具6/7（child已completed而未入队）。根HEAD到被审2e193仅一个F08提交，总控亲核，但本附加仅源于执行者报告并未替代新Reviewer。具体待最终冻结日志/SQL/bin/inputs按真实记录再核对。W14应准确接所有原消费者、序列、payload/raw与受限projection，必要配套测试保原取消与每个原事件完整断言并扩展真实新增投影事实；禁止删/忽略事件、变contains、修假数量或退休门禁。既有F08独立PASS限原精确份额，不等于这些原R03必需回归已绿。配额补证不可恢复跨工具持有模型许可来制造等待，F11 permit1原合同仍需保持。

## W04 新终态与历史依赖的分工

W04 当前执行者报告存储拒绝已真实形成 failed.storage_error、零非法消息/引用/模型调用，但无消息失败 Run 未被真实 history 入口呈现。总控明确由同一 W04 一次性执行者完成这一新增终态的最小完整生产与历史消费者修复，按原合同核对无消息锚、分页、ETag、分支隔离；不伪造用户消息、不将失败审计加入模型历史。F14 对最终 W04 冻结输入及所有共享路径接线并实际门禁验证，不额外代替 W04 做业务历史补修。当前仅报告/分工，W04新红与未知均保留，不继承任何 PASS；F01/F14 与所有实际受影响原消费者须在最终组合候选重新验收。

## 独占副本准备完成，仍未派发

2026-10-12T00:10:11.794056+00:00：普通独立 clone `/private/tmp/lingxi-r06-w14-executor-01`，own.git目录、detached HEAD bc2ce51fd005ea75b9949156974e15d2761eb734，原始用户设置未复制。已精确复制原任务书/不可变规格/全部原义务/当前控制/本Task准备材料/限定F08交付线索与own Node依赖；产品仍为纯已提交基线，无主dirty产品或未接受候选整文件覆盖。实际材料与两npm锁/基线Rust锁摘要见 `artifacts/rust-tauri/R06/AR/W14-PREP-01/own-clone-prepared-not-dispatched.json`。尚无执行角色、没有行为测试、没有验收。

W04当前追加v10分支锚与托管删除闭包、W11 embedding native receipt及未知费用处理仍在变化，不能假定已经提交/已冻结或预编造新SQL名称和hash。已受平台阻止的六类Review不得在本Task换名重试。共享行为或最终组合 source 需要其后全新集成Executor与Reviewer；本准备不分配新范围、不继承局部绿、不代替StageReviewer。
