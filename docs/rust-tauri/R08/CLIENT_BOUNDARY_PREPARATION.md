# T02 客户端边界准备与断言

结论：静态清点与边界断言已落盘，**不算 A04 通过，也不接受 R07 准入**。业务/宿主抽取（T02）已实施：组件/工具/设置不再直读 `window.hana`/`window.platform`，统一经宿主边界（`DesktopHost`）与服务客户端（`service-client`）。

`CLIENT_BOUNDARY_PREPARATION.json` 以 TypeScript AST 检查 701 个非测试桌面源码候选，并沿八个现有 HTML 入口、独立 theme 构建源码及相对导入追踪到 640 个渲染源码文件。按词法符号追踪局部 `hana`、`platform`、取出的方法别名、`usePlatform()` 返回及类型/非空断言；注释、字符串和测试不算生产调用（测试另有语法级审计，见下）。

## 边界断言（批次 E，PASS）

`boundary.result = PASS`：全部非测试源码中 `window.hana`/`window.platform` 的根引用、成员访问、调用共 0 处位于白名单之外；非测试渲染源码 `import/require 'electron'` 0 处。`--check` 在 JSON 不新鲜或断言非 PASS 时非零退出。

白名单（仅宿主适配层与既有引导/主进程文件）：

- `desktop/src/react/host/`：DesktopHost 三实现（electron / web / mobile 收编）。
- `desktop/src/modules/platform.js`：先于 React 的可视引导 + Electron 下 `window.platform=window.hana` 抢先别名（兼容桥，非职责分离豁免）。
- `desktop/src/shared/theme.ts`：独立 theme 构建源码。
- `desktop/src/react/mobile/mobile-platform.ts`：移动端宿主安装点。
- `TraceDetailOverlay.tsx`（Lead T01 范围，既有 `rust-chat-history` 动态 import 结构）、`office-pdf-helper.cjs` 与 `splash-avatar-path.cjs`（main 进程专用，经 `main.cjs`/`bootstrap.cjs` require，不进渲染构建）。

测试侧：`window.hana`/`window.platform` **赋值**视为宿主桩安装，收编 151 处；**读取**单列清单 193 处（含 `(window as any).platform` 断言形态），暂不计入阻断违例——`composer-send.transaction.test.ts`/`R02RustWebSocket.test.ts` 属 Lead T01 在飞范围不可触碰，其余断言读取迁移列入后续批次。这是显式记录的债，不是静默放行。

## 迁移后的边界结构

`usePlatform()` 返回 `getDesktopHost().api`：Electron 桥惰性收编为 electron 宿主（别名对象同一），无桥环境安装 Web 受限宿主并回写 `window.platform` 别名，移动端宿主由 `mobile-platform.ts` 显式安装。`explainHostCapability` 对受限能力给出注册原因或宿主通用说明，替代 null 对象式报错。preload 112 项，PlatformApi 110 个具名成员；`screenshotRender` 与两个 onboarding debug 成员仍未显式类型化，`closeBrowser` 有类型但未由 preload 提供（沿用既有缺口，本批次未扩大）。

Web 宿主 61 项（`web-desktop-host.ts`），56 项为显式 no-op/空值并附中文原因；四个读文件/文档 HTTP 分支 404→null、其余非 ok 显式抛错（`Web 宿主读取文件失败（HTTP nnn）`）；`openExternal` 空 catch 已改为显式失败对象。Mobile 18 项 `Partial` 强转保留（测试契约编码，桌面专属 FS 成员保持 undefined 而非 no-op）。

保留且未削弱的显式能力处理：截图缺桥错误提示、观测导出 Electron/File System Access 检测、快捷键占用 unavailable 探测、Rust 浏览器入口缺能力报错、`browser_desktop_unavailable`、快捷聊天窗口接口不可用提示。

## 可复用后端通道

十份 `rust-*` 服务适配（含知识库）列出真实导出与导入消费者；HTTP 主入口 `lingxiFetch/lingxiUrl` 经 `service-client.ts` 单一出口复用地址转换、连接解析与认证；ServerConnection 提供连接解析、HTTP/WS URL、票据与稳定身份。本清点不证明这些 R07 后端已获准入，也不检查无 final 的汇总/工具/思考/MOOD 真实点击。

生成与复查命令均已执行退出 0：

```bash
node scripts/rust-tauri/r08_client_inventory.mjs
node scripts/rust-tauri/r08_client_inventory.mjs --check
```

JSON 绑定源码摘要，列出调用/别名、preload IPC 通道、Web 常量与 HTTP 错误分支、缺失成员、测试桩/读取清单及现有适配入口。静态文件可达不保证每条运行分支可达；本方法不证明跨任意函数参数、对象字段和反射的完整数据流，也没有跑组件或真实界面验收。改动相关源码后需重新生成，并对具体行为另行验证。
