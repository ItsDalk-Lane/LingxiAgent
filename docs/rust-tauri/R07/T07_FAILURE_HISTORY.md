# T07 开发失败记录

这是开发过程的证据索引，不是最终验收报告。原失败、被中断和跳过的记录均保留；未修改源码或最终结果表。

本快照核对 60 份开发回执，原退出分布见JSON；全部日志摘要一致，带文件数组的前后源码摘要均重算一致。另单列 t07a 的8份、t07b的11份核心门禁回执，两候选均未获最终验收；t07b补充headless门禁单列。

前后源码相同 34 份，运行期间改变 13 份，未记录前后绑定 13 份。后两类不能当固定候选通过。带文件数组的摘要重算仅证明回执内部一致，本次未核验每份旧源码字节归档；前端单有摘要的旧回执同样保留此绑定限制。

完整逐份核对、原回执链接、失败行及同名测试后续轨迹见 [T07_FAILURE_HISTORY.json](T07_FAILURE_HISTORY.json)。下表仅按原因汇总，不覆盖原始失败。

| 原因 | 原失败回执 | 后续路径与边界 |
|---|---|---|
| rust-string-inference | `dev-resume-delta`, `dev-resume-integrated1`, `dev-resume-integrated2` | 修正实际字符串参数转换；后续集成已能编译并执行。 后续：`dev-resume-integrated3`, `dev-resume-build1`, `dev-resume-integrated8`。 |
| rust-lints | `dev-resume-clippy1`, `dev-resume-clippy2` | 按lint提示修具体使用点；未降低 -D warnings。 后续：`dev-resume-clippy3`, `dev-resume-clippy4`。 |
| test-root-lifetime | `dev-resume-integrated3`, `dev-resume-integrated4` | 测试根目录清理限定当前独有目录并检查相邻文件存活。 后续：`dev-resume-auth-isolated`, `dev-resume-integrated6`, `dev-resume-integrated8`。 |
| loop-wakeup | `dev-resume-integrated3` | 后续同名测试观察到通过；取消/唤醒接续修复见T07_RESUME_NOTES，不从本条断言反推唯一根因。 后续：`dev-resume-integrated4`, `dev-resume-integrated6`。 |
| workflow-scope-receipt-cancellation | `dev-resume-integrated3`, `dev-resume-integrated4`, `dev-resume-integrated5` | 范围与工具名单现场复核、外层归属/子节点取消自等待修复，以及具体fixture/计数调整；以同名测试后续结果逐项追踪。 后续：`dev-resume-integrated6`, `dev-resume-integrated8`。 |
| workflow-promise-failure | `dev-resume-integrated5` | 根Promise失败时终止其worker，不等待未完成兄弟请求回执；当前测试仍保留发送者证明未靠丢弃sender结束。 后续：`dev-resume-integrated6`, `dev-resume-integrated8`。 |
| goal-preference-projection | `dev-resume-integrated5` | 后续同名测试已通过；日志只证明GET投影与预期不符，不自行归因于单一存储或fixture错误。 后续：`dev-resume-integrated6`。 |
| summary-fixture-clock | `dev-resume-integrated6` | 使用同源ManualClock并显式advance；不通过放宽正式时间规则修测试。 后续：`dev-resume-integrated8`。 |
| rust-digest-format | `dev-resume-integrated7` | 改用现有明确十六进制转换；integrated8已进入执行。 后续：`dev-resume-integrated8`。 |
| workflow-latest-open | `dev-resume-integrated8` | integrated9及12同名三项已在后续开发版本通过；integrated8失败保持原样，最终新候选待验。 |
| frontend-subscription-timing | `resume-ui-r1` | 直接返回旧lingxiFetch Promise，保留原释放断言。 后续：`resume-ui-r2`, `resume-ui-r7`, `resume-protocol-ui-r2`。 |
| frontend-mock-typing | `resume-ui-types-r1` | 替身声明真实fetch参数类型，未减弱生产类型。 后续：`resume-ui-types-r2`, `resume-ui-types-r5`, `resume-protocol-types-r1`。 |
| real-ui-data-gate-fixture | `resume-real-ui-r1` | 从空隔离home正常启动后，经正式接口创建供设置页浏览的助手；未绕过数据闸。 后续：`resume-real-ui-r5`。 |
| real-ui-main-window-create-alias | `resume-real-ui-r2` | 本T07 fixture初始化使用正式路径；此结果不证明主窗口创建别名已修，保留T08/T12核对边界。 后续：`resume-real-ui-r5`。 |
| real-ui-model-free-browsing | `resume-real-ui-r3`, `resume-real-ui-r4` | T07设置只浏览已创建助手，不创建无关聊天；移除switch准备步骤，实际WorkTab点击仍走正式保存接口/磁盘。 后续：`resume-real-ui-r5`。 |
| frontend-command-filter | `resume-protocol-ui-r1` | 精确恢复--maxWorkers=2参数，随后20套494项通过；中断不记通过。 后续：`resume-protocol-ui-r2`。 |

`dev-resume-integrated8` 的三项失败完整保留：损坏日志恢复提交遇到 Busy、fanout计数不符、嵌套序号恢复等待超时。新增 `dev-resume-integrated9` 同源码开发检查退出 0（598 项 lib、25 项 Git、17 项 workflow），三项原失败均有同名通过行；这不会改写 integrated8 的失败，完整最终候选仍待验收。

`resume-real-ui-skip-r1` 虽退出 0，其中真实服务用例缺少指定二进制而跳过，仍记未执行。后续 `resume-real-ui-r5` 才实际运行真实服务并通过。它不能代替后续修改后的最终版本。

没有提交、推送，也没有将失败改写为通过。

新增独立源码核验：t07a 共 525 项，归档与当前文件的字节、长度、模式全部一致，含模式的新摘要与归档 SHA256 重算相符。另已向主任务报告循环启动等待期间停止/暂停仍可能提交 kickoff 的竞态；未修改源码。


## 后续失败及作废候选

T07 仍为 **IN_PROGRESS**。

- integrated10：缺少 ToolCallId 导入，编译退出 101，未执行测试。
- integrated11：错误 PATH 选中 cargo 1.93.0，主动中断 130；专门 interruption.json 保留，没有测试结果，不能当失败测试或通过测试。
- t07a：接口生成门禁退出 1，B1 执行边界门禁退出 1；独立审阅另发现启动等待期间停止/暂停的竞态，候选作废。fmt、clippy、protocol-tests、kernel、adapter、types 的原退出 0 仍保留，只属于当次版本。
- t07a 服务检查被候选绑定前置核对拒绝，产品测试命令没有运行，实际产品退出值为空；批次表中的 service=1 是包装器结果。build 和真实 UI 未运行。
- 边界检查先发现 beautify_generation，再发现 media_generate，两个原始失败都保留。dev-resume-boundaries-audit2 独立只读检查退出 0，但使用 --skip-deps，不能代替最终完整边界门禁。
- integrated12 已完整结束，同源码603项lib、25项Git、6项目标循环、10项media_admin、15项office、11项t05media及17项workflow通过；含真实DB锁检查。该回执之后新增的重启、冷快照、组合根Weak检查仍未运行。


## integrated13 的真实失败及下一轮待验

同源码运行退出101：lib 607项通过，goal_loop 6项通过、2项失败。两个新增真实服务检查都在“首轮结束后兜底alarm未保存”的前置条件超时，尚未走到Weak释放或重新bootstrap的断言，不能误报成引用释放/重启已验证。Cargo采用fail-fast，workflow整组未运行。

随后独立审阅确认三处待修：真实空闲检查早于最后会话许可释放；父子Run共用session导致循环轮记账归属不清；合作取消的提前返回遗漏统一终态观察。主任务正在修改通用permit释放通知、真实run_id及冻结代次登记、唯一新终态结算出口。对应真实父子轮先后结束及正式取消后新轮的检查已补，尚未编译实跑，T07仍进行中。


## integrated14 后续实际通过及新边界

同源码退出0：lib615项、goal_loop9项、workflow19项通过；原日志SHA256为 `ec9054cbad92775c5d081648fafc8b53e8347b74a340e94380b6104739589757`。真实同home循环重启、组合根Weak释放、工作流HTTP冷快照/权限/重启、父子Run各自记账和正式取消均实际运行；13原失败不变，后续通过另列。

独立审阅随后发现初始登记前窗口尚需补证：旧Loop投递已spawn而record_run_started仍在真实数据库等待时，新循环可能已经取代旧代次。身份必须沿原投递授权传入，不得在晚登记时从当前session重新认领。生产修复和真实卡点回归仍进行中，14不是最终候选通过。


## integrated15 的真实交错结果

同源码退出0：lib619项、goal_loop11项、workflow19项，共649项通过。日志SHA256为 `90534721c042cd0596a4b3308620c20890f5fd58fb6e6db441c5b78b154f1ec5`，前后源码摘要均为 `291448331fc456620981f1048ec6b5ea6375baf2feec76e309fa2001b221d0a8`，原回执数组重算相符。两个新增真实数据库交错均有同名ok：已登记取消范围/数据库尚无Run行，以及旧投递卡在真实受理读取。停止并保存新代次后，旧投递不记新预算、失败或控制新状态；已开始的旧Run进入真实终态。

原13失败及其当时未运行的workflow事实保留；14/15后续开发通过另列。该组不是最终固定候选，T07仍IN_PROGRESS、65叶NOT_RUN，等待t07b完整11门禁。


clippy5随后实际退出101：`loopx.rs` 末尾多余return违反原有 `-D warnings`。原日志SHA256为 `38002d8231cfe0e700fa1e6c37de603c373ce7f17aab4c85b800f3e09c3d5ef8`。主任务只删该return后冻结t07b（541项），完整11门禁正在运行；该修正不改写clippy5失败，也不提前发放65叶通过。


## t07b 服务门禁发现的页面失败（完整退出待回执）

`r06_agent_settings_live::native_pending_reference_deletion_current_electron_shows_and_retries_truthfully` 实际失败：页面没有出现预期“引用清理待处理”，原现场出现“至少保留一个助手”。原始UI JSON、真实磁盘删除receipt及preferences共3文件已复制到 `artifacts/rust-tauri/R07/T07/t07b-service-reference-pending-scene/`，每份原件与副本SHA256一致，路径和模式列于SCENE_MANIFEST.json。

独立审阅正在核对全局hook跨home竞争，尚不把推测当确定原因；服务整组尚待完整回执，不能先填最终退出值。已有8份t07b门禁exit0不覆盖此失败，T07仍IN_PROGRESS、65叶NOT_RUN，不写final通过。


## t07b 完整处置（原失败保留）

11项核心门禁全部结束：10项退出0，service退出101（1073通过、2失败）。全部回执前后源码同7192fa…、日志SHA256重算一致。完整服务日志SHA256为 `1c8eb272b82015b66c96b7e13ac55465a7562e2595887253e6af00b9bc5d6a7d`，处置及逐回执索引见 `artifacts/rust-tauri/R07/T07/t07b-DISPOSITION.json`。

- native pending页面失败已由独立审阅定位：两个并行home的单槽故障HOOK互相覆盖，产品拿到completed，UI不显示pending合理。这是原因定位，原失败不改，也不是后续重跑通过。
- workflow fanout在第605行attempts为2、预期1，仍在复现和修复，不提前归因或领通过。
- UI 21套495项通过，使用本候选新构建二进制，前后SHA256均 `468ff0a0c99c023f1265e72a9a4866503eeee3af65b3ac01d05f5ff3fd60c243`，build/UI回执绑定摘要吻合；补充headless 11项通过另列，不能覆盖service失败。两项人工浏览器检查本候选未运行，保留历史而不算当前通过。

源修复已开始，待冻结t07c重验。T07仍IN_PROGRESS；65当期NOT_RUN、22继承T03分开，A14正式导入联验留T08。开发649通过没有混入固定候选通过。


### t07b 第二失败的原始现场

仅从指定合成fixture `lingxi-r07-workflow-0c70000a5facc241` 保存journal、workflow活动与SQLite一致快照至 `artifacts/rust-tauri/R07/T07/t07b-workflow-fanout-scene/`。journal原字节为alpha attempts=1、beta attempts=2。数据库以mode=ro打开源、SQLite online backup生成快照，没有直接拷贝主db/WAL/shm；快照integrity_check=ok、foreign_key_check无异常。原journal/活动的副本摘要与原件一致，完整清单见SCENE_MANIFEST.json。

快照中beta首轮 `run_000001a3185c5000_000003` 已真实queued→running→failed.provider_error，model_call_started=0、usage行=0。因此已排除本次由派发前session_busy造成；待查真实provider_error。作者正在补诊断，暂无已确认业务修复，不改写attempts2原失败。未遍历其他home、未复制凭证、未改源码或运行Cargo。


## t07b 后续开发补证与边界

- integrated16：同源码exit0，lib623、native14、workflow19通过；日志SHA256 `50d444f9e092739af860a2a99d46358eed84bf4ef3d417e5eea891690988ff21`。
- fanout-diagnosis1：原fanout断言单跑30次皆退出0，日志 `d75fe79cac406675f9318283a8c2dfd6c504d5cb01bc64f687a15bba56ea23cd`；只是未复现，不能关闭原失败。
- prompt-race1：现役atomic_write真实16线程×128=2048写，出现287次ENOENT，exit101，日志 `61bf7f9df486d6c261a21b8a4f5035335125e842a38f4dcccfaa32c55adbe841`。这证实并发文件保存本身存在缺陷，原失败保留。
- 主任务改为PID+时间+计数唯一temp、create_new、写失败清理后，prompt-race2同源码exit0，完整workflow20通过，包含原fanout和2048并发写完整JSON/无tmp；日志 `691b076a21cc652dec7d8c93f7c7a7f019d3ea28ed2022526d649e8aa692213f`。

四份日志和前后源码数组摘要均重算吻合。并发写bug及修复已证实，但未直接还原旧t07b beta的provider_error唯一原因；不能把单跑未复现、后续通过当原失败不存在。待clippy和独立review后冻结t07c，65当期叶仍NOT_RUN、T07仍IN_PROGRESS。


## t07c 当前固定候选结果（尚未最终验收）

冻结543项，源码指纹 `f634f22f7b072c7d258b6dfaf48014c5d0a863ee8aa0fab3f123203a94bbcabe`；归档SHA256 `34e1ab9afb13b51f849cb83ce0372b696a70c47da1f408f5d9567fbaa6b64e30` 重算吻合。service实际50组1080通过/0失败，退出0，日志 `5b6bd73e3d314cc80c90a3ac410d94c6e9f6f7ce256af5779389c932555fa755`。fmt/clippy/protocol/protocol-tests/kernel/boundaries/types及随后已生成回执的build也退出0。

adapter退出101，唯一失败为真实popup/dialog目标（该目标9过1失败），helper第218行报告browser_closed/effect_unknown。b/c的14个浏览器源条目相同；原日志无法定位具体失败CDP命令，不能凭推测改成必然modal死锁。

同c原测试二进制10次独立诊断皆0，仅未复现，日志 `45effc938382c469776ab8c38c8be7ef10ae028b24ab806d359a705459ef8395`；原adapter失败不关闭。独立Chrome155 pipe探针两轮alert期间getNavigationHistory均在2.2秒内响应，可排除“必然modal死锁”；约0.58–1.23ms是正常页面history响应，不混成alert期间延迟。首探针另有CDP id2超时、未产生checks，原失败也保留。详情及原件引用见t07c-DISPOSITION.json。

构建和UI回执已在只读核对期间生成：11核心回执全部结束，10项0/adapter101。UI21套495通过，日志18ecc6c48592943cdd455c1fcacdb7ecf80d74fcebee56af29bdf6b41afcad0e；新二进制前后SHA256均915c392e1433d74a9351540e7bb0dbde0832c2ae293b6120b3d57b590e369f7b，build/UI回执绑定摘要吻合。精确清单见处置。等待错误保真取证方案和后续证据，T07仍IN_PROGRESS、65当期NOT_RUN，22继承分开，b全部原失败保留。


T07-D断点：C完整11门禁10过/adapter101；D针对性6项加UI共7项，6过/adapter101，UI21套495过且新二进制绑定核对一致。D的independent_persistent_profiles_auxiliary_locale_and_site_data_cleanup在首次关闭cookie后的Resume报browser_navigation，现役错误映射丢失精确信息；独立诊断和原fixture归档保留，原因未唯一定位。C popup失败与D导航失败不能断言同源，二者都不计通过。T07自有服务1080/页面495已验证，但完整门禁未通过，浏览器交T12复查；65当期NOT_RUN、22继承分开。按用户推进授权进入T08 IMPLEMENTING，非PASS。D处置见t07d-DISPOSITION.json；最新独立诊断不属于D源码候选。
