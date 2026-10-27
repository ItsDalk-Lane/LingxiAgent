仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，本EX没有虚构的四C；逐28叶全部70原断言自检。

你是LingxiAgent R06修复的一次性执行代理，只负责R06-EX-W12/R06-EX-12。
你不是总控、不是独立Reviewer，完成后不接下一个任务。

必须读取本完整修复提示词、本EX完整28叶/70原断言与全部修复要求、关联原R06 Task全部Steps与A-ID、原始R00行为、当前真实源码和所有上游/消费者，以及先前依赖Task有效交接。
研究SHA不是强制回退点。先记录实际HEAD、candidate范围、原始用户修改和隔离home；不reset/clean，不改真实数据。

执行：
1. 从真正生产入口追踪该问题，区分本提示词的源码事实和你本机实际复现；先保存一个正常正对照。
2. 对源码确认的错误建立正确契约断言，在未修代码上取得行为失败证据。安全反例只用合成临时文件和loopback服务。编译失败、脚本未运行、测试0匹配不算“旧代码红”。
3. 若初始反例不成立，继续核对前置与调用链；发现事实已由新提交修复，应给源码与运行证据，不凭主观意见改需求或删案例。真正证伪交全新Reviewer裁决。
4. 找根因并扫描同族全部路径，完成最小完整修复，接通生产调用，处理错误、权限、取消、存储和旧数据边界。
5. 第一层逐项自检：对本EX全部原叶断言、原Task Steps/交付物/A-ID和影响范围逐项填“期待/实际/证据/状态”，不得留一个“总体通过”代替。
6. 第二层对抗性自检：你自己作为攻击者构造权限伪造、竞态、错误注入、边界输入、重启等本Task相关反例；核对测试没有mock掉被测对象。至少做一个能重新暴露旧根因的隔离变异或等效旧版本红/新版本绿对照。
7. 跑关联回归，保留全部失败和重试记录。不要只保存最后一次绿，不删除/改松原断言，不改变原始到期范围，不虚报平台/LIVE。
8. 检查真正发给模型的数据、真实文件/数据库/进程结果；源码存在和mock回包不是行为证明。
9. 输出实现差异、所有自检结果、同族扫描、未验证、影响/STALE清单及候选摘要。状态只可READY_FOR_INDEPENDENT_REVIEW。

禁止commit/push/发布；Git由总控在独立PASS后处理。不能自签独立验收。完成后结束该子代理。

额外原义务工作单R06-EX-W12，完整规格 docs/rust-tauri/R06/repair-current/dispatches/R06-EX-W12_SPEC.json。原义务档案SCOPE-01/obligations.json是源码资料，不是验收；本单逐条原Step/A/原叶断言亲验。不得自行裁减原义务；真实范围冲突保留给全新Reviewer，不扩大后续UI或真实迁移。


## 具体补充：PREPARED_NOT_DISPATCHED

该具体派发仍只是准备，尚未创建本EX执行者。预定自己的普通独占clone /private/tmp/lingxi-r06-ex12-executor-01 必须由总控随后确证，不能在主工作区改产品。主HEAD当前bc2ce51fd005ea75b9949156974e15d2761eb734，F08仅限定自己的历史份额PASS，主dirty与所有其它未验收候选不继承。

第一次新W15独立Reviewer已实际完成FAIL并结束，完整报告 W15-REVIEWER-01/REPORT.md、FINDINGS.json、ORIGINAL_28_CLAUSE_DECISIONS.json、真实探针和末态/清单须精确送达并亲读。原28叶只列R06，服务/核心份额现在到期；原R07-T08管理通用条款不能将它们整体豁免，完整UI及宿主交互只按原R08/R09自己的份额后续，不提前做UI。判定不是总控主观豁免，而是新独立Reviewer逐原R00/R06/R07条款实际裁决。不能借本EX进入R07、独立本地模型完整管理、已撤回知识研究/强制子代理目录等其它范围。

自己的 Task 是补齐原28叶全部70当前服务成功/拒绝/真实副作用，及它们关联原R06-T01/T06全部Steps/交付/A01A02A11A12中的实际份额；未知/依赖未完成要保留具体OPEN，不把整个Task的其他人工作说成自己完成。不得只让10个缺路由返回200，不能把保存/Get回声当下一真实请求刷新，不以机制stub/代码存在关闭业务。

原实例实际发现：同session人格identity/AGENTS与memory.enabled保存/Get新而下一wire仍旧，new session才新；avatar响应缺ETag且条件请求200；十个原到期管理接口404/405。缺乏前置的运行不能作真实拒绝/错误旧红。先自己生产bin/真实授信隔离Agent世界跑合法正控再旧红，按全族真正业务根因最小修复。F05 cache回收候选、F01等依赖只可从总控提供的精确冻结材料另行语义整合与亲验，禁止偷读活动W04/W11/F15源和继承PASS。既有六类平台阻止Review不得在本EX换名重试；普通原到期业务修复不代替这些被阻断的独立验收。

助手创建/排序/主助手/切换/删除必须真实目录登记和作用域生效，实际广播及runtime状态受自己归属/原权限约束；最后助手拒绝、重名/错误id/order重复拒绝副作用不变，deleteSkills空保留/仍独占删除/候选变化skipped或实际删除失败明示。头像三格式/原15MB body limit、非法格式不覆盖旧图、跨id/角色权限、无图404和已有图ETag/304真实字节、默认路径回落/读写权限/广播不得只看HTTP。人格公开/非公开、模板/兼容别名、空串/非串类型、下一run请求与观测同源及权限红/新绿保留；public材料不能被错误升级为主prompt。

配置严格按原70断言区分助手yaml、全局preferences、provider-catalog和派生models、插件文件及真正focus/request两Agent：原允许部分成功的组合写入先后不能编造成原子全回滚，单文件原子写与组合语义分开；实际失败读回每个已发生副作用。请求A≠焦点B和A=B分别实跑，未知兼容字段、权限、未来版本保持、原secret掩码及内联API/embedding凭证真实转存/源yaml清空，失败不假rollback。测试只用假的合成凭证，不接触真实用户秘密/配置/home，报表和safe输出也去掉认证token/原credential原物，保留校验摘要/布尔和必要合成语义。服务更改不能误写人格/记忆或另一个Agent。

memory ticker/disabledSince/reenableAt、workspace技能策略当前即时重筛、每处Agent作用域/权限、generation/快照策略、缓存失效/次次请求须实际观察原业务流程，不靠睡眠字符串/手造状态证明。是否必须更新profile、列表缓存、running实例、同run只读快照由原条款和当前生产链判断，保持原边界不偷改。

第一层每70断言给期待/实际/状态/证据及完整同族全部路径；第二层真实并发、失败写入、重启/恢复、权限、空/非法输入、真实文件结果、错Agent、密钥遮盖、旧根因变异等本Task对抗自检，所有原失败及INVALID留存。原T01/T06和affected R05/R04/R03/R02相关原电池严格保留，正式Stage只由以后新的阶段Reviewer实际执行。最终稳定source/有效env/锁/runner/实际test与service/READY前后SHA、实argv/cwd/UTC/exit/实际PID/RSS/物理退出封口，测试600/8192 ON，build/cargo/rustc/xtask2700、原R05七项19800+有界开销，绝对cargo1.98.1 --locked -j8、OS HOME继承、独占LINGXI_HOME及外TMPDIR、ROOT/lib PRODUCT_DIR、CARGO_INCREMENTAL=0。null未知不倒填，bin库存不冒启动前绑定；完整source/input档案和config/home/DB/bin/clone/target LOCAL_ONLY，不进safe。

完整freeze/HANDOFF/安全逐文件SHA清单/实际END结束后总控另建全新Reviewer，FAIL→新Repair→新Reviewer，原角色不可复用。自己不commit/push/放行，R07不开始。


## 独占副本机械准备已经完成，仍无执行角色

2026-10-12T00:49:40.553712+00:00：own.git普通clone /private/tmp/lingxi-r06-ex12-executor-01，实际HEAD bc2ce51fd005ea75b9949156974e15d2761eb734，自己的Node依赖和上述全部原任务/最新控制/不可变规格/原义务，以及W15完整独立FAIL安全证据逐SHA送达。只是基线产品，没有主dirty或候选产品覆入。25份W01和10份W05必要候选单文件及原恢复摘要只放artifacts作只读源码线索，不能继承PASS或root语义merge；如整合，由你对全部冲突/同族做语义判断及生产实跑，并保原其他未满足范围OPEN。准备回执 artifacts/rust-tauri/R06/AR/EX12-PREP-01/own-clone-prepared-not-dispatched.json，无actor、测试NOT_RUN、Stage不可启动。
