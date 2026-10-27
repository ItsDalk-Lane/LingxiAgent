# W03/F03 新执行实例02：传输中断后的完整自检与交接

你是全新空历史一次性执行实例02。实例01在完成全族修复、定向自检与变异后发生传输断连；不是独立Reviewer FAIL，也不是Daybreak拒绝。本轮不能复用实例01，不能继承任何 PASS，更不是修补平台受阻的其它审查。你只负责完成 W03 当前未接受候选的实际完整两层自检、必要真实修复和冻结，之后结束。第一轮独立Reviewer尚未创建。

唯一可写产品工作区 `/Users/study_superior/.codex/r06-isolated-checkouts/w03-execution-source-02`，正常clone/own `.git`，避免原正式legacy准备环境缺口。真实 HEAD应为 `bc2ce51fd005ea75b9949156974e15d2761eb734`；36个继承但未接受的产品/测试文件，锁 `d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4`。主代理已逐文件核对并机械复制 `artifacts/rust-tauri/R06/AR/W03-PREP-01/interrupted-candidate01/source-map.json` 的36个文件。原中断工作区 `/Users/study_superior/.codex/worktrees/r06-w03-execution/LingxiAgent` 的产品/证据/target全部只读保留，不能改它。核对source SHA、diff、owner并保留全部旧证据。不能改主仓、W01/W05/W09/其它候选、锁/门禁来抹红、用户文件或Git。

完整读取实际原 `R06-AR-W03_EXECUTOR_TEMPLATE.md` 派发、ENTRY完整1219行规格、原九共同资料、原R06-T03全Steps/交付/A05/A06及R00完整到期叶、前置R03/R04/R05原运行/权限/取消/工具/usage合同和所有旧同族记录。被忽略原 Taskbook 用 `rg --no-ignore`，不能冒称缺失。

实例01的全部记录位于原中断工作区 `artifacts/rust-tauri/R06/AR/W03-E01`，必须只读保留不可改写，不盲复制整树/合成home/缓存。workspace-regression-05 有完整真实exit101/1308.9秒/14失败7targets；其中真实父夹具、durable background-start、原DB装饰器、strict gate/身份、物理blocking退出保护已经随后改动，不能继承那个workspace结果为当前。最新 focused-final-11 原日志九suite用例13/10/15/9/21/21/7/12/13报告绿、clippy02绿、私有证书负控与隔离drop-retained变异native write/edit两红及ResourceIO FIFO一红，但你须读实际record与SHA，不能只信文字。

workspace-regression-06只有run-start和部分stdout/stderr，无最终run.json和真实exit；旧tool session8216已不存在，root实际ps未发现相关运行。必须标 INCOMPLETE，不补填成功/原退出码、不删除临时home或原日志。准备新的 `W03-E02` 证据目录和 `rust/target-w03-e02`；不能改 W03-E01 runner或回填它记录，不能把旧target缓存产物当亲验bin。可读取旧证据与源码，仅自己的新输出可以排除自身绑定；有效Rust、非Rust、执行脚本/外部manifest等全绑定。

## 关键最终源码事实与必须亲验范围

- 真实 SessionSupervisor/SessionTree/SessionAdmin/SessionStore/Adapter RunDb 同canonicalDB路径共享Arc准入；不同home独立；权限先于Busy，持lease后再检查真正owner/permission/lifecycle/head-revision/folders，不捏造全局SQLite跨进程lease。正式bin的原InstanceGuard承担生产同home排他；public embedded合同要求调用宿主持Guard，不冒充未支持跨进程能力。
- opjournal追加v10保原1—9原字节；request/state/关键event真正transaction；crash未知file effects显式Unknown。删除源后的同owner旧journal收据只读可查、外人拒绝；有journal的旧SID退休不复用，普通无journal旧SID删除后按原合同可以重建。默认seed不复活退休ID。
- 核心public Run entry不能绕过实际Storage对应gate；borrowed/publicAPI原形态保持，精确绑定原主体/Session/Agent/真实同Storage、parentRun/CancelScope/真实授权谱系。原LocalUser广域和合法不同Agent child保持，Device/Web限本人。所有真实DB Storage装饰器完整转发gate/事实，纯mock None不得冒充生产。
- foreground caller断开只表达Cancel，物理task真正退出才释放lease；background accepted必须真durable RunStart/单driver，waiting断开不错误取消。TaskSupervisor实际future携带私有issued事实，Run注册绑定真实root Arc祖先；假身份/错gate/错scope不能借别人的permit。
- 原native `filetools.rs` write/edit与`resourceio.rs::spawn_blocking_io`实际blocking闭包必须持强permit至真正退出，不能只保外层异步wrapper。真实生产模型→native写hook屏障和可信宿主工具转发真实ResourceIO→合成FIFO的反例已红；当前保护需再亲跑Busy/alpha409直到真正写退，beta200、写退后alpha可运行。明确ResourceIO新增可信宿主Consumer不是宣称旧native已有该消费者。
- task-local存在而错actor/session/backend不能降为手工HTTP None旁路；同真实DB两个StorageArc正控、不同home错gate、错身份/scope、空SID正常无issued手工HTTP、运行忙时原合法手工编辑均覆盖。`sessionfiles.rs` crate-private gate getter依真实DB，不靠home字符串猜。
- 所有真实父→Runtime旧夹具保原调用/receipt/tier/ordered语义。r04递归拒绝case精确父+被测合法child两Run、全局1thread、0grandchild、被阻工具0dispatch；不能filter掉未知child来“零递归”。usage4calls/context原两prompt断言保留，新增真实父调用单独精确账本计数。原combined snapshot负例保留，另五个独立单字段负例（与原combined共六类）不得同时改多个混淆根因。

同源所有生产消费者完整扫一遍，逐个C01—C04和原Step/A/叶份额列真实证据/未验证。发现当前必要新缺口在自己本轮修、全族再检，精确报告必要owned扩展；不无限增加范围、不改旧需求。F01历史组合、F05缓存、F06配置、F02文件恢复平台受阻等仍OPEN，不能申报/R07关闭。共享源码修改使相应其它证据STALE，后组合须全新integration/Review。

## 新鲜两层检查与守卫

至少亲跑当前全部21个F03 cases和八原受影响suite/关键public证书、真实native write/edit与ResourceIO FIFO、正式生产bin双InstanceGuard/不同home、全部四C、异常transaction/源删除收据/退休SID/正确权限/模型wire和实际SQL/文件。实际隔离变异或准确旧红→当前绿核验不能仅继承旧作者文字；可以复核已保完整变异源与实际正确失败点，再在隔离副本新跑并保存当前绿对照，原候选不污染。

实际 fmt/allworkspace-alltargets clippy -Dwarnings/contracts/boundaries/registry、完整workspace --no-fail-fast --locked -j8、所有适用正式受影响链。每testbinary600秒/8192MiB ON；whole workspace外层2700秒，正式R05原映射预算19800+有界准备，不统一600误杀整套。Cargo绝对 `/Users/study_superior/.cargo/bin/cargo`1.98.1/当前lock；不任意改工具链。不隐藏孤儿/脱离宿主监督，真实TERM/KILL/reap/清理。

每次argv/cwd/time/exit/count/platform/mock/实际home/来源READY/全有效输入before-after/锁/binbefore-after/guard/原wire+SQL+文件/cleanup。未测RSS=null。Cargo重新发布不同bin窗口真实保留，不反填。原tests自造CLI temp home必须记录实际路径，不假冒外层env home。

完整whole非零、0匹配、guard触发、宿主终止、缺最终结果只能如实FAIL/INCOMPLETE，旧基线quota/F08投影事件严格向量或registry9vs7不自动豁免。不得删旧ordered/approval/cancel断言或改registrations“过门”。完整上游/Stage未亲跑明确NOT_RUN。

全部synthetic homes/config/privatekeys/credentials/target/bin/full archives本地LOCAL_ONLY_NEVER_GIT；不要清理旧运行合成home或源证据来收口。禁止真实数据、paid、真实外发、Git、主仓/他人候选写；当前自己的必要cleanup只能精确证明owned且进程已退出。

冻结最终REPORT/results/完整SOURCEFAMILY+scope(原Steps/A/到期叶原文保留)/候选ownedSHA和实际锁/bin/全部失败重试/两层自检/remaining与STALE/safe manifest/HANDOFF；状态只能 READY_FOR_INDEPENDENT_REVIEW。随后结束。总控会新建第一位全新Reviewer实际验收，不能由你自签。平台如果实际中止则保存原文、不得改写重试绕过，仅停被阻动作。
