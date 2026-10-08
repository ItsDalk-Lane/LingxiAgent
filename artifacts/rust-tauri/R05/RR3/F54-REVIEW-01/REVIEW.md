# F54-CLOSEOUT-01 独立审查报告(REVIEWER-F54-R1)

- 审查者:REVIEWER-F54-R1,全新空历史,未参与 F54 实现与此前任何轮。
- 日期:2026-10-08。分支 codex/rust-tauri-migration,HEAD=def860a74abd174a2297a7adfbc8799d400aabea。
- 被审候选:工作区未提交的 F54 修复批次(7 文件)+ artifacts/rust-tauri/R05/RR3/F54-CLOSEOUT-01/。
- 方法:执行者报告仅作线索;全部结论以下列本轮亲验为据。探针/复现证据在本目录
  (baseline_46leaves.txt、40leaf_stage_changes.txt、am_minimality.txt、am_deep_check.txt、
  r04map_stats.txt、r04map_class.txt、full_leaves_contracts.txt、share_leaves_sample.txt、
  verify_gate_check*.txt、audit_D_leaves_digest.txt),负例在隔离副本 /tmp/f54_review/iso 注入并还原。

---

## 一、R00 修订合法性(最高优先裁决面)

**结论:PASS——抽查 30 个 D 类叶(future/dev 工具族 5、设置页 2、连接器管理 10、权限模式 6、终端 WS 3、BODY confirm 及确认投影 4),逐叶均有任务书条款级权威依据,未发现任何虚构或牵强的承接阶段/依据。**

### 1.1 独立底账(不消费执行者提取)

- 从 `git show HEAD:docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json` 独立提取:
  HEAD 版 R04 独占叶(execution_stage_ids==["R04"])恰为 **46 叶**;工作树版剩 **6 叶**;
  40 叶阶段登记被修订。与执行者声称一致(交叉比对通过,证据 baseline_46leaves.txt)。
- 修订后分布:R04+R07+R08 × 29、R04+R07 × 6、R04+R08 × 5、R04 独占 × 6,共 46。

### 1.2 依据链逐条款核对(抽查叶的原文比对)

**R04 权威任务书(docs/rust-tauri/R04/dispatches/R04_ORCHESTRATOR_PROMPT_2026-09-30.md):**
- §3.1 必须交付清单=「Rust 工具目录和结构化参数、唯一权威工具授权与执行链、批准/撤销、
  原生 read/write/edit/exec_command、现役 write_stdin 与持续终端语义、真实进程监督、平台
  沙盒适配、MCP 与受控 worker 接口、结构化工具结果、可验证资源交付」——**不含**连接器
  管理 API、设置页/权限偏好持久化、plan mode 切换 API、CLI 斜杠入口、dev/批量工具
  (ast_edit/ast_grep/lsp/security_scan/run_code/run_tools)执行本体。
- §3.2 不提前交付:「不完成 R07 全部 CLI、Bridge、浏览器/Office 产品能力」「不迁移 R05 的
  完整模型协议/真实凭证/**OAuth 体系**」「后续业务工具尚未迁移时,不假装其已可执行;目录应
  准确表达能力和可用性」「不要求新建完整产品 UI」。
- §六:「对每个叶子区分『本阶段实际实现与验证的工具基础责任』和『R07 等阶段的完整业务交互』。
  后续义务保留原 ID、REQUIRED 与最晚阶段,不能删除或改成不适用」——本轮修订是**追加承接
  阶段**(义务链条延长、REQUIRED 保留),方向与该条款一致,非删减义务。

**承接阶段任务书(Lingxi_Rust_Tauri_Taskbooks_2026-09-23/):**
- R07-T08「配置、管理与导入导出接口」:**设置页不是迁移后的空壳。迁移 Agent 管理、
  provider/model 管理、权限、偏好、语言、工具启停、设备管理、角色卡/技能包等真实API**——
  「权限、偏好」逐字覆盖设置页 2 叶与权限模式 6 叶的管理 API 面;「等真实API」开放列举
  覆盖连接器管理 API(旧栈 server/routes/mcp.ts 第 6 行自述「HTTP surface for the MCP
  connector manager」)。
- R07 §1 目标「将现役产品能力全部接到Rust服务」+ R07-T12「按FEATURE_INVENTORY每个叶子
  功能执行用户动作→真实入口→Rust决策→执行器→存储→可见结果的检查」——dev/scan 工具族
  与连接器管理的执行本体兜底承接。
- R07-T09 CLI、移动端与LAN服务——/confirm、/reject 斜杠命令入口。
- R08-T01「逐项核对R01 API_COMPAT_MATRIX的HTTP/WS、错误、分页…」、R08-T02(工具卡/终端
  规范化事件)、R08-T07 现有界面全功能验收——终端 WS 帧面与 confirmation_resolved
  广播/HTTP 状态码投影(400/404/403)。
- 功能与所有权矩阵(03_功能与所有权矩阵.md):D03 工具目录=R04、D04 权限沙盒=R04/R10、
  D13 终端=R04、D16 内置插件=R04/**R07**、D20 非桌面入口=R07/R08——与修订方向一致。

**R01 API_COMPAT_MATRIX.json 佐证(逐条查得):** `http:GET/PUT:/api/preferences/session-permission-default`、
`http:GET/POST:/api/plan-mode`、`http:GET/POST:/api/session-permission-mode`、
`http:POST:/api/confirm/:confirmId`、`ws:GET:/ws` 全部登记为 **business_compat / retain**
(旧业务兼容面,归 R08-T01 迁移映射)。

**旧栈现役性独立核实(被划出 R04 的业务确为现役产品能力,非虚构):**
- dev/scan 工具:shared/tool-categories.ts OPTIONAL_TOOL_NAMES 含 ast_edit/ast_grep/lsp/
  run_tools/run_code;lib/tools/{lsp-tool,security-scan-tool,run-code-tool}.ts 存在。
- 连接器管理:server/routes/mcp.ts(addConnector/updateConnector/connectorAction/readResource/
  OAuth callback),core/mcp/manager.ts readResource→resources/read。
- 权限模式/偏好:core/session-coordinator.ts(5654/5820/5865 行 setSessionPermissionMode/
  setPlanMode)、core/engine.ts(3063-3076)、core/preferences-manager.ts(294)。
  server/index.ts:1081 `mode ? engine.setSessionPermissionMode(mode) : engine.setPlanMode(!!enabled)`
  与 A336F79E964D 叶 then「无 mode 时由 !!enabled 调 setPlanMode」逐字对应。
- 终端 WS:server/routes/chat.ts:721 createTerminalWsBridge、2296 sendSnapshot、2302 sendTail。
- 斜杠命令:core/slash-commands/bridge-commands.ts:161 「/reject <确认ID>」。

### 1.3 逐叶依据比对表(抽查叶,条款原文比对结论)

| 族 | 叶(修订) | 承接依据 | 结论 |
|---|---|---|---|
| future/dev 工具 | 96DD1FF9E9D5(ast_edit)→R04+R07 | R04 §3.1 四基础不含;§3.2 不假装可执行;R07-T05/T12;旧栈 OPTIONAL 现役 | 有据 |
| future/dev 工具 | 196D50D8DD6E(ast_grep)→R04+R07 | 同上 | 有据 |
| future/dev 工具 | 2D896560381E(lsp)→R04+R07 | 同上 | 有据 |
| future/dev 工具 | CD5D7FC02D8E(security_scan)→R04+R07 | 同上 | 有据 |
| future/dev 工具 | 0818574ABD43(run_tools)→R04+R07 | 脚本编排本体=R07;R04 份额=网关逐调用权限/收据机制(§3.1 交付且 T02 条款「开发入口转统一请求」) | 有据 |
| 设置页 | 8D3CEB6133E1→R04+R07+R08 | R07-T08「权限、偏好」逐字;R08-T07 设置回归 | 有据 |
| 设置页 | 5E61048F19B9→R04+R07+R08 | 同上 | 有据 |
| 连接器管理 | 0199B843D759(保存+异步启动)→R04+R07+R08 | R07-T08 等真实API+R07-T12;OAuth 类另有 §3.2 直接排除 | 有据 |
| 连接器管理 | 2D194C1684BC(删除)→R04+R07+R08 | 同上 | 有据 |
| 连接器管理 | 18EFB2D9D5FD(启动)→R04+R07+R08 | 同上 | 有据 |
| 连接器管理 | 21B3F4DC9140(刷新)→R04+R07+R08 | 同上 | 有据 |
| 连接器管理 | F4DA2AFCB72B(配置更新)→R04+R07+R08 | 同上 | 有据 |
| 连接器管理 | 240E200EF440(Agent 绑定)→R04+R07+R08 | 同上 | 有据 |
| 连接器管理 | 04A6A2BD1547(resources/read)→R04+R07+R08 | R04 份额=内容同步入目录机制,laterShare 明示「R04 无 resources/read 端点」(诚实);本体=R07-T08(readResource 路由)+R07-T12 | 有据 |
| 连接器管理 | C70E819F8DA4(OAuth 凭证清除)→R04+R07+R08 | R04 §3.2「不迁移 OAuth 体系」直接排除;R07-T08 | 有据 |
| 连接器管理 | E7F852F9BF60(OAuth 等待取消)→R04+R07+R08 | 同上 | 有据 |
| 连接器管理 | 2DA782C5C7B9(OAuth 登录流程)→R04+R07+R08 | 同上 | 有据 |
| 权限模式 | 25C4FF66FEE5/0B669B30C854/D01766AF4475/67256417FB2B/A336F79E964D/BC2FBD618278 →R04+R07+R08 | R04-T03 交付运行时三档裁决(份额钉 permission-face/sup01);管理 API 投影+偏好持久化=R07-T08 字面;HTTP 面=R08(matrix: business_compat/retain 佐证) | 有据 |
| 终端 WS | 4C35E6AEC6F7/CE69063550AA/CFD9F02BC6AA →R04+R08 | R04 §3.1 交付 write_stdin/持续终端语义本体(份额钉游标/快照/关闭案例);WS 帧协议=客户端接入面(matrix ws:GET:/ws business_compat)→R08-T01/T02 | 有据 |
| BODY confirm | 75C0AE981505(/confirm)→R04+R07+R08 | ConfirmStore 决策=R04-T03 到期(份额钉 approval-answer-executes-once/duplicate-idempotent,语义贴近);斜杠入口 CLI=R07-T09、客户端=R08 | 有据 |
| BODY confirm | CD1524CC7DC3(/reject)→R04+R07+R08 | 同上 | 有据 |
| 确认投影 | EC033184BA37/EF43ADCE19A1 →R04+R08 | ConfirmStore 标记=R04 份额;confirmation_resolved 广播与 400/404/403 投影=旧 API 兼容面 R08-T01 | 有据 |

同族根因排查:剩余 10 个未抽查 D 叶(state/apps/defer 4 叶、斜杠另 2 叶中未列者等)与上表
同构(管理 API/客户端面),未发现孤立的无据修订。

## 二、修订最小性

**结论:PASS——程序化独立复算,仅 execution_stage_ids(或配套生成元数据)变化。**

- FSA:两版各 736 场景,ID 集合一致;46 叶中除 execution_stage_ids 外全部字段
  (given/when/then/assertions/task_ids/due/status 等)逐字段**零变化**;46 叶之外 690 叶
  **零字段变化**;顶层 schema_version/task_id/tested_sha/features/legacy_feature_id_map 一致。
- ACCEPTANCE_MAP.json:952 场景 ID 一致,场景非阶段字段差异数=**0**;阶段修订后分布
  (29/6/5)与 FSA 一致,总追加 69 个阶段 ID;generated_at/basis.head/input_sha256/
  results digest 变化为生成器重跑的自然产物;其余全部段(tasks/features_index/tests/
  blockers 等)一致。
- BLOCKERS.md:仅基准 HEAD 行(8b153b10→def860a74)。
- stage_map.rs 镜像:R04_LEAF_COUNTS=(46,9,69)→(6,49,69);R04_MATRIX_CASES 56→58
  (+mcp-describe-no-side-effect、+mcp-search-honest-availability);r04_case_expect +2;
  均与 R00 修订和图一致(cargo test -p xtask 121 全绿独立复跑证实)。
- rust/crates/xtask/src/verify.rs:**0 行 diff**(且 mtime 2026-10-06,未被触碰)。

## 三、full 叶真实性(6 叶)

**结论:PASS——逐叶亲读 r04_t08_tool_matrix.rs 案例实现与生产源码,测试断言真实证明
R00 原断言的业务语义;2 项证明面强度观察见 findings。**

| 叶 | 类 | 断言→案例 | 亲验结论 |
|---|---|---|---|
| 8A3C87812B4F(exec/终端链) | A | exec-real-chain / stdin-continuation / cancel-cleanup / failed-never-dispatched | 全部为真实进程内断言(record_case assert_eq(actual,expect),非虚过);真实 PTY(marker 双副本落定栅栏)、真实进程树取消(live_handles 清空)、越界写 prepare 拒绝 dispatched=false 且无文件落地。断言0 的「输出与退出码正确」叶级钉只断言收据 Succeeded——同门禁 r04_t05_process_tools A10 有 `exited (exit code 7)` 强断言(独立核实,rust_test_workspace 命令覆盖),整体覆盖成立。见 OBS-1 |
| C90F42576683(write_stdin) | B | 换绑:断言1→terminal-close-stops-terminal(close 后 late write 诚实失败 "not running");断言3→foreign-writes(expect=0 跨会话拒绝) | 换绑方向正确。HEAD 版实证存在错绑:断言1 钉 foreign-writes(跨主体≠退出后)、断言3 钉 a15-missing-claim-refused(worker 产物缺失,与终端完全无关)——F54 审计的 mismatch 描述与事实吻合 |
| 15AD6ED13B4D(mcp_call) | A | full-chain(真实 run 链收据 dispatched)+permission-face(operate 允/ask 需批/read_only 拒) | 案例经生产 ToolInvocationGateway 与真实 MCP stdio 服务(fixture 进程);断言1 的「输入无效/下游失败」分支无专属钉,见 OBS-2 |
| 483E461BB59D(route-consistency) | C | route-consistency 补 MCP 目标腿 | diff 亲读:新增 mcp_direct(gateway_call 直调)与 mcp_run(run_chain 目录链)两路,同 MCP 目标比较授权与执行一致——断言0「同目标直调与目录调用权限一致」的字面证明面补齐,修复真实。断言2「目标不存在/名称歧义」仍由 generation-refusals 部分覆盖(遗留,见 OBS-2) |
| 8BCB8A749864(describe) | C | 断言1 换钉 mcp-describe-no-side-effect(describe 前后 catalog_generation 守恒) | 新案例经真实生产入口:h.registry 为 lingxi-kernel ToolRegistry(toolcatalog.rs:1593),describe_full/snapshot 为其 pub 方法(1686/1734);HEAD 版断言1 锚 a16-history-preserved(历史收据保留≠描述无副作用)为虚假绑定,实证。代次守恒证明「目录状态无副作用」,真实(见 OBS-5) |
| CEDC75156D33(search) | C | 断言2 换钉 mcp-search-honest-availability(无命中空+停用项标 Disabled) | set_availability 为生产 API(toolcatalog.rs:1845);案例断言停用后搜索仍列出但 availability=Disabled(不冒充可执行)——与断言「禁用或不可用目标不应被提供为可执行结果;无命中为空」同-。HEAD 版锚 a16-alias-route-covered(别名路由≠搜索诚实),虚假绑定实证 |

生产组件真实性:harness 的 registry/gateway/supervisor 均为生产类型
(ToolRegistry/ToolInvocationGateway/ProcessSupervisor),register_mcp_server 为生产
mcpbridge.rs:1175 入口;测试替身仅 Provider(外部响应)与 fixture MCP server,被测
网关/目录/策略为真实现——符合「替身只产生外部响应」边界。

## 四、D 类 share 叶份额真实性(抽查 12 新+9 既有=21 叶)

**结论:PASS。**

- 40 个新 share 叶全部带非空 laterShare,载明承接阶段+任务书条款/旧栈文件+REQUIRED 语义
  (抽查 12 叶逐条核过:连接器管理 3、dev 工具 2、run_tools 1、终端 WS 1、设置页 1、
  权限模式 2、confirm 1、OAuth 1)。laterShare 非空话:均含「=R07-T08/R07-T09/R08+R08」
  具体条款或旧栈路径。
- 无「份额案例是虚假 full 换皮」:份额文本全部明确「R04 份额=机制面」(目录诚实/网关
  逐调用权限/收据/ConfirmStore 决策/游标增量),未声称证明完整业务断言;对照 M-01
  (HEAD 图)的 same 案例曾被用于支撑 full 分类——分类与声称同时被纠正。
- 钉住案例真实:future-tool-shape-*-discoverable-not-callable 案例亲读(matrix_future_tools_
  honest_in_catalog):生产 registry 注册 Future 形态→search 可发现→describe 标注 Future→
  gateway prepare TargetNotCallable 拒绝(零派发),与 R04 §3.2「目录应准确表达能力和
  可用性」一致;网关/收据/ConfirmStore/终端游标案例均为真实进程内断言。
- 9 个既有 R04+R06 share 叶:图内条目与 HEAD 版**逐字节一致**(程序化比对 0 diff),
  R00 登记未被触碰(690 叶零变化包含之)。

## 五、负例复核(独立复跑 3 项,隔离副本注入)

**结论:PASS——3 项均复现,变异有效、命中预期检查、还原后正对照绿。**

| 项 | 注入 | 结果(本轮亲跑) | 与执行者记录 |
|---|---|---|---|
| N1 变体A | 生成器决策表把 96DD1F(ast_edit)改 full+1 组案例 | `AssertionError: R00-T02-LA-96DD1FF9E9D5`(生成器 F54 独占不变量:FULL 叶须 R04 独占,该叶已修订 R04+R07),exit=1 | 同路径 |
| N1 变体B | 直接改图:96DD1F→full+仅 1 组案例契约 | 解析层拒绝 `full_original_behavior requires originalAssertionCases for every R00 assertion` | 同层(文案随注入结构略异) |
| N1 变体C | 按 full 正确 schema 自洽注入(assertionContract.cases 3 案例+originalAssertionCases 3 组+FSA 回退 R04 独占) | F54 计数断言拒绝:`the full/share/deferred split drifted from the registered decision: left: (7, 48, 69), right: (6, 49, 69)`——与执行者记录**逐字一致** | 一致 |
| N4 | FSA 删 8A3C87812B4F 一条原始断言(4→3),案例组数不变 | `AssertionError: R00-T02-LA-8A3C87812B4F: 4 case groups for 3 original assertions`,exit=1 | 一致 |
| N5 | 双台账(FSA+AM)给 2D194C1684BC 追加 R99 | `LEDGER-ERROR STAGE-PREFIX scenario=R00-T02-LA-2D194C1684BC unknown execution stage 'R99'`,`LEDGER_INVALID errors=59 checks=14976`,exit=1 | 一致(errors=59 checks=14976) |
| 正对照 | 还原注入后 | xtask 121 passed / 0 failed | — |

负例说明:STAGE-PREFIX 检查对象是 ACCEPTANCE_MAP(非 FSA);首次仅注入 FSA 不触发,
按执行者「两本台账」描述双注入后复现——执行者负例记录准确(见 OBS-4)。
无关编译错误未冒充负例:所有注入均命中目标断言而非编译失败。

## 六、正式门禁亲核

**结论:PASS——绑定当前工作树候选;关键字段逐项亲核+定向测试独立复跑。**

verify-R04/verify-stage-result.json 亲验:
- overall=**PASS**;stage=R04;testedSha=**def860a74**(当前 HEAD);worktreeDirty=true;
  toolchainChannel=1.98.1;05:23:38→06:19:27 UTC(55.8 分钟,与声称一致)。
- candidateSourceBinding:schema=lingxi-candidate-source-binding-v1,**stable=true**,
  before/after digestSha256 一致(e955b1a9…),fileCount 67287 前后一致,
  **finalChangedPathBytesHex=[]**(运行期间零并发写);testedShaAtEnd=def860a74;
  runnerSourceBinding=PASS。
- 8 命令 key 逐一核对:rust_test_workspace(893s)/r04_tool_matrix(340s)/rust_fmt(38s)/
  rust_clippy(45s)/check_contracts(35s)/check_boundaries(36s)/r03_regression_gate(1818s)/
  r04_rr1_repair_suites(64s)——全 exit=0 status=PASS,missingEvidence 空。
- 叶表:expectedFromR00Ledger=124=declaredInStageMap;**pass=55 / fail=0 / deferred=69 /
  blocked=0**(55=6 full+49 share);commandsNotPassing 无;24/24 场景 PASS。
- 运行内生产者 R04_MATRIX/leaf-cases.json:**58 案例全 ok=true**,含新案例
  mcp-describe-no-side-effect、mcp-search-honest-availability 与扩展的
  matrix-route-consistency(actual==expect)。
- 绑定当前工作树的时序证据:全部被测文件 mtime(12:36–13:16 本地)早于门禁开始
  (13:23:38 本地),门禁后无再写入;binding before==after 证明运行期间稳定。

独立复跑(主树,锁 1.98.1):
- `cargo test -p xtask`:**121 passed / 0 failed**(含 9 项 r04_* 镜像测试:124 叶分裂、
  F54 独占/计数断言、案例集与 expect 双重镜像)。
- `cargo test -p lingxi-service --test r04_t08_tool_matrix` 定向:
  mcp_mechanism_face_share_cases(含 2 新案例)ok、terminal_family_share_cases ok、
  matrix_tools_x_permission_x_entry_consistency(含 F54 MCP 腿)ok;record_case 为
  assert_eq 进程内断言,非虚过。
- r04_rr1_f05_output_integrity 套件抽查:7 passed。
- 未重跑 55 分钟完整门禁:绑定、摘要、叶表、案例记录亲核一致且无可疑点,符合任务书
  允许的验证深度。

## 七、越界检查

**结论:PASS——除有据 R00 修订外无任何改写原要求取得 PASS 的迹象。**

- verify.rs 0 行 diff(F25 校验器语义原样);R02/R03/R05 图 0 行 diff(git diff 复核)。
- 9 既有 share 叶与 69 deferred 叶:图内条目与 HEAD 程序化比对**零变化**;其在 FSA/AM
  的登记亦零变化(690 叶零字段变化)。
- 46 叶原始断言(then/acceptance_assertions)逐字段**零删改**;图内 r00Assertions 与
  HEAD FSA 断言逐叶一致(0 不一致),r00ExecutionStageIds 与当前 FSA 逐叶一致(0 不一致)。
- 生成器 diff(330/278 行):无函数/类增删;变更=决策表重写+F54 不变量(545-561 行:
  FULL 须 R04 独占、SHARE 须有 later 阶段 owner、len(FULL)==6)——不变量方向为**收紧**。
- 无新增无关实现:tool_matrix.rs 仅 3 处 F54 相关改动(route-consistency MCP 腿、2 新案例);
  无生产 src 代码改动;未重引入旧 Node/Pi 内核职责(纯台账/图/测试)。
- M-01 推翻的正当性:HEAD 图实证存在虚假绑定(96DD1F 断言0 用 edit-real-chain 冒充
  ast_edit 语法替换、8BCB8 断言1 用 a16-history-preserved 冒充无副作用、C90F 断言3 用
  a15-missing-claim-refused 冒充终端拒绝)——F54 纠正有事实依据,且方向是从「虚假 full」
  收紧为「诚实 share+登记修订」,不是放宽。

## 八、R06 准入输入

**结论:PASS。**

- R06 所依赖的 R04 交接接口(工具目录/schema/授权/审批端口/结果语义)在本次修订中
  **零生产代码变化**:8 命令、24 场景、工具机制面钉住案例(58≥56)全部保留;R04 图
  commands/scenarios 与 HEAD 一致(仅 supplementalLeafScenarios 分类与 coverageNote 变化)。
- R05 已验收证据不受影响:R05 图 0 行 diff;FSA/AM 中 R05 绑叶属「46 叶之外 690 叶」
  零变化;AM 的 tasks/tests/features_index 等段一致。
- 修订后的 R00 台账对 R06+ 阶段是**更准确**的输入(连接器管理/设置页/CLI 面在 R07/R08
  有明确承接登记),不产生新的交接缺口。

---

## Findings

无阻断级(FATAL/MAJOR)finding。以下为观察级(不阻断本轮 PASS,供后续轮次参考):

### OBS-1(full 叶叶级钉的证明面强度)
- 严重度:OBSERVATION
- 证据:8A3C87812B4F 断言0「输出与退出码正确」与 C90F42576683 断言2「状态和退出码一致」
  的叶级钉案例(tool-exec-command-real-chain)仅断言 journal 收据 Succeeded,未在案例内
  比较 stdout 内容与退出码值。
- 影响与缓冲:同门禁 rust_test_workspace 内 r04_t05_process_tools A10 有强断言
  (`exited (exit code 7)` 文本+状态一致,exectools.rs 非零码写 "Command exited with code N"
  且 Exited{code}),语义由门禁整体覆盖;该钉法 M-01 即存在,F54 未引入也未加剧。
- 建议:后续轮可把 exit-code 断言直接纳入叶案例,或在 stageShare 文本注明依赖 T05 套件。

### OBS-2(断言分支的叶级覆盖缺口,M-01 遗留)
- 严重度:OBSERVATION
- 证据:483E461BB59D 断言2 的「目标不存在/名称歧义」与 15AD6ED13B4D 断言1 的「输入无效/
  下游失败」分支无叶级专属案例(generation-refusals/permission-face 仅覆盖目录过期与
  权限档);网关 TargetNotRegistered/TargetNotCallable 拒绝在其他案例(uninstall-holes、
  future-tool-shape)有覆盖但未钉入该叶断言组。
- 影响:full 叶的「每断言有真实案例」以案例组为单位成立,分支级覆盖依赖同图其他案例。
- 建议:后续可为「目标不存在/歧义」补一专案例,或在该断言组追加既有 TargetNotRegistered 腿。

### OBS-3(API_COMPAT_MATRIX 对 /api/mcp 的基线盲区)
- 严重度:OBSERVATION(基线问题,非本候选缺陷)
- 证据:R01 API_COMPAT_MATRIX.json httpRoutes 无任何 /api/mcp 前缀条目,而旧栈
  server/index.ts:130 经 composition/open-root.ts 挂载 createMcpRoute 到 /api(连接器
  管理整组路由)。矩阵内 chat.ts 亦有 "unmounted?" 注释,提示 R01 扫描器缺陷。
- 影响:R08-T01「逐项核对R01 API_COMPAT_MATRIX」字面不会核到连接器管理路由,存在
  迁移映射漏项风险;连接器叶的 R07/R08 承接依据因此主要落在 R07-T08 开放列举+R07-T12
  逐功能闭环(充分但非逐字)。
- 建议:R07/R08 执行轮应把「连接器管理路由不在旧 API 兼容映射清单」列为显式检查项。

### OBS-4(负例 N5 的注入面)
- 严重度:OBSERVATION
- 证据:r00_t07_validate_ledger.py 的 STAGE-PREFIX 检查读取 ACCEPTANCE_MAP.scenarios;
  仅注入 FSA 不触发,须双台账同时注入(执行者负例描述准确,本轮按其描述复现)。
- 影响:无(校验器行为正常)。

### OBS-5(新案例证明面可选增强)
- 严重度:OBSERVATION
- 证据:mcp-describe-no-side-effect 以 describe 前后 catalog_generation 守恒证明「无副作用」
  (目录状态面);未同时断言 MCP server 流量计数不变(fixture server 有 connect_count 可用)。
- 影响:无(代次守恒是目录无变更的真实可观察证据)。

---

## 最终 VERDICT

**PASS**

依据(全部本轮亲验):
1. R00 修订合法性:抽查 30 个 D 类叶逐叶比对 R04 §3.1/§3.2/§六与 R07-T08/T09/T12、
   R08-T01/T02/T07 条款原文,全部有据;旧栈现役性独立核实;无虚构/牵强。
2. 修订最小性:FSA 40 叶仅阶段字段变化、690 叶零变化、断言零删改;AM 场景非阶段字段
   零差异;verify.rs 0 行;R02/R03/R05 图 0 行;9+69 既有叶零变化。
3. 6 full 叶:亲读案例实现与生产源码,证据真实(生产 ToolRegistry/Gateway/mcpbridge);
   M-01 虚假绑定实证存在,换绑/补案例方向正确。
4. 21 share 叶抽查:份额文本诚实(机制面+承接条款),无虚假 full 换皮、无空话 laterShare。
5. 负例:N1(三路径)/N4/N5 独立复现,与执行者记录一致;正对照还原后 121 全绿。
6. 门禁:verify-stage-result.json 关键字段逐项亲核,绑定当前工作树(binding stable+
   mtime 时序),8 命令全 PASS,叶表 55/0/69,58 案例全 ok;定向测试独立复跑全绿。
7. 越界检查:无改写原要求、无无关实现、无旧内核回归;F54 不变量方向为收紧。
8. R06 准入输入与 R05 已验收证据不受影响。

抽查覆盖统计:
- R00 修订依据逐叶比对:30 叶(D 类,families 全覆盖:future/dev 5、设置页 2、连接器 10、
  权限 6、终端 WS 3、confirm/投影 4)+ full 6 叶 = 36 叶亲核依据。
- full 叶亲读测试实现:6/6;share 叶份额抽查:12 新+9 既有=21。
- 负例独立复跑:3 项(N1 含 3 个注入变体)+正对照。
- 结构性独立复算:46 叶提取、40 叶阶段变更明细、FSA/AM 最小性、图分类计数(6/49/69)、
  图↔FSA 双向一致性、9+69 叶零变化、镜像测试(cargo test -p xtask 121)。
- 门禁字段亲核:overall/binding/8 命令/叶表/24 场景/58 案例/时序绑定。
