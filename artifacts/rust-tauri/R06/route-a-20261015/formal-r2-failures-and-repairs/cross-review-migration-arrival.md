# 两测试文件独立交叉复核

结论：STATIC_CROSS_REVIEW_PASS。没有发现通过串行化、删除原断言或放宽时间/资源边界求绿。补丁与当前Main两文件精确相符；此次只读，不构建、不执行测试、不改Main或验收资料。作者已有动态结果经日志/SHA核对属其证据，不冒称本审阅者重新执行。

## 迁移检查

- 生产MIGRATIONS实际固定1..14，tip=14 / session_proactive_delegation_at_creation；V14真实SQL增加INTEGER NOT NULL DEFAULT 0 CHECK(IN(0,1))。新检查固定14及其独立名称，不是用supported_version自身推导期待值。
- 原v9名称r06_t05_session_files_resources保留；原顶端v13身份r06_session_workspace_identity移为显式历史版本断言，没有把v13改名为v14。旧生产SQL与独立R02登记均未改。
- 本审阅直接提取生产14段SQL，在内存计算SHA256，逐项等于独立登记的14个fingerprint/name/version；连续版本精确为1..14。
- 原V1..V7手工构库/SQL/收据/user_version=7、旧session输入、title=t/lifecycle=active、四张v9表存在要求不变。升级精确链从[8..13]补上真实14，current/supported/user_version均固定14。
- 加强旧收据五字段（版本/名/SQL指纹/时间/写入者）原样比较，并逐项对独立登记；重新apply后applied为空、schema/user_version与全部收据不变；旧行agent/owner/title/created/lifecycle及新默认0完整读回。
- 新SQLite约束场景实际写0/1并读回；2/-1/NULL必须按CHECK/NOTNULL精确extended error拒绝，且同一失败UPDATE中的title与delegation保持不变。没有仅检SQL字符串就冒称约束生效。
- 原后十项CRUD/去重/alias/ref/历史保留等区域与HEAD全文逐字相同。作者日志13/13 PASS；本次未重跑。

## 通知乱序修复

- 将新原始接收器包成ArrivalInbox；按(session,pop)先查pending，未命中才recv；非目标消息存入VecDeque，原run值不改，找到的那一条remove后恰消费一次。等待取消前已存的非目标事件仍在pending；只读有限测试通知，不修改生产steering容量或队列。
- 本审阅独立反替换恰好8处arrival_channel工厂后，原八个场景从第一测试到EOF逐字等于HEAD。所有assert、并发提交、两会话门闩、双门释放、下一Run隔离、取消、容量及结果顺序均保留。
- C02仍先提交A/B两条后台Run，先等A1再B1；先同时释放两边门，再等A2/B2，未将生产运行串行化。修的是观测端丢掉先到B通知，不是改变生产调度或延长等待。
- 600秒TEST_TIME_LIMIT_SEC默认guard源文件与HEAD逐字相同。原八场景等待/断言未改；新case的1秒/20毫秒仅用于确定性反例和重复消费检测，不替代或放宽原guard。

## 真实负控证据核验

新case在开始等待之前按固定顺序发送Beta1、Alpha2、Alpha1，随后必须按Alpha1、Beta1、Alpha2精确返回，再确认Alpha1已消费不能重复返回，取消等待后新Gamma仍可返回。发送顺序先固定，不依赖操作系统碰巧安排。

外置arrival-buffered-control.rs的helper+新case整体与当前Main对应块逐字相同；arrival-lossy-negative.rs只把helper恢复为旧“非目标直接丢弃”算法，新case正文逐字相同。旧算法等Alpha1时必已消耗Beta1/Alpha2，随后等Beta1时sender仍打开且没有该消息，必触发1秒超时。因此是稳定失败机理，不是概率性竞态猜测。

现存日志：旧算法exit101 / 0PASS1FAIL，明确Elapsed；缓冲对照exit0 / 1PASS（0.02秒）；原八+新增共9PASS（0.73秒）。作者JSON所列全部证据文件SHA与当前磁盘逐一匹配。原完整正式C02的600秒SIGABRT失败证据继续保留，未改为超时成功。

## 候选与范围

补丁SHA256：e55a0d9601b26e4e872f6a06d76da9db85f031b474e7dba82e0a60b6b27dc740。

当前session_file_store.rs SHA256：270a1a182b65c3aae5e94948921ffdf778815a5176e50521dc6215676282b62e。

当前background_steering.rs SHA256：7cf1c40e0bde65fbbb0dd670e4d15246750b4ea8dcbd92c8995d54eb8dcbb050。

confirmation.json记录独立内存补丁重放、原正文比较、14段SQL与登记SHA、作者日志/证据SHA及明确未重跑边界。此结论只覆盖这两份测试修复的真实性和标准保留，不代替后续整合后的正式全量窗口或跨平台运行验收。
