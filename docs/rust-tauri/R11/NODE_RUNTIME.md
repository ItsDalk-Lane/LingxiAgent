# R11 随包 Node 运行时与外围 worker

状态：**实现中；已随包构建验证，真实工具调用未逐项回归**

## 1. 业主裁决

按原任务书：旧 Agent 内核/Electron 生产链退出；**保留的 Node 外围 worker 可由应用
携带 Node 运行时**，不要求用户预装 Node。本文件记录该实现的供应链、打包与接线。

## 2. Node 运行时

| 项 | 值 |
|---|---|
| 版本 | `v24.16.0`（与 `engines.node >=24.12 <25` 一致） |
| 平台 | macOS arm64（当前范围） |
| 来源 | `https://nodejs.org/dist/v24.16.0/node-v24.16.0-darwin-arm64.tar.gz` |
| SHA-256 | `39189dab4eeb15706c424af0ac08a3044c9e48f7db12a7d77f6b7aafc7dd5df6` |
| 随包 | `desktop/src-tauri/resources/node/bin/node`（约 114 MB） |

脚本：`scripts/fetch-node-runtime.cjs`（`npm run fetch:node`）

- 只接受文件内固定的版本/URL/SHA-256；
- 解压后复制 `bin/node` 与 `LICENSE`，ad-hoc 重签并 verify；
- 目标目录在 `.gitignore` 内。

## 3. 外围 worker 资产

`scripts/stage-tauri-workers.mjs`（`npm run stage:tauri:workers`）生成
`desktop/src-tauri/resources/workers/`：

| worker | 入口 | 用途 |
|---|---|---|
| `office-document-worker.mjs` | `scripts/office-document-worker.ts` | Office 文档读取工具（预览另走纯 Rust） |
| `media-jimeng-worker.mjs` | `scripts/media-jimeng-worker.ts` | 即梦媒体参数/结果解析 |
| `bridge-sdk-worker.mjs` / `bridge-sdk-diagnostic.mjs` | `scripts/bridge-sdk-*.ts` | Bridge SDK 外围 |
| `ptc-worker.mjs` | `scripts/rust-tauri/r08-ptc-worker.mjs` | PTC 的 JS 沙箱 worker |
| `lsp-worker.mjs` | `scripts/rust-tauri/r08-lsp-worker.mjs` | AST/LSP 协议转换 |

同时复制：

- `resources/office/themes`（`desktop/dist-renderer/themes`）
- `resources/office/covers`（`desktop/src/assets/cover-gallery`）
- `resources/bin/ast-grep`（`bundled-bin/mac-arm64/ast-grep`）

worker 以 esbuild 单文件 ESM 打包（依赖尽量内联），并在打包时拒绝把旧
`core/agent`/`engine` 等执行循环带进来；`manifest.json` 记录大小/输入数。

## 4. 宿主接线

`desktop/src-tauri/src/host.rs::install_bundled_node_runtime` 在服务 spawn 前：

- 把 `resources/node/bin`、`resources/bin` 放到 `PATH` 最前；
- 设置 `LINGXI_BRIDGE_NODE` / `LINGXI_BRIDGE_WORKER`；
- 设置 `LINGXI_OFFICE_NODE` / `_WORKER` / `_THEMES` / `_COVERS`；
- 设置 `LINGXI_PTC_WORKER` / `LINGXI_LSP_WORKER` / `LINGXI_MEDIA_NODE` /
  `LINGXI_MEDIA_HELPER`（存在才覆盖，用户显式设置优先）。

`tauri.release.conf.json` 映射：

```text
resources/node -> node
resources/workers -> workers
resources/office -> office
resources/bin/ast-grep -> bin/ast-grep
```

Rust 服务侧：

- `coding_ast_lsp::worker` 支持 `LINGXI_LSP_WORKER`（开发回退仓库脚本）；
- `coding_ptc` 支持 `LINGXI_PTC_WORKER`；随包自包含 worker 不再要求仓库
  `lib/ptc`/`package.json` 进入 sandbox 白名单；
- AST/LSP/PTC 的 `node` 通过 `PATH` 解析到随包 Node；
- Office/Bridge/媒体沿用既有环境变量契约。

## 5. 已验证 / 未验证

已验证：

- `npm run fetch:node` 真实下载、SHA-256、解压、ad-hoc 签名；`node --version` = v24.16.0；
- 六个 worker 单文件打包成功，未包含旧 Agent core；
- 随包 Node 直接运行：
  - Office worker 非法请求返回结构化错误；
  - media worker 非法请求返回结构化错误；
  - PTC worker 非法帧 exit 71（协议拒绝）；
  - bridge worker 空输入 exit 0；
  - LSP worker `status` 返回 availability JSON（本机未安装语言服务，全部 false）；
- 五种 LSP 服务器在最终 `.app` 内由 PATH 直接解析，`status` 探针全部
  `available=true`（TypeScript/Python/Go/Rust/C++）；
- 宿主/服务 `cargo check` 通过；
- 真实 release `.app`（约 1.3 GB）内 `Resources/node/bin/node` = v24.16.0、
  `Resources/bin/ast-grep` = 0.45.3、六个 worker、Office themes/covers 与五种 LSP 服务器就位；
  主应用 `codesign --verify --deep --strict` exit 0（ad-hoc）；
- 在安装包路径下用随包 Node 运行 Office worker（真实 docx）返回正文；
  media/PTC/LSP/bridge worker 的非法/空输入均按协议拒绝或返回状态。

未验证：

- 真实宿主进程 + 服务进程下的工具端到端调用（只验证了 worker 直接运行）；
- Windows/Linux；
- 安装包体积/签名/公证（业主已决定不做正式签名）。

## 6. 许可

Node.js 为 MIT 许可；`resources/node/LICENSE` 随包。Chromium 见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。正式分发前需业主/法务确认。
