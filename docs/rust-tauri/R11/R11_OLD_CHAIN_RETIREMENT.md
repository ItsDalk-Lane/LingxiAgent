# R11 旧链清退记录

> 状态：**旧 Electron/Node 内核源码已删除；R11 最终验收未完成**
> 业主 2026-10-27 授权：保留 Node 外围 worker + 随包 Node 运行时，删除旧版应用
> 与旧内核源码。本文件记录实际删除范围、保留边界与未完成项。

## 1. 已删除

### 1.1 旧 Agent/服务/入口目录

| 目录 | 删除文件数 | 内容 |
|---|---|---|
| `core/` | 215 | 旧 Pi/Node Agent 核心（loop/session/model runtime） |
| `server/` | 113 | 旧 Node HTTP/WS 服务 |
| `cli/` | 11 | 旧 Node CLI（尚未有等价 Rust CLI） |
| `hub/` | 9 | 旧频道/调度入口 |

### 1.2 旧 Electron 源文件

删除 `desktop/main.cjs`、`preload.cjs`、`bootstrap.cjs`、`auto-updater.cjs`、
`mac-self-install.cjs`、`file-*`/`ipc-wrapper`/`keep-awake`/`login-item-settings`/
`quick-chat-session-handoff`/`speech-permissions`/`workspace-watch-registry`，
以及 `desktop/src/office-pdf-helper.cjs`/`office-pdf-fonts.cjs` 和
`vite.config.main.js`/`vite.config.preload.js`/`vite.config.server.js`/
`scripts/launch.js`。旧 Electron 已无法启动或打包。

### 1.3 旧测试与旧模块

- 删除 **1053 个旧 Node 测试文件**（`tests/**/*.test.*` 及引用了旧目录的
  renderer 测试）；旧内核测试不再运行/不再复用旧结果。
- `lib/` 从 482 个文件删到 **45 个文件**：43 个 worker 实际依赖文件 +
  2 个 Rust `include_str!` 共享文件（`browser-search-extractors.cjs`、`config.example.yaml`）；
  `plugins/` 从 34 个删到 **5 个 worker 实际依赖文件**。
- 保留的 `lib/`/`plugins/` 文件由 worker 构建验证（esbuild metafile 闭包），
  不再包含旧 Agent 执行循环。
- 旧 `package.json` 入口/脚本/依赖：electron-builder 配置、Electron 启动/打包
  脚本、Pi 运行时依赖、Node 服务/CLI 脚本、旧 server artifact 脚本等已删除。
- `package-lock.json` 已同步。

## 2. 保留的边界

### 2.1 外围 worker（允许的 Node）

六个受控 worker 由 Rust 服务通过 worker RPC 启动，不拥有任务终态/审批/上下文：

- Office 文档读取；
- 媒体/即梦解析；
- Bridge SDK 与诊断；
- PTC（JS 沙箱）；
- AST/LSP 协议转换。

随包资源：`resources/node`（Node 24.16.0）、`resources/workers`（六个单文件 ESM）、
`resources/office`（主题/封面）、`resources/bin/ast-grep`、`resources/lsp`
（TS/Python/Go/Rust/C++ 五种语言服务器）。宿主在服务启动前注入 PATH 与
`LINGXI_*` 路径；详见 [NODE_RUNTIME.md](NODE_RUNTIME.md)。

### 2.2 共享代码

`shared/` 原样保留（跨层契约/artifact-core 等）；`lib/`/`plugins/` 只保留 worker
闭包文件；`desktop/src/react` 为唯一 renderer。旧 `scripts/` 中有历史/证据工具仍
可能引用已删除目录，但它们不参与生产构建；R11-T02 的构建脚本收尾仍需清理。

## 3. 验证

| 检查 | 结果 |
|---|---|
| `npm run typecheck`（root/node/test） | PASS |
| 剩余 `npm test` | 511 个 renderer 测试：8 个文件/10 条失败，均为 live/env 类（R06/R07 live、composer、R02 WS）；旧内核测试已删除 |
| worker 重新打包（六个） | PASS；无旧 core 输入 |
| desktop 宿主 `cargo test --lib` | 267 passed（上一轮基线，Rust 未改） |
| service `cargo test --lib` | 708 passed / 3 failed（既有/环境类） |
| Tauri release 构建（含 Node/LSP/worker） | PASS；见 R11_REPORT |

R11-T06 的“最终 SHA 全量验证 + 新测试迁移”仍未完成：本次是**旧测试删除**，
没有把 1053 个旧测试逐条改写成 Rust/新契约测试；R11 不能宣称完成。

## 4. 回退

所有删除在 git 历史中可恢复；工作区未提交。恢复旧内核需恢复对应目录/测试与
`package.json`/lock 并重建；旧 Electron 生产入口仍不应恢复。正式发布/提交需另行授权。
