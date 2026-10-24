# PROGRESS-t02-core.md — R08-T02 / R08-A03 执行记录（跨会话权威台账）

执行者：t02-core。阶段：R08。仓库：LingxiAgent（分支 codex/rust-tauri-migration）。
范围：抽取 React 桌面能力接口（DesktopHost + Electron/Web 实现）+ 前端不直接依赖 Electron。
不 commit / 不 push。设计文档：docs/rust-tauri/R08/T02_DESIGN.md。

## 交付物状态

| 交付物 | 路径 | 状态 |
| --- | --- | --- |
| DesktopHost 接口 + 惰性解析 | desktop/src/react/host/desktop-host.ts | 完成（桥身份快照制） |
| 受限能力错误类型 | desktop/src/react/host/host-capability-error.ts | 完成 |
| Electron 宿主适配 | desktop/src/react/host/electron-desktop-host.ts | 完成 |
| Web 受限宿主（63 成员 + 理由登记） | desktop/src/react/host/web-desktop-host.ts | 完成（含裸 window 桩防御） |
| 移动端宿主（HEAD 契约保留） | desktop/src/react/mobile/mobile-platform.ts | 完成（kind='mobile'） |
| ServiceClient（lingxiFetch + 连接解析 + rust-* 适配） | desktop/src/react/services/service-client.ts | 完成 |
| 平台引导瘦身 | desktop/src/modules/platform.js | 完成（Electron 抢先别名 + win32 滚动保留） |
| 批次 A：ServiceClient 消费迁移 | react/services 等 9 文件 | 完成 |
| 批次 C：window.hana → usePlatform() | 23 文件 + utils/screenshot.ts | 完成 |
| 批次 D：window.platform → usePlatform() | ~65 文件 | **待做（Lead 裁决：要做，分目录批）** |
| 批次 E：inventory 边界断言模式 | scripts/rust-tauri/r08_client_inventory.mjs | 待做 |
| 批次 F：A03 Electron-free 组件测试 | __tests__/host/r08-a03-web-desktop-host.test.tsx | 待做 |
| 前端依赖边界报告 | CLIENT_BOUNDARY_PREPARATION.{json,md} | 待复算（批次 E 后） |

## 关键架构决策

1. **双边界**：业务服务调用 → ServiceClient（react/services/service-client.ts）；
   宿主能力调用 → DesktopHost（react/host/*），`usePlatform()` = `getDesktopHost().api`，
   纯函数、永不为 undefined、随处安全。
2. **getDesktopHost() 惰性解析 + 桥身份快照**（批次 C 期间返工）：
   缓存 currentHost 与 window.hana / window.platform 的对象身份快照；
   每次调用先比身份：不变→返回缓存；变化→重解析
   （hana 存在→Electron 宿主；否则 platform 存在→外来测试桩收编为 web 宿主；
   否则→createWebDesktopHost() 并写 window.platform 别名）。
   生产环境桥身份稳定，等价于只解析一次；测试中逐用例重挂桩不会被过期缓存遮蔽。
   installDesktopHost 写别名并刷新快照；resetDesktopHostForTests 清缓存、删别名、重快照。
3. **platform.js 瘦身但保留 Electron 抢先别名**：bridge 就绪后立即
   `window.platform = window.hana`（与 HEAD 行为一致），保障 React 启动前未迁移的
   window.platform 读取不塌；HEAD 的 63 成员 web fallback 不恢复，改由 web-desktop-host
   惰性安装。win32 滚动修复与 data-platform 逐字保留。
4. **window.hana 直连白名单**（仅 3 个非测试文件）：
   desktop/src/modules/platform.js、react/host/desktop-host.ts、
   react/host/electron-desktop-host.ts。其余全部经边界。
5. **循环依赖豁免**（import 清扫白名单）：react/services/** 内部、react/stores/**、
   react/hooks/use-hana-fetch.ts、react/hooks/use-stream-buffer.ts、react/settings/api.ts
   不得 import service-client（会被 stores→services→stores 环反噬）。
   settings/** 其余文件在清扫范围内。豁免理由待补入 T02_DESIGN.md。
6. **移动端契约保持「不存在即受限」**：mobile-platform.test.ts 明确断言
   selectFolder/selectFiles/writeFileIfUnchanged/openFolder/showInFinder/trashItem/
   getFilePath 必须为 undefined、openExternal 同步 void。不得以可调用空实现冒充。
   缺失成员经 capabilityReasons 登记可解释理由，保留 Partial→PlatformApi 窄化强转并注释。
7. **Web 宿主受限可解释**：55 个常量/空成员逐一登记中文理由（capabilityReason /
   explainHostCapability）；4 处 HTTP 读取 404→null（显式未找到），其余非 ok 抛带状态错误；
   openExternal 弹窗失败/被拦截返回 {ok:false,error}，不静默。

## 批次执行与修复记录

### 批次 A（ServiceClient 落地与消费迁移）
- t02-batch-a-typecheck-01/02 红 → -03 绿；t02-batch-a-vitest-01 绿。
- 决策：usePlatform() 纯函数化（去掉「可能 undefined」），消费侧 `?.` 是能力判空而非宿主判空。

### 批次 B（宿主实现 + use-hana-fetch/settings 迁移）
- typecheck-01 红 → -02 绿；vitest-01/02 红（lead T01 在途消费测试适配中间态，非我方回归）→ -03 绿（130 文件 / 1247 用例，stores+utils+settings）。
- vitest-04/-05 红：17 用例「vi.fn() 未被调用」——根因：旧 getDesktopHost 首调固化，
  测试逐用例重挂 window.hana 桩 + afterEach 删除，缓存宿主遮蔽新桩。
  修复：桥身份快照制（决策 2）+ platform.js 抢先别名恢复（决策 3）。

### 批次 C（window.hana 直连迁移，23 文件 + screenshot.ts）
- codemod：/tmp/t02_hana_sweep.py（MemberExpression 根替换为 usePlatform()，逐文件校验）。
- capture-once 修正（TS2722/TS2532）：TS 无法跨两次 usePlatform() 调用窄化可选成员，
  改为 `const platform = usePlatform();` 后对同一引用判空调用；既有显式能力守卫逐字保留：
  OnboardingApp.tsx:102（getServerConnectionInfo 回退 getServerPort/getServerToken）、
  QuickChatApp.tsx:325/713（'快捷聊天窗口接口不可用'）、rust-browser.ts:86
  （'browser_desktop_unavailable'）、ws-message-handler.ts:970（showNotification 判空 + 补花括号）。
- t02-batch-c-typecheck-01 红（6 错）→ -02/-03/-04 绿。
- t02-batch-c-vitest-01 绿（此前 3 个失败套件 33/33）；
  -02 全量红（5 文件 8 用例）→ 修复后 -03（5 文件）/-04（全量 413 文件）：
  411 过，仅剩 2 个 HEAD 既有红（见下）。
- 修复明细（vitest-02 暴露）：
  ① 桥身份快照重解析（desktop-host.ts）；
  ② platform.js Electron 抢先别名恢复；
  ③ mobile-platform.ts 回滚 HEAD 20 成员契约（原「必需成员补齐」违反 mobile 测试契约）；
  ④ web-desktop-host 防御裸 window 桩（vi.stubGlobal('window', {}) 下 location/localStorage 缺席）；
  ⑤ platform-web.test.ts 断言逐字保留、装载路径改 getDesktopHost() 惰性解析
  （契约已从 platform.js 迁入 web-desktop-host.ts，属架构迁移式断言适配，已注释记录）。

### HEAD 既有红（非本任务回归，已取证）
- composer-send.transaction.test.ts「Rust wire 只有握手时拒绝旧聊天帧且不清空输入」：
  期望的 `rust_core_unavailable` 全仓只存在于该测试自身，产品源码无此符号 → R02 期 TDD 先行红。
- R02RustWebSocket.test.ts「stays offline until handshake…」：撞 session-connection-owner 的
  useStore.subscribe 缺失（测试桩未提供）。
- 取证：git worktree 挂纯净 HEAD 复跑两文件，同样红（见 t02-head-red-baseline-01 证据）。
- Lead 裁决：归入 T01 在途债台账，T05 切换安全审查时裁决（实现握手拒绝 or 显式改测试），t02 不再介入。

## 事故记录（已独立成节，因果详见设计文档）

- 早前误用 `git checkout -- desktop/src/react/settings/` 目录前缀回滚，摧毁 lead 未提交 T01 成果；
  lead 已字节级恢复（TraceDetailOverlay.tsx sha256=54933714… 校验一致）。
  铁律加固：禁止一切目录前缀 git 回滚/清理；只逐文件回退本人产出的文件，
  回退前 git status 逐一核实；意外状态即停并问 lead。
- 本文件取代聊天记录成为跨会话唯一权威记录；每批次完成后回写。

## 批次 F / D / E 完成记录（2026-10-22 晚）

- **批次 F（A03）**：`__tests__/host/r08-a03-web-desktop-host.test.tsx` 5/5 绿
  （t02-a03-vitest-03）。Web 宿主惰性安装 + 显式能力理由（selectFolder 含「目录选择器」
  措辞）、404→null / 500→显式 throw、Authorization 仅 token 存在时下发。
- **批次 D1–D4b**：window.platform MemberExpression 根 codemod 共 63 个生产文件
  （D1 app/onboarding/splash 7；D2 services/stores/utils 14；D3 settings 10；
  D4a components 16；D4b desk/input/preview/right-workspace 16）。
  各批 typecheck+目标 vitest 绿；红证据均带根因保留。
  关键行为适配：Web 宿主 no-op 成员是函数 → 以 `data-platform="web"` 属性表达
  「Web 运行时」（session-file-expired 下载分支测试断言不变、仅桩对齐生产引导）。
- **批次 E**：扫描器扩 boundary 段（规则/白名单+理由/生产违例/测试桩收编/测试读取清单），
  web 成员抽取迁至 host/web-desktop-host.ts 的 api 字面量；`--check` 非零退出条件=
  JSON 不新鲜或 boundary 非 PASS。扫描器揭示 grep 盲区 3 处（media-source 断言形态、
  session-actions `(window as unknown).hana`、viewer-window-entry.tsx）已迁移；
  2 份 main 进程 .cjs 的 require('electron') 按白名单+理由登记（非渲染直连）。
  **结果：boundary PASS，生产违例 0；测试赋值桩收编 151 处；测试读取单列 193 处
  （显式登记的债：composer-send.transaction/R02RustWebSocket 属 Lead T01 不可触碰，
  其余断言读取迁移列入后续批次）。**
- 文档：CLIENT_BOUNDARY_PREPARATION.md 重写为「准备+断言」；T02_DESIGN.md §6.5 执行修订
  回写（stores 豁免理由 / settings 决策 / TraceDetailOverlay 事故因果 / E 修正）。
- 最终验证（本 span）：typecheck 绿（t02-batch-e-typecheck-01）；全量 react vitest
  399 文件 3637 用例过，仅 2 个 HEAD 既有红（Lead T01 范围，t02-batch-e-vitest-01）；
  inventory 生成与 --check 退出 0（t02-batch-e-scanner-02 / t02-batch-e-check-01）。

## 证据清单（artifacts/rust-tauri/R08/，新增在前）

- t02-batch-e-scanner-01（红，12 违例，根因=盲区已修）/-02（绿 PASS）；t02-batch-e-check-01（退出 0）
- t02-batch-e-typecheck-01（绿）；t02-batch-e-vitest-01（绿，仅 2 HEAD 红）
- t02-batch-d4a/d4b 系列（d4a-vitest-03 绿 166 文件 1313 用例；d4b 同口径绿）
- t02-a03-vitest-01/02（红，根因已修）/-03（绿 5/5）
- t02-batch-a-typecheck-01/02（红）/-03（绿）；t02-batch-a-vitest-01（绿）
- t02-batch-b-typecheck-01（红）/-02（绿）；t02-batch-b-vitest-01/02（红，lead 中间态）/-03（绿 130/1247）/-04/-05（红，宿主缓存根因已修）
- t02-batch-c-typecheck-01（红）/-02/-03/-04（绿）
- t02-batch-c-vitest-01（绿 33/33）/-02（红 5 文件 8 用例）/-03（绿，修复验证）/-04（411 过，仅 2 个 HEAD 既有红）
- t02-head-red-baseline-01（纯净 HEAD 复跑两红，取证）
- t01-trace-overlay-rendering-01/-02（lead 证据，不属本文件维护）

## 待办

1. ~~批次 F A03 测试~~ ✅（t02-a03-vitest-03）。
2. ~~批次 D window.platform codemod~~ ✅（D1–D4b 全绿）。
3. ~~批次 E inventory 边界断言~~ ✅（PASS + --check 退出 0）。
4. ~~设计文档补写~~ ✅（§6.5）。
5. ~~最终验证与报告~~ ✅（本 span 完成；已向 lead 报告）。
6. 后续批次（不在 T02 收口范围）：测试文件 193 处断言读取迁移（待 Lead T01 落地后启动，
   扫描器清单已就绪）。
