# R08-T03 冻结策略矩阵（freeze-policy matrix）

> 任务书第 4 条：关窗口 / 托盘退出 / 完全退出按冻结策略矩阵执行，退出超时报告残留 PID。
> 纪律：每格必须代码核对（行号锚定）或集成证据；win32 格标平台限定，不虚报。
> 代码核对基线：批次 C 实施时的 `desktop/main.cjs` 工作树（行号见各格）。
> 集成证据：A05（批次 D，local-owned）与 A06（批次 E，remote）演练补「实测」列。

## 模式列定义与全局归零事实

| 列 | 进入路径 | `serverProcess` | `reusedServerPid` / `reusedServerOwned` |
|---|---|---|---|
| node（生产基线） | startServer() dev/打包 spawn（main.cjs:2194） | 有值 | null / false |
| rust local-owned | startRustServer spawn（main.cjs:1437 起） | 有值 | null / false |
| rust local-reused | instance.json 复用（main.cjs:1391-1405） | null | pid / **false** |
| rust remote | readRemoteRustConnection 分支（main.cjs:1358-1373） | **null** | **null / false** |

关键：远端分支 publish 后显式 `serverProcess=null; reusedServerPid=null; reusedServerOwned=false`（结构性
零接触）；rust 复用实例永远 `reusedServerOwned=false`（外来实例不归本壳所有）。

`before-quit` 的接触判据（main.cjs:6966-6968）：
`hasActiveOwnedServer = (serverProcess 未退出) || (reusedServerPid && reusedServerOwned)`；
`resolveBeforeQuitServerAction`（desktop/src/shared/windows-server-guardian.cjs:63-68）：
`state==='complete' || !hasActiveOwnedServer → 'allow'`（不 preventDefault、不调 shutdownServer）。

## 矩阵本体

### 行 1：关闭主窗口（点 × / Cmd-W）

| node | rust local-owned | rust local-reused | rust remote |
|---|---|---|---|
| 拦截关闭 → `mainWindow.hide()`，子窗口同步隐藏，服务继续跑（main.cjs:3252-3265，`!isQuitting && !_isUpdating && !forceQuitApp` 时 `e.preventDefault()`） | 同左（同一 handler，与模式无关） | 同左 | 同左 |

四列同格同源：该 handler 不看服务身份。托盘在则 `window-all-closed` 不触发退出
（main.cjs:6824-6831）。quick-chat 窗口同策略（hide，main.cjs:2973-2978）；
设置窗口真关但仅置空引用（main.cjs:3395-3397），不碰服务。**平台差异：无。**

### 行 2：托盘「退出」

| node | rust local-owned | rust local-reused | rust remote |
|---|---|---|---|
| `isExitingServer=true; isQuitting=true; app.quit()`（main.cjs:2414 与 :5991 两处托盘构建）→ 入行 3 before-quit 全链 | 同左 | 同左 | 同左 |

入口同格；差异在 before-quit 内部（行 3）。

### 行 3：完全退出（Cmd-Q / 菜单退出 / 行 2 汇入）—— before-quit + shutdownServer

公共前段（四列相同）：`isQuitting=true`（main.cjs:6943）；`_isUpdating` 时直接放行
（main.cjs:6945-6946，server 清理由更新链自己做，main.cjs:5480-5485 序）；
隐藏全部窗口、清理浏览器视图（main.cjs:6948-6964）。

| node | rust local-owned | rust local-reused | rust remote |
|---|---|---|---|
| hasActive=true → preventDefault + shutdownServer：owned 路径（main.cjs:6865-6900）。POSIX：SIGTERM → 17s 宽限（SERVER_SHUTDOWN_GRACE_MS，main.cjs:725）→ 未退出 warn「强制终止」+ SIGKILL（main.cjs:6882-6884, signalPidOnPosix:784）→ 再 5s（:726）。win32（平台限定）：`requestServerShutdown` HTTP 优雅停 → guardian Job 收敛（main.cjs:6870-6875,6879-6881）。退出后清 server-info.json（main.cjs:6933-6935） | hasActive=true → 同一 owned 路径。POSIX 与 node 相同（SIGTERM/SIGKILL 对 rust 二进制一致）。win32（平台限定）：`serverNodeKind==='lingxi-service'` 不走 guardian 分支，改 `proc.kill()`（main.cjs:6880-6882 的 `else if` 落点） | hasActive=**false**（reusedServerOwned=false）→ `allow`，**不 preventDefault、零接触**；复用实例在退出后存活。**若** shutdownServer 因他因被调到且 reusedServerPid 非空：走「external server」detach 分支，提前 return，绝不 kill（main.cjs:6900-6908） | hasActive=**false**（双 null）→ `allow`，零接触：不发 SIGTERM、不发 HTTP shutdown、不动 server-info.json（removeServerInfo 分支根本不会执行——shutdownServer 未被调用） |

**退出超时残留 PID 报告**（任务书硬要求，四列中仅 node / rust-owned 可达）：
- owned POSIX 超时：warn 带 PID「server PID ${pid} 未在 17000ms 内退出，强制终止」（main.cjs:6883）；
  强杀后仍未确认：warn「launcher PID ${pid} 终止后仍未确认退出」+ **保留 server-info.json** 供下次
  启动识别残留（main.cjs:6896-6899,6936-6938），shutdownReason 上报。
- reused-owned（仅 node 可达；rust 复用恒非 owned）超时：warn「reused server PID ${pid} 未确认退出；
  不按裸 PID 终止」（main.cjs:6921-6924）+ 保留 server-info.json。
- before-quit 兜底：shutdownServer 抛错仅 console.error，finally 二次 `app.quit()` 放行
  （main.cjs:6979-6986）——退出不被服务卡死，残留以 PID 日志 + server-info.json 留存呈现。

### 行 4：window-all-closed 兜底（托盘缺失）

| node | rust local-owned | rust local-reused | rust remote |
|---|---|---|---|
| 托盘存活 → 不退出常驻（main.cjs:6824-6831）；托盘缺失/已毁 → `forceQuitApp=true; app.quit()` → 入行 3 | 同左 | 同左 | 同左 |

### 行 5：启动失败退出（launch-failed 对话框后）

| node | rust local-owned | rust local-reused | rust remote |
|---|---|---|---|
| `forceQuitApp=true; app.quit()`（main.cjs:6819-6820）→ before-quit 照常走行 3；owned server 已起则照杀 | 同左。远端分支在 verifyRustDesktopIdentity 失败时于 publish 前抛（main.cjs:1366-1368），serverProcess/reusedServerPid 从未被赋值 → 行 3 `allow` 零接触 | 同 node 列行为（本列实例非 owned → 零接触） | 同 rust local-reused 列行为：远端服务绝不受本壳启动失败牵连 |

### 行 6：更新退出（quitAndInstall / 列车更新）

四列同源：`_isUpdating=true` → before-quit 首行放行（main.cjs:6945-6946），server 停启由更新链
自管（main.cjs:5480-5485 注释序、exit 哨兵 main.cjs:2307-2311 防误报崩溃）。rust owned 在更新链
中的行为与 node 相同路径复用，无分叉代码。**win32 guardian 平台限定同上。**

## 与任务书第 3 条的交叉断言（remote 不 spawn 本地第二服务、不 kill 远端任务）

- remote 分支 return 于 `resolveRustBinary` 之前（main.cjs:1358-1374）：远端模式根本不解析/不起本地
  二进制 → 不可能 spawn 本地第二服务（结构性，非条件分支侥幸）。
- remote 列在行 1-6 全部为零接触；唯一网络动作是启动期 `verifyRustDesktopIdentity`
  （GET identity，只读，main.cjs:1366）。
- A06「退一个客户端另一个继续」由此矩阵保证：退出的 remote 客户端走行 3 `allow`，对服务与对端零影响。**已实测闭合（批次 E，drill-05，见下）。**

## 待补集成证据（批次 D/E 回填「实测」列）

- A05：local-owned 行 3 POSIX 全链实测（SIGTERM→退出确认→server-info.json 清理）+ 行 1 hide 实测。
- A06：remote 行 3 `allow` 零接触实测（退 A 后进程树无 lingxi-service 变化、B 会话续跑）。**✅ 已回填（drill-05）。**
- win32 各格本次仅代码核对，标平台限定；不实测、不虚报。

## 实测回填（批次 D，A05 local-owned，drill-08 / t03-a05-drill-08）

- **行 1（关主窗口 → hide）**：✅ 实测绿。Cmd+W（osascript 送达，status 0）后 CDP
  `document.visibilityState === 'hidden'`（hide≠destroy：页面目标仍在属预期，实证是可见性
  转移）；rust 服务 pid 存活；`/lingxi/v1/health` → 200。
- **行 3（Cmd+Q 完全退出）**：✅ 实测绿。Electron 主进程 45s 内退出；owned rust（pid 溯源
  instance.json）45s 内收敛退出；S3 进程树快照 `node-kernel=0 / rust-kernel=0`，合成 HOME
  在 args 的残留进程为零。
- 行 2/4/5/6 本批未实窗触发，仍依代码锚（见矩阵本体）。
- oracle 勘误（drill-07 红暴露）：hide 后 CDP 仍列出页面目标，"页面消失"不是 hide 的正确
  oracle；`/lingxi/v1/version` 路由不存在，活性探针用 `/lingxi/v1/health`。

## 实测回填（批次 E，A06 remote 双客户端，drill-05 / t03-a06-drill-05）

- **行 3 remote 列（客户端完全退出 → 对服务零接触）**：✅ 实测绿（回环-remote，传输替代见下）。
  A/B 双客户端 remote 共连同一 harness 服务（127.0.0.1 动态端口）：
  GSCI 硬断言 `host=127.0.0.1 + serverNodeKind=lingxi-service + 端口一致`（remote 分支三重
  oracle，标题 `Lingxi · Rust 预览（远端）` 为第四重）；两客户端 HOME 下均无
  `lingxi-service/instance.json`（零接触结构性证据）。
- **退 B → A 继续**：✅ 实测绿。B 退出后服务 pid/instanceId 连续不变、`/lingxi/v1/health`→200、
  A 的 GSCI 仍 remote；A 的腿 2 任务真实跑完（`assistant_run_end` 到达且 reason 为空 = 成功
  语义，服务侧历史含终结回答）。树快照 S2：harness 子树 Electron 主进程 2→1。
- **退 A（终态）**：✅ 实测绿。服务仍存活（零接触终态），harness SIGTERM 回收收敛；S3：
  `node-kernel=0 / rust-kernel=1 / Electron 主=0`。
- **remote 列附注（Lead 批次 E 裁决 (a) 第 4 点）**：零接触语义经回环-remote 证明；真实 LAN
  e2e 待 PPG-3（见附注节）。remote 各格的冻结语义不依赖传输层是回环还是 LAN（main.cjs:1354
  起 remote 分支结构性不起本地二进制、不碰远端生命周期），回环替代不改变矩阵结论。
- **退出机制替代（显式记录）**：验收语义是「服务零接触 + 对端连续」，非退出触发方式。实测中
  键盘路径（AXRaise→frontmost 确认→Cmd+Q）在双同名实例下前台激活竞态不可控（drill-01..04
  红迹保留），定稿键盘尽力而为 + SIGTERM 优雅兜底（Electron 捕获走正常退出链，exit code 0，
  before-quit 处理照常）；verdict.quitMechanisms 记为 `sigterm-graceful`，不静默。
- 模型侧交叉验证：桩日志恰 2 条 CHAT 请求、零工具接触（act ∉ {final,error} 数为零）。

## 附注：预览平价缺口 PPG-1 / PPG-2（A05 演练正式发现，Lead 裁决接受双 BLOCKED）

与本矩阵无冲突（冻结语义不受影响），但属 T03 必须成文的预览限定：

- **PPG-1（写腿）**：预览启动无 `--config` → 无 workspace → write/edit 不注册 → 模型工具面
  恒 14 项 → 写任务 `failed.provider_error`（drill-08 帧级硬断言：run_end
  reason=failed.provider_error + error run_failed + 零 assistant_segment）。
- **PPG-2（读腿）**：read 已注册但 `session_read_access` 零根（lib.rs:1498-1508）且唯一
  授权注入孔道只接 workspace access（lib.rs:2042-2047 → runs.rs:2949 不执行）→ 会话
  authorizedFolders 在预览模式永不生效 → read 结构性不可用（resource_access_denied）。
- 去向：T04/T05 MUST-ADJUDICATE（桌面预览 workspace 策略）+ T08 终检查清单。完整根因链与
  范式差异（旧 Node `/api/fs/*` vs Rust workspace 双闸）见 T03_DESIGN.md §5A。

## 附注：PPG-3（A06 批次 E 探针正式发现，Lead 裁决 (a) 登记）

**远端桌面 LAN 认证通道缺失**（状态 open，去向 T04/T05/T08 MUST-ADJUDICATE，与 PPG-1/2 并列）：

- 产品链：transport.rs:201-206 非回环推断 `ConnectionKind::Lan` → auth.rs:2558-2566 loopback
  token 在非 Local 连接恒 401 → readRemoteRustConnection 仅接受 `^[0-9a-f]{32}$` 形状
  （rust-local-service.cjs:369-372）→ 设备凭证 `hana_dev_` 前缀被该正则拒绝 → 签发面
  `POST /lingxi/v1/devices/credentials` 挂 local_only（lib.rs:9036-9037），LAN 绑定不可达。
- 环境链：macOS ALF 对 adhoc 签名 debug 二进制静默丢弃 LAN 入站包（t03-a06-auth-probe-04
  绿证三方对照：python3 控制组 200 / rust 回环 200 / rust LAN 超时）。
- 现状定性：remote 模式 = **dev-drill-only**（回环 token 通道可用；批次 E A06 全绿基于此）。
  真 LAN e2e 双重阻断（环境 + 产品），故本矩阵 remote 列的实测证据标注「回环-remote」。
