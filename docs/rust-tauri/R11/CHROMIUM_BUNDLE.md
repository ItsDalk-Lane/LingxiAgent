# R11 随包 Chromium（网页/PDF 预览）

状态：**实现完成；未做真机安装/签名分发验证**

## 1. 目标

Tauri 产品包不再要求用户机器预装 Chrome。网页查看器、PDF 渲染/打印所需的
浏览器组件随应用分发，由宿主在启动服务前注入固定路径。

## 2. 固定版本与校验

| 项 | 值 |
|---|---|
| 来源 | Chromium 官方快照桶 `chromium-browser-snapshots` |
| 平台 | macOS arm64（业主当前平台） |
| revision | `1727675`（与 2026-10-27 的 `Mac_Arm/LAST_CHANGE` 一致） |
| 下载 URL | `https://storage.googleapis.com/chromium-browser-snapshots/Mac_Arm/1727675/chrome-mac.zip` |
| SHA-256 | `87d91e250130b993b62f571a4c110a5d5c0ed729143fce8a9d9bbd8616c1247c` |
| 解压后 | `desktop/src-tauri/resources/chromium/Chromium.app`（约 373 MB） |

脚本：`scripts/fetch-chromium.cjs`（`npm run fetch:chromium`）

- 只接受脚本内固定的 URL + SHA-256；先验哈希再解压。
- 解压/复制后对嵌套 `Chromium.app` 重新做 **ad-hoc** 签名（快照原始签名资源
  不完整，直接随包会导致主应用 `codesign --verify --deep --strict` 失败）；
  不做正式签名。
- 目标目录在 `desktop/src-tauri/.gitignore` 内，不进入 git。

## 3. 打包与启动接线

- `package.json`：`build:tauri` 先执行 `fetch:chromium`，再调用 Tauri 构建。
- `desktop/src-tauri/tauri.release.conf.json`：
  `"resources/chromium": "chromium"`，进入
  `Lingxi.app/Contents/Resources/chromium/Chromium.app`。
- `desktop/src-tauri/src/host.rs`：
  - `bundled_chromium_executable()` 按 `LINGXI_CHROMIUM_PATH` → resource dir →
    开发目录顺序找 `Chromium.app/Contents/MacOS/Chromium`；
  - `install_bundled_chromium()` 在服务启动前设置环境变量；
  - 找不到时写 `chromium-unavailable` 诊断，服务显式报不可用，不静默降级。
- 服务端既有 `LINGXI_CHROMIUM_PATH` 校验（必须绝对路径且存在）不变。

## 4. 已验证 / 未验证

已验证：

- `npm run fetch:chromium` 真实下载、SHA-256 校验、解压、ad-hoc 签名；
- release `.app` 真实构建：`Lingxi.app` 约 1.3 GB（含 Node/LSP/worker 资源），
  `Contents/Resources/chromium/Chromium.app` 就位，`Chromium --version` 返回
  `Chromium 159.0.8128.0`；主应用 `codesign --verify --deep --strict` exit 0；
- 重复执行幂等（marker 命中直接跳过）；
- 嵌套 `Chromium.app` `codesign --verify --deep --strict` exit 0；
- 宿主 `cargo check` 通过。

未验证：

- 真实 release `.app` 安装后的 Chromium 启动、PDF 渲染、网页查看器；
- 主应用整体 codesign/公证（业主已决定不做正式签名）；
- Intel Mac / Windows / Linux（当前不在范围）；
- 许可合规/体积评审的最终签署（Chromium 为 BSD 风格开源许可，需随包附
  第三方说明；正式对外分发前由业主确认）。

## 5. 回退

删除 `desktop/src-tauri/resources/chromium` 并从 release 配置移除资源映射，
宿主会回到“系统 Chrome 路径发现”；服务缺失时显式不可用。不涉及用户数据。
