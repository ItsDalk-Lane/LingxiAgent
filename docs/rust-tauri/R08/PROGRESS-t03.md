# R08-T03 进度台账（t02-core 兼）

> 任务：R08-T03「Electron启动Rust服务的预览模式」+ A05/A06（任务书 L125-165）。
> 纪律：合成 HOME、t03-* 证据前缀、r08_run.py 包裹、红证据保留、不碰真 ~/.lingxi、不改 rust/ 产品源码、不改生产默认。
> 本文件为跨会话唯一权威记录，随写。

## 2026-10-22 晚 — 批次 A：设计评审

- 接 Lead 任务指派（T02 闭合后）。已读 T03_SCOPING.md（Lead 预研）+ 任务书 L125-165。
- **现状复核完成**（全部行号锚定，结论与预研一致）：
  - env 门 `rustDesktopEnabled` rust-local-service.cjs:186；READY 握手+identity+publish main.cjs:1338-1464；
    实例复用 main.cjs:1363-1376；失败清理带退出确认 main.cjs:1463-1479；
    before-quit→shutdownServer 残留报 PID main.cjs:6820-6904,6906-6952；
    `localAddress()` loopback 强制 rust-local-service.cjs:270-277（远端路径确不存在）。
  - 渲染侧 serverNodeKind 消费链已通（get-server-connection-info → activeServerConnection）；
    web host 无 getServerConnectionInfo 成员（T02 裁决），预览标记天然桌面限定。
  - debug 二进制 rust/target/debug/lingxi-service 就绪（19:31）；模型面可由
    `{home}/lingxi-service/models.json` 独立配置（config.rs:1159-1189）；
    A04 替身 Provider 模式与 R06 office-live Electron 驱动先例均已消化。
- **产出**：`docs/rust-tauri/R08/T03_DESIGN.md`（同 T02 规格，§1-10），已交 Lead 评审。
- **评审结论（Lead）**：批准动批次 B。裁决① About badge 不做；② A06 首选 LAN 绑定，127.0.0.1
  退路可接受但须显式偏差+报复核；③ 远端 dev-only 批准（packaged+remote 抛错）。
  **必修补丁**：host 传播链断裂（server-connection.ts:228 硬编码 127.0.0.1、IPC 无 host 字段）——
  已补 §3.2.1（main→IPC→types→createLocalServerConnection→6 处调用点全链 + main 侧回环拼串审计 +
  单测断言 remote host 入 baseUrl/wsUrl），无需复审即动工。

## 2026-10-22 晚 — 批次 B：远端模式 + host 传播链（完成）

**实现**（全部落在批准写域内）：
- `desktop/src/shared/rust-local-service.cjs`：新增 `isLoopbackRustHost` / `remoteAddress` /
  `readRemoteRustConnection`（env 三元组：REMOTE_URL + REMOTE_TOKEN_FILE 绝对路径 + 显式
  ALLOW_CLEARTEXT=1 门；URL 形状 WHATWG 校验：仅 http(s)、显式端口、拒绝 userinfo/path/search/hash；
  token 文件 readPrivateJson owner-only、32 位小写 hex；REMOTE_URL 与 RUNTIME≠rust 矛盾即抛；
  win32 抛错；URL 先于令牌文件校验）。顺序微调：地址校验前置，坏 URL 不触发文件读。
- `desktop/main.cjs`：`serverHost` / `rustConnectionMode` 全局；startRustServer 远端分支
  （packaged+remote 抛错、verifyRustDesktopIdentity 不弱化、publish 后
  serverProcess/reusedServerPid/reusedServerOwned 归零 → 结构性零接触）；reuse/owned 分支分别记
  `local-reused`/`local-owned`；IPC `get-server-connection-info` 返回 `host`；
  `server-restarted` 事件负载补 `host`；createMainWindow 标题 opts 期
  `Lingxi · Rust 预览`；buildServerCrashDiagnostics 增 Server runtime + Rust connection mode 两行。
- 渲染侧：`types.ts` PlatformApi 返回型补 `host?: string | null`；`server-connection.ts`
  `createLocalServerConnection` 增 `serverHost` 参——非回环远端走手动连接归一化
  （kind=lan/trustState=lan/credentialKind=device_credential/serverId=remote:&lt;host&gt;/token 保留/
  authState=paired），非 rust 运行时给远端 host 即抛，脏 host 形状（冒号/@/斜线，括号 IPv6 除外）即抛；
  回环 host 保持既有 local 形态。
- **调用点 host 全链（设计 6 处 + 实施中发现第 7 处 OnboardingApp.tsx:107，rust 模式本就
  throw-before-use，仍传 host 保持纵深）**：app-init.ts 启动块+重启回调、SettingsContent.tsx ×2、
  QuickChatApp.tsx、utils/screenshot.ts:380（fetchAvatarAsDataUrl 改用 getServerConnectionInfo）。
  `services/websocket.ts:198` 显式参分支不改：全仓无调用点再传 port/token（7 处均空调用走
  resolveServerConnection），记录在案。
- **main 侧回环拼串审计**（4 处，均无须 host 补丁）：
  ① main.cjs:1244/1257 health/identity 探测——server-info.json 复用路径，仅 node 模式可达
  （rust 远端前已拒绝 server-info.json）；② :2265 legacy GPU 迁移——rust kind 前置抛错；
  ③ :4172 closeBrowserSessionViaServer——旧浏览器宿主 sessionPath 在 rust 模式永不建立，不可达；
  ④ :4801 /internal/browser WS——rustHost 前置 return，rust 模式不开此 WS。结论写入设计 §3.2.1 注记。
- 窗口标题双层现实：BrowserWindow opts.title 仅覆盖页面加载前窗口期，文档标题由
  app-init.ts `document.title` 在启动后覆写（本地 `Lingxi · Rust 预览`，远端加（远端）后缀）。

**测试/门禁**：
- tests/desktop-rust-local-service.test.mjs +4 用例（13/13 绿，证据 t03-batchb-unit-02；
  红证据 t03-batchb-unit-01 保留：端口 70000 被 WHATWG 解析前置拒绝，期望正则与分支不符，已修）。
- server-connection.test.ts +2 用例含 Lead 验收断言（远端 host 入 baseUrl/wsUrl、kind≠local、
  credentialKind≠loopback_token），35/35 绿（t03-batchb-unit-03）。
- 类型检查全绿（t03-batchb-typecheck-01）；触及模块无回归 33/33（t03-batchb-noregress-01：
  app-init / SettingsContent / R07McpRustAPI）。

## 2026-10-22 晚 — 批次 C：进程树记录器 + 冻结矩阵（完成）

**Lead 批次 B 验收通过**（5 收据核验、验收断言位置、形状校验顺序、矛盾门、零接触均确认），批准动 C。

- `scripts/rust-tauri/r08_t03_process_tree.mjs`：全量进程表采集（POSIX ps / win32 PowerShell CIM），
  四类分类（rust-kernel=lingxi-service / node-kernel=bootstrap.ts·hana-server·bootstrap.js·bundle/index.js /
  electron-ui=Electron* 含 macOS comm 16 字符截断形态 / other）；--root-pid 输出祖先链+后代子树；
  JSON schema `r08-t03-process-tree/v1`。自测绿（t03-batchc-tree-selftest-01：916 行、子树链正确；
  分类器正则另做合成样本校验：Electron/Helper 各形态命中、Electronical/node/lingxi-service 不误伤）。
- `docs/rust-tauri/R08/T03_FREEZE_MATRIX.md`：6 行 × 4 列矩阵全代码核对（行 1 关窗口 hide :3252-3265；
  行 2 托盘退出 :2414/:5991；行 3 before-quit+shutdownServer 四列分叉 :6854-6987 +
  resolveBeforeQuitServerAction 的 allow 语义 windows-server-guardian.cjs:63-68；行 4 无托盘兜底
  :6824-6831；行 5 启动失败 :6819-6820；行 6 更新退出 :6945-6946）。退出超时残留 PID 报告锚定
  :6883/:6896-6899/:6921-6924 + server-info.json 留存 :6936-6938。远端零接触结构性证明
  （publish 后双 null → hasActiveOwnedServer=false → allow）。win32 格全部标平台限定（guardian 仅
  node；rust win32 走 proc.kill()），不实测不虚报。A05/A06 实测列留 D/E 回填。

## 2026-10-22 深夜 — 批次 D：A05 演练（完成；结论 = 双 BLOCKED，Lead 裁决 (a) 接受）

夹具 `scripts/rust-tauri/r08_t03_a05_drill.mjs`（~850 行）+ 确定性替身
`r08_t03_stub_provider.mjs`（marker 驱动三幕：read/final/write；journal `ax` 单写；/info
schema r08-t03-stub-provider/v2）。演练序列 drill-06 → 07 → 08，红证据全部保留。

**A05 最终呈现（Lead 2026-10-22 裁决）：读腿 BLOCKED(PPG-2) + 写腿 BLOCKED(PPG-1)。**

- **PPG-1（Preview Parity Gap #1，写腿）**：预览启动路径 main.cjs:1437 仅传 `--home`（无
  `--config`）→ rust main.rs:455 workspace_root 唯一赋值点缺位 → lib.rs:1459-1476
  workspace_access=None → lib.rs:1666 核心写工具仅在 workspace 分支注册 → 模型工具面
  （14 项，drill-07 surface-tools.json）无 write/edit → 桩按任务书原语义请求 write →
  run 落定 `failed.provider_error`。drill-08 帧级硬断言全绿：写腿唯一
  `assistant_run_end reason=failed.provider_error` + error 帧 `run_failed` + 零
  `assistant_segment` 伪回答（drill-08 trace.jsonl）。
- **PPG-2（读腿，同总根因、不同失效机制）**：read 工具在预览模式"已注册+已声明进模型面"
  但结构性不可用——lib.rs:1498-1508 `session_read_access = ResourceAccess::new(&[])`
  （注释自承"零根策略不授予任何目录"）；唯一每轮授权注入 runs.rs:2949
  `replace_session_roots(会话 authorized_folders)` 经 lib.rs:2042-2047 只接
  workspace_access（预览=None）→ 永不执行；其余孔道全堵：`sessions/new` 带 cwd/folders →
  503"工作区工具未配置"（session_focus.rs:169-178）、`restore_session_roots` 要 workspace
  plane → provider_not_available（resourceio.rs:934-936）、`grant_root` 生产零调用方（仅
  tests）。drill-08 实测：`POST /lingxi/v1/sessions` 合法携带
  `authorizedFolders=[合成home/a05-drill]`（CreateSessionBody 合法字段 lib.rs:5371-5387，
  201 接受、目录存在性校验通过），read 仍 `resource_access_denied … not inside any
  authorized root`。夹具无任何合法注入路径，不留绕道，显式 BLOCKED 上报。
- **范式差异（入 PPG-1/2 记录）**：旧 Node = `/api/fs/*` 任意路径 HTTP 路由（T01 残账
  DIRECT 6）；Rust = workspace 约束的工具注册 + 会话授权目录双闸，范式不同。
- **PPG-1/PPG-2 去向**：登记 T04/T05 必裁决项（MUST-ADJUDICATE：桌面预览的 workspace
  策略——是否桌面生成 `--config` 指向默认书桌——属产品语义决策，不在 T03 现场发明）；
  并列 T08 终检查清单：R08 收口前两者必须要么被 workspace 策略修复、要么作为成文的预览
  限定被正式接受。Lead 裁决：A05 双 BLOCKED 不阻断 T03（模式工作与 A06 独立）。

**批次 D 正向实绩（不因 BLOCKED 减损）**：
- drill-08（证据 t03-a05-drill-08）：3 红全在读腿（PPG-2 预期红），其余全绿——写腿帧级
  红证据硬断言、14 工具面无 write/edit、S1/S2/S3 `node-kernel=0` 硬断言、freeze 行 1
  （Cmd+W → CDP `document.visibilityState==='hidden'` + `/lingxi/v1/health` 200 +
  rust 存活）、行 3（Cmd+Q → electron + owned rust 双双 45s 内收敛退出，S3 无残留）。
- 夹具 oracle 三处修正（均 drill-07 红暴露）：①hide≠destroy——CDP 页面目标仍在属预期，
  改 visibilityState 断言；②`/lingxi/v1/version` 路由不存在（实测 404），活性探针改
  `/lingxi/v1/health`；③写腿改帧级正断言。

**更正记录**：drill-06 记的"provider_error 不发 run-end 帧"系旧桩行为差异所致的误记；
drill-07/08 trace 实证服务会发终结帧（reason=failed.provider_error），只是无终结内容。
以 drill-07/08 为准。

## 2026-10-23 凌晨 — 批次 E：A06 演练（完成；绿，回环-remote 替代 + 退出机制替代均显式留痕）

**前置：远端认证探针（probe-01..04）与 PPG-3 登记**
- probe-01/02/03 红链：真 LAN 路径上桌面侧认证与传输双重失败，逐环定位为
  ①产品链（PPG-3：transport.rs:201-206 Lan 推断 → auth.rs:2558-2566 loopback token 恒 401
  → rust-local-service.cjs:369-372 只认 `^[0-9a-f]{32}$` 拒绝 `hana_dev_` 设备凭证 →
  签发面 lib.rs:9036-9037 local_only 不可达）+ ②环境链（ALF 对 adhoc debug 二进制静默丢包）。
- probe-04（绿）：三方对照 python3 控制组 200 / rust 回环 200 / rust LAN 超时，判词
  `env-blocked-lan-source-chain-stands`；`socketfilterfw --getappblocked` 对 adhoc 二进制是
  误导 oracle，教训记录。
- **Lead 批次 E 裁决 (a)（接受并授权）**：A06 走回环-remote 退路（核心验收语义全部硬断言不变）
  + **新增硬断言**：A/B 必须 GSCI 证 `rustConnectionMode==='remote'`（host/port/kind 字段，
  防静默降级回 local-owned/reuse）+ PPG-3 登记 T04/T05/T08 MUST-ADJUDICATE（remote 模式定性
  dev-drill-only）+ 冻结矩阵 remote 列附注「零接触经回环证明，真 LAN 待 PPG-3」。

**A06 演练（r08_t03_a06_drill.mjs，~820 行）**
- 红迹保留：drill-01（新鲜度路径笔误 + 远端标题带「（远端）」后缀两个夹具 oracle 缺陷 +
  run_end reason 成功为空语义发现 + quit-a 键盘竞态超时）；drill-02（quit-b 键盘竞态全失败）；
  drill-03（进程内定向 keystroke 对非活跃 App 不触发菜单键等价：status 0 但无效果 —— 新教训）；
  drill-04（frontmost 确认化后确定性 `frontmost-not-achieved`：双同名实例前台激活不可控）。
- **定稿 drill-05：34/34 PASS，verdict `two-client-remote-pass__zero-touch-quit-verified`**：
  双客户端 remote 三重 GSCI oracle + 标题第四重；两 HOME 零 instance.json；S1/S2/S3 树快照
  node-kernel=0（硬）+ rust-kernel=1（pid=harness 服务）+ Electron 主 2→1→0；退 B 后服务
  pid/instanceId 连续、health 200、A GSCI 仍 remote、A 腿 2 真实跑完（run_end reason 空 + 服务侧
  历史含终结回答）；终态服务存活由 harness SIGTERM 回收收敛；桩日志恰 2 条 CHAT、零工具接触。
- **退出机制替代（显式，不静默）**：键盘路径（AXRaise→frontmost 确认→Cmd+Q）两轮尽力而为后
  SIGTERM 优雅兜底（Electron 捕获走正常退出链 exit 0，drill-01..04 cleanup 实证）；
  `verdict.quitMechanisms={A,B: sigterm-graceful}` + note 字段成文。验收语义（零接触+对端连续）
  不依赖触发方式；若 Lead 要求真键盘路径验收，属环境层待决项。
- wsTicket 链路实证：渲染内 `POST /lingxi/v1/ws-ticket`（Bearer，非 local_only，auth.rs:3752-3755）
  → `ws://...?wsTicket=` 一次性票据在回环-remote 下全流程可用（腿 a1/a2 各一次）。

## 2026-10-23 凌晨 — 批次 F：收口门禁（完成；Lead 复核补两份回执后闭合）

- `t03-batchf-typecheck-01`（绿）：`npm run typecheck` tsc ×3，exit 0，r08_run.py 回执。
- **`t03-batchf-vitest-scoped-01`（闭合门禁，Lead 口径修正后补跑）**：与 T02 基线同 argv
  `npx vitest run desktop/src/react/__tests__ --exclude **/r08-a04-*` → **恰 2 红同名同数**
  （composer-send 1 + R02RustWebSocket 1），文件计数 413 与 t02-batch-e-vitest-01 全等
  （399 过 / 12 跳 / 2 红），T03 闭合成立。
- `t03-batchf-vitest-01`（exit 1 = 全量基线红，口径大于 T02 基线，作意外收获登记）：全量
  `npx vitest run` 1545 文件 / 15689 测试，22 文件 123 红。额外 20 个红文件逐一定性，
  零条指向 T03 批次 B/E 改动面（两条已登记 PROGRESS.md 待办：network-guard-negative
  隔离仍红 = T04 开工前必查；persistence 三件套 = task-5 staged 复验）：
  ①隔离复测转绿 = 并行负载 flake：rust-conversation-events-http(1)；
  ②环境/夹具依赖（裸 vitest 必红）：r06-ar-f12 ×3（硬要求合成 LINGXI_HOME，30 红）、
  round2/round3-delivery-evidence（artifacts/f1-f12-repair ENOENT，64 红）、
  knowledge-search(1)/knowledge-multigrain-migration(2)；
  ③在途 R06/R07 产品漂移（ teammates 域）：persistence-schema-tripwire(4)/
  persistence-startup-receipt(1)/persistence-store-registry(5)（dangling persistence rule
  knowledge-indexes → vector-search-backend-factory.ts）、open-boundary-lint(2)、
  style-discipline(1 → desktop/src/tauri-resources/style.css hardcoded-color)、
  R06Experiments/R06Skills/R07SkillsFailureBoundariesLive(8)、r07-a17/t09（rust 服务
  boot 夹具错）、migration/network-guard-negative(1，60s 超时，隔离复测仍红)；
  ④post-verification-audit-seal(1) = 未提交漂移下封印门禁预期红（AGENTS.md 成文语义）。
- `tests/desktop-rust-local-service.test.mjs` ✅ 13/13（vitest；误用 node --test 的红是 runner
  错误非真红，vitest 复测绿）。
- 冻结矩阵回填：批次 E 实测节 + remote 列附注（Lead 裁决第 4 点）+ PPG-3 附注节。
- 本批零 desktop/rust 产品源码改动（批次 E 纯 harness + 证据 + 文档；Lead 独立核实 mtime）。

## 待办

1. ✅ Lead 评审 T03_DESIGN.md（批次 A，批准 + §3.2.1 必修已补）。
2. ✅ 批次 B：远端模式 + host 传播链 + 标题/诊断 + 单元测试扩面（Lead 验收通过）。
3. ✅ 批次 C：进程树记录器 + 冻结矩阵文档（2026-10-22 晚闭合，待 Lead 验收）。
4. ✅ 批次 D：A05 夹具 + 演练证据（drill-06/07 红保留，drill-08 定稿；结论 = 读腿
   BLOCKED(PPG-2) + 写腿 BLOCKED(PPG-1)，Lead 裁决接受；PPG-1/PPG-2 登记 T04/T05
   MUST-ADJUDICATE + T08 终检查清单）。
5. ✅ 批次 E：A06 夹具 + 演练证据（probe-01..04 + drill-01..04 红保留，drill-05 定稿 34/34 绿；
   Lead 裁决 (a) 全部落地：GSCI remote 硬断言、PPG-3 登记、冻结矩阵 remote 列附注）。
6. ✅ 批次 F：收口门禁（typecheck + 13/13 + 35/35 绿；HEAD 红基线对照无新增红）+ 台账回写完成。

## 证据目录（artifacts/rust-tauri/R08/）

- `t03-batchb-unit-01`（红，保留：远端 URL 用例端口 70000 期望分支错误）→ 修复后 `t03-batchb-unit-02`（绿，13/13）
- `t03-batchb-unit-03`（绿：server-connection 35/35，含远端 host 验收断言）
- `t03-batchb-typecheck-01`（绿：npm run typecheck）
- `t03-batchb-noregress-01`（绿：app-init/SettingsContent/R07McpRustAPI 33/33）
- `t03-batchc-tree-selftest-01`（绿：记录器自测 916 行 + 子树链 + 分类器合成样本）
- `t03-batchc-tree-selftest-02`（绿：分类器修复后 --self-test 14/14）
- `t03-a05-drill-06`（红，保留：旧桩形态 + 三处夹具 oracle 缺陷暴露；"不发 run-end 帧"记录
  后被 drill-07/08 证伪更正）
- `t03-a05-drill-07`（红，保留：读腿零根拒绝首证 + 写腿帧级行为首证 + row1 oracle 缺陷；
  stub-journal/surface-tools/trace/history 全量固化，PPG-1 工具面 14 项清单在此）
- `t03-a05-drill-08`（定稿：读腿 3 红 = PPG-2 预期红；写腿帧级硬断言 + 工具面 +
  node-kernel=0 ×3 + freeze 行 1/行 3 全绿；verdict.json schema r08-t03-a05-verdict/v2）
- `t03-a06-auth-probe-01/02/03`（红，保留：真 LAN 认证逐环失败定位；PPG-3 产品链首证）
- `t03-a06-auth-probe-04`（绿：三方对照锁死 ALF 环境层，`env-blocked-lan-source-chain-stands`）
- `t03-a06-drill-01`（红，保留：dist-renderer 路径 + 远端标题后缀 + run_end reason 成功为空
  三个 oracle 修正源；quit-a 键盘竞态超时首证）
- `t03-a06-drill-02`（红，保留：quit-b 键盘竞态全失败）
- `t03-a06-drill-03`（红，保留：进程内定向 keystroke 对非活跃 App 不触发菜单键等价）
- `t03-a06-drill-04`（红，保留：frontmost 确认化 → 确定性 frontmost-not-achieved，
  双同名实例前台激活不可控定案）
- `t03-a06-drill-05`（**定稿绿**：34/34 PASS，verdict
  `two-client-remote-pass__zero-touch-quit-verified`；quitMechanisms 显式 sigterm-graceful）
- `t03-batchf-typecheck-01`（绿：npm run typecheck 回执）
- `t03-batchf-vitest-scoped-01`（**闭合门禁**：T02 同口径 scoped，恰 2 红同名同数，
  文件计数 413 与基线全等）
- `t03-batchf-vitest-01`（exit 1 = 全量基线红回执：22 文件 123 红，逐文件定性见批次 F 节；
  T03 邻域切片全绿 + HEAD 红基线同名同数）

## 偏差记录

- 批次 B 实施中发现设计 §3.2.1 漏列第 7 处 createLocalServerConnection 调用点
  （onboarding/OnboardingApp.tsx:107）：rust 模式下构造后即抛「Rust onboarding is not migrated」，
  无实际远端消费，仍补传 host 保持纵深；已在本台账披露。
- websocket.ts:198 显式 port/token 分支未传 host：全仓 7 个 connectWebSocket 调用点均为空调用，
  该分支当前不可达；未改，记录在案。
- **批次 E（Lead 裁决 (a) 复核通过）**：A06 传输层为 127.0.0.1 回环-remote 而非真实 LAN
  （双重阻断：PPG-3 产品链 + ALF 环境链，probe-04 绿证）；核心验收语义全部硬断言不变。
- **批次 E 退出机制**：双同名 Electron 实例前台激活竞态不可控（drill-01..04 红迹），定稿
  键盘尽力而为 + SIGTERM 优雅兜底（exit 0 正常退出链）；verdict.quitMechanisms 显式成文，
  验收语义（服务零接触 + 对端连续）不受影响。真键盘路径验收若被要求 = 环境层待决项。
