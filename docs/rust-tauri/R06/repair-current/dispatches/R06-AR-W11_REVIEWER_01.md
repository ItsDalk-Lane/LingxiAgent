仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是R06-AR-W11/R06-AR-F11第一次的全新独立对抗性Reviewer。
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


## 本次准备：尚无Reviewer，等待当前Executor实际冻结结束

你将是总控随后新空历史创建的一次性首个W11 Reviewer，只在自己的普通own.git clone /private/tmp/lingxi-r06-w11-reviewer-01运行，不参与现活动执行者或其它Reviewer。当前产品纯bc已提交基线、own Node，只复制必要只读原输入/根准备资料；没有活动W11或W04源、13个未接受依赖的产品overlay，不继承局部绿。角色仅在W11完整final source/input/实现差异/全路径表/全部自检与对抗/安全逐文件清单/HANDOFF/END真实到齐后创建。

按不可变完整文件F11-C01到C04实际逐项第三层验收，关联原R06-T07全4Steps/交付物/A13A14、R05-T05/T07原当前份额、所有原到期叶/90上游份额完整保留。不得只证明KnowledgeEmbedder携带可选context。所有依赖/完整当前缺口未验不能变成整体Task PASS；原独立F09Review仍平台BLOCKED，不可用本F11改名替六类W01/W02/W03/W06/W09/W12受阻Review，不签它们独立通过。对合法新F11运行/原回归仅给准确本scope结果，不丢原义务、不恢复撤回独立知识研究/完整localmodel management、不开始R07。

旧线索而非你的结论：执行者明确13个精确恢复W09 union F10源属UNACCEPTED_DEPENDENCY；新embedding_operation.rs必须进入最终候选与有效源/runner binding。原KnowledgeEmbedder固定None导致Run/Session/Attempt/Tool缺归属及父deadline丢失；后台job应稳定服务铸造身份和显式绝对预算；执行者当前选择300000ms只是待核对的实现线索，须按原配置及真实开始时间校验预算，不能每次reload/恢复延长或将该数字强加成新原需求。真实model permit=1不能跨Tool等待期间被父Call持有造死锁，也不能提前归还仍物理活动请求/IO资源。

必须真实走两条链：主模型ToolGateway hybrid检索embedding，读取实际Session/Run/Attempt/Tool/parentcause/账本/wire；授权独立摄入job，按真实job/request/attempt因果与预算恢复、重启/hot reload/新cause并核对不伪装用户Run。模型参数自报身份不可信；凭证/permit等待、网络请求/流、每64批边界写回/取消/超时/已终态迟到使用真实屏障，不mock待验核心。只外部loopback provider边界固定响应，ModelGateway/usage/TaskSupervisor/job/向量native/真实SQLite与文件不mock。无匹配/编译/旧错误夹具不算旧红。

原Reviewer受阻过程中已记录paid result/usage插入失败重复HTTP线索仅供当前F11原因核对；本scope须亲测当前真实paid结果先强保留，usage失败/SQLite占用/native持久化FIFO blocked/物理spawn_blocking waiter被drop或cancel，原TaskRef/claim/OwnedRequestGuard持续至物理实际exit。单次绝对deadline和取消范围覆盖journal/usage/vector写回，不因Future结束就退许可或重启再付，取消后无后续批、不ready、无跨任务污染。已付token/response/vector保存与usage不可丢、immutable/readback；8个预留slot全占时第9在HTTP前明确拒绝，不淘汰未持久化已付项，slot随真实安全完成归还。FD.metadata非普通/FIFO应在truncate前校验，真正普通文件正例与错误路径无误写。所有迟到/write完成条件亲证。

同模型/跨模型/SIGKILL/hot-reload需按最终明确代码重跑：真实已付64+未知尾批不得自动/同cause手动再次付，unknown transport/费用NULL不记0，删除旧source/job仍保不可撤销旧账，真实新cause可以新请求。旧64与新65/同维不同model alpha→beta首次真实reload后B全部正确模型ready，已付尾批按同真job/request复用，不能偷用A向量或新身份掩重复请求。以实际请求计数+每条usage/task cause/request/attempt/vector model/物理文件核对，不凭总数相等就PASS。

执行者新增真实HTTP400合法手动retry问题必须独立检验：真实dispatcher typed非2xx durable rejected_response仍已发/transport1/fees NULL，不能当‘确定未发0’；原明确失败仅真手动原入口按jobcause/下一attempt一次授权消费，重复并发应只一次实际请求。重启自动不续用、已授权新attempt发出未响应再SIGKILL后unknown不复用旧授权、不自动或手动再次HTTP，真新cause正例；usage INSERT失败后Failed/Cancelled旧终态保留，不能恢复强改Unknown来堵合法手动重试。非2xx、timeout/连接故障/无真实response不是同类，不靠文案猜HTTP或确认零费用。不能为通过改松未修改原c02_failed_checkpoint_resumes_only_gaps；scope未知失败仍OPEN。

额外journal OS锁假设已被执行者证伪或未证：shipping同HOME原instance.lock已经拒第二writer、library bootstrap原合同由调用者持singlewriterlease，原bad预期probe不算可达产品旧红。执行者已撤回额外journal锁；旧实验/old green/window/source-hypothesis-correction原件保留。不要重新要求未证明必要的额外锁，不继承已撤回版本的11绿/九变异/全回归。只最终实际最小source的实验能算当前结果。

原完整workspace首稳定最小源101/1085s/7失败targets，后续typed400候选运行也须如实完整保存。严格事件新增tool_result_model_projection、parked quota准备屏障、sandbox、zero-call parentcancel、原noncooperating expected unconfirmed/actualCompleted不删断言、不全部归合法冲突。noncooperating新独立仅时序观测保持原断言绿，cancelled54ms时物理read未exit、305ms真exit；旧全量Completed原因仍OPEN，不被这次绿覆盖。Reviewer须亲跑原失败和定位真实时序/调用范围，一次报全同族，若需原消费者配套列STALE并保原语义，不代替其它角色语义修改。

独立全C/原Step/A/叶逐项期待/实际/原始wire/SQL/文件/终态/物理退出/候选绑定；至少一个作者未直接展示的真实同族反例。旧根因准确隔离mutant实际101/正确命中后精确还原正常0，不因错误bin/future schema/失效正控让mutants假绿。原九实际变异是线索，重跑所需负例/正控，发现全量production gaps就整体FAIL，真实环境缺失BLOCKED/NOT_RUN不冒PASS。

每PID+birth/lstart+PPID含不同PGID真实守卫600秒/8192MiB ON，cargo/rustc/build/xtask2700，原R05七项19800+有界准备保持原命令/断言。绝对cargo1.98.1 --locked -j8、OS HOME继承、独占LINGXI_HOME、ROOT/lib PRODUCT_DIR、外TMPDIR、CARGO_INCREMENTAL=0。候选全有效Rust/nonRust/contracts/原输入/doc/locks/runner/env/实际程序执行前后SHA/READY/argv/UTC/RSS/exit/真实wait/cleanup准确，未知null/first_seen/库存不冒启动前记录。根准备未证明你新候选执行，你自己建立外部准确必要集合不信自报子集。完整source/home/config/DB/bin/targets LOCAL_ONLY，safe必要自有探针/无秘密记录和最后逐文件manifest/END。

本Reviewer不改产品/原测试/权限，不commit/push/付费/用户迁移/真实对外，不提前Stage。所有必需真实满足才独立VERDICT PASS，否则一次报告全部FAIL/finding/根因/同族全部路径/重跑范围，结束一次性角色。FAIL后新Repair→新Reviewer，两完整同族FAIL才新RootCause，执行自检失败不冒充正式FAIL。证据 artifacts/rust-tauri/R06/AR/W11-REVIEWER-01。只有全Task有效PASS、完整原义务和全新StageReviewer实际R06及R05→R04→R03→R02门禁/放行十条件满足才R06接受，R07不开始。
