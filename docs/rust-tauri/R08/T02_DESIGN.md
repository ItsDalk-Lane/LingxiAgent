# R08-T02 设计文档：抽取 React 桌面能力接口（DesktopHost / ServiceClient）

> 任务：R08-T02「抽取React桌面能力接口」+ R08-A03「前端不直接依赖Electron」
> 作者：t02-core。状态：已按设计实施（见 PROGRESS-t02-core.md）。
> 基线数据：docs/rust-tauri/R08/CLIENT_BOUNDARY_PREPARATION.md / .json（本任务结束时重新生成并带边界断言）。

---

## 1. 目标与非目标

### 目标
1. `window.hana` / `window.platform` 的 84+146 处直连拆分为「业务服务调用」与「宿主能力调用」：
   - 业务服务调用 → 唯一 **ServiceClient**（聚合既有 9 个 rust-* 适配 + HTTP 核心 + 连接解析）。
   - 宿主能力调用 → **DesktopHost** 接口：Electron 实现（preload 桥）+ Web 受限实现（现有 web fallback 移入并升级）。
2. 保留现有 React / Zustand / CSS / i18n / 快捷键 / 布局，**不做** Rust/WASM 重写。
3. 工具卡片、thinking-MOOD 卡片、附件、终端、观测、历史保持既有组件 ID 与事件形态，不清理。
4. 纯浏览器（无 Electron API）组件测试可运行：受限能力得到**明确解释**，消灭 `TypeError: ... is not a function` / `Cannot read properties of undefined` 类失败。
5. 前端依赖边界检查：`window.hana` / `window.platform` 直连只许在白名单宿主适配文件内，扫描器断言为证。

### 非目标（明确不做）
- 不改 preload.cjs / main.cjs IPC 通道与协议。
- 不重排 PlatformApi 的 110 个成员语义（不重命名、不改签名）。
- 不迁移 websocket 传输、不动 settings 窗口独立传输 settings/api.ts（见 §6.3 白名单）。
- 不改工具卡片/附件/终端等组件 ID 与归一化事件（任务书第 4 步）。
- 不做组件树重写；ServiceClient 是纯收拢（re-export），无行为变化。

---

## 2. 现状摘要（来自 CLIENT_BOUNDARY_PREPARATION.json）

| 维度 | 数值 |
| --- | --- |
| 扫描 renderer 文件 | 634（排除 __tests__ / *.test.*） |
| `window.hana` 调用 | 84 处 / 23 文件（另有 2 处静态不可达，PostUpdateAnnouncement.tsx） |
| `window.platform` 调用 | 146 处 / ~65 文件 |
| renderer `import 'electron'` | 0 |
| preload 暴露成员 | 112；PlatformApi 显式类型 110（`screenshotRender`、`debugOpenOnboarding`、`debugOpenOnboardingPreview` 靠索引签名兜底；`closeBrowser` 类型有而 preload 无，使用侧判空） |
| web fallback 成员 | 63（55 个常量/空实现） |
| mobile fallback 成员 | 18（`Partial<PlatformApi> as PlatformApi` 强转） |
| 可复用 rust-* 服务适配 | 9 个 |

关键链式关系（现状）：

```
desktop/main.cjs ──IPC──> desktop/preload.cjs ──exposeInMainWorld──> window.hana (112 成员)
desktop/src/modules/platform.js（HTML <script> 非模块，先于 React）
   ├─ Electron: window.platform = window.hana
   └─ Web:      window.platform = {63 成员 web fallback}（含 4 处 HTTP 非 2xx→null、openExternal 空 catch）
desktop/src/react/hooks/use-platform.ts ──> return window.platform
desktop/src/react/types.ts PlatformApi（110 命名成员 + [key:string]: unknown）
desktop/src/react/mobile/mobile-platform.ts ──> window.platform = Partial as PlatformApi
业务消费者：157 处 window.platform / 57 行 window.hana 直连散布于 components/hooks/stores/utils/settings/services
```

---

## 3. 目标架构与关系图

```
                        ┌──────────────────────────┐
                        │  preload.cjs             │
                        │  window.hana (112 成员)  │
                        └────────────┬─────────────┘
                                     │ 唯一直读点（白名单）
        ┌────────────────────────────┼────────────────────────────┐
        │ host/electron-desktop-host.ts            shared/theme.ts │
        │ createElectronDesktopHost(window.hana)   (pre-React IIFE,│
        │                                          仅 syncWindowTheme)
        ▼                                           
┌────────────────────────────────────────────────────────────────┐
│ host/desktop-host.ts                                            │
│   DesktopHost { kind: 'electron'|'web'|'mobile',               │
│                 api: PlatformApi, capabilityReason(name) }      │
│   installDesktopHost / getDesktopHost(惰性安装) /               │
│   explainHostCapability / resetDesktopHostForTests              │
└───────┬──────────────────────┬──────────────────────┬──────────┘
        │ install              │ install              │ install
┌───────▼─────────┐  ┌─────────▼──────────┐  ┌────────▼───────────┐
│ electron host   │  │ web-desktop-host   │  │ mobile-platform.ts │
│ (window.hana 全 │  │ (63 成员移入+升级, │  │ (18 成员+补齐必需  │
│  能力)          │  │  HostCapabilityError│  │  项, 去除强转)     │
└─────────────────┘  └────────────────────┘  └────────────────────┘
        ▲                      ▲
        │ usePlatform() = getDesktopHost().api（永不为 undefined；未安装时惰性创建 Web 宿主）
        │
┌───────┴─────────────────────────────────────────┐
│ 业务消费者（components/hooks/stores/utils/…）     │
│   宿主能力 → usePlatform()                       │
│   业务服务 → services/service-client.ts          │
│     ├─ re-export: lingxiFetch/lingxiUrl (HTTP 核心)│
│     ├─ re-export: server-connection 连接解析      │
│     ├─ re-export: rust-* ×9 适配                 │
│     └─ re-export: chatFetch 等                   │
└─────────────────────────────────────────────────┘

platform.js（非模块 <script>，保持白名单直连）：
  仅保留「必须在 React 之前执行」的可视引导：data-platform 属性 + win32 滚动条行为，
  不再安装 window.platform。宿主安装由 getDesktopHost() 首次调用惰性完成；
  安装时同步写 window.platform 别名（向后兼容既有测试桩与残留读取）。
```

### DesktopHost 接口草案（TS）

```ts
// desktop/src/react/host/desktop-host.ts
import type { PlatformApi } from '../types';

export type DesktopHostKind = 'electron' | 'web' | 'mobile';

export interface DesktopHost {
  readonly kind: DesktopHostKind;
  readonly api: PlatformApi;
  /** 受限能力的可解释原因；能力可用或未登记时返回 null。 */
  capabilityReason(name: string): string | null;
}

export function installDesktopHost(host: DesktopHost): void;
export function installDesktopHostApi(
  api: PlatformApi, kind: DesktopHostKind, reasons?: Readonly<Record<string, string>>,
): DesktopHost;
/** 永不为 undefined：已安装→当前；window.platform 存在→收编；window.hana 存在→Electron；否则→Web。 */
export function getDesktopHost(): DesktopHost;
export function explainHostCapability(name: string): string | null;
export function resetDesktopHostForTests(): void;
```

```ts
// desktop/src/react/host/host-capability-error.ts
export const HOST_CAPABILITY_UNAVAILABLE = 'HOST_CAPABILITY_UNAVAILABLE' as const;
export class HostCapabilityError extends Error {
  readonly code = HOST_CAPABILITY_UNAVAILABLE;
  constructor(
    readonly capability: string,
    readonly hostKind: string,
    readonly reason: string,
  ) { super(`宿主能力不可用（${hostKind}）：${capability} — ${reason}`); }
}
```

```ts
// desktop/src/react/host/web-desktop-host.ts
export interface WebDesktopHostOptions {
  /** 测试注入；默认全局 fetch。 */
  fetchImpl?: typeof fetch;
  serverPort?: string;  // 默认按 dev config / __LINGXI_DEV_WEB__ / ?port=
  token?: string;       // 默认按 ?token= / localStorage / dev config
}
export function createWebDesktopHost(options?: WebDesktopHostOptions): DesktopHost;
```

```ts
// desktop/src/react/host/electron-desktop-host.ts（白名单：唯一直读 window.hana 的 TS 文件）
export function createElectronDesktopHost(bridge?: PlatformApi): DesktopHost;
```

```ts
// desktop/src/react/hooks/use-platform.ts（改为返回当前宿主；签名不变）
export function usePlatform(): PlatformApi;  // = getDesktopHost().api
```

### ServiceClient 草案

```ts
// desktop/src/react/services/service-client.ts
// 唯一业务服务入口：HTTP 核心 + 连接解析 + 9 个 rust-* 适配。纯 re-export，无行为变化。
export { lingxiFetch, lingxiUrl, translateLegacySessionCall } from '../hooks/use-hana-fetch';
export {
  resolveServerConnection, requireServerConnection, appendConnectionAuth,
  buildConnectionUrl, buildConnectionWsUrl, requestConnectionWsTicket,
} from './server-connection';
export type { ServerConnection, ResolvedServerConnection /* …现有导出类型原名保留 */ } from './server-connection';
export * from './rust-agent-activities';
export * from './rust-automation';
export * from './rust-bridge';
export * from './rust-browser';
export * from './rust-chat-history';
export * from './rust-media';
export * from './rust-observability';
export * from './rust-settings';
export * from './rust-workbench';
export { chatFetch, captureChatUploadScope } from './chat-client';
```

> `settings/api.ts` 是设置窗口独立传输（读 settings store 的 port/token），属既有专门通道，
> 不并入 ServiceClient；列入导入边界白名单（§6.3）。services/* 内部互相直接导入属实现层细节，不受边界约束。

---

## 4. Web fallback 63 项逐条裁决

裁决类别：
- **REAL**：真实能力（HTTP / 浏览器 API），行为保留。
- **UPGRADE**：保留语义但消除静默降级（显式错误或显式失败返回）。
- **NOOP(reason)**：显式无操作 + 在 host.capabilityReasons 登记理由（任务书允许「显式 no-op 并标注理由」）。
- **ABSENT(reason)**：可选成员不提供，消费侧 `?.()` 天然跳过；理由登记供 explainHostCapability 查询。

| # | 成员 | 现实现 | 裁决 | 理由 / 说明 |
| --- | --- | --- | --- | --- |
| 1 | getServerPort | 返回 serverPort | REAL | dev-web 连接引导，保留 dev config/__LINGXI_DEV_WEB__/?port= 逻辑 |
| 2 | getServerToken | 返回 token | REAL | 同上，?token=/localStorage/dev config |
| 3 | appReady | async ()=>{} | NOOP | Web 无主进程 ready 握手 |
| 4 | syncWindowTheme | ()=>{} | NOOP | Web 主题是 DOM 内 data-theme，无原生窗口同步 |
| 5 | runEditCommand | async ()=>false | NOOP | 返回 false = 未处理，消费侧回退浏览器默认编辑行为（InputContextMenu/EditorContextMenu 已按 false 分支） |
| 6 | readFile | HTTP `/api/fs/read`，非 ok→null | **UPGRADE** | 404→null（显式「未找到」，消费侧用于 file-moved 判断）；其他非 ok→`throw Error('web host 读取失败（HTTP <status>）')`；网络错误原样抛出。消灭 500→null 的静默降级 |
| 7 | readFileSnapshot | async ()=>null | NOOP | Web 无版本快照；消费侧判空走 readFile |
| 8 | readFileBase64 | HTTP，非 ok→null | **UPGRADE** | 同 #6 |
| 9 | readDocxHtml | HTTP，非 ok→null | **UPGRADE** | 同 #6 |
| 10 | readXlsxHtml | HTTP，非 ok→null | **UPGRADE** | 同 #6 |
| 11 | writeFile | async ()=>false | NOOP | Web 不写本地盘；false=未写入，消费侧（PreviewEditor）已有只读提示 |
| 12 | writeFileBinary | async ()=>false | NOOP | 同上 |
| 13 | copyFile | async ()=>false | NOOP | 同上 |
| 14 | writeFileIfUnchanged | async ()=>({ok:false}) | NOOP | 同上，形状保留 |
| 15 | watchFile | async ()=>false | NOOP | Web 无 fs watch |
| 16 | unwatchFile | async ()=>false | NOOP | 同上 |
| 17 | onFileChanged | ()=>{} | NOOP | 无事件源；不注册即无回调，语义保留 |
| 18 | watchWorkspace | async ()=>false | NOOP | 同 watchFile |
| 19 | unwatchWorkspace | async ()=>false | NOOP | 同上 |
| 20 | onWorkspaceChanged | ()=>{} | NOOP | 同 onFileChanged |
| 21 | spawnViewer | async ()=>null | NOOP | Web 无派生窗口；null=未派生，preview-actions 判空 |
| 22 | viewerRequestLoad | async ()=>null | NOOP | Web 无 viewer 载荷；viewer-window 判空显式报错 |
| 23 | viewerClose | ()=>{} | NOOP | 同上 |
| 24 | onViewerClosed | ()=>{} | NOOP | 无 viewer 事件源 |
| 25 | getFilePath | ()=>null | NOOP | 浏览器 File 无本地路径；null=不可得，消费侧判空 |
| 26 | getAvatarPath | ()=>null | NOOP | Web 无头像资源目录；splash/onboarding 判空用默认图 |
| 27 | getSplashInfo | async ()=>({}) | NOOP | Web 无 splash 元信息；空对象=无个性化，消费侧默认值 |
| 28 | selectFolder | async ()=>null | NOOP | 浏览器无目录选择器；null=取消/不可用 |
| 29 | selectFiles | async ()=>[] | NOOP | 浏览器无多文件系统选择器（input[type=file] 由组件自管）；[]=未选择 |
| 30 | selectSkill | async ()=>null | NOOP | 同 selectFolder |
| 31 | selectPlugin | async ()=>null | NOOP | 同上 |
| 32 | openFolder | ()=>{} | NOOP | 浏览器不能开 Finder/资源管理器 |
| 33 | openFile | ()=>{} | NOOP | 同上；本地路径在 Web 不可打开 |
| 34 | openExternal | try window.open catch{} | **UPGRADE** | window.open 抛异常或返回 null（弹窗拦截）→ 返回 `{ok:false, error:'web_host_open_external_unavailable'}`；成功返回 undefined。类型允许 `void | {ok,error?}` |
| 35 | showInFinder | ()=>{} | NOOP | 同 openFolder |
| 36 | trashItem（web 缺失，PlatformApi 可选） | — | ABSENT | Web 无回收站 |
| 37–44 | browserGoBack/Forward/Reload/NewTab/SwitchTab/CloseTab/EmergencyStop + closeBrowserViewer | ()=>{} | NOOP | Web 无内嵌浏览器视图（BrowserViewerApp 是 Electron 窗口） |
| 45 | openSkillViewer | ()=>{} | NOOP | Web 无技能查看器窗口 |
| 46 | listSkillFiles | async ()=>[] | NOOP | Web 不列本地技能目录 |
| 47 | readSkillFile | async ()=>null | NOOP | 同上 |
| 48 | onboardingComplete | async ()=>{} | NOOP | Web 无 onboarding 窗口生命周期 |
| 49 | debugOpenOnboarding | async ()=>{} | NOOP | 调试入口，仅桌面 |
| 50 | debugOpenOnboardingPreview | async ()=>{} | NOOP | 同上 |
| 51 | getPlatform | async ()=>'web' | REAL | 语义真实 |
| 52 | windowMinimize | ()=>{} | NOOP | 浏览器无窗口控制 |
| 53 | windowMaximize | ()=>{} | NOOP | 同上 |
| 54 | windowClose | ()=>{} | NOOP | 同上 |
| 55 | windowIsMaximized | async ()=>false | NOOP | 同上 |
| 56 | onMaximizeChange | ()=>{} | NOOP | 无事件源 |
| 57 | openSettings | ()=>{} | NOOP | Web 设置入口由应用内路由承担，宿主无动作 |
| 58 | settingsChanged | ()=>{} | NOOP | 单窗口 Web 无跨窗广播（settings 与主窗同页） |
| 59 | onSettingsChanged | ()=>{} | NOOP | 同上，无事件源 |
| 60 | onOpenSettingsModal | ()=>{} | NOOP | 同上 |
| 61 | reloadMainWindow | async ()=>location.reload() | REAL | 语义真实 |
| 62 | openBrowserViewer | ()=>{} | NOOP | 同 browser 系列 |
| 63 | （web fallback 中其余成员，含 quickChat/autoUpdate/train/keybindings/observabilityExport*/showNotification/getAppVersion 等 50 个 missingTypeMembers） | 缺失 | ABSENT | PlatformApi 可选成员，消费侧 `?.()` 判空；理由统一登记 `web_host_capability_absent: 仅 Electron 桌面提供` |

**4 处 HTTP 非 ok→null（任务书点名）逐条裁决**：#6 readFile、#8 readFileBase64、#9 readDocxHtml、#10 readXlsxHtml 全部升级为「404→null，其余非 ok→抛带状态错误」。证据：消费侧 preview-file-content.ts 的 `p.readFile?.()` 链路已有 try/catch（openFilePreview 捕获并展示错误态），抛出后呈现「读取失败」而非错误地显示「文件已移动」。

**openExternal 空 catch（任务书点名）裁决**：#34，升级为显式失败返回对象。

---

## 5. Mobile 18 项逐条裁决（去除 `Partial as PlatformApi` 强转）

mobile-platform.ts 改为 `installDesktopHostApi(api, 'mobile', reasons)`。既有 18 项保留并登记理由；**补齐 PlatformApi 必需（非可选）成员**为显式不可用实现，使类型不再需要强转。

既有 18 项：
| 成员 | 裁决 | 理由 |
| --- | --- | --- |
| getServerPort | REAL | location.port |
| getServerToken | REAL | 空 token（移动端凭 session 连接） |
| openSettings | REAL | CustomEvent 应用内路由 |
| onOpenSettingsModal | REAL | 同上，真实 unsubscribe |
| getFileUrl | REAL | browserSafeUrl 白名单转换（本地路径→''） |
| openFile | REAL | browserSafeUrl + window.open |
| openExternal | **UPGRADE** | window.open 失败返回 {ok:false,error}，与 web host 对齐 |
| settingsChanged | NOOP | 单窗口，无跨窗广播 |
| syncWindowTheme | NOOP | 无原生窗口主题 |
| onSettingsChanged | REAL(noop unsubscribe) | 无事件源，返回显式 unsubscribe |
| onSwitchTab | 同上 | 同上 |
| onServerRestarted | 同上 | 同上 |
| appReady | NOOP | 无主进程握手 |
| getPlatform | REAL | 'web'（移动端复用 web 平台标识，现状保留） |
| showNotification | NOOP | 无 Notification 权限流；桌面通知由服务端 WebSocket 提示承担 |
| getAppVersion | REAL | ''（移动端无壳版本） |
| getPendingAnnouncement | REAL | null（无合订本） |
| ackAnnouncement | NOOP | 无合订本状态 |

补齐的必需成员（PlatformApi 非可选、mobile 现缺 → 显式不可用，消灭强转后的潜在 TypeError）：
openBrowserViewer、selectFolder、selectFiles、selectSkill、readFile、writeFile、watchFile、unwatchFile、onFileChanged、readFileBase64、readDocxHtml、readXlsxHtml、spawnViewer、openFolder、showInFinder、closeBrowser。
裁决：读取类 `readFile/readFileBase64/readDocxHtml/readXlsxHtml` → 复用 Web 宿主同款 HTTP 实现（移动端经服务端 HTTP 读文件是真实能力，`/api/fs/read` 在 server 上存在）；写/选择/窗口类 → 显式 NOOP(false/null/[]) 并登记 `mobile_host_capability_unavailable`；closeBrowser/openBrowserViewer/showInFinder/openFolder/spawnViewer/onFileChanged → NOOP + 理由。

---

## 6. 边界白名单（显式保留的直连）

### 6.1 `window.hana` 直连白名单
| 文件 | 理由 |
| --- | --- |
| `desktop/src/react/host/electron-desktop-host.ts` | Electron 宿主适配器本体 |
| `desktop/src/react/host/desktop-host.ts` | 惰性安装时探测 `window.hana` 存在性（`typeof window.hana !== 'undefined'`） |
| `desktop/src/modules/platform.js` | 非模块引导脚本；仅探测 + 可视引导，不再安装 window.platform |
| `desktop/src/shared/theme.ts` | 独立 IIFE 包（lib/theme.js），先于 React 加载；仅 `window.hana?.syncWindowTheme?.()` 可选链调用，Web 下天然 no-op。若改经 host 模块会把宿主注册表打进 theme 包形成双实例，收益为负 |
| `desktop/src/react/utils/screenshot.ts` | 见 §6.4 说明后决定：**迁移**。`screenshotRender` 走 `(window as any).hana`，迁移为 `usePlatform()` + 局部类型收窄；属于业务工具文件，不留白 |

### 6.2 `window.platform` 直连白名单
| 文件 | 理由 |
| --- | --- |
| `desktop/src/react/host/desktop-host.ts` | 收编既有 `window.platform`（测试桩兼容）+ 安装时写别名 |
| `desktop/src/react/mobile/mobile-platform.ts` | 移动端宿主安装（经 installDesktopHostApi，内部由 desktop-host 写别名；本文件不再直写） |

### 6.3 业务服务导入边界白名单
- 业务文件（services/** 之外、测试之外）不得直接 import `services/server-connection`、`services/rust-*`、`services/chat-client`、`hooks/use-hana-fetch`，必须经 `services/service-client`。
- 白名单：`desktop/src/react/settings/api.ts`（设置窗口独立传输，既有专门通道，改造风险>收益，登记保留）。
- services/** 内部文件之间互相直接导入不受限（实现层）。

### 6.4 迁移规则（codemod 语义）
- `window.hana?.X` → `usePlatform().X?.`；`window.hana.X` → `usePlatform().X`；`const hana = window.hana` → `const hana = usePlatform()`；`(window as any).hana` → 迁移 + 局部类型收窄。
- `window.platform` 同规则。成员级 `?.` 全部保留（成员可选性不变）。
- 每文件添加 `import { usePlatform } from '<相对路径>/hooks/use-platform';`（合并进既有 import 块）。
- 语义等价论证：Electron 宿主下 api===window.hana 全成员存在，行为不变；Web 宿主下原本 `window.hana?.X` 为 no-op 的成员，Web 宿主保持 ABSENT（可选）或 NOOP（必需）后行为仍为 no-op/显式值。

### 6.5 执行修订记录（批次 D/E 落地后回写）
- **stores/** 导入豁免**：stores/** 文件与 hooks/use-stream-buffer.ts 禁止 import service-client（循环依赖：service-client→stores→service-client）。stores 允许 import `host/desktop-host`（宿主层为叶模块，不回导 stores）。由此 `session-actions.ts` 的 `isDesktopShell()` 由直读 `window.hana` 改为 `getDesktopHost().kind === 'electron'`，语义等价（Electron 下 hana 在⇔electron 宿主；mobile/web 均非 electron）。
- **settings/** 决策：除 `settings/api.ts` 白名单外，settings/** 全部纳入 window.platform 清扫与 ServiceClient 收拢（批次 D3）。
- **扫描器揭示的三处 grep 盲区**（批次 E 修正）：`media-source.ts` 的 `(window as any).platform` 断言形态、`session-actions.ts` 的 `(window as unknown).hana` 形态、`desktop/src/viewer-window-entry.tsx`（React 根之外的独立入口）——均迁移经宿主边界；viewer 入口局部 `ViewerPlatform` 结构子集接口保留为收窄视图。此后生产违例 0。
- **TraceDetailOverlay.tsx 事故因果**：批次 C 期间该文件（Lead T01 范围）被意外改动，按 Lead 裁决字节级回退恢复；根因是操作前未先核 git status 归属。教训——白名单/他人范围文件必须显式登记（扫描器 whitelistFiles + 理由），不可口头默认。
- **main 进程 .cjs 不属于渲染边界**：`office-pdf-helper.cjs`、`splash-avatar-path.cjs` 经 main.cjs/bootstrap.cjs require，`require('electron')` 为主进程合法 API，扫描器按白名单+理由登记，不视为渲染直连违例。

---

## 7. 消费者迁移分批计划

### 批次 A：宿主基础（本设计 §3 模块）
host/*（4 文件）+ hooks/use-platform.ts + modules/platform.js + mobile/mobile-platform.ts。
验证：typecheck + 现有 window.platform 桩测试抽跑（settings/AccessTab、components/PreviewRenderer.html 等）。

### 批次 B：ServiceClient + 导入收拢（纯 import 路径改写）
业务文件中 `from '<rel>services/server-connection'` / `'<rel>services/rust-*'` / `'<rel>hooks/use-hana-fetch'` / `'<rel>services/chat-client'` → `'<rel>services/service-client'`。范围：services/** 之外的非测试文件（基线：use-hana-fetch 64 文件、server-connection 76 文件、rust-* 12 文件、chat-client 4 文件，去重后约 100 文件）。settings/api.ts 白名单跳过。
验证：typecheck。

### 批次 C：window.hana 业务迁移（23 文件 → 0）
1. app-init.ts；2. settings/tabs（About/Access/General/Keybindings/observability-export-save/update-history-actions）；3. onboarding（OnboardingApp、TutorialStep）、quick-chat/QuickChatApp、splash/SplashApp；4. components（SkillViewerOverlay、SidebarNoticeSlot、PostUpdateAnnouncement[静态不可达 2 处一并迁移]）；5. hooks（use-auto-update-state、use-release-check、use-train-update-state）；6. services（rust-automation、rust-browser、ws-message-handler）；7. browser-viewer（BrowserViewerApp、RustBrowserViewer）；8. utils/screenshot.ts。
每子批验证：typecheck + 相关既有测试（settings/*、hooks 相关）。

### 批次 D：window.platform 业务迁移（~65 文件 → 0）
按目录：stores/ → utils/ → services/ → components/ → settings/ → onboarding/splash/viewer-window-entry/mobile/…
每子批：typecheck + 受影响目录测试。

### 批次 E：扫描器边界模式 + 文档再生
r08_client_inventory.mjs 增加 `boundary` 段（规则+违规+状态）与 `--check` 断言；重新生成 JSON/MD。

### 批次 F：A03 组件测试（批次 A 完成后即可先行）
`desktop/src/react/__tests__/host/r08-a03-web-desktop-host.test.tsx`：不注入 window.hana / window.platform，jsdom。
- 会话链：lingxiFetch 会话接口（mock fetch 200/500）+ `usePlatform()` 在宿主未安装时惰性 Web 化，getServerPort/getServerToken 显式返回。
- 设置链：render GeneralTab（mock settings store/api 模式复用 AccessTab.test.tsx），getAutoLaunchStatus ABSENT → 不 crash；`explainHostCapability('getAutoLaunchStatus')` 返回中文理由；settingsChanged NOOP 不炸。
- 文件预览链：readFileForPreview（mock fetch 文本）→ 返回内容；404→null；500→显式 throw；writeFile→false 显式未写入。
断言真实渲染输出/返回值，无 TypeError，无 `not a function`。

---

## 8. 可度量收口标准

1. `node scripts/rust-tauri/r08_client_inventory.mjs` 生成 JSON 中：
   - `boundary.hanaInvocationsOutsideWhitelist = 0`
   - `boundary.platformInvocationsOutsideWhitelist = 0`
   - `boundary.rendererElectronImports = 0`
   - `boundary.serviceImportViolations = 0`
   - `boundary.status = 'PASS'`
2. `node scripts/rust-tauri/r08_client_inventory.mjs --check` 退出码 0（JSON 新鲜 + boundary PASS）。
3. `npm run typecheck` 全绿。
4. A03 三个组件测试文件全绿；断言含「明确解释」文本/理由串。
5. 受影响目录既有测试不劣化（抽跑清单见 PROGRESS）。

---

## 9. 与任务书 5 步逐条核对（自审）

| 任务书步骤 | 本设计落点 | 一致性 |
| --- | --- | --- |
| ①清点 hana/preload/Electron 依赖并分类 | 复用并扩展 CLIENT_BOUNDARY_PREPARATION（responsibilityNotes + DESKTOP_HOST_CAPABILITY/CONNECTION_BOOTSTRAP/MIXED_*）；本设计 §4/§5 逐条裁决 | ✅ |
| ②业务→ServiceClient；宿主→DesktopHost（Electron 实现 + Web 受限实现） | §3 架构与接口草案 | ✅ |
| ③保留 React/Zustand/CSS/i18n/布局/快捷键，不做 Rust/WASM 迁移 | §1 非目标；全部迁移为 import 路径与访问器替换，无组件树/样式改动 | ✅ |
| ④工具卡片/thinking-MOOD/附件/终端/观测/历史保持 ID 与事件；仅在有完整替换证据时清理重复解析 | 不触碰组件 ID/事件；无重复解析清理（未获替换证据，按任务书保留现状） | ✅ |
| ⑤纯浏览器组件测试：明确能力提示而非空对象错误 | §7 批次 F；惰性 Web 宿主 + HostCapabilityError/理由登记 + 3 条链断言 | ✅ |

**与任务书的显式冲突/取舍登记**：
1. `shared/theme.ts` 保留 1 处 `window.hana?.syncWindowTheme?.()` 直连（§6.1）——任务书要求「window.hana 只许在白名单适配文件内」，theme.ts 列入白名单即满足「白名单」字面要求；其 pre-React IIFE 性质决定了经 host 模块会造成双实例，取舍理由已记录。
2. `settings/api.ts` 不并入 ServiceClient（§6.3 白名单）——任务书「业务调用转到唯一 ServiceClient」以「业务文件改为从这里导入」为操作定义；settings/api 是设置窗口既有独立传输通道而非业务调用副本，登记为白名单偏离。
3. `window.platform` 别名继续存在（安装时写入）——任务书要求「业务代码零直连」，别名仅为向后兼容测试桩与收编惰性安装，业务文件扫描断言为 0 直连，边界检查按调用点而非别名存在性判定。
4. 4 处 HTTP 非 ok→null 升级为「404→null、其余抛错」是行为变化（向好）：消灭 500 静默降级。消费侧链路已有 catch，风险受控；在 PROGRESS 记录证据。

---

## 10. 风险与缓解

| 风险 | 缓解 |
| --- | --- |
| codemod 误改字符串/JSX 属性 | TS-AST 级改写仅限 `window.hana`/`window.platform` MemberExpression 根；改后 typecheck + 目录测试 |
| HTTP null→throw 影响预览 UX | 仅非 404 改变；openFilePreview 已有错误展示路径；A03 预览链测试覆盖 404/500 |
| platform.js 不再装 window.platform，pre-React 脚本依赖 | 已核实 lib/theme.js 仅用 window.hana（可选链）；connection-csp.js/i18n.js 不读 window.platform；win32 滚动与 data-platform 保留在 platform.js |
| 测试桩 `window.platform = {...}` 与新注册表冲突 | getDesktopHost 收编既有 window.platform；resetDesktopHostForTests 供隔离 |
| 与并行队友（rust 测试 / r08-a04 测试）冲突 | 我只改 services/host/hooks/components/modules 源文件与指定脚本/文档；不改 rust/、rust-chat-history.ts、turn-projector.ts、r08-a04-* |
