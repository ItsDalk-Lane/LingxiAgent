仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是LingxiAgent R06修复的一次性执行代理，只负责R06-AR-W04/R06-AR-F04。
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

初始使用实际已验收基线 bc2ce51fd005ea75b9949156974e15d2761eb734 的全新普通 clone，不带主工作区未接受产品；只有 F08 自身份额独立通过。准备文件 artifacts/rust-tauri/R06/AR/W04-PREP-01/source-contract-readback.json 是当前主工作区混合未接受候选的只读源码记录，不是实际运行证明。先在自己实际HEAD重新读全族生产路径。

原 T03/T05 全 Steps/交付/A05A06A09A10/本阶段原叶断言及 EX05/EX10 关联份额保留，不只四C；上下游 fork/live run许可/原文件回退与缺文件错误必须保留原生产合同。不提前实现整套 UI，不把本阶段必需服务行为移 R07；其他 Tasks 的缺口 OPEN。

C01真实服务上传/产物→正式消息→正常fork保留消息→新分支读旧sf_身份，父支仍可读、两次fork与兄弟同时存在、删除一支不影响另一支、伪造另一主体/Session被拒绝；不得旁路直接fork_session_files当提交证明。C02使用可控时钟、真实活动登记与正式清理入口，必须真正删一个合规无引用缓存并保住引用交付；真实时间回退/并发活动不能把会话误判冷。C03要有确定性真实引用提交/清理竞态窗口，并证明新引用不被检查后删除，不接受全永久引用使清理永远no-op。C04真实旧 sidecar→追加真实产物→登记中断/缺文件→重启/重复恢复，存在性/身份/主体/引用一致；无需真实用户迁移。

正式文件历史接线 NoopChangeLog、消息/工具结果引用、跨Session引用归属、managed-cache字节操作与DB登记的失败补偿都需扫描。不能全局旧ID别名跨分支暴露，事务/锁要覆盖“检查到实际删除”的整个窗口；阻塞IO取消后物理线程未退出须持续保住正式租约，不能仅记录已取消就释放。未验证的F02/F03/shared结果不继承，后续集成单独新演员实际跑。

至少一个隔离变异移除实际活动/引用/fork接线，正对照绿/至少1真实matching反例红。真实command/exit/候选与有效输入/actualbin before-afterSHA/独立合成home与READY/原请求SQL文件/取消线程退出和清理保留，不把grep存在或mock回包当业务证明。所有guard每bin600秒8192MiB开启，cargo --locked -j8，workspace外层2700秒、R05原7项目19800秒+有界开销；RSS未测null。targets/homes/credentials/bin LOCAL_ONLY；安全证据manifest逐SHA和最终owned product清单。逐项自检再对抗自检全部完成后freeze/end；无commit/push，自身仅READY_FOR_INDEPENDENT_REVIEW，不创建/兼任Reviewer。

正式生产二进制资源目录补充：若实际 executable 在工作区外的 owned target，原 resolve_product_dir 无项目祖先会回落 synthetic HOME/product；先实际确认内置 yuan/identity 材料位置。使用受支持显式 LINGXI_PRODUCT_DIR=本候选 ROOT/lib、真实记录有效env及材料SHA，使原业务断言运行；保留故意测试缺材料/错误配置的负例，不静默fallback、不补写旧env。W03同actual bin/原C02 exact1matching仅此环境差异正控真实PASS，记录为 artifacts/rust-tauri/R06/AR/W03-PREP-01/external-target-product-dir-positive-readback.json；未来演员不继承该结果，自己实际证明。

后续新运行资源约定（不改变当前活动运行）：合成资料根指 LINGXI_HOME，不是覆盖 OS HOME；OS HOME/系统工具与用户设置继承原值，所有实际服务/测试的有效 LINGXI_HOME 必须是本轮独占合成目录并证READY/路径来源。TMPDIR使用工作区外独占目录。针对已实证并行增量缓存造成的磁盘压力，后续新独立构建可明确记录 CARGO_INCREMENTAL=0，减少可重建中间缓存；保持原toolchain/--locked/-j8、产品特性、正式命令、断言、每bin guard600/8192 ON和原预算不变。把实际环境/产物执行时前后SHA加入绑定，不能把这个编译缓存选择写成关闭guard或隐瞒编译差异。已有已启动链不改环境。

临时缓存清理如确需执行，先证本任务归属/退出/无句柄和真正执行程序的SHA；权限位X_OK会把零字节incremental .lock误算成程序，精确被授权cache子树内锁文件的预期删除不得冒称真实程序变更。完整before原记录保留，缓存子树外全部executables与所有实际执行路径逐SHA核对。原helper若exit1就如实保留；未知原lsof/raw/free-before不能事后回填。清理不是行为验收，不能冲销原FAIL。
