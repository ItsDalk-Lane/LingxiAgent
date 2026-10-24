# R08-T03 后续 — T04 进度台账：193 处测试直连读取迁移经宿主边界

任务卡：team task-4（owner t02-core）。数据源 `r08_client_inventory.mjs` 的
boundary.testDirectReads（193 处 / 63 文件），排除 Lead T01 范围两文件
（composer-send.transaction.test.ts 7 处、R02RustWebSocket.test.ts 0 处在扫描清单内
——实际排除 7 处），迁移对象 **186 处 / 61 文件 / 21 目录**。

## 迁移模式（Lead 任务卡给定方向 + 实现选型）

- 读取改写：`window.hana.X` / `window.platform.X` → `getDesktopHost().api.X`
  （宿主边界访问器；electron 宿主 api === 桥本体身份，platform-only 收编宿主
  api === platform 身份，vi.fn 断言对象引用不变，语义等价）。
- 每个迁移文件 import `getDesktopHost`（来自 `desktop/src/react/host/desktop-host`），
  相对路径按文件深度计算。
- 双桥并存注意：window.hana 与 window.platform 同时设置且不同体时，
  getDesktopHost() 优先 hana——逐站点核对 window.platform 读取处是否存在该形态，
  存在则单独处理并记录。
- 红线：只动 __tests__ 测试/夹具文件，零产品源码改动；无合法宿主孔道 → BLOCKED 上报。

## 批次计划（按目录聚合，每批 vitest 绿 + r08_run.py 回执后才进下一批）

| 批 | 目录 | 处数 | 状态 |
|---|---|---|---|
| 1 | `__tests__`（根） | 3 | ✅ 绿（t04-batch01-tests-root-01） |
| 2 | `__tests__/utils` | 43 | ✅ 绿（t04-batch02-utils-01）；42 迁移 + 1 豁免（screenshot.test.ts hana 桩卸载 delete） |
| 3 | `__tests__/components`（不含 Lead 两文件） | 52 | ✅ 恰 1 基线红（composer-send，T01 域内既有）无新增（t04-batch03-components-01）；35 迁移 + 9 platform 桩卸载→resetDesktopHostForTests() + 8 豁免（7 hana 卸载 delete + InteractiveCard 桥身份保存） |
| 4 | `__tests__/components/shared/MediaViewer` | 20 | ✅ 绿（t04-batch04-mediaviewer-01）；14 迁移 + 6 platform 桩卸载→resetDesktopHostForTests() |
| 5 | `__tests__/mobile` | 22 | ✅ 绿（t04-batch05-mobile-01，53 测试全绿）；三文件均为 mobile shim（白名单模块 mobile/mobile-platform.ts）产品面测试，api===shim 身份迁移 |
| 6 | `__tests__/services` | 10 | ✅ 恰 1 基线红（R02RustWebSocket，T01 域内既有）无新增（t04-batch06-services-01）；2 platform 桩卸载→resetDesktopHostForTests() + 8 豁免（r07 真服务 6 处桥身份保存/恢复 + mcp-oauth 1 + ws-notification hana 卸载 1） |
| 7 | `__tests__/settings` + `settings/observability` | 9 | ✅ 绿（t04-batch07-settings-01，46 文件/329 测试）；7 迁移 + 2 豁免（observability-export-save hana 卸载 delete ×2） |
| 8 | `__tests__/modules` | 6 | ✅ 绿（t04-batch08-modules-01）；4 迁移（T02 已述：装载步骤已走 getDesktopHost，本次收残余直读）+ 1 platform 卸载→reset + 1 豁免（hana 卸载 delete） |
| 9 | `__tests__/host` + `stores` | 5 | ✅ 绿（t04-batch09-host-stores-01，472 测试）；desk-actions 2 迁移（宿主惰性安装别名断言身份不变）+ r08-a03-web-desktop-host 3 豁免（宿主边界自测：raw 全局缺席断言 / hana 卸载 / platform===host.api 别名契约断言） |
| 10 | live 系列（chat-app-live/chat-live/quickchat-live/session-*/slash-live） | 12 | ✅ typecheck 绿（t04-batch10-live-typecheck-01，0 TS error）+ esbuild parse-check 8/8；live 目录无 vitest 用例（仅 launch.mjs/main.cjs/preload.cjs/surface.tsx，Electron+真服务 harness），运行时复验属 live 演练阶段；全部 12 处迁移（真桥 api===hana 身份） |
| 11 | `onboarding/__tests__` + `settings/tabs/__tests__` | 4 | ✅ 绿（t04-batch11-colocated-01，13 文件/124 测试）；3 迁移（含 1 处 cast 成员赋值）+ 1 platform 卸载→reset；写域经 Lead 中段扩批覆盖 |

## 写域说明

任务卡 writeScopes = `desktop/src/react/__tests__` + `desktop/src/__tests__`；
`desktop/src/react/onboarding/__tests__`（2 处）与 `desktop/src/react/settings/tabs/__tests__`
（2 处）在声明写域外但属红线允许的 __tests__ 测试文件，已报 Lead 备查，安排在末批。

## 豁免登记（完成判据：testDirectReads 仅剩显式豁免）

扫描器复跑结果：193 → 30 处；排除 Lead T01 两文件后残余 **23 处**，全部登记如下
（163 处已迁移经宿主边界）。豁免类与逐条理由：

**A. Lead T01/T05 评审域（任务卡显式排除，7 处）**

- `components/composer-send.transaction.test.ts`（7 处）：T05 评审时处理。
- `components/R02RustWebSocket.test.ts`：同上（当前 0 处在读清单内）。

**B. hana 桩卸载 delete（12 处）**——宿主边界无卸载 window.hana 的合法孔道
（resetDesktopHostForTests 按设计只清 platform 别名与宿主缓存，不动 hana——hana 是
preload 桥本体，宿主层无权删除）；保留 `delete window.hana` 直操作与安装端
`window.hana = …`（白名单赋值）对称：

- `utils/screenshot.test.ts:62`；`services/ws-notification.test.ts:35`；
  `settings/observability/observability-export-save.test.ts:92,121`；
  `modules/platform-web.test.ts:20`；`components/InputArea.composer-layout.test.tsx:188`、
  `InputArea.draft-sync.test.tsx:287`、`InputArea.media-send.test.tsx:308`、
  `InputArea.pending-identity.test.tsx:215`、`InputArea.status-stack.test.tsx:278`、
  `InputArea.system-dictation.test.tsx:322`、`InputArea.welcome-placeholder.test.tsx:179`
  （行号为迁移后复跑值；InputArea 七文件该 delete 与 platform 桩安装同处 beforeEach，
  语义=确保 web 宿主路径）。

**C. 桥身份保存/恢复脚手架（8 处）**——捕获原始桥引用供 afterEach 恢复；
getDesktopHost() 永不返回 undefined 且有惰性安装 web 宿主 + 写 platform 别名的副作用，
无法用于「捕获原始可能为 undefined 的全局身份」：

- `components/InteractiveCard.test.tsx:18`（previousHana）；
  `services/r07-interface-late-write-real-service.test.tsx:24`（platform+hana 同行 2 处）；
  `services/r07-interface-preferences-real-service.test.tsx:20,21`；
  `services/r07-mcp-oauth-real-service.test.tsx:12`（original.platform）；
  `services/r07-oauth-real-service.test.tsx:29`；`services/r07-settings-entry-real-service.test.tsx:21`。
  对应恢复端赋值（`window.platform = previousPlatform` 等）属白名单赋值不计入读取。

**D. 宿主边界自测（3 处）**——`host/r08-a03-web-desktop-host.test.tsx` 是宿主边界
模块自身的测试，读取对象即被测机制本身：

- L66 断言 raw `window.hana` 全局缺席（经 getDesktopHost 无法表达缺席断言）；
- L72 hana 卸载 delete（同类 B）；
- L96 断言 `window.platform === host.api` 别名契约（读取即契约本体）。

## 批次日志

- 批 1（tests 根，3 处）：app-init.test.ts 3 处 typed 成员读 → api；假 window 经
  globalThis 构造，宿主身份快照重解析确认安全。t04-batch01-tests-root-01 绿。
- 批 2（utils，43 处）：file-preview 15 + preview-file-refresh 11 + screenshot 15 迁移
  （screenshot 含 T02-batch-D 别名行 `platform = getDesktopHost().api` 与 typed
  readFileSnapshot）；link-open 1 处 platform 卸载 → resetDesktopHostForTests()；
  screenshot hana 卸载 delete 1 处豁免（类 B）。t04-batch02-utils-01 绿。
- 批 3（components，52 处）：16 文件 35 处迁移（typed expect/vi.mocked + `?.` 可选链 +
  SidebarNoticeSlot cast 成员赋值 5 处 → api 同体赋值）；9 处 platform 卸载 → reset；
  8 处豁免（InputArea×7 hana 卸载类 B + InteractiveCard 桥身份保存类 C）。
  KnowledgePage 安装经 Object.defineProperty(window,'platform',…)、AccessTab 类同
  Object.assign(window,{hana}…)，均为白名单赋值不受影响。
  t04-batch03-components-01：恰 1 基线红（composer-send，T01 域内既有）无新增。
- 批 4（MediaViewer，20 处）：14 迁移 + 6 platform 卸载 → reset；media-source 含
  「故意缺 getFileUrl」空桩用例，api===空桩身份变换前后一致。t04-batch04-mediaviewer-01 绿。
- 批 5（mobile，22 处）：三文件均为 mobile shim（白名单模块 mobile/mobile-platform.ts）
  产品面测试（成员缺席断言、事件契约、getFileUrl 路由），全部读取发生在
  installMobilePlatform() 之后，api===shim 身份不变；MobileApp L440 `!` 非空断言下的
  成员赋值与 bind 接收者同步改写为 api。t04-batch05-mobile-01 绿（53 测试）。
- 批 6（services，10 处）：2 platform 卸载 → reset；8 豁免（r07 真服务桥身份保存/恢复
  类 C×7 + ws-notification hana 卸载类 B×1）。t04-batch06-services-01 恰 1 基线红
  （R02RustWebSocket，T01 域内既有）无新增。
- 批 7（settings + observability，9 处）：AccessTab 2 + WorkTab 4 + InterfaceTab 归批 11；
  AgentTab platform 卸载 → reset；observability 2 处 hana 卸载豁免（类 B）。
  t04-batch07-settings-01 绿（46 文件/329 测试）。
- 批 8（modules，6 处）：platform-web.test.ts 系 T02 已改造文件（装载已走
  getDesktopHost），本次收 4 处残余直读 → api；platform 卸载 → reset；hana 卸载豁免
  （类 B）。t04-batch08-modules-01 绿。
- 批 9（host + stores，5 处）：desk-actions 2 处迁移（宿主惰性安装写 platform 别名后的
  断言，api===别名同体）；r08-a03-web-desktop-host 3 处豁免（类 D 宿主边界自测）。
  t04-batch09-host-stores-01 绿（472 测试）。
- 批 10（live 系列，12 处）：8 个 surface.tsx 全部迁移（真 Electron 页 preload 装 hana，
  api===hana 身份；quickchat 别名行 `window.platform = getDesktopHost().api`）。
  目录无 vitest 用例（仅 launch.mjs/main.cjs/preload.cjs/surface.tsx，Electron+真服务
  harness 属 live 演练阶段），批证据 = 全仓 typecheck 绿（t04-batch10-live-typecheck-01）
  + 8/8 esbuild parse-ok。
- 批 11（共置 __tests__，4 处）：OnboardingApp 2 迁移（vi.stubGlobal('hana',…) 安装 +
  cast 成员赋值 → api 同体）、InterfaceTab 1 迁移、SkillsTab platform 卸载 → reset；
  写域经 Lead 中段扩批（writeScopes 扩为 `desktop/src/react/**/__tests__`）覆盖。
  t04-batch11-colocated-01 绿（13 文件/124 测试）。

## 收口门禁（全部批次后）

- [x] 扫描器复跑：testDirectReads 193 → 30；排除 Lead T01 两文件后 23 处全部显式豁免
  （类 B 12 / 类 C 8 / 类 D 3，见豁免登记逐条理由）— t04-closure-scanner-check-01 exit 0
- [x] t02-batch-e-check 等价（`node scripts/rust-tauri/r08_client_inventory.mjs --check`）绿
  （同回执）
- [x] scoped vitest（`desktop/src/react/__tests__ --exclude **/r08-a04-*`）恰 2 基线红无新增
  —— t04-closure-vitest-scoped-01：2 failed 文件 = composer-send.transaction +
  R02RustWebSocket，失败测试名与基线逐字一致；399 passed/12 skipped/2 failed = 413 文件
  与 t03-batchf-vitest-scoped-01 基线吻合
- [x] typecheck 绿 —— t04-closure-typecheck-01 exit 0（0 TS error，覆盖全部批次后终态）

## 偏差记录

- 批 10 无 vitest 门禁（live 目录不含 vitest 用例），以全仓 typecheck + esbuild parse
  为批证据；live 运行时复验属 live 演练阶段（launch.mjs harness），不在本任务范围。
- 写域中段扩批：批 11 两目录原在声明写域外，Lead 扩批 writeScopes 至
  `desktop/src/react/**/__tests__` 后纳入，无需 BLOCKED。
- 零产品源码改动；零 BLOCKED；未发现落在非 __tests__ 路径的直连读取。
- 双桥并存逐站点核对结果：仅 r07 真服务文件存在 hana/platform 异体并存（其读取全部为
  类 C 豁免，无迁移处），其余文件单桥或同体别名，api 身份变换全部安全。
