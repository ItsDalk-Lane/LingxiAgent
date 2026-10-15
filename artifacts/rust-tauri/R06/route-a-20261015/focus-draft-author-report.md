# D901 + B9 作者交付（仅此两叶）

实际新建按指定助手、已授权真实目录、记忆开关和冻结模型/思考等级落入真实会话、元数据、唯一模型设置与焦点偏好；模型随后真正读取新目录，创建成功通过现役鉴权 WebSocket 广播。草稿在真实生产输入框按稳定 sessionId 保存和重开，旧 path 只作解析入口，拒绝不改原资料。

最终补丁：`focus-draft-final.patch`，SHA256 `f9dc6e93043c4cc40ed74ed74385cd90930f12124b6e42766d0d4e5ceab39898`。精确24文件/base/candidate逐文件及聚合SHA见 `focus-draft-final-source-sha.json`。基线为 c095f606c + 冻结聊天44源（manifest dc5d6053…）+ Root已修 fetchSessionHistoryPage 的 `notModified === true` 单行；该单行不重交。三文件 focused-runtime extension 已含，旧 first28、根别名、skills7、完整聊天核心均不重复。只含明确24源，不含清缓存导致的artifact删除。新模块、sidecar和单一prepared SQL共用真实身份与存储，不新增依赖或迁移。

原失败完整保留：

- `focus-original-negative-405/result.json`：现役实际创建入口 HTTP405；早期未捕获原CLI不可变hash，报告不补造。
- `focus-native-joint-fourth.log` 与 raw `.../lingxi-r06-session-focus-45607-1792033089867475000`：元数据正确但真实read返回全局默认目录哨兵。原因是阻塞线程没继承任务工作目录；修复filetools/discovery执行边界明确传入冻结目录，exec既有边界保留。
- `focus-broadcast-real-negative.log`、`focus-broadcast-original-binary-sha.json` 与 raw `.../lingxi-r06-session-focus-89390-1792034827651303000`：真实WS握手/ClientHello成功，创建200后所属连接等待创建通知超时。修复仅ChatWire新增生命周期发帧方法，创建完全成功才发；不伪造Run、不占流seq/replay，原WS逐连接凭证及会话owner复核不变。

最终同候选证据：

- `focus-final-broadcast-joint.log`：11/11（既有会话6含原45模型调用CLI父、新焦点1、旧focus3、旧studio1）。新焦点父启动当前CARGO_BIN_EXE CLI和两台真实服务，原生产InputArea/Tiptap窗口6检查真实通过；其中同ID跨服务零错误PUT，并额外硬断言原子关闭旧侧会话和path override。
- raw `/private/tmp/lingxi-r06-agent-delete-env/tmp/lingxi-r06-session-focus-2797-1792035154886570000/result.json`：当前3个真实用户Run分别read/ls/exec，每Run精确3个模型请求+3条usage，合计9，全部 chat/second/high。逐Run账本purpose=chat、origin=user、provider=chat、model=second，不把额外调用忽略或混算。读取/ls/pwd均证明消费选定目录，未消费同名全局目录。免费创建、列表、拒绝和根被替换后的失败Run均零额外模型请求/费用。
- 同一结果：所属owner真实收到完整 `session_created`，内容与HTTP创建结果相等，foreign有效凭证WS零创建帧；没有runId/streamId/seq。独立detached真实创建保留主焦点、默认choice及助手目录历史字节，parent/depth确实同SQL事务写入。已授权根后来被替换为链接时真实403/Run failed，零费用。
- 有效无chat权限凭证403、错误助手/相对路径/不存在模型/思考等级拒绝且原焦点/row不变。macOS preferences真实uchg造成下游500，fresh row/choice/metadata撤回、原偏好不变，不把错误呈现成功。
- `focus-final-old-ui.log`：原两文件92/92，未改旧断言。三TS通过，日志 `focus-final-{root,node,test}-ts.log`。本包实际lib/bin/四父 strict Clippy通过 `focus-final-selected-strict.log`。
- `focus-final-broadcast-strict.log` 的全all-targets失败如实保留：聊天新UserMediaTurn令旧match非穷尽、model_history测试模块后声明以及本树未同步EX09两个sql夹具；Root正在独立修公共前置。没有用allow/削弱旧测试隐藏。

写失败边界：按现役先写助手目录历史、后写焦点偏好的顺序，偏好落盘失败可能已记录目录历史。错误明确告知“助手工作目录历史已记录”，不虚称所有步骤回滚；fresh会话初始化资料与焦点实际撤回。只有完全成功才广播。

已核实授权：原先对cwd权限的假设经TS和Rust现役authorized-folders入口核对，已认证Scope(chat)就是用户显式授权目录的合同。本包保持这项合同、真实会话owner、受保护目录和根身份复核，不改变LocalOnly，不冒称发现越权漏洞。

限制：只申请登记 D901/B9；FBDE browser与8E2 map继续后包，未登记挂载id明确400、无browser宿主明确unavailable。没有把纯metadata冒充模型消费。只证明macOS现役Electron+真实Rust服务，不声称Tauri或Win/Linux窗口已测。供应商为本机真实HTTP替身，未调用外部付费供应商。Main/登记/门禁/白名单/原断言未改，未提交推送；Root负责审查、整合、绑定。

给已应用preview23的聊天作者单独增量：`focus-after-preview-incremental.patch`（fetch ===true若其已手工修则跳过该已应用行）；不要与完整最终补丁重复应用。下一V14主动委派作者将按本prepared创建单路径扩展布尔冻结，不另造SQL。
