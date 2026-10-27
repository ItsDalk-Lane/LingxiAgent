# R02 全范围修复与验收交接（R18，2026-09-28）

> **阶段结论：FAIL；R03 不得启动。** 当前源码的无端口检查不能代替真服务、完整 npm、安装包和独立阶段验收。此页只描述本轮事实；R1–R15 原始评审、首败、复跑、缺失日志及旧文的错误时间继续保留，不改写为本轮通过。

## 原合同与开工冻结

- 开工前分支 `codex/rust-tauri-migration`、HEAD `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f`，58 个未提交路径（54 修改、4 新增）。[开工前全范围问题表](R02_FULL_SCOPE_AUDIT_PRE_REPAIR.md)先于本轮修复写成，逐项列出 R02 八任务、A01–A16、34 项 R00 原叶及 R1–R15 全部历史问题的来源、正反要求、受影响入口、根因组、证据能力、修复条件和阻碍；[任务级补遗](R02_TASK_FINDINGS_ADDENDUM_R16.md)补足跨入口缺口。
- [原始冻结矩阵](../../../artifacts/rust-tauri/R02/audit-r16/contract-matrix-prechange.json)逐项保留 R00 两原账的 `then`、全部 `assertions`、`due`、身份和 R02/R07 双责任。开工前 9716 个非忽略文件的 [SHA-256 全清单](../../../artifacts/rust-tauri/R02/audit-r16/prechange-full-sha256-corrected.txt)、[58 路径清单](../../../artifacts/rust-tauri/R02/audit-r16/prechange-status-corrected.txt)、历史阶段报告和 18 份任务报告摘要均在同一冻结目录。最初的采集自引用错误原文件也保留，带 `-corrected` 的文件才是冻结基准。
- 后续用户明确授权：受平台允许时执行 cargo/npm/构建/测试、对 A15 被拒编辑重新按平台审批处理、仅隔离 Git 测试仓库的模拟提交与强制回退，以及把原排 R07 的完整客户端行为提前纳入 R02。授权没有解除先前真实服务端口操作的 `Operation not permitted` / `PermissionDenied`；本轮没有换人、工具或包装重试等效端口动作。
- `origin` 实际地址为 `https://github.com/ItsDalk-Lane/LingxiAgent.git`，只读 `git ls-remote --heads origin codex/rust-tauri-migration` 返回与本地相同的 HEAD。`upstream` 仍指上游仓库。审计坐标未推进；没有新提交、正式封印或推送。

## 当前合同判定

| 范围 | 当前判定 | 证据与未满足条件 |
|---|---|---|
| R02-T01 / A01–A02 | 叶行为缺口 FAIL；动态 BLOCKED | 无桌面启动及负向根/版本场景的生产者已扩展；R00 的前台 channel 行为尚未完整证明，当前候选真服务未获准运行。 |
| R02-T02 / A03–A04 | BLOCKED | 单实例、目录优先级和 Windows 私有文件路径仍需当前候选跨平台真运行。 |
| R02-T03 / A05–A06 | 原叶行为/证据缺口 FAIL；动态 BLOCKED | 34 原叶中仍有局部定义；认证、Origin、票据、客户端连接、网页退出后会话撤销需要真服务及原叶证据。 |
| R02-T04 / A07–A08 | BLOCKED | 运行中存储故障专属测试和检查器已接门禁，只完成编译/静态检查，真服务故障未运行。 |
| R02-T05 / A09–A10 | BLOCKED | 事件连续性、过期游标、并发/重启要在当前候选真服务上重验。 |
| R02-T06 / A11–A12 | BLOCKED | 在线备份/恢复与损坏库负向需真库、进程和多平台检查。 |
| R02-T07 / A13–A14 | BLOCKED | 敏感日志、轮转、慢客户端、超时清理已做静态配套，压力与真日志未运行。 |
| R02-T08 / A15–A16 | A16 静态合同冲突 FAIL；动态 BLOCKED；阶段 FAIL | A15 原有产品验收缺陷已落静态修正，但当前候选未真跑。A16 E1 要求旧桌面相关路径相对基线完全无变更，且主程序不得出现 Rust 服务入口；本轮获授权提前实现原 R00 客户端行为已触发两条拒绝条件。完整 A16/npm/verify、打包、独立验收、封印与推送均未闭合。 |

八任务的 Goal 是可独立运行且错误可解释的 Rust 服务；Gate 是 16 项基础 A、R00 34 项原叶、全量 npm/A16、完整 verify、构建和必要失败分支均按当前候选合法运行并通过；Handoff 还要有真实接口、版本、错误、资源上限和可复核证据。旧 18 份任务报告的通过仅代表各自旧时点，不替代当前候选验收。R02 阶段最终判定依赖一次全新、完整、独立的阶段验收，以及 `PROGRESS.md` 的正式封印和普通推送。

## R1–R15 统一问题表及本轮根因修复

[修复前统一问题表](R02_FULL_SCOPE_AUDIT_PRE_REPAIR.md#4-统一问题表r1r15修复前)收录 R1–R15 每条 F/O 编号、原生或后修引入、严重程度、历史真实复现与静态推断、涉及入口、旧状态和闭合条件；其第 5 节按 G1–G11 根因合并方案。此冻结表不得改成“已通过”。[R18 统一逐号账](R02_UNIFIED_ISSUES_R18.md)进一步把 52 条阶段级 F/O 与 31 条任务级旧记录放在同一索引，逐项交代要求、证据、影响、同类入口、验证方式和未闭合条件。本轮发现按来源单列：LAN Origin、桌面 Rust 冷启动、真实聊天核心、A07 运行中故障证据、进程清理、Windows 审计/ACL、CLI 状态假绿和打包缺 Rust 程序属于原 R02/R00 范围；打包签名后摘要不一致风险由本轮打包修复引入。它们的静态修正及剩余项见下节。R9-F09 原始产品 **FAIL** 与当时修复编辑被拒是两件不同事实：用户后来撤销重试限制，当前探针已逐项核旧 session、run/event 身份、内容和 1..head 连续性；首个拒绝记录不变，最终候选 A15 真运行仍 BLOCKED。


### 按根因组的现行自查（产品、调用方、失败分支、证据和门禁）

下表的“静态修正”只表示代码和检查定义已接上。依赖服务端口的真实行为都未在当前候选上重验；各组旧首败与已缺失原件按冻结表保留。任务级旧问题 T02/T03/T04/T05/T08 的逐号来源、旧状态和责任见[任务级补遗](R02_TASK_FINDINGS_ADDENDUM_R16.md)。

| 根因组 / 旧问题 | 同类入口与本轮实际处理 | 当前缺口、未修改原因、关闭条件 |
|---|---|---|
| G1 存储身份与事务；R1/R2/R5 | run ID、重启种子、opaque ID、execute 并发、事务提交、A07/A15 的 run/event 读回；沿旧修检查并发分配与库读接口，本轮未改事务语义 | 当前候选真实并发、故障和重启未运行；需真库逐 ID/内容核对，BLOCKED |
| G2 事件连续；R1/R5/T05-R2 | snapshot、hold/release、purge/resume、HTTP/WS、A09/A10/A15；A15 探针现逐事件核 ID、内容、序号和旧/新令牌 | 当前候选竞态/重启和真实 WS 未运行；T05 旧 215/2400 首败继续保留，BLOCKED |
| G3 关停与恢复；R1/R2/R9 | 信号、transport、队列、DB flush、实例记录、数据版本/根选择、A01/A11/A12；A01 负向脚本补齐 CLI/env/config 的拒启和根哈希，清理流程按进程归属检查 | 真超时/版本错配/重启不切根未运行；部分 Windows 路径未编译，BLOCKED |
| G4 新鲜度与版本；R1、R18-N02 | 阶段图、34 叶、16 A、13 命令、候选摘要与原始日志；`verify.rs` 消费原 R00 字段和案例身份、实际/预期/时效，缺案例 BLOCKED、真实错值 FAIL；新 `candidate.rs` 在前/每命令后/结束核 Git 跟踪及非忽略新增文件 SHA-256，漂移强制整体 FAIL | 锁定 Rust 定向 62/62 仅证明门禁逻辑；完整 verify 未合法运行。单条命令内部改后恢复的瞬态不由离散快照检出，最终需外部完整清单与日志一起封存，BLOCKED |
| G5 进程归属与清理；R1–R12 | xtask 和服务探针的正常、失败、EXIT、超时与子进程路径；生产者共用进程组所有权检查并限制清理范围，A01 负向也覆盖启动失败后的清理 | 只完成无端口静态/定向检查；运行中拒绝、PID 复用、异常退出需在允许的真进程上复核，BLOCKED |
| G6 A16 归因；R1/R4–R11 | 纯旧基线、全 npm 首败/复跑、文件身份/详情/汇总、未知失败、seal 族；沿旧修核脚本，未改历史日志或分类口径；[隔离 Git 对照](../../../artifacts/rust-tauri/R02/audit-r18-git-fixture/RESULT.md)真实复现旧污染与现构造纯净，夹具 PASS | E1 的零变更与启动路径零 Rust 字符串合同均与获授权的客户端接入冲突，按原门禁必然 FAIL；全 npm/A16 尚未在当前候选合法执行，隔离夹具不能代替它 |
| G7 资源边界；R1–R4 | HTTP 首字节/头/体、WS 数量、DB 队列与 SQLite 实际上界、慢消费者 A14；沿旧修读配置到真实消费者 | 真压力、错误码和断开后资源回收未运行，BLOCKED |
| G8 安全与证据；R9–R11、T03 | 随机源、认证/Origin/cookie/票据、A04 独立根、A13 真轮转日志；补上 LAN Origin、Web session 和 `/me` 逐行为案例；Windows 日志/SQLite/备份 ACL 拒绝宽权限；桌面在身份确认前不发布连接 | 真服务管理案例和 Windows 编译/运行均缺；Windows 桌面与 CLI 两个旧服务令牌消费者尚未核已打开句柄的 owner/DACL，CLI 连打开前后文件身份也未完整核；T03 协议静态修正仍待真运行，不能沿用旧 PASS |
| G9 A15；R9-F09 | 探针现核旧 session、run/event 的 ID、内容、连续序号与新旧 token；修改后的读取和比较分支均静态核对 | 原产品 FAIL 与旧审批拒绝独立留存；现修未真运行，A15 仍 BLOCKED |
| G10 R00 34 叶；R12–R15 | 原始双账、逐叶 case、生产者/消费者、R02/R07 双责任；客户端两叶补真实 UI/CLI 专属检查；Web session `/me` 补专属案例，21 通用认证假证据不会被判 PASS | 14 叶仍只有局部定义；其中 7 叶需要后续 Agent/模型/工具核心的真实行为，不能用固定回复伪造；其余需真服务证据，阶段 FAIL/BLOCKED |
| G11 交接与身份；R1/R3/R6–R15、T08 | 58 路径开工冻结、18 Task/29 历史报告哈希、远端只读关系、当前 R18 交接；旧报告与错误时间原样留存 | 终稿清单、完整独立验收、正式封印/推送尚无，现行 JSON 导航需在终稿同步 |
| G12 参数预扫描；T02-R1-F01 | 服务 CLI 的 help/version 只允许单独选项捷径，取值选项缺值/未知/重复仍须按严格解析失败 | 源码静态修正；当前候选真实二进制负向未执行，BLOCKED |
| G13 协议后续消费；T03-R1-F01–F04 | 本轮同时修了 WS 客户端掩码/控制帧/长度、运行中设备与 Web session 撤销、升级拒绝状态和 Key、尾斜杠路由；WS 读取另改为独立任务，避免取消半读帧。相应生产者扩展到 44 个案例 | 源码、定向测试和编译静态自查完成；真 TCP 矩阵仍受平台限制，旧问题只能记为“静态已修、验收 BLOCKED”，不能借 R07 放行 R02 |
| G14 运行中故障证据；T04-R1-F05 | QueueFull/写失败的 HTTP 返回、库无成功终态、重启读回；专属故障生产者及门禁已接 | 无端口编译/静态检查不证明线上响应；真服务故障注入 BLOCKED |

本轮原范围内新发现还包括 CLI `status` 把健康但无 Agent/模型视为成功、桌面冷启动假连接，以及安装包未包含 Rust 服务；对应 CLI 退出码/状态、桌面会话发布和四条打包入口都已作静态修正，仍需当前候选实际运行。签名后可执行文件摘要不一致是**本轮打包修复引入**的风险，打包脚本新增归一化摘要和签名核验；真实签名产物仍未验。Windows ACL 合同属于原 R02 多平台/身份范围，但两个消费者文件是在开工冻结后新增，因此它们的已打开句柄 ACL/身份缺口准确归类为**本轮修复引入的同根漏项**；先前只修生产者，漏排消费入口及路径替换/宽权限失败枝，现单列 FAIL，不以生产者静态修正覆盖。

## R00 34 项原叶的当前逐项状态

原始 `then/assertions/due` 全文及责任方见冻结矩阵；这里的“完整定义”只指已登记专属生产者和逐断言案例，绝不代表案例真实执行通过。未完成定义者同时有静态产品/证据缺口。34 项都没有当前候选的完整合法 Gate 结果。

| 原叶 | 原行为摘要 | 门禁定义及真实生产者 | 当前 |
|---|---|---|---|
| `R00-T02-LA-D3710D637C19` | 选择或创建会话后通过 WS 提交消息，终端流式显示回复和工具进度 | 局部：protocol_basis；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-3B86332B042C` | 解析目标会话后恢复其历史并可继续提交，终端显示该会话回复 | 局部：protocol_basis；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-B8A1AD32A8E1` | 打印命令与参数说明并以 0 退出 | 完整定义；`supplemental_client_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-1B09760C2B1C` | 以前台服务进程运行，保留指定 channel 与数据降级约束 | 局部：protocol_basis；`a01_smoke` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-200D4E5D52C9` | 从已认证服务读取并列出最多 20 个会话；空列表显示 No sessions yet | 局部：protocol_basis；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-EEC1A2A5CD04` | 显示 URL、版本、Studio、Agent、模型和认证来源 | 局部：protocol_basis；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-D1BEE19A95BB` | 端口/短期令牌传给 renderer 后 HTTP/WS 连接可建立 | 局部：protocol_basis；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-5816DA563ED8` | 读取已认证主体并签发绑定连接类型及 /ws 的短期票据；无主体返回 403 | 局部：route_basis_present_static；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-093F22C4FF63` | 登记设备/凭证并返回一次性 secret 和桌面访问地址 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-747E0ADC941B` | 登记设备/凭证并返回一次性 secret 和手机访问地址 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-4BCE8CCFD5DC` | 返回版本、身份及按 principal 投影的能力 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-D2657E4AB5FF` | 连接状态、失败原因与重连结果在 UI 可见 | 局部：protocol_basis；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-25AB678E7108` | 仅本地主人可清除密码；更新账号并记录安全审计，失败时返回错误 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-F1754C6F755B` | 仅本地主人可设置密码；更新账号并记录安全审计，失败时返回错误 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-F006E094F028` | 保存 username/displayName 并返回 account | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-73CDC44696D3` | 保存网络配置、更新运行时摘要与安全审计 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-5E4A9CF58BED` | 返回手机访问地址 SVG，LAN 地址不可用为 400 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-229B77A7BA69` | 返回网络、设备和账号概况 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-B8F8CF9E8AF5` | 仅本地主人读取脱敏的设备、凭证与配对会话列表 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-7498024422D7` | 令设备失效并返回脱敏设备状态，记录安全审计 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-43A126149416` | 令指定凭证失效并返回脱敏状态，记录安全审计 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-756217C74101` | 仅本地主人得到配对码及到期时间，注册记录并写安全审计 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-5E1C3363A070` | 仅本地主人用配对码签发设备密钥，返回密钥一次及设备作用域并写安全审计 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-8BC1A036AFAA` | 有桌面构建产物时服务静态页面及资源，否则按路由返回指引或错误 | 完整定义；`supplemental_static_web_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-3291CFD5F7E2` | 有移动端构建产物时服务静态页面及资源，否则按路由返回指引或错误 | 完整定义；`supplemental_static_web_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-2A1C298F62FC` | 返回当前 Agent、语言、头像可用性、工作区和偏好；顺带清理过期持久化 | 局部：auth_primitive_only；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-8ED658F9DB9E` | 返回指定会话或新会话默认级别 | 局部：auth_primitive_only；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-F8935B6B0221` | 有 sessionPath 时只修改该会话，返回解析后的状态 | 局部：auth_primitive_only；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-39AD35E1FD71` | 无 sessionPath 时修改默认级别，不追改旧会话 | 局部：auth_primitive_only；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-066B54983F6A` | 有效 cookie 返回净化后的 principal；无效/过期返回 authenticated:false | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-C4E6F27D7873` | 校验 token 或本地账号密码，创建 14 日 Web session 并设 HttpOnly cookie | 局部：auth_primitive_only；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-4C1A9735CF04` | 撤销当前 Web 会话并清除会话 Cookie，返回成功 | 完整定义；`supplemental_management_matrix` | BLOCKED：真证据/完整 Gate 未完成 |
| `R00-T02-LA-000E6E1301C0` | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | 局部：auth_primitive_only；`a05_a06_auth_matrix` | 产品/证据缺口 FAIL；动态 BLOCKED |
| `R00-T02-LA-32FFEC05BAA7` | 逐动作核对实际状态与页面投影；失败不伪装成空数据 | 完整定义；`supplemental_client_matrix` | BLOCKED：真证据/完整 Gate 未完成 |

静态计数：20 项完整检查定义、14 项局部定义；两项原纯客户端冲突（`LA-B8A1AD32A8E1`、`LA-32FFEC05BAA7`）仍单列，其客户端专属无端口结果不能替代完整阶段 Gate。

## 当前候选的实际检查与原始证据

下表只按实际执行的命令定状态。`PASS` 严格限于该行写明的检查范围，不能向上推成 A 项、原叶或阶段通过。一次检查的 UTC 起止、退出码、stdout/stderr 原件和源码摘要在所链接目录；无端口总批次开始及结束时的 2801 个执行源码文件摘要完全相同，清单 SHA-256 均为 `96a6ec4cb441a7ef3c689308d87a6d6d28b9c977795a413bb7ff3c131a87f948`。环境为 macOS arm64、Node `v24.16.0`、npm `11.13.0`、锁定 Rust `1.98.1`、离线 Cargo。该批次不含真实服务监听。

| 检查 | 状态 | 本轮原件及范围 |
|---|---|---|
| Rust 格式、全 workspace/all targets 编译、严格 clippy | PASS | [无端口批次](../../../artifacts/rust-tauri/R02/audit-r18/final-noport-1/summary.json)中的 `cargo-fmt`、`cargo-check`、`cargo-clippy` 各自 JSON 与 stdout/stderr；均 exit 0 |
| xtask 单元测试 | PASS | 同批次 `xtask-unit` exit 0，56/56；仅检查门禁自身逻辑，不产生 34 叶的真服务证据 |
| R00 管理与静态网页案例编译 | PASS | 同批次 `management-compile`、`static-web-compile` exit 0；`--no-run`，案例没有执行 |
| TypeScript 类型、桌面 Rust 助手定向测试、客户端定向测试 | PASS | 同批次 `npm-typecheck` exit 0、`desktop-rust-helper` 8/8、`targeted-vitest` 11 文件/102 测试；均为无端口范围 |
| `npm run build:client`、锁定工具链的 `npm run build:rust-service`、Rust 包内清单核验 | PASS | 同批次 `build-client`、`build-rust-service`、`verify-rust-package` exit 0；不能证明正式安装或联网功能 |
| `npm run build:server` 首次执行 | FAIL | [首次原件](../../../artifacts/rust-tauri/R02/audit-r18/final-noport-1/build-server.json) exit 1：无 `LINGXI_SIGN_KEY`；旧日志保留 |
| `npm run build:server` 本地复跑 | PASS | [独立复跑原件](../../../artifacts/rust-tauri/R02/audit-r18/final-noport-1/build-server-throwaway-rerun/record.json) exit 0：仅用仓库外一次性测试签名密钥，私钥已删；不代表正式发布签名 |
| 当前源码的 `electron-builder --dir` 正式默认打包尝试 | FAIL | [本轮打包原件](../../../artifacts/rust-tauri/R02/audit-r18/final-noport-1/electron-builder-final/record.json) exit 1：提取、资源核验和临时签名后，缺 Apple 公证凭据而停止；首次中间候选的网络失败原件另存，二者不可互改 |
| 仅本地结构用途的 `npm run build:shell` | PASS | [本地结构包原件](../../../artifacts/rust-tauri/R02/audit-r18/final-noport-1/build-shell-local/record.json) exit 0；只用临时公开 keyset，关闭正式签名自动发现及公证。包结构、seed 与 Rust 程序核过，未安装启动，也不是正式公证包 |
| R02 脚本语法、现行 JSON、差异空白、交接本地链接 | PASS | [30 项静态核查原件](../../../artifacts/rust-tauri/R02/audit-r18/static-final.json)全部 exit 0；不运行脚本行为 |
| Windows 生产者 ACL 代码 | BLOCKED | [Windows 专项静态记录](../../../artifacts/rust-tauri/R02/windows-acl-r18/WINDOWS_ACL_STATIC_CHECK.md)：macOS 检查和定向测试通过，Windows target 未装，Windows 分支未编译/运行；旧宽 ACL 数据目录的兼容性还需实机验证 |
| Windows 桌面与 CLI 的旧服务令牌读取路径 | FAIL | `desktop/src/shared/rust-local-service.cjs` 只核已打开句柄/前后路径身份，未核该句柄 owner/DACL；`cli/rust-service.ts` 还缺等价的句柄/路径身份及 ACL 检查。原合同属 R02；两文件开工后新增，具体缺陷由本轮后修引入，Windows 真运行未复现。路径替换、宽权限失败枝尚未修闭；不能以 Rust 生产者已设 ACL 代替 |
| T03 旧四项协议问题 | BLOCKED | [T03 当前静态/定向原件](../../../artifacts/rust-tauri/R02/audit-r18-t03-open/t03-stable-check.meta.json)以及相邻 WS、设备、Web 会话记录：源码修正、无端口测试通过；44 项真协议矩阵未合法运行 |
| 仅隔离 Git 测试仓库的 A16 基线构造对照 | PASS | [19 命令与首败/复跑原件](../../../artifacts/rust-tauri/R02/audit-r18-git-fixture/RESULT.md)，只说明旧基线污染已真实复现、现构造在夹具中纯净；真实 A16 未运行 |
| 全量 `npm test` 与其首败、复跑分类 | BLOCKED | 当前候选未执行；先前平台拒绝该受影响完整动态检查，过去变通观察不升级为本候选结果。需相应平台限制真实解除后原命令运行并留首败与复跑 |
| A01–A16 当前候选的完整服务、故障、重启与旧基线矩阵 | BLOCKED；A16 E1 另有静态 FAIL | 未运行真服务/TCP/完整 A16；此前真端口操作遭 `Operation not permitted` / `PermissionDenied`，不得换工具或执行者达到相同效果。A16 原 E1 本身仍与客户端提前接入冲突 |
| 原始 34 叶的完整生产者与完整 `xtask verify-stage R02` | BLOCKED；其中 14 叶另有静态 FAIL | 34 叶的当前候选真行为原件不齐，20 项只是完整检查定义；当前完整 verify 未执行，不能将单元测试替代 Gate |
| 全新独立静态复核 | FAIL | 新评审者一次对照八任务、16 A、34 原叶、R1–R15 的 52+31 项、Goal/Gate/Handoff、代码和证据；确认 14 叶局部定义、A16 E1 冲突、Windows 两消费者新缺口和门禁未提交字节绑定缺口。只读静态结论不是完整阶段验收 |
| 完整独立阶段验收 | 未执行 | 依赖真服务、全 npm/A16、34 叶、完整 verify 和多平台合法结果；全新静态评审不能代替这些检查，不能预写 PASS |
| 正式封印、最终检查、普通推送 | 未执行 | 阶段合同未过，未依 `PROGRESS.md` 推进封印；没有提交或推送。`origin` 已核对，不能用远端可访问代替已推送 |

## 阻碍、恢复条件与交付边界

1. **产品/证据缺口：FAIL。** 14 项原叶的完整行为或专属证据仍缺，其中 7 项需要真实 Agent、模型与工具核心，不能用固定文本或基础认证案例代替。其余前台 channel、CLI 会话列表、桌面真实 HTTP/WS、票据与 UI 状态、手机完整启动和设置实际动作仍需逐项生产证据。若获准将核心提前纳入 R02，须实现其真实调用与所有失败分支，再补逐叶生产者；仅新增检查字段不会关闭缺口。提前实施完整客户端行为已有授权，但不等于这 14 项已经通过。
2. **A16 原合同冲突：FAIL。** E1 固定旧桌面相关文件与启动路径零 Rust 变化；已获授权的客户端提前接入触及该范围。原门禁未改，不能删门禁或放宽断言求绿。是否把 A16 改为“旧模式行为保持原样、逐入口执行旧测试、显式区分新模式”的合同问题已向用户提出，尚无答复；在有明确决定及对应原要求处理前按原合同 FAIL。
3. **真端口和完整动态门禁：BLOCKED。** 先前真实端口执行遭平台拒绝；恢复条件是平台对相同动作真实放行，届时直接按合同执行 A01–A16、全 npm、所有 34 叶生产者、完整 verify、故障与重启。需每次留命令、UTC 起止、退出码、原始日志、环境和当前候选完整摘要。沉默或换人/命令包装都不构成放行。
4. **多平台与正式包：BLOCKED/FAIL。** Windows target 与实机缺席；桌面及 CLI 令牌消费者尚有同根因产品缺陷，需同时修闭已打开句柄 ACL/身份检查与路径替换、宽权限失败枝，并在 Windows 核新旧目录和服务/桌面/CLI 入口。Mac 默认包当前 exit 1，需正式可用的签名及公证环境，并安装启动当前包后核程序身份、资源与升级路径；本地跳过公证的结构包只作静态观察。其他发行平台也需合同所需环境。
5. **A15 独立状态。** R9-F09 首次真实产品 FAIL 和当时平台拒绝编辑的历史均保留。用户后来授权按平台审批处理，当前探针已在源码中补 run/event 身份、内容、连续性及旧/新凭证核对；**仅静态已修**，真重启 A15 当前候选仍 BLOCKED，不得写“已关闭”。
6. **正式收尾。** 全新独立评审须依据开工冻结表一次覆盖全阶段，再依据真实 PASS 证据、当前候选摘要和完整合法门禁确认；之后才按 `PROGRESS.md` 执行封印、最终检查及授权范围内的普通推送。任何缺项使 R02 保持 FAIL，R03 不得启动。

## 全新独立静态复核及取证缺口

全新评审者已按开工冻结表完成一次**只读全范围静态复核**，结论 **FAIL**；它不能签发依赖动态门禁的完整独立阶段 PASS。核对结果确认 16 A 与 34 原叶身份齐全，20 项完整检查定义/14 项局部定义；A15/T03 从代码看有修正但未真运行，A16 E1 按原合同必然 FAIL。新增问题的完整字段在[统一问题账 R18-N01/N02](R02_UNIFIED_ISSUES_R18.md#r18-全新独立静态复核新增项不混入-r1r15-原编号)。

- R18-N01：Windows 桌面与 CLI 两消费者的同句柄权限/身份检查缺口。原安全要求在 R02 合同内，**具体缺陷由开工后新增的两文件引入**；此前只扫 Rust 生产者而遗漏两个消费者。这是 MAJOR 静态推断，Windows 未真实复现，当前产品 FAIL。
- R18-N02：完整门禁原先只写 HEAD 与是否改动，不能单独证明未提交候选的全部字节。原 R02 证据链设计缺口，MAJOR 静态推断。本轮源码已加门禁前、每命令后、结束的逐文件 SHA-256；[锁定 Rust 定向结果](../../../artifacts/rust-tauri/R02/r18-gate-binding/RESULT.md) fmt/check/clippy exit 0、xtask 62/62。当前外部 10496 文件清单只证明先前静态冻结；正式 Gate 仍须合法运行并连同其前后摘要及原始日志封存。单条命令内部瞬时改后恢复不是离散快照能证明的范围，不可据此宣布阶段 PASS。

## 文件清单和摘要

[开工前 58 路径](../../../artifacts/rust-tauri/R02/audit-r16/prechange-status-corrected.txt)与[9716 文件完整摘要](../../../artifacts/rust-tauri/R02/audit-r16/prechange-full-sha256-corrected.txt)是不可改的比较基准。12:10 UTC 的[当前 10496 文件完整摘要](../../../artifacts/rust-tauri/R02/audit-r18/candidate-final-full-sha256.txt)、[Git 状态](../../../artifacts/rust-tauri/R02/audit-r18/candidate-final-status.txt)和[逐路径差异 JSON](../../../artifacts/rust-tauri/R02/audit-r18/pre-to-final-diff.json)列出新增 780、内容改变 94、删除 0、未变 9622，摘要文件本身的 SHA-256 为 `6967b614a76c9f3f87fd54be3b5caf30b1bde2c30fea3abf8d509ede5df94139`。清单覆盖 Git 已跟踪和非忽略新增文件，只排除三个会自引用的清单输出；忽略的构建/缓存不作为候选源码。独立复核如要求后续修正，须重新生成清单和受影响检查，旧摘要留作历史观察，不能冒充新候选。
