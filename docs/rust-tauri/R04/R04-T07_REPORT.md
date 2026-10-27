# R04-T07 报告｜MCP 与插件 worker 接入

执行代理：EXECUTOR-R04-T07-**E02**（一次性续做代理）。日期：2026-09-30。
分支 `codex/rust-tauri-migration`，基线 `26251dc92`（R04-T06 已独立 PASS 并推送确认）。
状态：**READY_FOR_REVIEW**（两层自查完成；不自行判独立 PASS）。

> **E01 中断与 E02 续做的如实声明**：前一位执行代理 EXECUTOR-R04-T07-E01 在实现中途
> 超时中断，未写报告/证据/自查。E02 启动时工作区含 E01 的未提交进展
> （`mcpbridge.rs`/`workerrpc.rs`/`src/bin/r04_t07_fixture.rs`/
> `tests/r04_t07_mcp_and_workers.rs` 全新 + `lib.rs`/`Cargo.toml`/`Cargo.lock` 改动）。
> E02 独立审计了上述全部落盘代码（不依赖 E01 聊天记忆），发现 E01 骨架方向正确但
> **从未跑通验证**：测试套件因合成分页 fixture 的游标 bug 无限挂死（首轮运行 8 分钟无
> 输出，进程采样定位），另有行读取无界内存、6 处断言与真实契约不符、2 处 clippy 违
> 规、以及派单对抗清单中「取消后迟到响应」「T03 权限面」两族场景缺失。E02 按根因修
> 复/补齐并完成全套验证。逐项区分见 §2.3；中断历史与 E01 原有成果均如实保留。

---

## 1. 目标与结论

外围工具进入同一 Rust 授权、执行和恢复链，不形成第二内核：

- **MCP 适配**：官方 rmcp 3.4.1（R01 D-06 锁内版本+同一 feature 集，无新包进锁）真实
  接入——initialize 握手、协议版本协商、分页 tools/list（有界）、tools/call、变更通
  知计数、断连重连（重连=新握手，从不重放调用）。HTTP/SSE 未携带（锁外 feature）：
  `McpEndpoint::Unsupported` 响亮拒绝，绝不回落。
- **身份与代次**：`ToolOrigin::Mcp{server_id}` 复用 T01 命名空间（`tool:mcp:{server}:{tool}`，
  与现役 TS registry 字节兼容）；两个 server 同名工具=两个不同 target；listing 变化映射
  registry generation（update→target generation 上升→旧句柄拒绝；消失→uninstall）。
  server 注解/描述/schema/结果全部按不可信输入处理（注解只进 declared_permission 审计，
  权限恒为 Execute、恢复能力恒为 CONSERVATIVE；远程资源 URI 永不映射本地 ResourceRef）。
- **worker RPC**：最小单操作请求（CSPRNG 调用 ID/期限/资源许可/大小上限），stdin/stdout
  行协议双向长度受限（读取**中途**强制 cap）；并发信号量、进程组 kill、有界 grace；
  未确认结局（stdin 失败/超时/无结果退出）=Unknown，绝不伪造干净失败。
- **受控宿主回调**：`WorkerModelPort`（R05 ModelGateway 边界面）；生产形态
  `UnconfiguredWorkerModel` 对一切回调诚实返回 `model_capability_not_configured`（无伪造完
  成、无凭证下发）；`BoundedWorkerModel` 在内层端口之前宿主侧强制回调数与输出 token 预算。
- **网关集成**：MCP 与 worker 工具全部经 T02 网关（prepare→T03 策略/批准→execute）注册执
  行；T03 面真实裁定（operate→Allowed / ask→NeedsApproval→真批准往返 / read_only→
  PolicyDenied 拒）；结果回结构化 ToolSuccess（内容块/校验过的资源引用/截断标志），真链
  成功后 durable wire 事件携带真实内容。

结论：R04-A13/A14 及全部追加对抗项在合成/受控实例上真实执行通过（§4）；fmt/clippy/
双门禁/T01-T06 套件/R03 十套件+A15+A16 回归全绿（§5 退出码在案）；workspace 全量测试除
已登记的 r00 防火墙环境项外全绿。

## 2. 实现摘要（关键文件:行）

### 2.1 MCP 桥（E01 原有主体，E02 修补两处）

`rust/crates/lingxi-service/src/mcpbridge.rs`（1323 行含 4 单测）：

- 环境白名单 `SAFE_MCP_ENV_PASSTHROUGH`（L80：PATH/LANG/LC_ALL/LC_CTYPE/TZ/TMPDIR——
  **无 HOME**、无任何服务凭证；stdio 子进程 env_clear 后仅注入白名单+宿主显式项）；
  输出截断上限 `MCP_MAX_RESULT_CONTENT_BYTES`（L85，诚实 `truncated:true`）；默认调用期限
  `MCP_DEFAULT_CALL_DEADLINE_MS`（L88）；**E02 补：listing 有界性 `MCP_MAX_LIST_PAGES`（L94）
  /`MCP_MAX_LISTED_TOOLS`（L98）**——不可信 server 永续游标/工具洪水时响亮拒绝（此前
  listing walk 无页数上限，恶意 server 可将宿主钉死在无限分页循环里——正是 E01 合成
  fixture 自身 bug 触发的那个循环形态）。
- `breakable_duplex`（L180）：两条独立单向 pipe 各带共享断路开关——`ECONNRESET` 型真实
  传输死亡（确定性故障点，A13 断回执场景）；`McpEndpoint`（L214）：Stdio 子进程（无
  shell、白名单 env、env 名/值校验）/Duplex 工厂/Unsupported（响亮拒绝）。
- `BridgeClientHandler`（L246）：server 主动请求保持默认拒绝（无 sampling/roots/
  elicitation——宿主不被不可信 server 驱动）；`tools/list_changed` 通知计数
  （`McpServer::tool_list_changed_notifications`，宿主下次 refresh 读取）。
- `connect`（L444）：真实 initialize 握手（rmcp `serve`）；返回协商协议版本+server 身份；
  `connect_count` 审计（A13 证据：重连恰发生一次）。重连从不重放任何调用。
- `list_tools`（L533，E02 加界）：分页走全量（cursor 传递），**页数/工具数双上限**。
- `call_raw`（L582）：期限包裹的 tools/call；**只有 transport 断与超时是 Unknown 候选**
  ——server 返回的 `is_error` 结果是回执（完成但失败）；`input_required`/async task
  形态按契约拒收（本桥不替不可信 server 回答交互、不携带任务轮询）。
- `manifest_for_tool`（L661）：listing 项→T01 manifest。注解不可信：只进
  declared_permission；权限恒 Execute；恢复能力恒 CONSERVATIVE（`readOnlyHint` 声明
  不授予任何东西）；input/output schema 逐字节保留交 T01 fail-closed 校验（$ref 等
  不兼容形态按该工具拒绝，不放宽）。
- `sync_server_tools`（L737）：listing→registry 同步——新工具注册（同名跨 server 是不
  同 target，不覆盖）；变化走 `registry.update`（target generation 上升，旧 prepared
  句柄/旧批准随代次/卸载失效——T01/T02/T03 语义复用）；消失走 uninstall；同一 listing
  内重复名折叠首个（确定性合并）。`McpSyncReport`（L708）记录代次前后值。
- `McpToolExecutor`（L841，实现 ToolExecutorPort）：绑定 target→server 工具名+输出
  schema 快照；未绑定 target=明确失败；参数必须是对象；结果侧——is_error=完成但失败；
  structured_content 违反该工具注册的 output schema=失败（`validate_structured_output`
  L928 复用 T01 校验词汇，且「依赖 schema 默认值补全的结果」也算违反——server 产的输
  出必须自己成立）；内容映射 `map_content_block`（L882，图像/音频/资源/资源链接全部
  映射为文本描述——**远程 URI 永不铸造本地 ResourceRef**）；256KiB 截断上限（诚实截断
  标志+说明）；空结果带诚实标注。
- `register_mcp_server`（L1175）/`refresh_mcp_server`（L1201）：组合入口（生产默认
  bootstrap 不调用——与 T04/T05/T06 同口径）。refresh 在 listing 失败于死传输时**恰一次**
  新握手重连后重列（E02 简化了 E01 的 map_err 死绑定——clippy）。
- 4 条内嵌单测：target 命名空间、注解不软化权限/恢复、远程 URI 不映射本地引用、结构
  化输出校验词汇。

### 2.2 worker RPC（E01 原有主体，E02 补有界行读取）

`rust/crates/lingxi-service/src/workerrpc.rs`（1272 行含 4 单测）：

- 常量（L78-99）：env 白名单（同 MCP 口径，无 HOME/凭证）、`WORKER_RPC_PROTO`、请求/
  行双向字节上限（256KiB）、默认期限 60s、kill grace 2s、每调用回调上限 4。
- **宿主模型端口**（L106-253）：`WorkerModelRequest`（purpose/prompt/max_output_tokens——
  **载荷无凭证**）；`WorkerModelRefusal`（CapabilityNotConfigured/BudgetExceeded/
  ProviderRefused 稳定码）；`WorkerModelPort` trait；`UnconfiguredWorkerModel`（R04 生产
  形态：一切回调诚实拒）；`BoundedWorkerModel`（宿主侧逐 invocation 回调计数+输出
  token 上限，**在内层端口之前强制**——即使 R05 接入真实网关，预算仍是宿主事实）。
- wire 类型（L255-310）：request/callback/result/content 行（serde）；result 行携带
  `claimed_files`。
- `WorkerLimits`（L315）/`WorkerFailure`（L337，稳定码 + ProtocolError 映射：
  SandboxRefused/ClaimedUnauthorizedPath→Forbidden 等）。
- `WorkerRuntime`（L400）：并发信号量（cap=并发 worker 数）、可选 T06 `SandboxPort`
  包裹（**包裹在前，拒绝=响亮 spawn 失败，绝不裸跑**）、白名单 env、Unix
  `process_group(0)`（有界 kill 达孙进程）；`WorkerChild`（L494）：killpg+try_wait+
  2s 有界 wait（kill_on_drop 仅最后手段）。
- `WorkerToolSpec`（L523）：plugin 命名空间身份、allowlisted 单操作 op、路径参数键
  （推导资源许可）、argv（无 shell 无插值）、显式 env、cwd、模型端口。
- `WorkerToolExecutor`（L557）：执行侧**重新推导** grant（T05 模式——按当前文件系统状
  态，只授权 host 允许的工作区范围）；`mint_request_id`（L619，getrandom CSPRNG——熵
  失败=拒绝，不降级可猜测 id）；**E02 补 `read_line_bounded`（L638）**——行读取在
  **读取中途**强制字节上限（fill_buf 逐块累计，越限立即 InvalidData 错误）：恶意
  worker 流式发送无换行的超长行时，宿主内存被钉死在 cap 附近，而不是先无界缓冲整行
  再计数（E01 原实现 `read_line` 的缺口）；协议循环——callback（计数上限+模型端口
  回复，拒绝码原样回传）/result（**id 必须匹配本次 CSPRNG 调用 id**——伪造/跨调用
  陈旧 id 拒绝；首个被接受的结果唯一入账，worker 随即被杀，第二条结果行无法再入账）
  /其他 kind=violation；deadline→协议 cancel+有界 kill→**Unknown**（副作用未确认）；
  stdout 早死/EOF 无结果→**Unknown**；worker 自报 ok:false=完成但失败；
  **claimed_files 验证**（grant 内+磁盘存在才铸造本地 ResourceRef——越权声明=响亮拒
  绝且不铸造，伪造本地文件链接的正路）；空内容诚实标注。
- `register_worker_tool`（L1103）：registry+gateway 注册（`ToolOrigin::Plugin` 命名空
  间、PermissionKind::Execute、CONSERVATIVE 恢复）。
- 4 条内嵌单测：未配置模型诚实拒、宿主侧回调预算+token 上限（内层端口之前）、无内
  层仍拒 not-configured、请求信封字段序列化。

### 2.3 E01 中断审计与 E02 修复/补齐清单（如实区分）

**E01 原有且经 E02 审计后保留**：mcpbridge.rs 与 workerrpc.rs 的主体结构（端点模型/
连接管理/manifest 推导/sync/执行器、worker 运行时/协议循环/模型端口）、fixture 二进
制、测试文件骨架与 16 个测试中的 13 个、lib.rs 模块接线与再导出、Cargo.toml 的 rmcp
边（锁内 3.4.1+client,server——E02 核对 `git diff rust/Cargo.lock` 仅新增一条
lingxi-service→rmcp 依赖边，无新包版本进锁，符合派单约束）。

**E02 发现并修复的 E01 缺陷（根因修复，非掩盖）**：

| # | 缺陷 | 根因 | 修复 |
|---|---|---|---|
| 1 | 测试套件无限挂死（首轮 8 分钟零输出；进程采样定位在 `adversarial_mcp_pagination_walks_all_pages` 的 list_tools 循环） | E01 合成分页 server 的游标逻辑 `offset = if cursor=="start" {0} else {size}` 对每个非首页都回到第二页，`next_cursor` 永不为 None | fixture 游标改为数值 offset（确定性走页）；**同时**给桥的 listing walk 加 `MCP_MAX_LIST_PAGES/MCP_MAX_LISTED_TOOLS` 上限（对不可信 server 的同形态攻击面真实防御）+ 新对抗测试 `adversarial_endless_paging_is_refused_loudly` |
| 2 | worker 行读取无界内存 | `read_line` 先无界缓冲整行再检查 cap——恶意 worker 可用无换行超长行打爆宿主 | `read_line_bounded`：fill_buf 逐块累计、越限立即中止（cap+8KB 有界）；`huge_line` 用例回归 |
| 3 | A13 全链测试第二 run 期望 Succeeded 实得 Unknown | E01 测试在断传输后未走重连恢复路径就发起第二次调用（桥的契约：重连是 refresh_mcp_server 的显式恢复动作，不在 call 内隐式重连） | 测试插入 `refresh_mcp_server`（恰一次新握手，connect_count==2）后核验查询——与 A13 场景「客户端重连并恢复」一致 |
| 4 | worker 沙盒测试期望越界**读**被拒，实测 READ_OK | T06 冻结策略**允许**边界外读取（deny-read 集只有 auth.json 等；受限于可写根的是**写**）——E01 断言与 T06 冻结矩阵矛盾；且 E01 把受限哨兵放在 $TMPDIR 下（T06 报告 §7.2 已登记该假对照陷阱：冻结契约允许写 $TMPDIR） | 重写为越界**写**探针（fixture 新 `write_outside` 模式）：逃逸目标置于 CARGO_TARGET_TMPDIR（$TMPDIR 外），断言 WRITE_DENIED+磁盘无痕迹+边界内允许对照；顺带修复空 argv 元素被沙盒拒绝的问题（extra 为空时省略该参数） |
| 5 | happy-chain 测试断言 journal detail 含 "count=" 实为摘要 | T01 冻结的收据契约：journal detail 携带摘要/进程状态，**内容本体在 durable wire 事件**（T01 报告 §2.2） | 断言改为诚实契约：receipt Succeeded+dispatched+dedup+「external content digest」；**另**查询 durable `tool_call_completed` wire 事件 payload 断言真实内容 `count=` 在场（比 E01 断言更强） |
| 6 | callback_storm 断言 "model-ok" 在最终文本 | fixture 只报告 refusals 计数，不回显成功回调内容 | fixture 补 `oks` 计数；断言 refusals=4 **且** oks=2（预算内调用完成+超额被拒双向钉住） |
| 7 | double_result 期望「第二次结果行=协议违规」 | 循环在首个接受结果后即 break——「第二个结果违规」检查是不可达死代码；真实语义：首结果唯一入账，worker 随即被杀，第二条结果行无从再入账 | 删除不可达检查（注释言明单次入账语义）；测试改为断言恰得首个结果一次（「first」）；跨调用票据复用由 bad_id 腿（id 不匹配拒绝）钉住 |
| 8 | clippy `-D warnings` 2 处违规（clone_on_copy、manual_inspect 死绑定） | E01 未跑过 clippy | `*budget`；refresh 的 match 臂去掉无用绑定 |
| 9 | 派单对抗清单「取消后迟到响应」无任何测试 | E01 中断 | 新测试 `cancelled_call_never_consumes_a_late_response`：副作用落地后 CALL 级 abort 在途回执调用→释放服务端迟回执（写入**存活**传输）→取消任务无产出/计数不增/句柄单次不可重放/新核验查询 count=1 |
| 10 | 派单核心交付 5「执行类权限按 T03 面」无 MCP/worker 工具的覆盖（全部测试只用 operate） | E01 中断 | 新测试 `mcp_and_worker_tools_follow_the_t03_permission_face`：MCP+worker 工具在 operate/ask/read_only 三档的真实 prepare 裁决（read_only=PolicyDenied 拒绝、ask=NeedsApproval、operate=Allowed）+ ask 会话真链 park→真实批准面→恰执行一次 |
| 11 | 两个 env 探针测试共用一个进程级环境变量名（E02 对抗自查发现的并行 flake 源） | `std::env::set_var/remove_var` 是进程全局状态，cargo 并行测试线程下 A14 的 set/remove 窗口可与 stdio 探针的断言竞态 | 拆分为每测试独立命名秘密（A/B），fixture 探针报告双名，各断言各的——无共享窗口可竞态；连续 3 次全套件复跑全绿 |

**E01 完全未做（E02 补齐）**：报告、证据目录、R04_TEST_MAP.json T07 条目（E01 中断前
均为空）、上述 9/10 两族测试、全部验证执行。

### 2.4 测试 fixture（外部系统，非被测面替身）

`rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs`（276 行）：

- 单操作 worker fixture：argv[1] 选行为（ok/slow_ok/env_probe/ask_credentials/
  callback_storm(E02 补 oks 计数)/claim_ok/claim_outside/**write_outside(E02 替换
  read_outside)**/bad_id/double_result/huge_line/malformed/unknown_kind/hang/
  exit_early/error_result）——包含故意敌对形态，用真实子进程走真实 spawn 路径。
- `--mcp-stdio-server`：最小但真实的换行 JSON-RPC stdio MCP server（initialize/
  tools/list/tools/call），作为 rmcp 客户端侧的外部 server（子进程 transport+env 探针）。
- 该二进制仅测试支持（文档注明），不接线任何生产入口（cargo src/bin 自动发现；
  `CARGO_BIN_EXE_` 引用与既有 config_bounds/service_health 测试同模式）。

## 3. 修改文件清单

- 新增：`rust/crates/lingxi-service/src/mcpbridge.rs`（E01）、
  `rust/crates/lingxi-service/src/workerrpc.rs`（E01）、
  `rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs`（E01，E02 两处修改）、
  `rust/crates/lingxi-service/tests/r04_t07_mcp_and_workers.rs`（E01 骨架，E02 修复+
  新增 3 测试，终态 19 集成测试）、
  `artifacts/rust-tauri/R04/T07-E02/**`（证据；E01 未建证据目录，E02 以 T07-E02 为唯一
  证据根）、本报告。
- 修改（产品）：`lingxi-service/src/lib.rs`（模块声明+再导出，E01 主体+E02 补两个新常
  量导出）、`lingxi-service/Cargo.toml`（rmcp 边，E01）、`rust/Cargo.lock`（仅
  lingxi-service→rmcp 一条依赖边，无新包版本——E02 复核）。
- 修改（文档）：`docs/rust-tauri/R04/R04_TEST_MAP.json`（T07 条目，E02）。
- 未触碰：kernel crate、协议 wire 面、toolgateway/approval_service/runs/sessions/
  subagents/exectools/sandbox/procsupervisor/resourceaccess（T01-T06 冻结面零改动）、
  生产默认入口（bootstrap 不调用任何 register_*——T04/T05/T06 同口径）、xtask stage
  maps、check 脚本白名单、`ORCHESTRATOR_PROGRESS.json`、现役 TS/Node 栈（npm 侧按
  「如触碰」条件不适用）。

## 4. 验收场景与逐项结果（真实链：bootstrap_with_deps 组合根→真实注册表→真实网关→
真实 T03 策略/批准面→真实 ResourceAccess→真实 rmcp 3.4.1 协议栈→真实子进程/沙盒；
替身仅外部系统/外部答复者）

| ID | 要求 | 实现/测试 | 结果 |
|---|---|---|---|
| R04-A13 | 计数动作+断回执→重连→不盲重发/Unknown 处理/计数不盲增 | `r04_a13_mcp_reconnect_never_replays_the_side_effect`：合成分页 rmcp server（真实 initialize+协商 2025-11-25 形态+身份回传）注册 7 工具；BreakBeforeReceipt 模式下服务端执行计数并**驻留回执**→宿主断传输（ECONNRESET 故障点）→客户端得 **Unknown**（reason 含 never blindly retried）→`refresh_mcp_server` 死传输 listing 触发**恰一次**新握手（connect_count==2、新协商协议）→计数==1、received==1（无盲重发）→verified 状态查询 get_count=count=1。`r04_a13_full_chain_...`：同一场景走真实 run 链（provider 工具轮→驱动→journal）：**journal 收据 Unknown**（不伪造失败/成功）；重连后新 run 的核验查询 Succeeded、计数仍 1 | PASS |
| R04-A14 | worker 请求超 grant 文件/模型凭证→宿主拒；grant 不扩大；秘密不泄露；主服务可用 | `r04_a14_worker_cannot_bypass_grants_or_model_credentials`：(1) 模型凭证——worker 请求 model.complete，**真实 UnconfiguredWorkerModel** 回 `model_capability_not_configured`，输出无任何 host-secret 串；(2) 越界文件参数——`../restricted/secret.txt` 被**真实 ResourceAccess** 拒（worker grant refused），worker **从未 spawn**，受限哨兵字节不变；(3) 越界 claimed_files——worker 声称受限路径→`worker_claimed_unauthorized_path` 拒绝、不铸造本地引用；(4) env 探针——LINGXI_T07_SECRET=false、HOME=false、PATH=true（白名单）；(5) 之后合法 worker 调用经同网关成功（主服务可用） | PASS |
| 对抗·两 server 同名工具 | 命名空间隔离 | `adversarial_two_servers_same_tool_name_hit_their_own_targets`：alpha/beta 同列 get_count——两 target 各自注册互不覆盖；按精确 target 调用各命中各的 server（received_calls 1/0→1/1） | PASS |
| 对抗·tools/list 变化 | 代次映射+旧形状失效 | `adversarial_tools_list_change_updates_generation_and_invalidates_old_shapes`：echo schema 增必填字段→refresh `updated=1`+catalog generation 上升→旧形状调用 `ArgumentsInvalid` 拒（零派发）→新形状执行成功；boom 从 listing 消失→`removed=1`+target 停止解析 | PASS |
| 对抗·错误/超大输出/不符 schema/伪文件链接 | 结果是不可信输入 | `adversarial_error_oversized_schema_violating_and_forged_results`：is_error 结果=完成但失败回执（mcp_tool_error+上游文本）；300KiB 文本→诚实 `truncated:true`+截断说明+内容有界（cap 调 8KB 钉住）；structured_content 违反注册 output schema（count=-5<0）→失败；`file:///etc/passwd` ResourceLink→文本描述、`resource_refs` 为空（远程身份不是本地授权事实） | PASS |
| 对抗·取消后迟到响应（E02 补） | 迟到结果不消费/不复活 | `cancelled_call_never_consumes_a_late_response`：副作用落地后 abort 在途 execute_prepared（CALL 级取消形态）→释放服务端驻留回执（写入**存活**传输——真实迟到响应到达已消失的调用方）→取消任务零产出（JoinErr cancelled）、计数不增、**已花费句柄重放被拒**（consumed/unknown）、新核验查询 count=1 | PASS |
| 对抗·跨调用复用 ticket | 调用 id 单次 | `adversarial_worker_protocol_violations_are_refused` 的 bad_id 腿（伪造/陈旧 id=violation 拒绝+主服务仍可用）+ double_result 腿（首结果唯一入账「first」）+ CSPRNG 每调用新 id（`mint_request_id`） | PASS |
| 对抗·伪造本地文件链接 | 声明≠事实 | A14 (3) 腿（worker 声称越界路径→拒且不铸造）+ `worker_claim_in_grant_mints_a_verified_resource_ref`（grant 内且存在的声明才铸造 ResourceRef，file:// URI+size）+ MCP forge_file 腿（远程链接→文本） | PASS |
| 对抗·永续分页（E02 补） | 不可信 server 有界性 | `adversarial_endless_paging_is_refused_loudly`：server 每页都回游标→注册失败（page bound 文案）、零 target 注册；正常分页 `adversarial_mcp_pagination_walks_all_pages`（5 工具 3 页全量注册） | PASS |
| T03 权限面（E02 补） | 执行类权限按 T03 面 | `mcp_and_worker_tools_follow_the_t03_permission_face`：MCP count_up/get_count（含自带 readOnlyHint 的 get_count）+worker 工具在 operate=Allowed / ask=NeedsApproval / read_only=**PolicyDenied(ACTION_BLOCKED_BY_READ_ONLY)**（hint 不软化）；ask 会话真链：count_up 调用 park 于真实批准面（pending 视图形状摘要 `{}` 无值）→批准→journal Succeeded+dispatched、计数恰 1 | PASS |
| MCP transport 面 | stdio 子进程+白名单+不支持形态 | `mcp_stdio_child_transport_and_env_whitelist_hold`（真实子进程 server：身份回传、env 探针无秘密/HOME、真实 tools/call）+ `mcp_unsupported_endpoint_refuses_loudly`（streamable-http→`mcp_endpoint_unsupported`「refusing instead of falling back」、注册表零变化） | PASS |
| worker 生命周期 | 期限/并发/回收/沙盒 | `adversarial_worker_deadline_and_silent_exit_are_unknown_never_clean_failures`（hang→700ms 期限→有界 kill→Unknown；exit_early→Unknown）；`worker_concurrency_is_bounded_by_the_runtime_semaphore`（cap=1 两调用串行≥780ms）；`worker_sandbox_binding_denies_out_of_boundary_writes`（真实 T06 seatbelt：CARGO_TARGET_TMPDIR 外逃逸目标 WRITE_DENIED+磁盘无痕迹；边界内允许对照）；`adversarial_worker_error_result_and_callback_budget_are_host_owned`（自报错误=完成但失败；回调风暴 refusals=4/oks=2 宿主预算、无凭证） | PASS |
| 真链结构化结果 | ToolSuccess 可消费 | `mcp_tool_success_flows_through_the_full_run_chain`：真 run 链 Succeeded+dispatched；journal 收据携带派生摘要（dedup）+**durable `tool_call_completed` wire 事件 payload 含真实内容 `count=`** | PASS |
| 单测 | 边界词汇 | mcpbridge 4（命名空间/注解不软化/远程 URI/结构化校验）+ workerrpc 4（诚实拒/预算/无内层/信封字段） | PASS |

## 5. 验证（真实命令与退出码）

环境：macOS darwin 27.0.0 arm64；rustup 1.98.1（`/Users/study_superior/.cargo/bin/cargo`
先于 /opt/homebrew/bin）。仓库根执行；原始输出归档 `artifacts/rust-tauri/R04/T07-E02/`，
复跑脚本 `run_t07_validation.sh`。以下为最终冻结候选一轮：

| # | 命令 | 退出码 | 备注 |
|---|---|---|---|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | E01 落盘时 2 违规，E02 修复后 0 |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast` | 101（仅 1 环境失败） | **867 通过 / 0 断言失败**（80 套件 ok+1 环境失败套件）；唯一失败 `r00_management_leaves` 见 §5.1。T06 轮 840+本 Task 19 集成+8 单测=867，逐项对账吻合 |
| 4 | `cargo test -p lingxi-service --locked --test r04_t07_mcp_and_workers` | 0 | **19/19**（A13×2+A14+对抗 13+真链 1） |
| 5 | `cargo test -p lingxi-service --locked --lib mcpbridge / workerrpc` | 0 | 4+4 单测 |
| 6 | `cargo run … -p xtask -- check-contracts` | 0 | 626 entries 零漂移（未触碰协议面） |
| 7 | `cargo run … -p xtask -- check-boundaries`（含 `--self-test`） | 0 | O1-O8+D1-D5+B1；N1-N17 |
| 8-13 | T01-T06 套件回归（`--test r04_t01..t06` + kernel lib） | 0 | 6/13/11/9/14/10 复绿；kernel 77 |
| 14 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh <fresh dir>` | 0 | 十套件钉数精确全绿 |
| 15 | `bash scripts/rust-tauri/r03_t08_matrix.sh <abs dir>` | 0 | A15 全绿 |
| 16 | `bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh <abs dir>` | 0 | A16 四相全绿 |

npm/TS 侧未运行：本 Task 未触碰任何 TS/Node 源（§3），按派单「如触碰」条件不适用。

### 5.1 workspace 全量与 r00 环境项

全 workspace `--locked --no-fail-fast`：**867 通过 / 1 失败 / 0 ignored**（80 套件 ok+1
套件失败）；唯一失败 `r00_management_leaves::
r00_management_positive_and_negative_branches_on_real_service`——panic 文本自证为
macOS 应用防火墙/代理 TUN 拦截未签名测试二进制的入站连接
（"…stalled during write/read exchange (POST /lingxi/v1/web-auth/login, 0 bytes read
so far) … macOS application firewall / proxy TUN may be blocking… this is an
environment failure surfaced honestly"），与 T01/T02/T06 报告登记的同一间歇环境项同形
（kernel/service 代码变化→测试二进制哈希变化→防火墙视为新未签名 app）。隔离复跑
（`gates/r00_management_leaves_isolated_rerun.log`）复现同一防火墙文案。该测试与
MCP/worker 面无任何交集；867 个绿测试与其互不掩盖。T08 最终候选重跑 R03 Gate 时若再现
需环境层处置，不销项。

### 5.2 E01 状态下的首次执行（保留事实）

E02 对 E01 落盘代码的第一次真实执行（`cargo test --test r04_t07_mcp_and_workers`）在
约 8 分钟零输出后判定挂死，进程采样（macOS `sample`）定位：
`adversarial_mcp_pagination_walks_all_pages` 阻塞于 `register_mcp_server →
sync_server_tools → McpServer::list_tools → rmcp Peer::list_tools`——合成分页 server
的游标 bug 使 listing walk 无限循环（详见 §2.3 #1）。这不是环境失败，是 E01 交付物的
真实缺陷；E02 修复后同套件 19/19 通过。

## 6. R03 行为不变量与回归证明（R04-SUP-05）

- **T01-T06 冻结面零改动**：toolgateway/approval_service/runs/sessions/subagents/
  exectools/sandbox/procsupervisor/resourceaccess/kernel 的 diff 为零（git status 核
  对）；T01-T06 套件 6/13/11/9/14/10 复绿+kernel lib 77 全绿。
- Unknown 语义与禁止盲重试：A13 两腿把「已派发无可信回执→Unknown」钉在 journal 层；
  R03 十套件（含 tool_receipt_unknown 验证链）+A15+A16 复跑全绿。
- 取消/迟到：新增取消迟到响应测试与 T03 的 cancel-during-wait 族互补；十套件
  background_steering/request_id_canonicalization 等钉数精确复绿。
- check-contracts 626 entries 零漂移（本 Task 无协议面改动）。

## 7. 两层自查记录（普通→对抗）

普通逐项：§4 表逐 ID 给出前提/动作/预期/实际/退出码；每个拒绝场景都有允许对照
（get_count 核验查询、ok 模式 worker、边界内写/跑、预算内回调 oks=2、分页正常腿）。

对抗性自查（推翻自己的尝试、发现与修复）：

1. **「分页修好了就够了吗？」**：修 fixture 游标只是让测试能跑；同形态攻击（恶意
   server 永续游标把宿主钉死在 listing 循环）是桥的真实缺口——补
   `MCP_MAX_LIST_PAGES/MCP_MAX_LISTED_TOOLS`+对抗测试。自查又发现工具数上限同样必
   要（64 页×N 工具的洪水面），双上限都加。
2. **「行 cap 检查存在就安全了吗？」**：E01 已有 cap 检查但在 `read_line` **之后**——
   `read_line` 无界缓冲先行。以 fixture `huge_line`（64KiB，cap 16KiB）只能证明「事后
   拒绝」；真正的攻击面是无换行超长行（`read_line` 会一直读到 EOF/换行才返回）。
   重写为 fill_buf 逐块累计的中途强制（cap+8KB 有界内存），huge_line 用例继续通过。
3. **「沙盒腿改成写探针后还有假对照吗？」**：逃逸目标首版放在 h.restricted（$TMPDIR
   下）——实测 WRITE_OK（冻结契约允许写 $TMPDIR 临时资源），与 T06 报告 §7.2 记录的
   陷阱同型；迁到 CARGO_TARGET_TMPDIR 后拒绝/允许唯一差异=是否在可写根。另发现空
   argv 元素（extra=""）会被沙盒按 config-invalid 拒绝，register_worker 改为省略空参
   数（行为中性）。
4. **「取消测试真的测到迟到响应了吗？」**：首版设计曾考虑断传输+释放；改为**不断传
   输**只释放——迟到回执真实到达存活的 rmcp 客户端（请求方已消失），比断路变体更接
   近「迟到响应到达」的本意；并以句柄重放拒绝+计数不增双向钉住。
5. **「double_result 删掉违规断言是放松吗？」**：不是——原断言对应的检查是不可达死
   代码（循环在首结果后 break）。真实不变量：首结果唯一入账+worker 即杀（第二条无
   从入账）+跨调用 id 匹配（bad_id 腿）。删死代码+改断言是把测试对准真实契约，断言
   强度不降（「first」精确相等）。
6. **「MCP 权限会不会被 server 自述软化？」**：manifest 推导恒 Execute/CONSERVATIVE；
   T03 面测试特意选了带 readOnlyHint=true 的 get_count——read_only 会话仍拒、注解仅
   进审计（单测 `server_annotations_never_soften_the_permission_or_recovery` 钉住）。
7. **「stdio 子进程会不会带走服务秘密？」**：env_clear+白名单（A14/stdio 两处 env 探
   针断言 LINGXI_T07_SECRET=false、HOME=false、PATH=true）；显式 env 项有名字/值校
   验（=、NUL、8KiB）。
8. **「worker 拿到 grant 列表后能否越权读写？」**：执行侧重推导（T05 模式）+claimed
   文件复验（grant 内+存在）+OS 沙盒（T06 真实 seatbelt 写探针）；grant 列表本身只含
   host 授权路径。
9. **「自己的测试会不会是 flaky？」**（E02 续做轮新增发现）：两个 env 探针测试共用
   `LINGXI_T07_SECRET` 一个名字——cargo 并行线程下 set/remove 窗口与对方探针竞态
   （T02 自查 #5 同族）。修复：每测试独立命名（A/B）+fixture 双名探针；随后连续 3 次
   全套件复跑全绿（1.7-2.0s 稳定）。

## 8. 未验证项与风险（如实登记）

1. **r00 环境项**：macOS 防火墙拦截未签名测试二进制的间歇失败（T01 起登记）；本轮状
   态见 §5.1；需环境层处置，不销项。
2. **跨平台**：仅 macOS arm64 实测；MCP duplex/stdio 与 worker RPC 为纯 Rust+tokio，
   但 killpg/process_group 是 Unix 分支（非 Unix 走 start_kill——孙进程回收弱化，与
   T05 登记口径一致）。Linux/Windows 未真机验证。
3. **HTTP/SSE MCP transport 未携带**（D-06：锁外 feature）——`Unsupported` 端点响亮
   拒绝；R07 按需开启并重测，不是静默降级。
4. **`tools/list_changed` 的自动刷新**：通知计数已实现（宿主可读），但**没有**后台自
   动 refresh 任务——刷新是宿主显式动作（refresh_mcp_server）。现役 Rust 侧无 MCP 配
   置面/生命周期管理产品面（R07 外围接入范围）；本 Task 交付的是接入契约与真实执行
   面，不虚构产品生命周期。
5. **MCP 每调用期限**固定用 `MCP_DEFAULT_CALL_DEADLINE_MS`（manifest.timeout_ms 尚无
   传导——rmcp listing 无超时注解，T01 manifest 的 timeout_ms 字段保留扩展位）；登记
   为受控缺口（期限已真实生效，只是不可按工具配置）。
6. **MCP server 身份回读**：`server_identity` 取 serverInfo.name（客户端侧身份断言）；
   `McpConnection` 的 negotiated_protocol/server_identity 字段目前仅审计（`#[allow(
   dead_code)]` 保留），报告如实登记。
7. **worker grant 目前只推导 Read 操作**（path_args→ResourceOp::Read）：本 Task 的合
   成 worker 形态（读输入→产出声明）与现役单操作执行器对应；写授权 worker 的 grant
   形态（op=write 的资源许可+写目标校验）在 T04 资源面已具备、worker 侧组合留待真实
   插件接入（R07）按需开——不是绕过（host 侧授权链真实存在且被测试）。
8. **worker 「插件生命周期管理」交付面**：任务书必须交付三项中的第三项以
   `register_worker_tool`/`uninstall`（T01 注册表语义）+manifest 注册构成；完整的安装/
   更新/卸载产品流（发现→安装→禁用 UI）属 R07 外围接入，未提前虚构。
9. **测试替身边界**：合成 rmcp server（分页/驻留回执/敌对结果）、r04_t07_fixture 二
   进制（敌对 worker/stdio server）、OkModel/StepsProvider 是**外部系统/外部答复者替
   身**；被测的桥、网关、策略、批准面、注册表、资源层、journal、驱动、沙盒全部真实
   实现，无替身替代。
10. **cargo src/bin 目录**：`src/bin/r04_t07_fixture.rs` 随 crate 构建（测试支持用途
    文档化）；与既有 CARGO_BIN_EXE 引用（config_bounds/service_health）同模式，不接
    线生产入口。

## 9. 回退

回退范围＝本 Task 获准修改（§3）：删除 mcpbridge.rs/workerrpc.rs/bin/r04_t07_fixture.rs/
r04_t07_mcp_and_workers.rs，还原 lib.rs/Cargo.toml/Cargo.lock 与 TEST_MAP T07 条目即回
到基线 26251dc92 形态。rmcp 边删除后 lingxi-spike 仍持有锁内 rmcp（D-06 不受影响）；
生产默认入口全程未接线（无回退风险）。

## 10. 证据索引

- `artifacts/rust-tauri/R04/T07-E02/run_t07_validation.sh`——复跑脚本（绝对路径口径）。
- `gates/`：rust_fmt_check / rust_clippy / workspace-test-cargo-test-workspace-locked /
  check_contracts / check_boundaries（含 selftest）/ r04_t07_acceptance_tests /
  r04_t07_unit / T01-T06 回归 / kernel_lib / r03_g07_repair_suites / r03_t08_matrix_a15 /
  r03_t08_a16_seed 各 .log。
- `e01-audit/`：E01 状态首次执行的挂死取证（进程采样摘要）——保留中断历史事实。
- 新测试文件：`rust/crates/lingxi-service/tests/r04_t07_mcp_and_workers.rs`；单测内嵌
  于两个源文件。
