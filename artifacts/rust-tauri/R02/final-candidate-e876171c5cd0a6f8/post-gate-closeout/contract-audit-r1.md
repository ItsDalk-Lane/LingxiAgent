# R02 最终收口合同审计报告（R1）

审计者：`R02-CONTRACT-AUDITOR-R1`（只读子代理，未修改任何仓库文件；输出仅写 `/tmp/r02-final/`）
日期：2026-09-28｜仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`｜分支 `codex/rust-tauri-migration`｜HEAD `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`（工作区干净）

## 0. 审计依据与方法

- **现行最高指令**：R02 Gate 只须证明 R02 真正承担的 primitive/service obligation——认证、资源范围、WS/HTTP 原语、服务端语义（含**管理面服务端语义**）、存储、事件、备份恢复、日志资源上限、独立交付。CLI/UI/客户端完整用户可见行为、Agent loop、真实模型调用、完整工具执行、Ctrl+C 完整 Agent run 行为属 R03–R07。R17 轮“把 R07-T09 完整客户端行为提前纳入 R02”的授权其**门禁含义被推翻**；提前实现不删除、继续不破坏，但不得作为 R02 PASS 前置。
- 核对输入：R00 两账本原件（34 叶 `execution_stage_ids=[R02,R07]`、`task_ids=[R02-T03,R07-T09]`）、R02 任务书（8 任务/16 基础验收，T03=HTTP/WS 认证与资源范围）、R07-T09 任务书（CLI/移动端/LAN 服务）、现行 `R02.json`（19 命令/16 场景/34 supplemental 叶）、`verify.rs`+`stage_map.rs` 判定链、8 个生产者脚本、`R02_ACCEPTANCE_LEDGER.json`/R17 闭合矩阵/R18 handoff。
- 不继承旧报告 PASS：以当前 stage map 图钉 + 当前生产者真实可执行内容为准；全部图钉案例名已逐一与生产者脚本输出核对（a05/a06/ws 案例对应 `r02_t03_auth_matrix.sh`+`r02_t03_ws_probe.py`；management 63 例对应 `r02_management_leaf_matrix.py` EXPECTED_CASES+TLS；static 9 例、client 7 例、cli_sessions 10 例、cli_rust 9 例均存在）。

## 1. 结构性发现（先于逐叶结论）

- **F1（blocking）**：叶 `R00-T02-LA-1B09760C2B1C`（CLI serve）`assertionContract.producerCommand = evidenceCommandRefs[0] = "supplemental_cli_rust_matrix"`，但 R02.json 的 19 个 commands 中**未注册该命令**。`stage_map.rs:616` 解析即硬错（unknown producer command），`verify.rs:1539` 亦会 internal error——**当前阶段图无法加载，verify-stage R02 一次也跑不起来**。脚本 `scripts/rust-tauri/r02_cli_rust_matrix.py` 已存在、可产出全部 9 个图钉案例（`cli-rust-cases.json`）。该缺陷由 HEAD 提交引入（新增了叶定义与 `supplemental_cli_sessions_matrix` 命令，漏注册 cli_rust）。
- **F2（medium）**：thinking-level 3 叶（27/28/29）r02Share 文本“路由不在 R02 现行面”**已过时**：`/lingxi/v1/session-thinking-level` 路由现存在（`rust/crates/lingxi-service/src/management.rs:583`）。但取值语义依赖模型状态权威（无模型配置时 503 `model_state_unavailable`，`rust/crates/lingxi-service/tests/auth_matrix.rs:2133-2166`），完整原行为须等 R05/R06，且现有任何叶案例生产者都没有 thinking-level 案例。
- **F3（low）**：文档计数漂移：LEDGER `basis_kinds` 计数（route4/protocol7/auth21/conflict2）与 R18 handoff“20 完整/14 局部”为旧快照；现行图为 full23/protocol5/auth5/route1，`supplementalSemanticLimit` 自述“22 full”亦滞后（serve 叶已升 full）。终局需一次性对齐；不改变叶集合与 R00 镜像。
- **F4（info）**：移动端 workbench 引导路由在现行 Rust 服务确实不存在（全路由清单核对），叶 26 表述仍准确。

## 2. 34 叶逐项责任矩阵

图例：`当前 Gate` = 现行 R02.json 的 basisKind + 图钉案例（`=期望值`）；`判定` = 是否越界；`重判定` = 按现行指令重新归属后的状态。所有叶的 `r00TaskIds=[R02-T03,R07-T09]`、`r00ExecutionStageIds=[R02,R07]`、`r00LedgerStatus=SPECIFIED_NOT_EXECUTED`（下文不再重复）。


### A 组｜协议/原语份额（6 叶）：图钉内容在 R02 份额内，roll-up 语义越界（被永久 BLOCKED）

#### 1. `R00-T02-LA-D3710D637C19` | `F-D20-CLI-CLI-CHAT-D3710D`

- **原始用户可见行为（r00Then）**：选择或创建会话后通过 WS 提交消息，终端流式显示回复和工具进度
- **R02 真正承担的 primitive/service obligation**：认证 WS/execute 提交原语与有序事件流协议面：owner 持 token 可 execute(200) 并拿到 runId；无凭据 401；ws-ticket 签发后合法 Origin 升级 101 并完成 session_read；A09/A10 事件快照+游标、A15 全链由 evidenceCommandRefs 强制通过。
- **R03–R06 obligation**：R03（run/attempt/取消恢复）、R04（工具执行进度）、R05（真实模型流式回复）——终端流式显示的真实内容不在 R02。
- **R07 obligation（保留，防丢失）**：R07-T09 终端聊天客户端：会话选择/创建 UI、流式显示回复与工具进度、错会话帧不显示、Ctrl+C 只取消当前 stream、连接/身份失败非零退出。
- **当前 R02 Gate 要求**：basisKind=`protocol_basis`｜producer=`a05_a06_auth_matrix`｜图钉：a05-positive-control-owner-execute=200; a05-no-credential-execute=401; ws-ticket-legit-origin-upgrade=101; ws-session-read-owner=1｜refs：a05_a06_auth_matrix, a09_a10_events_matrix, a15_full_chain
- **是否越界**：**越界（仅 roll-up 语义）**——图钉案例全部是服务器侧原语（execute 200/401、ws-ticket 101、ws-session-read），未要求客户端行为；但 verify.rs:675-692 因 r00ExecutionStageIds 含 R07 把它永久 BLOCKED——等于要求 R02 先证 R07 客户端行为才能解锁，属门禁 roll-up 越界。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：a05-positive-control-owner-execute=200; a05-no-credential-execute=401; ws-ticket-legit-origin-upgrade=101; ws-session-read-owner=1｜refs：a05_a06_auth_matrix, a09_a10_events_matrix, a15_full_chain｜现有图钉即 R02 份额，无需改案例；只改 basisKind/roll-up 语义即可 PASS。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（份额案例已可由 r02_t03_auth_matrix.sh + r02_t03_ws_probe.py 真实执行）。
- **后续 Stage 保留要求**：[R03-R05] 真实 agent loop/模型流式/工具进度事件（本叶原 then 的内容生产方）；[R07] 终端聊天完整客户端行为（流式显示、Ctrl+C 仅取消当前 stream、非零退出、错会话帧不显示）；R07 须重验不得复用 R02 原语证据充当客户端行为

#### 2. `R00-T02-LA-3B86332B042C` | `F-D20-CLI-CLI-CONTINUE-3B8633`

- **原始用户可见行为（r00Then）**：解析目标会话后恢复其历史并可继续提交，终端显示该会话回复
- **R02 真正承担的 primitive/service obligation**：认证会话身份与历史读取面：sessions 按 principal 作用域（owner 200）；跨主体读 403；伪造 sessionId 404 且无副作用——服务端“不悄悄新建或串会话”的份额即此。
- **R03–R06 obligation**：R06（会话/历史语义权威）；继续提交依赖 R03。
- **R07 obligation（保留，防丢失）**：R07-T09 CLI continue 命令：目标会话参数解析、历史恢复显示、不存在/跨权限时的用户可见拒绝且不悄悄新建。
- **当前 R02 Gate 要求**：basisKind=`protocol_basis`｜producer=`a05_a06_auth_matrix`｜图钉：a05-sessions-list-owner=200; a05-cross-principal-read=403; a05-forged-session-id=404｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（仅 roll-up 语义）**——图钉（owner-list 200 / cross-principal 403 / forged-session-id 404）全为服务端份额；被 verify.rs 阶段冲突分支永久 BLOCKED，属 roll-up 越界。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：a05-sessions-list-owner=200; a05-cross-principal-read=403; a05-forged-session-id=404｜refs：a05_a06_auth_matrix｜现有图钉即 R02 份额，直接沿用。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无。
- **后续 Stage 保留要求**：[R06] 会话历史/恢复语义权威；[R07] CLI 续接完整命令行为（解析、恢复显示、拒绝显示、不悄悄新建）

#### 6. `R00-T02-LA-EEC1A2A5CD04` | `F-D20-CLI-CLI-STATUS-EEC1A2`

- **原始用户可见行为（r00Then）**：显示 URL、版本、Studio、Agent、模型和认证来源
- **R02 真正承担的 primitive/service obligation**：/lingxi/v1/health + /lingxi/v1/me + 本地 token 来源机制：无凭据 /me 401；伪造 principal 头被忽略仍按真实凭据 200；loopback Host 200；CLI 依 query token 本机读取 local token（a06-cli-query-token-local）。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07-T09 CLI status 命令：URL/版本/Studio/Agent/模型/认证来源的终端显示、服务不可达诊断、身份读取失败不伪造认证来源。
- **当前 R02 Gate 要求**：basisKind=`protocol_basis`｜producer=`a05_a06_auth_matrix`｜图钉：a05-no-credential-me=401; a05-me-with-forged-header=200; a06-localhost-host-ok=200; a06-cli-query-token-local=200｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（仅 roll-up 语义）**——图钉全为服务端原语；被阶段冲突分支永久 BLOCKED。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：a05-no-credential-me=401; a05-me-with-forged-header=200; a06-localhost-host-ok=200; a06-cli-query-token-local=200｜refs：a05_a06_auth_matrix｜沿用现有图钉。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无。
- **后续 Stage 保留要求**：[R07] CLI status 完整显示与诊断行为（含 Studio/Agent/模型来源与失败不伪造）

#### 7. `R00-T02-LA-D1BEE19A95BB` | `F-D20-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-SERVICE-CONNECTION-D1BEE1`

- **原始用户可见行为（r00Then）**：端口/短期令牌传给 renderer 后 HTTP/WS 连接可建立
- **R02 真正承担的 primitive/service obligation**：本地 token 引导与 ws-ticket 短期凭证签发原语：owner 签发 200；无凭据 403；合法 Origin 票据升级 101——renderer 接线所依赖的全部服务端原语。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07/R09 桌面壳：端口/短期令牌传 renderer、HTTP/WS 连接建立、服务重启重取端口/令牌、令牌不作用户可见设置。
- **当前 R02 Gate 要求**：basisKind=`protocol_basis`｜producer=`a05_a06_auth_matrix`｜图钉：a05-ws-ticket-issue-owner=200; a05-no-credential-ws-ticket=403; ws-ticket-legit-origin-upgrade=101｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（仅 roll-up 语义）**——图钉全为服务端原语；被阶段冲突分支永久 BLOCKED。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：a05-ws-ticket-issue-owner=200; a05-no-credential-ws-ticket=403; ws-ticket-legit-origin-upgrade=101｜refs：a05_a06_auth_matrix｜沿用现有图钉。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无。
- **后续 Stage 保留要求**：[R07/R09] 桌面壳 renderer 端口/令牌接线、重启重取、令牌不出现在用户可见设置（R17 已做静态接线但端到端属后续阶段）

#### 8. `R00-T02-LA-5816DA563ED8` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-WS-AUTH-WS-TICKET-WEBSOCKET-5816DA`

- **原始用户可见行为（r00Then）**：读取已认证主体并签发绑定连接类型及 /ws 的短期票据；无主体返回 403
- **R02 真正承担的 primitive/service obligation**：WS 票据签发的完整服务端语义（本叶原 then 本身就是服务器行为）：读取已认证主体签发绑定连接类型及 /ws 的短期票据；无主体 403；scope 不足 403；票据重放 401；过期 401。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07-T09 客户端票据消费与端到端 WS 连接行为（完整入口）。
- **当前 R02 Gate 要求**：basisKind=`route_basis_present_static`｜producer=`a05_a06_auth_matrix`｜图钉：a05-no-credential-ws-ticket=403; a05-ws-ticket-issue-owner=200; a05-ws-ticket-insufficient-scope=403; ws-ticket-replay=401; ws-ticket-expired=401｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（仅 roll-up 语义）**——route_basis_present_static 的图钉已覆盖签发语义全部正负路径（403/200/scope/replay/expired），全为服务端；但 roll-up 落入阶段冲突分支永久 BLOCKED，且 basisKind 文本还停留在“静态可见、未执行”。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：a05-no-credential-ws-ticket=403; a05-ws-ticket-issue-owner=200; a05-ws-ticket-insufficient-scope=403; ws-ticket-replay=401; ws-ticket-expired=401｜refs：a05_a06_auth_matrix｜沿用现有图钉；basisKind 文本应同步改为已执行口径。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无。
- **后续 Stage 保留要求**：[R07] 客户端票据消费、连接类型绑定消费与端到端 WS 入口行为

#### 12. `R00-T02-LA-D2657E4AB5FF` | `F-D20-UI_BEHAVIOR-UI-BEHAVIOR-SERVICE-CONNECTIVITY-D2657E`

- **原始用户可见行为（r00Then）**：连接状态、失败原因与重连结果在 UI 可见
- **R02 真正承担的 primitive/service obligation**：/health 与显式拒绝/失败码面（UI 判定连接状态的服务器依据）：foreign Host 403、evil Origin 403、evil Origin + 有效 token 仍 403、loopback Host 200。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07/R08/R09 UI 连接状态、失败原因与重连结果的完整桌面行为（断开显示与重试、不静默假在线）。
- **当前 R02 Gate 要求**：basisKind=`protocol_basis`｜producer=`a05_a06_auth_matrix`｜图钉：a06-foreign-host-health=403; a06-evil-origin-health=403; a06-localhost-host-ok=200; a06-evil-origin-with-valid-token=403｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（仅 roll-up 语义）**——图钉全为服务端份额；被阶段冲突分支永久 BLOCKED。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：a06-foreign-host-health=403; a06-evil-origin-health=403; a06-localhost-host-ok=200; a06-evil-origin-with-valid-token=403｜refs：a05_a06_auth_matrix｜沿用现有图钉。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无。
- **后续 Stage 保留要求**：[R07/R08/R09] UI 连接/断开/重连完整行为，不静默假在线（R17 桌面静态接线已保留，端到端归后续）


### B 组｜CLI serve 启动语义（1 叶）：内容不越界，阶段图结构性损坏（生产者命令未注册）

#### 4. `R00-T02-LA-1B09760C2B1C` | `F-D20-CLI-CLI-SERVE-1B0976`

- **原始用户可见行为（r00Then）**：以前台服务进程运行，保留指定 channel 与数据降级约束
- **R02 真正承担的 primitive/service obligation**：服务启动语义（R02-T02/T06 的服务端义务经真实 CLI→Rust spawn 路径证明）：当前源码全新构建、候选不变、前台 serve ready 且 Rust 子进程身份、显式 stable channel、beta 缺失不静默回落 stable、降级拒绝、启动错误非零、过新 epoch 不切数据根、SIGTERM 干净退出。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07-T09 serve 命令的客户端集成面（参数 UX、与届时正式客户端的重验）。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_cli_rust_matrix`｜图钉：cli-rust-current-binary-build=1; cli-rust-candidate-unchanged-during-probe=1; cli-rust-serve-ready=1; cli-rust-channel-stable-selected=1; cli-rust-serve-sigterm-clean=1; cli-rust-serve-beta-no-silent-stable=1; cli-rust-serve-downgrade-refused=1; cli-rust-serve-startup-error=1; cli-rust-serve-newer-epoch-no-root-switch=1｜refs：supplemental_cli_rust_matrix, a01_smoke, a02_boundary_negative, a03_dual_instance
- **是否越界**：**不越界（内容），但结构性损坏**——图钉 9 例全为服务启动语义（无客户端 UX），内容不越界；但 assertionContract.producerCommand=supplemental_cli_rust_matrix 未在 R02.json commands 注册（19 命令中无此键），stage_map.rs:616 解析即硬错、verify.rs:1539 也会 internal error——当前阶段图根本无法加载/运行，属结构性损坏而非范围问题。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：cli-rust-current-binary-build=1; cli-rust-candidate-unchanged-during-probe=1; cli-rust-serve-ready=1; cli-rust-channel-stable-selected=1; cli-rust-serve-sigterm-clean=1; cli-rust-serve-beta-no-silent-stable=1; cli-rust-serve-downgrade-refused=1; cli-rust-serve-startup-error=1; cli-rust-serve-newer-epoch-no-root-switch=1｜refs：supplemental_cli_rust_matrix, a01_smoke, a02_boundary_negative, a03_dual_instance｜必须先注册命令 supplemental_cli_rust_matrix：argv=["python3","scripts/rust-tauri/r02_cli_rust_matrix.py","{EVIDENCE}/CLI_RUST"]，timeoutSecs 建议 1800（内含全新 cargo build），evidencePaths 至少含 {EVIDENCE}/CLI_RUST/cli-rust-cases.json（脚本已产出该文件）。脚本已存在且案例齐备。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**修复后可 PASS（需注册命令）**；不足需补：需补的不是案例而是命令注册（stage map 结构修复）；脚本 r02_cli_rust_matrix.py 已可真实执行。
- **后续 Stage 保留要求**：[R07] 按届时正式客户端候选重验 serve 命令集成（不复用本轮证据）


### C 组｜管理面服务端语义（17 叶）：不越界，保留 R02 门禁

#### 9. `R00-T02-LA-093F22C4FF63` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-DESKTOP-CREDENTIAL-093F22`

- **原始用户可见行为（r00Then）**：登记设备/凭证并返回一次性 secret 和桌面访问地址
- **R02 真正承担的 primitive/service obligation**：桌面设备凭证签发服务端语义：本地主人登记设备/凭证、一次性 secret + 桌面访问地址返回、非法/越权不持久化、存储失败无 secret 泄漏、标准安全审计 JSONL（主体/目标/动作/metadata、无 secret、重启去重）。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-desktop-credential-secret-address-audit=1; management-desktop-credential-invalid-unauthorized-unchanged=1; management-desktop-credential-store-failure-no-secret=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-desktop-credential-secret-address-audit=1; management-desktop-credential-invalid-unauthorized-unchanged=1; management-desktop-credential-store-failure-no-secret=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 10. `R00-T02-LA-747E0ADC941B` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-MOBILE-CREDENTIALS-747E0A`

- **原始用户可见行为（r00Then）**：登记设备/凭证并返回一次性 secret 和手机访问地址
- **R02 真正承担的 primitive/service obligation**：手机设备凭证签发服务端语义：同上但返回手机访问地址。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-mobile-credential-secret-address-audit=1; management-mobile-credential-invalid-unauthorized-unchanged=1; management-mobile-credential-store-failure-no-secret=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-mobile-credential-secret-address-audit=1; management-mobile-credential-invalid-unauthorized-unchanged=1; management-mobile-credential-store-failure-no-secret=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 11. `R00-T02-LA-4BCE8CCFD5DC` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SERVER-IDENTITY-SERVER-IDENTITY--4BCE8C`

- **原始用户可见行为（r00Then）**：返回版本、身份及按 principal 投影的能力
- **R02 真正承担的 primitive/service obligation**：服务器身份服务端语义：/lingxi/v1/me（兼 /server/identity 投影）返回版本、身份与按 principal/scopes 投影的能力；伪造头被忽略；拒绝时凭证库逐字节不变。evidenceCommandRefs 同时绑定 a05_a06_auth_matrix（/me 认证族交叉）。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-me-owner-version-identity-capabilities=1; management-me-device-version-identity-capabilities=1; management-me-forged-header-ignored=1; management-me-denied-no-state-change=1｜refs：supplemental_management_matrix, a05_a06_auth_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。（本叶兼跨 a05 /me 认证族与 management 投影族，两处图钉均为服务端。）
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-me-owner-version-identity-capabilities=1; management-me-device-version-identity-capabilities=1; management-me-forged-header-ignored=1; management-me-denied-no-state-change=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 13. `R00-T02-LA-25AB678E7108` | `F-D20-ROUTE_BEHAVIOR-BEHAVIOR-ACCESS-ACCESS-ACCOUNT-PASSWORD-DELETE-25AB67`

- **原始用户可见行为（r00Then）**：仅本地主人可清除密码；更新账号并记录安全审计，失败时返回错误
- **R02 真正承担的 primitive/service obligation**：移除本地账号密码的服务端语义：仅本地主人可清除、账号更新 + 安全审计、越权保持原状态、存储失败不变、失败副作用与状态码一致。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-password-clear-account-audit=1; management-password-clear-unauthorized-keeps-prior=1; management-password-clear-store-failure-unchanged=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-password-clear-account-audit=1; management-password-clear-unauthorized-keeps-prior=1; management-password-clear-store-failure-unchanged=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 14. `R00-T02-LA-F1754C6F755B` | `F-D20-ROUTE_BEHAVIOR-BEHAVIOR-ACCESS-ACCESS-ACCOUNT-PASSWORD-PUT-F1754C`

- **原始用户可见行为（r00Then）**：仅本地主人可设置密码；更新账号并记录安全审计，失败时返回错误
- **R02 真正承担的 primitive/service obligation**：设置本地账号密码的服务端语义：仅本地主人可设置、账号更新 + 安全审计、无效输入保持原密码、存储失败不变。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-password-set-account-audit=1; management-password-invalid-keeps-prior=1; management-password-set-store-failure-unchanged=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-password-set-account-audit=1; management-password-invalid-keeps-prior=1; management-password-set-store-failure-unchanged=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 15. `R00-T02-LA-F006E094F028` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-ACCOUNT-PROFILE-PU-F006E0`

- **原始用户可见行为（r00Then）**：保存 username/displayName 并返回 account
- **R02 真正承担的 primitive/service obligation**：本地账号资料修改服务端语义：保存 username/displayName 返回 account、非法/越权不持久化、存储失败不变。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-profile-success-account-audit=1; management-profile-invalid-and-unauthorized-unchanged=1; management-profile-store-failure-unchanged=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-profile-success-account-audit=1; management-profile-invalid-and-unauthorized-unchanged=1; management-profile-store-failure-unchanged=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 16. `R00-T02-LA-73CDC44696D3` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-NETWORK-PUT-73CDC4`

- **原始用户可见行为（r00Then）**：保存网络配置、更新运行时摘要与安全审计
- **R02 真正承担的 primitive/service obligation**：网络配置写入服务端语义：保存 + 运行时摘要 + 安全审计；非法/越权/存储失败不变；保存后重启真实绑定且审计不重复；CLI 显式覆盖真实生效；损坏 saved 设置拒启。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-network-success-summary-audit=1; management-network-invalid-and-unauthorized-unchanged=1; management-network-store-failure-unchanged=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1; management-network-saved-restart-real-bind-no-audit-duplicate=1; management-network-cli-override-real-bind=1; management-network-corrupt-saved-settings-refused=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-network-success-summary-audit=1; management-network-invalid-and-unauthorized-unchanged=1; management-network-store-failure-unchanged=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1; management-network-saved-restart-real-bind-no-audit-duplicate=1; management-network-cli-override-real-bind=1; management-network-corrupt-saved-settings-refused=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 17. `R00-T02-LA-5E4A9CF58BED` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-MOBILE-QR-SVG-READ-5E4A9C`

- **原始用户可见行为（r00Then）**：返回手机访问地址 SVG，LAN 地址不可用为 400
- **R02 真正承担的 primitive/service obligation**：手机配对二维码服务端语义：返回手机访问地址 SVG、LAN 地址不可用 400、越权无 SVG。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-mobile-qr-svg-present=1; management-mobile-qr-lan-unavailable-refused=1; management-mobile-qr-unauthorized-no-svg=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-mobile-qr-svg-present=1; management-mobile-qr-lan-unavailable-refused=1; management-mobile-qr-unauthorized-no-svg=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 18. `R00-T02-LA-229B77A7BA69` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-ACCESS-ACCESS-SUMMARY-READ-229B77`

- **原始用户可见行为（r00Then）**：返回网络、设备和账号概况
- **R02 真正承担的 primitive/service obligation**：接入概况服务端语义：初始真实状态、填充后与 stores 一致、越权无状态变化、registry 失败显错而非空数据。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-summary-initial-real-state=1; management-summary-populated-matches-stores=1; management-summary-unauthorized-no-state-change=1; management-summary-registry-failure-not-empty=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-summary-initial-real-state=1; management-summary-populated-matches-stores=1; management-summary-unauthorized-no-state-change=1; management-summary-registry-failure-not-empty=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 19. `R00-T02-LA-B8F8CF9E8AF5` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-B8F8CF`

- **原始用户可见行为（r00Then）**：仅本地主人读取脱敏的设备、凭证与配对会话列表
- **R02 真正承担的 primitive/service obligation**：设备/凭证/配对列表服务端语义：仅本地主人、脱敏（secret 不回传）、含 pending 配对（脱敏）、越权拒绝、registry 失败显错。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-device-list-redacts-secrets=1; management-device-list-includes-pending-pairing-redacted=1; management-device-list-unauthorized-refused=1; management-device-list-registry-failure-not-empty=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-device-list-redacts-secrets=1; management-device-list-includes-pending-pairing-redacted=1; management-device-list-unauthorized-refused=1; management-device-list-registry-failure-not-empty=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 20. `R00-T02-LA-7498024422D7` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-DEVICEID-REVOKE-749802`

- **原始用户可见行为（r00Then）**：令设备失效并返回脱敏设备状态，记录安全审计
- **R02 真正承担的 primitive/service obligation**：撤销设备服务端语义：目标失效 + 脱敏返回 + 审计、目标鉴权拒绝、缺失无副作用、存储失败目标仍有效。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-device-revoke-target-auth-rejected=1; management-device-revoke-missing-no-side-effect=1; management-device-revoke-store-failure-target-still-active=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-device-revoke-target-auth-rejected=1; management-device-revoke-missing-no-side-effect=1; management-device-revoke-store-failure-target-still-active=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 21. `R00-T02-LA-43A126149416` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-CREDENTIALS-CRED-43A126`

- **原始用户可见行为（r00Then）**：令指定凭证失效并返回脱敏状态，记录安全审计
- **R02 真正承担的 primitive/service obligation**：撤销设备凭证服务端语义：仅目标凭证失效 + 审计、缺失无副作用、存储失败目标仍有效。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-credential-revoke-target-only=1; management-credential-revoke-missing-no-side-effect=1; management-credential-revoke-store-failure-target-still-active=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-credential-revoke-target-only=1; management-credential-revoke-missing-no-side-effect=1; management-credential-revoke-store-failure-target-still-active=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 22. `R00-T02-LA-756217C74101` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-PAIRING-SESSIONS-756217`

- **原始用户可见行为（r00Then）**：仅本地主人得到配对码及到期时间，注册记录并写安全审计
- **R02 真正承担的 primitive/service obligation**：创建配对会话服务端语义：仅本地主人得配对码 + 到期时间、注册记录 + 审计、非法/越权不变、存储失败不发码。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-pairing-create-code-expiry-audit=1; management-pairing-create-invalid-unauthorized-unchanged=1; management-pairing-create-store-failure-no-code=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-pairing-create-code-expiry-audit=1; management-pairing-create-invalid-unauthorized-unchanged=1; management-pairing-create-store-failure-no-code=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 23. `R00-T02-LA-5E1C3363A070` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-DEVICES-DEVICES-PAIRING-SESSIONS-5E1C33`

- **原始用户可见行为（r00Then）**：仅本地主人用配对码签发设备密钥，返回密钥一次及设备作用域并写安全审计
- **R02 真正承担的 primitive/service obligation**：批准配对服务端语义：配对码一次签发设备密钥 + scope + 一次性 secret + 审计；错误 code/scope/鉴权不变；存储失败仍 pending；重放不签发；并发仅一个凭证；过期 code 不签发。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-pairing-approve-once-scope-secret-audit=1; management-pairing-approve-wrong-code-scope-auth-unchanged=1; management-pairing-approve-store-failure-still-pending=1; management-pairing-replay-does-not-sign=1; management-pairing-concurrent-only-one-credential=1; management-pairing-expired-code-does-not-sign=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-pairing-approve-once-scope-secret-audit=1; management-pairing-approve-wrong-code-scope-auth-unchanged=1; management-pairing-approve-store-failure-still-pending=1; management-pairing-replay-does-not-sign=1; management-pairing-concurrent-only-one-credential=1; management-pairing-expired-code-does-not-sign=1; management-standard-audit-path-failure-no-write-recovery=1; management-standard-audit-all-actions-identities-no-secrets=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 30. `R00-T02-LA-066B54983F6A` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-WEB-AUTH-WEB-AUTH-SESSION-READ-066B54`

- **原始用户可见行为（r00Then）**：有效 cookie 返回净化后的 principal；无效/过期返回 authenticated:false
- **R02 真正承担的 primitive/service obligation**：Web 会话读取服务端语义：有效 cookie 返回净化 principal、无效/过期 authenticated:false、mobile scope 最小化且不写盘、连接类型不匹配 false、secret 不泄漏。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-web-session-valid-sanitized=1; management-web-session-invalid-false=1; management-web-session-expired-false=1; management-web-session-mobile-scope-no-write=1; management-web-session-connection-mismatch-false=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-web-session-valid-sanitized=1; management-web-session-invalid-false=1; management-web-session-expired-false=1; management-web-session-mobile-scope-no-write=1; management-web-session-connection-mismatch-false=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 31. `R00-T02-LA-C4E6F27D7873` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-WEB-AUTH-WEB-AUTH-LOGIN-POST-C4E6F2`

- **原始用户可见行为（r00Then）**：校验 token 或本地账号密码，创建 14 日 Web session 并设 HttpOnly cookie
- **R02 真正承担的 primitive/service obligation**：Web 登录服务端语义：token/密码 + credential 优先级、14 日 session + HttpOnly cookie、clientKind 决定 scope、真实 HTTPS（tls_web_login）下密码安全、远端 HTTP/伪造 forwarded proto 拒绝、store 失败无 session、LAN browser origin 登录-会话-登出闭环。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-web-login-password-cookie-14day-desktop-scope=1; management-web-login-token-positive-14day-mobile-scope=1; web-login-https-password-secure-14day-cookie=1; web-login-https-session-logout-revoked=1; management-web-login-credential-priority-denies-invalid-token=1; management-web-session-mobile-scope-no-write=1; management-web-login-store-failure-no-session=1; management-lan-browser-origin-login-session-logout=1; web-login-foreign-origin-denied=1; web-login-http-forwarded-proto-denied=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。（tls-web-login-cases.json 由独立 cargo 集成测试产出，同属服务端。）
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-web-login-password-cookie-14day-desktop-scope=1; management-web-login-token-positive-14day-mobile-scope=1; web-login-https-password-secure-14day-cookie=1; web-login-https-session-logout-revoked=1; management-web-login-credential-priority-denies-invalid-token=1; management-web-session-mobile-scope-no-write=1; management-web-login-store-failure-no-session=1; management-lan-browser-origin-login-session-logout=1; web-login-foreign-origin-denied=1; web-login-http-forwarded-proto-denied=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为

#### 32. `R00-T02-LA-4C1A9735CF04` | `F-D20-ROUTE_BEHAVIOR-BEHAVIOR-WEB-AUTH-WEB-AUTH-LOGOUT-POST-4C1A97`

- **原始用户可见行为（r00Then）**：撤销当前 Web 会话并清除会话 Cookie，返回成功
- **R02 真正承担的 primitive/service obligation**：Web 登出服务端语义：仅撤销当前 cookie 对应会话、清除会话 cookie、store 失败会话仍存活、无效 cookie 不误撤其他会话。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（T08/T09）客户端投影/UI 接线：管理入口在网页/移动/桌面设置页的显示、toast、二维码渲染等按届时候选重验；服务端语义回归由保留运行的管理矩阵持续保障。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_management_matrix`｜图钉：management-web-logout-only-current-cookie=1; management-lan-browser-origin-login-session-logout=1; management-web-logout-store-failure-session-alive=1; management-web-logout-invalid-cookie-no-other-revoke=1｜refs：supplemental_management_matrix
- **是否越界**：**不越界**——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原 then/assertions 全部落在服务器侧（状态码、脱敏、审计、失败副作用）；管理面服务端语义属现行指令列举的 R02 份额，图钉案例全部由 supplemental_management_matrix 驱动真 Rust 服务（cargo test r00_management_leaves + tls_web_login）执行，未要求任何客户端/UI 行为。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：management-web-logout-only-current-cookie=1; management-lan-browser-origin-login-session-logout=1; management-web-logout-store-failure-session-alive=1; management-web-logout-invalid-cookie-no-other-revoke=1｜refs：supplemental_management_matrix｜沿用该叶现有全部图钉（见 currentGate.cases）；management 矩阵整体属 R02 份额。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**可 PASS（份额案例现有生产者可执行）**；不足需补：无（案例与生产者均已在位，等待合法运行）。
- **后续 Stage 保留要求**：[R07] 客户端投影/UI 接线重验；不得以 R02 服务端证据冒充客户端行为


### D 组｜CLI sessions（1 叶）：图钉部分越界，拆分后 R02 份额可 PASS

#### 5. `R00-T02-LA-200D4E5D52C9` | `F-D20-CLI-CLI-SESSIONS-200D4E`

- **原始用户可见行为（r00Then）**：从已认证服务读取并列出最多 20 个会话；空列表显示 No sessions yet
- **R02 真正承担的 primitive/service obligation**：已认证会话列表的服务端份额：sessions 按 principal 过滤——owner 200 且含自身会话、foreign 200 且不含 owner 会话、空列表 shape、无凭据 401（不读其他主体会话的服务端保证）。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07-T09 CLI sessions 命令客户端行为：真实 CLI 渲染 owner 列表/foreign 空列表/未授权错误显示、最多 20 条渲染上限（当前用模拟 25 条成功响应核渲染分支）、空态 No sessions yet 显示。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_cli_sessions_matrix`｜图钉：a05-sessions-list-owner=200; a05-sessions-list-owner-contains-own-session=1; a05-sessions-list-foreign-principal=200; a05-sessions-list-foreign-excludes-owner-sessions=0; a05-sessions-list-empty-shape=1; a05-sessions-list-no-credential=401; a05-cli-sessions-owner-list=1; a05-cli-sessions-foreign-empty=1; a05-cli-sessions-unauthorized-error=1; a05-cli-sessions-limit-20=1｜refs：supplemental_cli_sessions_matrix
- **是否越界**：**越界（图钉部分为客户端行为）**——10 个图钉中 6 个（a05-sessions-list-*）是服务端份额；4 个（a05-cli-sessions-owner-list / foreign-empty / unauthorized-error / limit-20）是 CLI 客户端渲染行为（其中 limit-20 是 mocked fetch 的合成渲染测试），把它们作为 R02 full_original_behavior PASS 前置 = 要求 R02 证明 R07 客户端行为，越界。
- **正确的 R02 验收内容**：basisKind 改为 `r02_share_satisfied`｜门禁图钉：a05-sessions-list-owner=200; a05-sessions-list-owner-contains-own-session=1; a05-sessions-list-foreign-principal=200; a05-sessions-list-foreign-excludes-owner-sessions=0; a05-sessions-list-empty-shape=1; a05-sessions-list-no-credential=401｜refs：supplemental_cli_sessions_matrix｜门禁图钉缩编为 6 个服务侧案例（同一生产者已产出）；4 个 CLI 案例保留在 supplemental_cli_sessions_matrix 中继续运行（命令不删），但结果不计入 R02 判定，登记为提前实现证据归 R07。
- **保留运行不阻塞 R02 的提前实现案例**：a05-cli-sessions-owner-list=1; a05-cli-sessions-foreign-empty=1; a05-cli-sessions-unauthorized-error=1; a05-cli-sessions-limit-20=1
- **重判定**：**拆分后可 PASS（图钉缩到服务侧案例）**；不足需补：需把 R02 判定图钉缩到 6 个服务侧案例；生产者无需改动（其证据文件同时含两类案例）。
- **后续 Stage 保留要求**：[R07] CLI sessions 完整客户端行为（含真实 25 条服务端响应下的上限行为与空态文案）按届时客户端重验


### E 组｜纯客户端叶（2 叶）：全部图钉为客户端行为，转 DEFERRED_TO_R07

#### 3. `R00-T02-LA-B8A1AD32A8E1` | `F-D20-CLI-CLI-HELP-B8A1AD`

- **原始用户可见行为（r00Then）**：打印命令与参数说明并以 0 退出
- **R02 真正承担的 primitive/service obligation**：无 R02 服务份额：help 文本、未知参数错误与退出码、不启动服务均为 CLI 客户端参数面行为（node cli/entry.ts 子进程），不触及 Rust 服务原语；R02 的“独立 CLI 测试入口/独立交付”义务由 A01/A15/a01_smoke 家族覆盖，与本叶无专属交集。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07-T09 CLI help 完整行为：打印命令与参数说明以 0 退出；未知参数打印错误和帮助以 1 退出；不启动服务（合成数据根不变、无 server-info、无 READY 标记）。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_client_matrix`｜图钉：cli-help-exit0-no-start=1; cli-unknown-arg-exit1-no-start=1; cli-serve-unknown-arg-exit1-no-start=1｜refs：supplemental_client_matrix
- **是否越界**：**越界（图钉全部为客户端行为）**——3 个图钉（cli-help-exit0-no-start / cli-unknown-arg-exit1-no-start / cli-serve-unknown-arg-exit1-no-start）全部是客户端行为，full_original_behavior 把它们作为 R02 PASS 前置，越界（R17 授权已按现行指令废除其门禁含义）。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜叶转 DEFERRED_TO_R07：supplemental_client_matrix 的 CLI 部分保留注册、继续运行、计入 freshness 检查，但其结果不进 R02 overall；R07 阶段图必须消费本叶（REQUIRED）。
- **保留运行不阻塞 R02 的提前实现案例**：cli-help-exit0-no-start=1; cli-unknown-arg-exit1-no-start=1; cli-serve-unknown-arg-exit1-no-start=1
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：无 R02 份额需补。
- **后续 Stage 保留要求**：[R07] 按届时迁移后的 CLI 重验 help/未知参数/退出码/不启动服务全矩阵；本轮 Node CLI 证据仅作回归基线

#### 34. `R00-T02-LA-32FFEC05BAA7` | `F-D20-UI-UI-SETTINGS-SHARING-32FFEC`

- **原始用户可见行为（r00Then）**：逐动作核对实际状态与页面投影；失败不伪装成空数据
- **R02 真正承担的 primitive/service obligation**：无 R02 服务份额：截图配色/宽度/字体/分段上限均为客户端 localStorage 写入与渲染投影，无服务端原语参与。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07（UI 设置页）完整分享设置页行为：容器初始化（缓存/15 秒超时/错误 ready）、三色卡、宽度卡、字体与上限的默认/写入/拒绝投影及存储禁止时显式失败。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_client_matrix`｜图钉：sharing-container-cold-cache-timeout-error=1; sharing-color-success-default-denied=1; sharing-width-success-default-denied=1; sharing-font-limit-success-default-denied=1｜refs：supplemental_client_matrix
- **是否越界**：**越界（图钉全部为客户端行为）**——4 个图钉（sharing-container-cold-cache-timeout-error、sharing-color/width/font-limit-success-default-denied）全部是 React UI 行为（vitest 渲染真实 SettingsContent/SharingTab），作为 R02 PASS 前置越界。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜同上：矩阵保留运行不阻塞；R07 消费本叶。
- **保留运行不阻塞 R02 的提前实现案例**：sharing-container-cold-cache-timeout-error=1; sharing-color-success-default-denied=1; sharing-width-success-default-denied=1; sharing-font-limit-success-default-denied=1
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：无 R02 份额需补。
- **后续 Stage 保留要求**：[R07] 分享设置页完整动作矩阵按届时 UI 候选重验（含 localStorage 禁止存储的显式失败）


### F 组｜静态托管（2 叶）：提前实现的 R07-T09 服务能力，转 DEFERRED_TO_R07

#### 24. `R00-T02-LA-8BC1A036AFAA` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-MOBILE-STATIC-DESKTOP-8BC1A0`

- **原始用户可见行为（r00Then）**：有桌面构建产物时服务静态页面及资源，否则按路由返回指引或错误
- **R02 真正承担的 primitive/service obligation**：无独立 R02 份额：静态托管是 R07-T09 第 2 项“托管现役 Web/mobile 静态资源及 API”的入口服务能力，不属现行指令列举的 R02 义务类别（认证/资源范围/WS-HTTP 原语/管理面服务端语义/存储/事件/备份/日志上限/独立交付）；R02 任务书 8 任务与 16 基础验收也不含静态托管。通用 HTTP 安全面（路径穿越拒绝、有界资源）已由 A02/A05/A06 家族在服务层覆盖。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07-T09 静态托管完整行为：有构建产物时服务页面与资源（内容类型/权限）、缺产物按路由返回指引、显式无效 dist 503、路径穿越拒绝、超限资源显式错误——R07 须对其届时产物重验。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_static_web_matrix`｜图钉：static-desktop-dist-page-assets=1; static-desktop-guide-without-dist=1; static-desktop-invalid-explicit-dist-503=1; static-web-traversal-secret-refused=1; static-web-oversized-asset-explicit-error=1｜refs：supplemental_static_web_matrix
- **是否越界**：**越界（份额归属：提前实现的 R07 能力/无 R02 份额）**——图钉案例虽全为服务端（真 TCP），但整个能力是提前实现的 R07-T09 服务份额；按现行指令它不得作为 R02 PASS 前置（提前实现保留运行、继续不破坏即可）。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜矩阵保留注册继续运行（其路径穿越/超限错误与 R02 HTTP 安全面同源，运行即持续回归），结果不计入 R02；叶归 R07 消费。
- **保留运行不阻塞 R02 的提前实现案例**：static-desktop-dist-page-assets=1; static-desktop-guide-without-dist=1; static-desktop-invalid-explicit-dist-503=1; static-web-traversal-secret-refused=1; static-web-oversized-asset-explicit-error=1
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：若总控改为“静态托管服务端语义计入 R02”，现有 9 案例已足够，无需新写生产者——但按现行指令文本建议 DEFERRED。
- **后续 Stage 保留要求**：[R07] 静态托管按届时构建产物重验全部正负路径；不得删除已实现的 static_web.rs 服务能力（继续不破坏）

#### 25. `R00-T02-LA-3291CFD5F7E2` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-MOBILE-STATIC-MOBILE-3291CF`

- **原始用户可见行为（r00Then）**：有移动端构建产物时服务静态页面及资源，否则按路由返回指引或错误
- **R02 真正承担的 primitive/service obligation**：无独立 R02 份额：静态托管是 R07-T09 第 2 项“托管现役 Web/mobile 静态资源及 API”的入口服务能力，不属现行指令列举的 R02 义务类别（认证/资源范围/WS-HTTP 原语/管理面服务端语义/存储/事件/备份/日志上限/独立交付）；R02 任务书 8 任务与 16 基础验收也不含静态托管。通用 HTTP 安全面（路径穿越拒绝、有界资源）已由 A02/A05/A06 家族在服务层覆盖。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07-T09 静态托管完整行为：有构建产物时服务页面与资源（内容类型/权限）、缺产物按路由返回指引、显式无效 dist 503、路径穿越拒绝、超限资源显式错误——R07 须对其届时产物重验。
- **当前 R02 Gate 要求**：basisKind=`full_original_behavior`｜producer=`supplemental_static_web_matrix`｜图钉：static-mobile-dist-page-assets=1; static-mobile-guide-without-dist=1; static-mobile-invalid-explicit-dist-503=1; static-web-traversal-secret-refused=1; static-web-oversized-asset-explicit-error=1｜refs：supplemental_static_web_matrix
- **是否越界**：**越界（份额归属：提前实现的 R07 能力/无 R02 份额）**——图钉案例虽全为服务端（真 TCP），但整个能力是提前实现的 R07-T09 服务份额；按现行指令它不得作为 R02 PASS 前置（提前实现保留运行、继续不破坏即可）。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜矩阵保留注册继续运行（其路径穿越/超限错误与 R02 HTTP 安全面同源，运行即持续回归），结果不计入 R02；叶归 R07 消费。
- **保留运行不阻塞 R02 的提前实现案例**：static-mobile-dist-page-assets=1; static-mobile-guide-without-dist=1; static-mobile-invalid-explicit-dist-503=1; static-web-traversal-secret-refused=1; static-web-oversized-asset-explicit-error=1
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：若总控改为“静态托管服务端语义计入 R02”，现有 9 案例已足够，无需新写生产者——但按现行指令文本建议 DEFERRED。
- **后续 Stage 保留要求**：[R07] 静态托管按届时构建产物重验全部正负路径；不得删除已实现的 static_web.rs 服务能力（继续不破坏）


### G 组｜路由不在 R02 面/依赖后续阶段权威（5 叶）：转 DEFERRED_TO_R07

#### 26. `R00-T02-LA-2A1C298F62FC` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-MOBILE-WORKBENCH-MOBILE-BOOTSTRA-2A1C29`

- **原始用户可见行为（r00Then）**：返回当前 Agent、语言、头像可用性、工作区和偏好；顺带清理过期持久化
- **R02 真正承担的 primitive/service obligation**：无 R02 专属份额：移动端 workbench 引导路由（Agent/语言/头像/工作区/偏好 + 过期持久化清理）在现行 Rust 服务不存在（路由清单核实），其内容权威（Agent/工作区/偏好）属 R06/R07；通用认证原语已由 A05/A06 作为基础验收覆盖，不需按叶重复图钉。
- **R03–R06 obligation**：R06（Agent/偏好/工作区数据权威）。
- **R07 obligation（保留，防丢失）**：R07-T09 移动端初始化完整入口：返回当前 Agent、语言、头像可用性、工作区和偏好；顺带清理过期持久化；拒绝/下游失败不冒空态。
- **当前 R02 Gate 要求**：basisKind=`auth_primitive_only`｜producer=`a05_a06_auth_matrix`｜图钉：a05-no-credential-read=401; a05-forged-principal-headers=401; a05-cross-principal-read=403; a05-positive-control-owner-execute=200; a06-evil-origin-health=403; a06-foreign-host-health=403｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（份额归属：提前实现的 R07 能力/无 R02 份额）**——auth_primitive_only 的 6 个通用图钉（no-credential-read/forged-headers/cross-principal/owner-execute/evil-origin/foreign-host）打在 /sessions、/execute、/health 等通用端点上，与本叶路由无关——R15 结论（通用案例证明不了叶行为）成立，且该叶在 R02 无可实现份额，保持 REQUIRED-BLOCKED 只会把 R07 义务伪装成 R02 失败。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜叶转 DEFERRED_TO_R07；通用 a05/a06 案例继续由 A05/A06 场景自身消费，不再按叶挂接。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：无 R02 份额需补。
- **后续 Stage 保留要求**：[R06/R07] 实现移动引导路由（内容权威 + 清理语义）并按原断言验收

#### 27. `R00-T02-LA-8ED658F9DB9E` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SERVER-INDEX-API-SESSION-THINKIN-8ED658`

- **原始用户可见行为（r00Then）**：返回指定会话或新会话默认级别
- **R02 真正承担的 primitive/service obligation**：R02 份额至多为路由级认证/校验面：/lingxi/v1/session-thinking-level 路由已提前实现（management.rs:583，get+post），无凭据拒绝与非法 level 400 属服务端；但原断言的取值语义依赖模型状态权威——当前无模型配置时返回 503 model_state_unavailable（auth_matrix.rs 实测），即完整原行为必须等 R05/R06 的模型配置权威，不在 R02 可证范围。且现有叶案例生产者（management 矩阵 EXPECTED_CASES、auth matrix leaf-cases）均无 thinking-level 案例。
- **R03–R06 obligation**：R05/R06（模型配置/思考级别权威：真实取值、409 冲突、默认不追改旧会话的语义来源）。
- **R07 obligation（保留，防丢失）**：R07-T09 完整入口行为：指定会话/新会话默认级别读取与设置、query 边界（sessionPath/pendingNewSession）不扩大披露、无效 level/409 不改变目标状态。
- **当前 R02 Gate 要求**：basisKind=`auth_primitive_only`｜producer=`a05_a06_auth_matrix`｜图钉：a05-no-credential-read=401; a05-forged-principal-headers=401; a05-cross-principal-read=403; a05-positive-control-owner-execute=200; a06-evil-origin-health=403; a06-foreign-host-health=403｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（份额归属：提前实现的 R07 能力/无 R02 份额）**——auth_primitive_only 通用图钉与本叶路由无关（同 R15 结论）；stage map 的 r02Share 文本“思考级别路由不在 R02 现行面”已过时（路由现已存在），但即使按新事实，完整原行为依赖 R05+ 模型权威，无法在 R02 以现有生产者证明。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜推荐 DEFERRED_TO_R07（路由保留、auth_matrix.rs 中既有 503/400 cargo 断言继续运行不阻塞）。若总控坚持 R02 证路由份额，需在 r02_management_leaf_matrix.py 新增 3-5 例（401 无凭据 / 400 invalid level / 越权 403）并同步 EXPECTED_CASES——这是新案例工作，不是“现有生产者可执行”范畴。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：若不采纳 DEFERRED：需为 thinking-level 新增叶案例（当前无任何生产者输出该叶案例）。
- **后续 Stage 保留要求**：[R05/R06] 模型状态/思考级别权威（真实取值与 409 语义）；[R07] 完整入口行为与 query 边界验收；保留已实现路由不回退

#### 28. `R00-T02-LA-F8935B6B0221` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSION-THINKING-LEVEL-SET-F8935B`

- **原始用户可见行为（r00Then）**：有 sessionPath 时只修改该会话，返回解析后的状态
- **R02 真正承担的 primitive/service obligation**：R02 份额至多为路由级认证/校验面：/lingxi/v1/session-thinking-level 路由已提前实现（management.rs:583，get+post），无凭据拒绝与非法 level 400 属服务端；但原断言的取值语义依赖模型状态权威——当前无模型配置时返回 503 model_state_unavailable（auth_matrix.rs 实测），即完整原行为必须等 R05/R06 的模型配置权威，不在 R02 可证范围。且现有叶案例生产者（management 矩阵 EXPECTED_CASES、auth matrix leaf-cases）均无 thinking-level 案例。
- **R03–R06 obligation**：R05/R06（模型配置/思考级别权威：真实取值、409 冲突、默认不追改旧会话的语义来源）。
- **R07 obligation（保留，防丢失）**：R07-T09 完整入口行为：指定会话/新会话默认级别读取与设置、query 边界（sessionPath/pendingNewSession）不扩大披露、无效 level/409 不改变目标状态。
- **当前 R02 Gate 要求**：basisKind=`auth_primitive_only`｜producer=`a05_a06_auth_matrix`｜图钉：a05-no-credential-read=401; a05-forged-principal-headers=401; a05-cross-principal-read=403; a05-positive-control-owner-execute=200; a06-evil-origin-health=403; a06-foreign-host-health=403｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（份额归属：提前实现的 R07 能力/无 R02 份额）**——auth_primitive_only 通用图钉与本叶路由无关（同 R15 结论）；stage map 的 r02Share 文本“思考级别路由不在 R02 现行面”已过时（路由现已存在），但即使按新事实，完整原行为依赖 R05+ 模型权威，无法在 R02 以现有生产者证明。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜推荐 DEFERRED_TO_R07（路由保留、auth_matrix.rs 中既有 503/400 cargo 断言继续运行不阻塞）。若总控坚持 R02 证路由份额，需在 r02_management_leaf_matrix.py 新增 3-5 例（401 无凭据 / 400 invalid level / 越权 403）并同步 EXPECTED_CASES——这是新案例工作，不是“现有生产者可执行”范畴。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：若不采纳 DEFERRED：需为 thinking-level 新增叶案例（当前无任何生产者输出该叶案例）。
- **后续 Stage 保留要求**：[R05/R06] 模型状态/思考级别权威（真实取值与 409 语义）；[R07] 完整入口行为与 query 边界验收；保留已实现路由不回退

#### 29. `R00-T02-LA-39AD35E1FD71` | `F-D20-SEMANTIC_EFFECT-SEMANTIC-EFFECT-SESSION-THINKING-LEVEL-DEFAULT-S-39AD35`

- **原始用户可见行为（r00Then）**：无 sessionPath 时修改默认级别，不追改旧会话
- **R02 真正承担的 primitive/service obligation**：R02 份额至多为路由级认证/校验面：/lingxi/v1/session-thinking-level 路由已提前实现（management.rs:583，get+post），无凭据拒绝与非法 level 400 属服务端；但原断言的取值语义依赖模型状态权威——当前无模型配置时返回 503 model_state_unavailable（auth_matrix.rs 实测），即完整原行为必须等 R05/R06 的模型配置权威，不在 R02 可证范围。且现有叶案例生产者（management 矩阵 EXPECTED_CASES、auth matrix leaf-cases）均无 thinking-level 案例。
- **R03–R06 obligation**：R05/R06（模型配置/思考级别权威：真实取值、409 冲突、默认不追改旧会话的语义来源）。
- **R07 obligation（保留，防丢失）**：R07-T09 完整入口行为：指定会话/新会话默认级别读取与设置、query 边界（sessionPath/pendingNewSession）不扩大披露、无效 level/409 不改变目标状态。
- **当前 R02 Gate 要求**：basisKind=`auth_primitive_only`｜producer=`a05_a06_auth_matrix`｜图钉：a05-no-credential-read=401; a05-forged-principal-headers=401; a05-cross-principal-read=403; a05-positive-control-owner-execute=200; a06-evil-origin-health=403; a06-foreign-host-health=403｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（份额归属：提前实现的 R07 能力/无 R02 份额）**——auth_primitive_only 通用图钉与本叶路由无关（同 R15 结论）；stage map 的 r02Share 文本“思考级别路由不在 R02 现行面”已过时（路由现已存在），但即使按新事实，完整原行为依赖 R05+ 模型权威，无法在 R02 以现有生产者证明。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜推荐 DEFERRED_TO_R07（路由保留、auth_matrix.rs 中既有 503/400 cargo 断言继续运行不阻塞）。若总控坚持 R02 证路由份额，需在 r02_management_leaf_matrix.py 新增 3-5 例（401 无凭据 / 400 invalid level / 越权 403）并同步 EXPECTED_CASES——这是新案例工作，不是“现有生产者可执行”范畴。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：若不采纳 DEFERRED：需为 thinking-level 新增叶案例（当前无任何生产者输出该叶案例）。
- **后续 Stage 保留要求**：[R05/R06] 模型状态/思考级别权威（真实取值与 409 语义）；[R07] 完整入口行为与 query 边界验收；保留已实现路由不回退

#### 33. `R00-T02-LA-000E6E1301C0` | `F-D20-UI-UI-SETTINGS-ACCESS-000E6E`

- **原始用户可见行为（r00Then）**：逐动作核对实际状态与页面投影；失败不伪装成空数据
- **R02 真正承担的 primitive/service obligation**：R02 份额极薄：设置访问页 7 条动作矩阵是 React UI 行为；其消费的服务端入口（access summary、network、credentials、devices revoke、account profile/password）的服务端语义已由本审计管理面 17 叶覆盖。R17 的 AccessTab 局部组件测试（access-ui-probe 系列）本身也是客户端证据。
- **R03–R06 obligation**：—
- **R07 obligation（保留，防丢失）**：R07 完整 UI 动作矩阵：容器自动初始化（缓存/15 秒超时/供应商摘要不阻塞）、访问总览与二维码自动显示、LAN/端口/公开地址保存、凭证生成与二维码、复制/连接远端、撤销设备/凭证、账号资料与密码编辑——每动作成功/空态/错误三态，失败不伪装空数据。
- **当前 R02 Gate 要求**：basisKind=`auth_primitive_only`｜producer=`a05_a06_auth_matrix`｜图钉：a05-no-credential-read=401; a05-forged-principal-headers=401; a05-cross-principal-read=403; a05-positive-control-owner-execute=200; a06-evil-origin-health=403; a06-foreign-host-health=403｜refs：a05_a06_auth_matrix
- **是否越界**：**越界（份额归属：提前实现的 R07 能力/无 R02 份额）**——auth_primitive_only 通用图钉证明不了任何一条原断言（R15 结论）；将纯 UI 叶保持为 R02 REQUIRED-BLOCKED 是把 R07 UI 义务伪装成 R02 缺口。
- **正确的 R02 验收内容**：basisKind 改为 `deferred_to_r07`｜门禁图钉：（无 R02 份额图钉）｜refs：—｜叶转 DEFERRED_TO_R07；R17 已有的 AccessTab 组件测试保留运行（不阻塞），服务端语义由管理面 17 叶在本门禁证明。
- **保留运行不阻塞 R02 的提前实现案例**：—
- **重判定**：**DEFERRED_TO_R07（保留 REQUIRED，验收归 R07）**；不足需补：无（服务端语义已由管理面叶覆盖，UI 部分归 R07）。
- **后续 Stage 保留要求**：[R07/R08] 设置访问页 7 条动作 × 三态完整 UI 验收（按届时 UI 候选；R17 局部组件证据只作回归基线）


## 3. 4 个 supplemental 矩阵命令的定位（+1 个未注册生产者）

| 命令 | 生产者 | R02 份额案例（进门禁） | R07 提前实现案例（保留运行、不阻塞 R02） |
|---|---|---|---|
| `supplemental_management_matrix` | `r02_management_leaf_matrix.py`（cargo test r00_management_leaves + tls_web_login，真 Rust 服务） | **全部**（management-cases 59 例 + web-login + TLS 4 例）：管理面服务端语义正/负/审计/失败副作用，支撑 17 叶门禁 | 无（矩阵内无客户端案例；各叶 R07 份额=客户端投影，由 R07 自行验证） |
| `supplemental_cli_sessions_matrix` | `r02_cli_sessions_leaf_matrix.py`（内部重跑 r02_t03_auth_matrix.sh sessions 段 + 真实 CLI + mocked limit） | 6 例：`a05-sessions-list-owner/-owner-contains-own-session/-foreign-principal/-foreign-excludes-owner-sessions/-empty-shape/-no-credential`（服务端 sessions 按 principal 过滤） | 4 例：`a05-cli-sessions-owner-list / -foreign-empty / -unauthorized-error / -limit-20`（真实 CLI 渲染 + 模拟 25 条渲染上限，客户端行为） |
| `supplemental_client_matrix` | `r02_client_leaf_matrix.py`（node cli/entry.ts 子进程 + vitest 渲染真实 SharingTab） | **无** | 全部 7 例：CLI help 3 例（叶 3）+ Sharing 4 例（叶 34）——纯客户端行为 |
| `supplemental_static_web_matrix` | `r02_static_web_leaf_matrix.py`（真 TCP 对真服务） | 无专属（通用 HTTP 安全面已由 A02/A05/A06 覆盖） | 全部 9 例：页面/资源/指引/503/路径穿越拒绝/超限错误——R07-T09 静态托管能力提前实现 |
| `supplemental_cli_rust_matrix`（**未注册，F1**） | `r02_cli_rust_matrix.py`（当前源码全新 cargo build + 真实 CLI spawn Rust 服务） | **全部 9 例**：构建/候选不变/serve ready/channel stable/SIGTERM 干净/beta 不静默回落/降级拒绝/启动错误/epoch 不切根——R02-T02/T06 服务启动语义 | 无（脚本另有未图钉观察如 status 案例天然非门禁） |

## 4. 汇总结论（按“R02 份额”重新判定）

| 类别 | 数量 | 叶 |
|---|---|---|
| 可 PASS（份额案例现有生产者可执行，门禁语义修正后） | **23** | 管理面 17 叶（093F22/747E0A/4BCE8C/25AB67/F1754C/F006E0/73CDC4/5E4A9C/229B77/B8F8CF/749802/43A126/756217/5E1C33/066B54/C4E6F2/4C1A97）+ 协议原语 6 叶（D3710D/3B8633/EEC1A2/D1BEE1/5816DA/D2657E） |
| 修复后可 PASS（注册 supplemental_cli_rust_matrix 命令，F1） | **1** | 1B0976（serve） |
| 拆分后可 PASS（图钉缩到 6 个服务侧案例；4 个 CLI 渲染案例保留运行不阻塞） | **1** | 200D4E（sessions） |
| 转 DEFERRED_TO_R07（保留 REQUIRED、验收归属 R07；提前实现保留运行不阻塞） | **9** | B8A1AD（help）、32FFEC（sharing）、8BC1A0/3291CF（static×2）、2A1C29（mobile bootstrap）、8ED658/F8935B/39AD35（thinking×3）、000E6E（UI access） |

**判定“越界”的叶（17 个，按越界类型）**：
- roll-up 语义越界（案例本身在份额内，被 verify.rs:675-692 阶段冲突分支永久 BLOCKED，等于要求 R02 先证 R07 客户端行为）：`D3710D637C19`、`3B86332B042C`、`EEC1A2A5CD04`、`D1BEE19A95BB`、`5816DA563ED8`、`D2657E4AB5FF`（6）。
- 图钉内容越界（客户端行为案例作为 R02 PASS 前置）：`200D4E5D52C9`（部分：4/10 案例为 CLI 渲染）、`B8A1AD32A8E1`（全部 3 案例）、`32FFEC05BAA7`（全部 4 案例）（3）。
- 份额归属越界（提前实现的 R07 能力 / 无 R02 可证份额被保持为 R02 REQUIRED-BLOCKED）：`8BC1A036AFAA`、`3291CFD5F7E2`、`2A1C298F62FC`、`8ED658F9DB9E`、`F8935B6B0221`、`39AD35E1FD71`、`000E6E1301C0`（7）。
- 结构性损坏（内容不越界但当前图无法运行）：`1B09760C2B1C`（1）。
- **不越界**：管理面服务端语义 17 叶——原叶 GIVEN 即以“具备相应权限的 HTTP 客户端直接请求”定义，原断言全部落在服务器侧，属现行指令明确列举的“管理面服务端语义”R02 份额。

**需补什么**：管理面 17 叶与协议 6 叶无需补任何案例（等待合法运行即可）；serve 叶需补**命令注册**（非案例）；sessions 叶需把判定图钉缩编（生产者无需改）；thinking 3 叶若不采纳 DEFERRED 则需在 management 生产者新增案例（当前无任何生产者输出该叶案例——不满足“现有生产者可执行”约束，故推荐 DEFERRED）。

## 5. SPEC_ISSUE 检查

**结论：无需 SPEC_ISSUE。**

- R00 原件两账本不需要任何字段修改：`execution_stage_ids=[R02,R07]`、`task_ids=[R02-T03,R07-T09]` 本就表达双阶段责任；本报告全部建议只动 R02.json 的 `basisKind/assertionContract/r02Share/r07Share` 文本与 verify.rs 判定语义，`r00_*` 镜像字段逐字保留，`cross_check_supplemental_coverage` 的全等核对不受影响，最终产品要求不变（34 叶仍 REQUIRED）。
- “R02 份额已证 + 余款 REQUIRED 留 R07”由 stage map 新 basisKind + verify.rs 新状态即可完整表达，不存在必须改 R00 原件才能表达的情形。
- 仅有的 R00 层面事项是**登记流**（份额执行后 result_ids/test_ids 登记、ledger_status 推进时机），属 R00-T07 既有约定，非字段/语义修改。
- 需要修正的过时文本在 R02 侧：叶 27–29 r02Share（路由已存在）、`supplementalSemanticLimit` 计数、LEDGER/handoff 计数对齐（F2/F3）。

## 6. verify.rs / stage_map 判定逻辑修改建议清单（只给建议，不改代码）

1. **V1**：新增 basisKind `r02_share_satisfied`（或语义化改造 protocol_basis/route_basis_present_static）：份额图钉全过即 PASS，并把 r07Share 文本 + `deferredToStage=R07` 写入结果 JSON；删除/绕过 `verify.rs:675-692`“另属后续阶段→BLOCKED”分支（越界的根源）。
2. **V2**：新增 `deferred_to_r07`：无 R02 份额叶不要求 assertionContract，roll-up 输出 DEFERRED_TO_R07（≠NOT_APPLICABLE/optional：仍 REQUIRED，验收归 R07）；解析不变量：仅当 r00_execution_stage_ids 含后续阶段才允许标记。
3. **V3**：overall 计算（`verify.rs:1700-1703`）改为 {PASS} ∪ {DEFERRED_TO_R07} 覆盖 34 叶；分开计数 pass/deferred/fail/blocked，且 deferred 集合与 stage map 显式声明全等（防静默扩大）。
4. **V4**：full_original_behavior 的 assertionContract 拆分 `r02ShareCases/deferredR07Cases`：R02 判定只消费份额图钉；deferred 案例所在命令保留注册照常运行（freshness/清理/证据检查不变），失败记 `earlyEvidenceObserved=false` 显式标注、不计入 R02 overall（叶 5 即此形态）。
5. **V5**：**注册缺失命令** `supplemental_cli_rust_matrix`（argv=`["python3","scripts/rust-tauri/r02_cli_rust_matrix.py","{EVIDENCE}/CLI_RUST"]`，timeoutSecs≈1800，evidencePaths 含 `{EVIDENCE}/CLI_RUST/cli-rust-cases.json`）——不修复则阶段图解析即硬错（F1）。
6. **V6**：`auth_primitive_only`（`verify.rs:621-632`）不再作为长期状态：升格 r02_share_satisfied（补专属份额图钉）或转 deferred_to_r07；现存 5 叶建议全部转 deferred_to_r07。
7. **V7**：结果 JSON 每叶输出份额/递延案例归属与 r02Share/r07Share 文本，供 R07 阶段图机器消费，防“R02 已判份额”被误读为整叶完成。
8. **V8**（后续 R07）：R07.json 必须消费全部 34 叶的 R07 份额（客户端行为、thinking-level 模型权威取值、移动引导路由、静态托管、UI 动作矩阵），沿用 supplementalLeafScenarios + assertionContract 机制。
9. **V9**：保持 r00_* 镜像全等核对不变；新增解析期校验——deferred 叶不得声明 gating 案例、producer 注册完整性在加载期一次性校验（把 F1 类缺陷前移到加载期报错）。

---
机器可读版：`/tmp/r02-final/contract-audit-r1.json`（34 叶逐项对象 + 结构性发现 + 矩阵定位 + 汇总 + SPEC_ISSUE + 修改建议）。
