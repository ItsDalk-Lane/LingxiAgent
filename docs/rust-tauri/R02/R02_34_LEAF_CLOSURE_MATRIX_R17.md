# R02 的 34 项 R00 补充场景：完整闭合缺口

状态：**工作中的逐项静态核对**，不是运行结果或阶段验收。原始 `then / assertions / due` 全文、身份、R02/R07 双责任和旧阶段图记录，保存在 [开工时冻结矩阵](../../../artifacts/rust-tauri/R02/audit-r16/contract-matrix-prechange.json)。本表每行按该原文核对正向、拒绝、失败后的实际副作用；原件不改。用户现已明确授权把原排 R07 的完整客户端行为及证据提前纳入 R02，因而下列“原 R07 部分”现在也必须在 R02 真正完成，不能因旧分工豁免。

## 已有接线与共同门禁缺口

R17 开工时 Rust 服务静态可见的路由仅为 `/lingxi/v1/health`、`/me`、`/sessions`、`/sessions/{id}`、`/sessions/{id}/execute`、`/sessions/{id}/events`、`/ws-ticket`、`/devices/credentials`、`/ws`。旧 `cli/client.ts` 请求 `/api/*` 与 `/ws`，旧 `cli/server-runner.ts` 启动 Node。当时 `SharingTab` 检查只断言字体默认显示，21 项 `auth_primitive_only` 没有各自的 Rust 路由。以下逐项差距以该开工快照为基准；并行修复正在改变代码，最终须再按候选及真实运行结果逐项回填。

现有阶段图的 34 项均引用 R00 原字段。R15 时 21 项复用通用认证案例，11 项只覆盖协议/路由局部，2 项纯客户端冲突固定 `BLOCKED`。R17 已为后两项登记专属生产者及原断言到案例的映射，独立首轮运行记录见本轮证据目录；阶段 Gate 尚未完整验收。其余场景仍需逐行由本轮真实程序产生正向、拒绝、故障、副作用及客户端投影证据，门禁核对案例身份和内容。共同证据格式要绑定新鲜运行目录、候选摘要、生产者退出、原始输出及持久化前后对照；当前通用状态码不能替代下面任何完整场景。

| 原叶 ID 尾号 | 原行为与必须覆盖的反面条件（详见冻结原文） | 当前 Rust/客户端缺口 | 闭合所需真实证据 |
|---|---|---|---|
| `D3710D637C19` | CLI 新建/选中会话，WS 提交并流式显示回复/工具；连接或身份失败非零，错会话帧不显示，Ctrl+C 仅取消当前流 | Rust WS/execute/event 原语已有，旧 CLI 协议路径和消息形态未适配 | CLI 对 Rust 的终端实跑；流式内容和会话/stream 身份；三条拒绝/取消分支 |
| `3B86332B042C` | CLI 续接目标会话、历史并继续提交；不存在/越权不悄悄新建或串会话 | Rust session 读取局部已有，旧 CLI 目标解析和 API 未适配 | 续接前后同一会话/run/event 身份；404、403 后库中无新会话 |
| `B8A1AD32A8E1` | CLI help 成功为 0；未知参数打印错误和帮助、以 1 退出，不启动服务 | 开工时纯客户端冲突；旧 `cli/args.ts` 会把一些未知 flag 放进 `passthrough`。R17 已修参数入口并新增三项真子进程证据，仍须按正式候选复核 | 命令级 stdout/stderr/退出码及前后进程、数据根不变；正反参数矩阵。不能用独立生产者首轮通过代替完整 Gate |
| `1B09760C2B1C` | 前台 `serve` 保留 channel/数据约束；启动或版本失败明确错误，不换数据根 | Rust 前台服务已有，旧 CLI `serve` 仍起 Node；A01 旧证据仅正向与退出，新增四项负例刚接入图钉 | CLI 启动 Rust、channel/降级选项真实行为；启动失败和过新数据版本的非零/错误/无 READY/双根不变；A01 原始日志 |
| `200D4E5D52C9` | CLI 最多列 20 个已认证会话、空态 `No sessions yet`；断连/未授权报错，不读他人 | Rust `/sessions` 归属过滤已有局部案例，旧 CLI 请求 `/api/sessions` 且预期数组 | CLI 数量/排序/空态输出；401/断连/他人隔离与数据库会话身份 |
| `EEC1A2A5CD04` | CLI 状态显示 URL、版本、Studio、Agent、模型、认证来源；断连诊断、身份失败不能伪造来源 | Rust health 最小化，`/me` 只回 principal；旧 CLI 用旧 `/api/*` | 六项字段来源与显示；断连与身份失败两分支，认证来源不能由默认值冒充 |
| `D1BEE19A95BB` | 端口/短令牌传 renderer 建立 HTTP/WS；重启重取，令牌不露为设置 | Rust 令牌/WS 票据局部已有；桌面壳到 Rust 的引导未接 | 启动/重启两轮 renderer 真实连接、令牌轮换、设置面不显示秘密 |
| `5816DA563ED8` | `/ws` 票据绑定身份、连接类型与目标；无主体 403；无效/越权/下游错不呈成功 | Rust `/ws-ticket` 有路由和无主体修复；完整客户端消费与下游故障缺 | 票据内容/一次性/目标绑定、403、错误传播与无副作用；HTTP 到 WS 联动 |
| `093F22C4FF63` | 登记凭证并一次返回 secret 与桌面地址；拒绝不持久化错数据 | Rust `/devices/credentials` 返回 secret 等，却未返回桌面地址或完整安全审计 | 本地主人登记、一次性 secret、桌面 URL、库及审计；拒绝前后记录不变 |
| `747E0ADC941B` | 同上，但返回手机地址 | 同一 Rust 端点未返回手机 URL；手机入口未接 | 手机 URL、secret 一次性、作用域、拒绝不持久化与审计 |
| `4BCE8CCFD5DC` | 返回版本、身份及按 principal 投影能力；拒绝不可泄漏或错持久化 | Rust `/me` 目前仅回 principal，版本和能力投影缺 | 不同身份的版本/能力差异、401/403 与敏感字段不泄漏 |
| `D2657E4AB5FF` | UI 显示连接状态、失败原因和重连结果；坏端口/令牌/服务重启不能假在线 | Rust 健康与错误码局部已有；renderer 状态/重试尚未接 Rust | UI 断开、原因、自动/手动重试及恢复截图/状态记录；无假在线 |
| `25AB678E7108` | 仅本地主人清除密码、更新账号并审计；失败状态与副作用一致 | Rust 无账号密码移除路由和持久化行为 | 成功账号/审计前后；非主人/存储故障拒绝且密码不变 |
| `F1754C6F755B` | 仅本地主人设置密码、更新账号并审计；失败显错 | Rust 无账号密码设置路由 | 成功登录验证及审计；非主人、冲突、失败后原凭证有效 |
| `F006E094F028` | 保存 username/displayName 并返回 account；非法/越权不持久化 | Rust 无账号资料修改路由 | 返回与数据库一致；非法/越权前后账号摘要不变 |
| `73CDC44696D3` | 保存网络配置、更新运行摘要并审计；拒绝不落错状态 | Rust 无对应管理路由/配置原子更新 | 正向配置与运行摘要/审计一致；非法、远端、写失败不变 |
| `5E4A9CF58BED` | 手机地址二维码 SVG；LAN 不可用 400 | Rust 无二维码端点 | SVG 内容编码实地址；LAN 关闭 400；拒绝无越权 |
| `229B77A7BA69` | 返回网络、设备、账号概况；失败不伪装空数据 | Rust 无访问概况路由 | 多源数据与真实存储一致、无权限/下游错显错而非空数组 |
| `B8F8CF9E8AF5` | 仅本地主人读取脱敏设备、凭证、配对列表 | Rust 有签发但无列表/脱敏路由 | 所有三类列表、秘密不回传、远端/下游错误 |
| `7498024422D7` | 撤销设备，返回脱敏状态并审计；拒绝不失效其他设备 | Rust 无设备撤销路由 | 目标设备失效、旧凭证拒绝、审计和其他设备保持有效 |
| `43A126149416` | 撤销指定凭证、脱敏返回并审计；拒绝不误撤 | Rust 无凭证撤销路由 | 精确 credential ID、旧 secret 拒绝、审计与其他凭证有效 |
| `756217C74101` | 仅本地主人创建配对码、到期记录并审计 | Rust 无配对会话路由 | 配对码/期限/持久化/审计；越权和失败不创建 |
| `5E1C3363A070` | 仅本地主人用配对码一次签发设备密钥与 scope，并审计 | Rust 无批准配对路由 | 正确 code/设备/scope/secret 一次性；重复/过期/越权不签发 |
| `8BC1A036AFAA` | 桌面静态资源存在则服务资源，不存在给路由指引/错误 | Rust 无桌面静态页面托管 | 有产物页面/资源/类型/权限；无产物明确指引；错误不伪成功 |
| `3291CFD5F7E2` | 移动静态资源存在则服务资源，不存在给路由指引/错误 | Rust 无移动静态页面托管 | 手机页面/资源与缺产物两支；权限和路径穿越拒绝 |
| `2A1C298F62FC` | 初始化手机端：Agent/语言/头像/工作区/偏好，清过期持久化 | Rust 无移动引导路由/清理 | 响应与实际配置一致，清理前后对照；拒绝/下游失败不冒空态 |
| `8ED658F9DB9E` | 读指定会话或新会话默认思考级别；query 边界不扩大披露 | Rust 无思考级别读取路由 | sessionPath/pendingNewSession 两支及越权/无效边界 |
| `F8935B6B0221` | 有 sessionPath 只修改该会话；无效 level/409/拒绝保持原状态 | Rust 无会话级别更新路由 | 目标/默认/其他会话前后差异及错误不变 |
| `39AD35E1FD71` | 无 sessionPath 只改默认级别，不追改旧会话；失败不变 | Rust 无默认级别更新路由 | 默认/旧会话前后差异及 409/拒绝不变 |
| `066B54983F6A` | 有效 cookie 回净化 principal；无效/过期为 `authenticated:false`，不漏 secret | Rust 无 Web session 查询路由 | 有效、无效、过期 cookie 与作用域；响应秘密扫描 |
| `C4E6F27D7873` | token 或密码登录，14 日 session、HttpOnly cookie；HTTPS/scope/credential 优先级 | Rust 无 Web 登录路由或 cookie 会话存储 | token/密码正向、优先级、HTTP 远端拒绝、过期和 cookie 属性 |
| `4C1A9735CF04` | 注销当前 Web session 并清 cookie；失败副作用一致 | Rust 无 Web 登出路由 | 登出前后 cookie 与 session 失效，其他 session 不受影响；失败不假成功 |
| `000E6E1301C0` | 设置访问页：初始化、概况、网络、凭证/二维码、连接、撤销、账号等每动作成功/空态/错误投影 | React 旧 AccessTab 可复用；R17 先真实复现“读取失败误显暂无设备”，产品修正并复跑；新增凭证生成、无剪贴板投影及 Rust/旧连接路由与二维码真实组件检查，只覆盖原断言第 2、4、5 条的一部分，完整原 7 条及真实服务仍无闭合证据 | 按 R00 原 `assertions` 每动作的三态完整 UI 记录 + API/存储前后对照；重读失败和剪贴板拒绝不得被吞。局部 UI 首次 FAIL 见 `r17-leaf-gate/access-ui-probe-3`，修后 PASS 见 `access-ui-probe-5` 和 `access-route-probe-2`，仍为 BLOCKED |
| `32FFEC05BAA7` | 分享设置页：截图配色/宽度/字体/分段上限、默认值与 localStorage 失败 | 开工时纯客户端冲突；旧 SharingTab 测试仅看默认字体。R17 已新增真实组件七项检查并把四项案例接入 Gate，仍须按正式候选复核 | 每一控件默认/写入/预览投影、规范化、禁止存储抛错及不伪成功；独立首轮通过不等于阶段 PASS |

## 可复用实现路径与门禁接法

1. **Rust 管理 API 族**：在现有认证中间件下实现账号、设备/配对、访问概况、网络、二维码、思考级别、Web session、静态资源。逐入口沿用当前 `Principal`/`RoutePolicy`/持久化事务边界；旧 `server/routes/*` 仅作行为对照，不把 Node 当 Rust 服务运行依赖。每个写入口必须测真实库状态和安全审计，不能用 HTTP 状态码代替副作用。
2. **客户端适配族**：复用旧 CLI 的参数/输出与 React 页面，建立指向 `/lingxi/v1/*` 的适配层，前台 `serve` 启 Rust。桌面引导负责端口/短令牌、服务重启轮换与连接状态。按 R00 原文逐动作验证，旧 Node 测试通过不代表新 Rust 入口通过。
3. **独立生产者与 Gate**：每叶登记专属完整案例身份，运行时证据来自真实 CLI/UI/Rust 服务及合成数据根；包含成功、拒绝、故障后的状态快照，当前候选新鲜目录。`verify-stage` 继续完整匹配 R00 两原账并失败优先，34/34 完整证据齐备前不从 `BLOCKED` 改 `PASS`。11 项局部协议证据可保留作基础，但不能顶替客户端余项。

## 本轮门禁具体修补

`verify.rs` 旧读法对同名案例用第一条 `.find`，后条失败可能被忽略；本轮改为同一证据文件所有案例均须结构完整、身份唯一且自身 `ok/actual/expect` 一致，另对阶段图内同一叶的重复图钉身份做解析拒绝。`LA-1B09760C2B1C` 已在阶段图登记四个 A01 负面案例；生产脚本和真实运行需分别核验，未运行不写成通过。

R17 已独立运行客户端证据生产者，7/7 案例通过；原始命令、退出状态、起止时间、文件摘要及原始 stdout/stderr 在 `artifacts/rust-tauri/R02/r17-leaf-gate/client-probe-4/`；probe-1/2/3 是旧 CLI 候选观察，均原样保留。AccessTab 新发现的假空态首跑 5/6 FAIL，产品修正后补足加载中/成功空态并复跑 7/7；两种连接身份的请求与二维码路径复跑 2/2，`tsconfig.test.json` 类型检查通过。各次原日志分别在 `access-ui-probe-3/`、`access-ui-probe-5/` 与 `access-route-probe-2/`。这些检查没有覆盖访问页原 7 条断言和 Rust 真实服务，因此仍是局部观察。xtask 整包获平台允许后复跑 56/56 通过，先前新夹具 1 项失败的原日志和修正后日志均保存在 `r17-leaf-gate/`。这些结果不替代当前正式候选的完整 `verify-stage`、34 叶验收或独立阶段评审。

随后 Rust 管理入口和标准安全审计 JSONL 已接入服务，不能再把上表“开工时缺路由”当作当前实现。管理独立生产者在合成数据根上真实启动服务，逐项核账号、网络、二维码、设备、凭证、配对、网页登录的正反及存储失败，现定义 **59 个固定身份案例**；它还把原始 `security-audit.jsonl` 和两份已提交审计源意图保存到独立证据目录，并双重核对动作/目标/metadata/时间、主体、eventId 不重复及 secret 不外泄，并验证网络设置保存→停服→重启真实绑定、显式 CLI 覆盖、损坏记录拒启。新增第 59 项核标准审计日志路径故障时管理与设备两类写入均拒绝且不提交，路径恢复后日志和读取正常；这是静态新增的测试定义，尚未运行。`audit-r17-management-probe-9` 58/58 PASS，命令 exit 0、运行前后源码摘要一致；这是当时候选观察，随后 service 源码再次变更，正式 Gate 必须在新目录重跑。此前 `probe-4` 的离线依赖缺失、`probe-5` 的测试字段错误、`probe-6` 的旧故障注入与 fresh reload 冲突、`probe-7` 把旧 Node 合法的 `deviceKind=unknown` 错当非法、`probe-8` 的测试路径错误及源码漂移，都分别保留为原始 FAIL，不改写。`probe-7` 后改用真正非法的 `invalid-kind`，旧 Node 原合同核对见 `core/device-registry.ts`。

手机、桌面静态页面独立生产者用真实 TCP 核有构建产物时的页面/资源与内容类型、无产物的指引、两条路由各自的路径越界、损坏构建、超限资源错误，现固定 **9 个案例**。`audit-r17-static-probe-1` 的 8/8 与 `probe-2` 的 9/9 测试本体均过，但两轮运行期间 service 源码改变，两个生产者均按摘要不一致判 **FAIL**；稳定源码后的本轮真 TCP 检查因平台 bind `PermissionDenied` 被拒，必须保持 BLOCKED，不能把案例局部通过写成叶 PASS。

阶段图当前为 **18 个完整行为门禁定义**（2 客户端、14 管理、2 静态），另 **16 个仍为局部 BLOCKED**（7 协议、7 认证基础、2 路由基础）。每个完整定义都明确把原 R00 的两条或多条断言映射到固定案例；这只说明门禁有了完整检查办法，尚未表示当前候选通过。客户端 `audit-r17-client-probe-6` 在当前所核 CLI/Sharing 源码摘要下独立 7/7 PASS、exit0、进程组无残留；这只支持两个客户端叶的专属证据，完整 Gate 尚未执行。`xtask` 整包本轮首次 53/56 FAIL 的 3 项仅因当时默认沙箱对 `ps` 返回 `operation not permitted`，原日志 `audit-r17-leaf-xtask-run-2` 保留；获平台允许的同一检查 `run-3` 56/56 PASS、起止 xtask 源码摘要一致。新增审计故障案例的 Rust 测试只完成无端口编译（`management-audit-negative-compile-3`，exit0），新增门禁映射只完成无端口定向 2/2（`audit-map-targeted-1`）；两者均不等于案例运行。稳定源码后的管理新目录 `audit-r17-management-probe-10` 在首个真实 bind 就得到 `Operation not permitted`、Cargo exit101、0 案例，原始日志保存；按平台边界已停止这项和静态网页真 TCP 的等效重试。Web HTTPS/凭证、CLI 聊天模型和工具流、移动引导权威源、思考级别真模型来源等原始断言仍有缺口；完整 `verify-stage`、34 叶独立验收和正式封印未完成，R02 不能 PASS 或进入 R03。

## 当前 34 项逐叶门禁状态

下表是现行阶段图的**检查定义**，不代表阶段结果。已具备完整定义的叶项仍需绑定最终候选在新目录合法执行；其余叶项保留 BLOCKED。上表是开工快照，不能与此列混用。

对仍未完整的 16 项，按原行为做静态依赖划分：`D3710D637C19`、`3B86332B042C`、`EEC1A2A5CD04`、`2A1C298F62FC`、`8ED658F9DB9E`、`F8935B6B0221`、`39AD35E1FD71` 共 7 项涉及真实聊天回复、工具进度、Agent/模型权威状态或思考级别，完整闭合必须把原计划 R03–R05 的相应核心提前实现；仅补路由或造固定回复不足以通过。`1B09760C2B1C`、`200D4E5D52C9`、`D1BEE19A95BB`、`5816DA563ED8`、`4BCE8CCFD5DC`、`D2657E4AB5FF`、`066B54983F6A`、`C4E6F27D7873`、`000E6E1301C0` 共 9 项主要缺 R02 入口、客户端接线或完整证据，不以 R03–R05 模型工具核心为必然前提。此划分是当前代码和原合同的静态判断，后续独立评审应重点复核移动引导与设置容器的 Agent/模型数据权威边界；所有 16 项仍保持 BLOCKED。

桌面 Rust 接线的新增静态发现归于这 9 项现有缺口：`D1BEE19A95BB` 的本机冷启动没有 Rust 服务类型标识，仍走旧 `/api/server/identity`；`desktop/main.cjs` 的本机服务复用也仍查旧身份路由；识别已保存的 Rust 远程身份后，`app-init.ts` 后续仍查询旧 `/api/health`、`/api/config`、`/api/agents`，不能凭单次身份请求判整条 HTTP/WS 连接通过。此轮已在源码修掉同族可独立处理的失败分支：`5816DA563ED8` 客户端遇 200 空票据即拒绝且不开无认证 WebSocket；`D2657E4AB5FF` 的 Rust 握手核协议范围、数据版本与服务身份，缺失/不符或 10 秒无回复都保持断开并重试，状态栏显示受控失败原因和短暂恢复结果；主窗口、设置窗口重启桥缺/坏端口令牌或复用旧令牌时清除旧本机连接，不沿用旧值，后续有效事件可恢复且保留原 Rust 类型标识。无端口五组定向测试在修复后 `desktop-ws-targeted-8` 63/63 PASS、`desktop-ws-typecheck-5` exit0，运行前后相关源码摘要一致；第一次测试失败及中途同根因漏修失败原件保留。以上只是静态与无端口结果；三条原叶所需的桌面壳→真实 Rust HTTP/WS、重启和 UI 端到端证据仍未执行，均保持 BLOCKED。

后续 N02/N03 同组静态修复：桌面壳现可经显式 `LINGXI_DESKTOP_SERVER_RUNTIME=rust` 选路启动 Rust，启动后核对本轮实例记录、进程、READY 地址、短令牌和认证身份；复用既有 Rust 服务也须身份认证成功，旧 Node 实例记录存在时拒绝双开。主窗口、设置窗口、快速对话和首次设置均取得服务类型及 HTTP(S) 传输；重启事件带同一身份，坏字段不恢复旧连接。Rust 连接的旧 `/api/*` URL 在公共构造器处直接失败，主窗口不再把旧初始化的失败伪装成空配置，快速对话和首次设置不把旧服务入口当 Rust 可用；旧聊天帧在准备和提交两处阻断，状态栏明确显示聊天核心尚未迁移。`r17-n02-n03-desktop/no-port-final` 的定向无端口测试、语法检查、语言文件解析和后续类型复跑均 exit0；首次测试串扰/状态显示漏修及首次类型失败日志分开保留。该结果只证明源码和无端口行为，不能把 `D1BEE19A95BB`、`D2657E4AB5FF` 升为 PASS：真实桌面启动/HTTP/WS/重启联动因平台已拒绝同类端口动作而 BLOCKED；Rust 聊天模型/工具流和发行包装尚无完整产品实现。

后续同组打包修复：四条 `pack`/`dist` 入口现先用根目录固定的 rustup `1.98.1` 编译当前平台 Rust release 程序，`extraResources` 装入 `rust-service/`，构壳和装箱钩子比对平台、架构、全部 Rust 工作区文件、工具链、应用版本及程序摘要；macOS 把裸程序纳入签名并在签名后核签。Windows 签名允许改 PE 校验和与证书表，但程序正文必须与构建记录一致，安装版启动时还须与主程序具有同一有效签名者。打包桌面显式 Rust 模式固定从应用资源目录取程序，先激活原签名页面归档，并要求其版本与壳及 Rust 程序一致，再复用或启动 Rust 服务；不会由 `LINGXI_SERVICE_BIN` 换掉打包程序。独立内容 OTA 尚无与旧 Rust 程序兼容的已验证坐标，因此在显式 Rust 模式下版本不同时明示拒绝。无端口记录见 `artifacts/rust-tauri/R02/r17-rust-desktop-package/no-port-final-1/record.json`：当时的 mac arm64 release 编译和程序摘要检查、定位测试 5/5、壳清单 45/45、main bundle 构建及语法检查均 exit0，检查前后所涉 11 文件摘要一致；但该 release 实际误用了 PATH 上的 Homebrew Rust `1.93.0`，且此后 Rust ACL 源及打包核验代码继续修改，所以全部只是中间候选观察，不能作为最终发行程序。独立目录的真 `electron-builder --dir` 装箱曾运行，因下载 Electron 42.8.1 时 `github.com` DNS `ENOTFOUND` 而 exit1；原始日志见 `artifacts/rust-tauri/R02/r17-rust-desktop-package/package-structure-1/check-02.log`，故安装包实际内容、签名及运行仍未验证。Windows 本地令牌读取补了打开前后文件身份比较，但 Node 层尚不能可靠确认 ACL 为仅属主可读及父目录重解析点；此项仍是静态缺口。上述代码和无端口通过不改变两条客户端完整叶的 BLOCKED，也不证明 Rust 聊天模型/工具流已迁移。

| 原叶尾号 | 检查定义 | 当前候选结论 | 生产证据入口 |
|---|---|---|---|
| `D3710D637C19` | 仅协议局部 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `3B86332B042C` | 仅协议局部 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `B8A1AD32A8E1` | 原断言案例已逐项钉住 | 专属 7/7 PASS；完整 Gate 未执行 | `supplemental_client_matrix` |
| `1B09760C2B1C` | 仅协议局部 | BLOCKED：完整原行为仍缺 | `a01_smoke` |
| `200D4E5D52C9` | 仅协议局部 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `EEC1A2A5CD04` | 仅协议局部 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `D1BEE19A95BB` | 仅协议局部 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `5816DA563ED8` | 仅路由局部 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `093F22C4FF63` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `747E0ADC941B` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `4BCE8CCFD5DC` | 仅路由局部 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `D2657E4AB5FF` | 仅协议局部 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `25AB678E7108` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `F1754C6F755B` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `F006E094F028` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `73CDC44696D3` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `5E4A9CF58BED` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `229B77A7BA69` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `B8F8CF9E8AF5` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `7498024422D7` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `43A126149416` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `756217C74101` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `5E1C3363A070` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `8BC1A036AFAA` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_static_web_matrix` |
| `3291CFD5F7E2` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_static_web_matrix` |
| `2A1C298F62FC` | 仅认证基础 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `8ED658F9DB9E` | 仅认证基础 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `F8935B6B0221` | 仅认证基础 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `39AD35E1FD71` | 仅认证基础 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `066B54983F6A` | 仅认证基础 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `C4E6F27D7873` | 仅认证基础 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `4C1A9735CF04` | 原断言案例已逐项钉住 | BLOCKED：本轮真 TCP bind 被平台拒绝 | `supplemental_management_matrix` |
| `000E6E1301C0` | 仅认证基础 | BLOCKED：完整原行为仍缺 | `a05_a06_auth_matrix` |
| `32FFEC05BAA7` | 原断言案例已逐项钉住 | 专属 7/7 PASS；完整 Gate 未执行 | `supplemental_client_matrix` |
