仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是R06-AR-W04/R06-AR-F04第一次的全新独立对抗性Reviewer。
你没有参与该Task的任何执行、修复或前一轮审查；不继承实现者聊天上下文。

读取原需求和完整F-ID/C-ID，再独立读真实diff、生产入口、调用者和测试。实现者报告仅作线索，不作结论。

必须执行第三层验收：
A. 逐项检查每个原Task Step/交付物/A-ID和本F-ID四个C-ID。每一项都有真实执行/源码推断/未验证的明确区分。
B. 从真正业务入口亲自复跑全部当前C-ID；至少额外提出一个实现者自检没有直接展示的同族反例。隔离外部响应可以mock，当前被验的编译器、网关、状态机、存储和消息组装不能mock。
C. 复核“旧红→新绿”的旧红确实为契约错误，而非不编译/少依赖/空测试；隔离变异应能被正确测试抓住。
D. 逐项核对实际请求、实际数据/文件、收据、权限、版本及资源清理；不要只看日志里写PASS。
E. 检查是否将当前到期功能改名share/deferred、将调用未接线写成已完成、将机制echo/握手错绑完整业务、将未测指标写0。
F. 检查对其它F-ID与R02—R05的影响，列出必须重跑的范围。没有新增风险的内容不无限扩大成新产品任务。
G. 候选绑定必须与你亲验的文件内容一致。新的产品/测试/配置变更使受影响PASS失效。

每个发现写：ID、违反的原条款、源码位置、复现条件、实际/期待、根因、全部同族路径、修复要求、必须重跑的C-ID。
一次报告全部找到的阻断与同族问题，不只报第一个。

只有当前所有必需项实际满足且无未关闭问题，输出VERDICT: PASS；否则FAIL。真正外部环境缺失则BLOCKED并精确说明，不将注册为延期等同通过。
你不得修改产品或测试实现；仅可在独立证据位置保存测试输出、探针和审查报告。完成后结束子代理。


## 准备状态：尚未创建Reviewer，等待全新执行者最终freeze/END

你须由总控新空历史创建，只在自己的普通own.git新clone /private/tmp/lingxi-r06-w04-reviewer-01 工作，不参与实现、修复或先前审阅，不接手现活动执行者。当前产品仍纯已提交bc2ce51基线，未复制W04活动V3或其它未接受产品。待执行者真实结束、最后候选/全部source/input/自检/对抗窗口/全路径表/安全清单/END精确到齐才开始。准备副本与当前root读回不能替任何行为PASS。

完整F04四C及原T03/T05全部Steps/交付/A05A06/A09A10、当前到期460项原联合义务，EX05原5叶10断言与EX10原19叶53断言完整保留；从原scope逐当前份额裁决，原A/叶成功和拒绝/副作用要分别真实发生。F04不能仅12/15个本地用例，完整Task不足就FAIL，注册、后续R07或诚实OPEN不算满足。原家庭工具/五协议/handoff/URL/mount/current history源/物理资源等当前核心服务依赖不得整体后移；只具体UI/宿主后续按原条款保留。尚无完整原人类10/08启动文本的SPEC_GAP不虚构。

必须亲读最后执行者V1/V2/V3所有失败/无效前置和配套路径表，仅作线索。总控readback：V2原12绿只属局部；原完整workspace9FAIL/7targets；原R05七项20400预算实际完整2913.872795秒结束exit1，native68479文件前后等、7项exit101/0/0/0/0/0/1；递归R04/R03/R02严格事件/迁移登记仍FAIL。V3后这些窗口source旧，不得改写为新source PASS。原RR1实跑通过的范围也不可扩成整个Task。此前first_seen/文件库存/未采样出生与reap未知均保持限制，不将root当前PS消失等同你的实际wait。

从真正HTTP upload/import/history/archive/fork/delete 与真实AvailableToolGateway stage_file/LocalFile/ResourceIO/模型wire亲测，不能mock当前网关、树/文件存储、真实模型请求组装或物理IO。外部loopback模型传输可以固定替身，实际pending/final/usage/ref/file/state/终态必须自己SQL/实际磁盘/记录核对。至少一个作者没有直接展示的同族反例；不能只改报告，不改产品/原断言。

已核对旧红线索需独立复验并检查全部同族：
1. 真实Run/文件/用户消息/最终结果/tool refs/aliases相互原子；upload/stage/import等待、取消/保存拒绝要无非法部分结果，无模型调用不能写0冒假未测。fork完整复制实际合法ref/id隔离、rewind达不到节点拒绝；完整原分页/ETag/无消息四终态history锚不伪造用户消息，不把审计材料加入模型历史。
2. 新cache-root现存外部文件误授Managed后Session delete误删；跨Session真正stage+next wire+Final committed ref引用同物后原Session delete破坏有效ref；合法符号路径/大小写canonical与目录后代共享引用真实可读性；剩余有效引用消失后真正回收。不要发明所有Unix inode永远保留：独立hardlink的合法剩余实际路径8B仍可读时，unlink已无ref原名可以符合合同。作者旧‘原名也须留’强期待INVALID必须保留，不算原合同旧红。不能为保护而将所有原Managed都降External/永不回收。
3. 物理LocalFile/read/stat/list/search/materialize实际执行仍活时不能因逻辑Future cancel就归还资源并清扫；取消/权限scope/activeRun protection/冷72h真实activity与archive/delete不要只metadata busy。旧read future cancelled而物理blocked误清扫真实红，应按原真正close/wait和物理退出后清理正控复验。亲测device/outside/mount/url/stream/TOCTOU原真实许可与拒绝份额，不能恒拒绝代替成功。
4. 原Owned blob上传201/8B→仅停机合成activity置旧→真实startup清扫expired/gone→同内容合法再上传旧500 UNIQUE(owner/source_key)；最终应保持原id/source/aliases真实available/字节恢复、SQL错误/取消补偿不损原物；可信missing/legacy sidecar的恢复与恶意伪造不能混淆。只凭cache路径/sidecar自称owned不能给用户外物管理删除资格。保持真正create_new owned或可信已登记来源，所有同族已有/expired/missing/并发/重启/最后引用回收都核对。

v10新run_history_anchors是W04自己需配套真实migration/终态锚的源线索，不是已经接受的新基线。亲跑v9→v10/new/idempotence/restart/旧程序future schema拒启；严格保留v1–v7原hash，v8/v9/v10按最终真实SQL追加，原登记缺项FAIL归实际，不强绿、不drop strict registry。新工具stage_file目录改变F07实际discover/describe/call历史和受影响消费者须列STALE，不能为了固定catalog数量禁用该必需工具。

独立候选全有效Rust/nonRust/contracts/完整原需求/doc/input/lock/runner/实际env/READY与实际程序执行前后SHA双向绑定，不能接受报告自报子集或数量。每实际PID+birth/lstart/PPID含不同PGID600秒/8192MiB守卫ON；cargo/rustc/build/xtask2700，原R05七项19800+有界准备，原命令保留。绝对cargo1.98.1 --locked -j8，OS HOME原值、独占LINGXI_HOME、ROOT/lib PRODUCT_DIR、外TMPDIR、CARGO_INCREMENTAL=0；真实wait/exit/RSS/cleanup/短寿命未知null如实记录，库存不冒启动前binding。原所有FAIL/INVALID/PREP_FAILURE保留。

第三层逐项全部四C/原Task/Steps/A/原叶/旧红与新反例实际裁决，一次列全finding、根因、全部同族路径和回归范围。缺失既知原回归/业务依赖必须如实FAIL/NOT_RUN/BLOCKED，不能仅因为作者披露就PASS。全部原必需满足才VERDICT PASS，否则完整FAIL；本单第一次正式FAIL不把旧自检FAIL累加。结束后新Repair→新Reviewer，第二次同族正式FAIL才新RootCause。你仅独立Review证据，不改产品/测试、commit/push/真实数据/付费/外发，不重试六类平台受阻Review，不提前Stage、不开始R07。

证据目录 artifacts/rust-tauri/R06/AR/W04-REVIEWER-01；完整源/home/config/DB/bin/targets只LOCAL_ONLY，safe必要自有探针/无秘密记录/最终逐文件清单和实际END。阶段全新Reviewer只有所有Task独立PASS后另创建；你不能签阶段放行。
