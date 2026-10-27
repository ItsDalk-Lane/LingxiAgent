# R11 阶段工作记录（进行中）

> 状态：**旧 Electron/Node 内核源码已删除；R11 最终验收未完成**
> 业主 2026-10-27 授权：保留外围 Node worker + 随包 Node/LSP，删除旧版应用与
> 旧内核源码。删除范围/保留边界见 [R11_OLD_CHAIN_RETIREMENT.md](R11_OLD_CHAIN_RETIREMENT.md)。

## 1. 已完成

### 1.1 外观设置异常内容校验

`rust/crates/lingxi-service/src/userprefs.rs`：

- `appearance` PUT 在合并写入前校验 `theme`：非字符串/空/含控制字符/超 64 字符
  一律 400，不写 `preferences.json`；
- 未知主题 id 仍允许（客户端主题表可扩展）；
- 单测 `appearance_patch_requires_safe_theme_string` 通过（711 个 service lib
  测试中过滤执行）。

### 1.2 正式版固定“文件任务”入口

- 欢迎页新增固定按钮（`data-testid="file-task-entry"`，五语言文案）；
- 通过 `desktop/src/react/stores/file-task-actions.ts` 桥接到输入区真实附件流程；
- 欢迎页无会话时先落草稿会话再选文件，避免“请先创建会话”；
- 新单测 `file-task-actions.test.ts` 3/3 通过。

### 1.3 随包 Chromium

见 [CHROMIUM_BUNDLE.md](CHROMIUM_BUNDLE.md)。要点：

- 固定 revision 1727675 + SHA-256，`npm run fetch:chromium` 下载/校验/解压/ad-hoc 重签；
- `tauri.release.conf.json` 资源映射；宿主启动前注入 `LINGXI_CHROMIUM_PATH`；
- 真实 release 构建验证：`Lingxi.app` 约 1.3 GB（含 Chromium/Node/LSP/worker 资源），内置
  `Chromium.app`（`--version` = `Chromium 159.0.8128.0`），主应用
  `codesign --verify --deep --strict` exit 0（ad-hoc）。

### 1.3b 随包 Node 运行时与外围 worker

按业主裁决，外围 Node worker 保留实现，由应用携带 Node 运行时。详见
[NODE_RUNTIME.md](NODE_RUNTIME.md)。固定 Node `v24.16.0`（SHA-256 校验、ad-hoc
重签），并把 Office/媒体/Bridge/PTC/LSP 六个 worker 打成单文件 ESM，连同
Office 主题/封面与 `ast-grep` 一起装入 `resources/`；宿主在服务启动前注入
`PATH` 与 `LINGXI_*` 路径；五种 LSP 语言服务器（TS/Python/Go/Rust/C++）也全部随包，
最终 app 内 `status` 探针全部可用。

### 1.4 旧版应用生产入口退出

见 [R11_OLD_CHAIN_RETIREMENT.md](R11_OLD_CHAIN_RETIREMENT.md)。`package.json` 已移除
Electron 入口/启动/打包脚本、electron-builder 配置与依赖；Tauri 成为唯一桌面生产
构建入口。release `.app` 的 `Contents/MacOS/` 只有 `lingxi-resource-desktop` +
`lingxi-service`，无 Electron/asar；Node 作为受控资源在 `Contents/Resources/node`。

## 2. 未完成（不得声称 R11 完成）

- 旧内核源码已删除，但 **1053 个旧 Node 测试是被删除，不是逐条迁移**；
  Rust/新契约测试覆盖缺口仍需 R11-T06 补齐；
- 旧数据导入/回滚、最终 SHA 全量验证、SBOM、四平台产物、正式发布均未做；
- `scripts/` 中仍有历史证据工具引用已删除目录（不参与生产构建），
  R11-T02 构建脚本收尾仍需清理；
- 全量 `npm test` 剩余 8 个失败文件/10 条（live/env 类，见 §3），
  不能作为 R11 通过证据。

## 3. 验证

| 检查 | 结果 |
|---|---|
| release Tauri 构建（含 Chromium） | PASS；`Lingxi.app` 约 1.3 GB（含 Chromium/Node/LSP/worker 资源） |
| `codesign --verify --deep --strict` | exit 0（ad-hoc） |
| `Chromium --version` | `Chromium 159.0.8128.0` |
| desktop `cargo test --lib` | 267 passed / 0 failed（本轮功能改动前基线；随后新增单测另行通过） |
| service `appearance_patch_requires_safe_theme_string` | 1 passed |
| service `cargo test --lib` 全量 | 708 passed / 3 failed（background media、github driver、session receipt，均为既有/环境类，非本轮 Node 打包改动） |
| `npm run typecheck`（root + node + test 三套） | PASS |
| `file-task-actions.test.ts` | 3 passed |
| `r09-t01-tauri-desktop-host.test.tsx` | 16 passed（已把已实现的更新命令从“未接线”断言移除） |

### 设置页“刷新失败: model plane unavailable”修复（2026-10-27）

首启/未配置模型时，所有设置子页顶部显示全局 `刷新失败: model plane unavailable`。
根因不是设置数据损坏：设置页初始化会读取 `/api/models` 模型目录，而
`chat_wire::models_route` 先调用 `setting_choice`；没有模型来源/助手时该调用返回
503，目录读取失败被前端记为 `runtimeModelsError`，于是全局横幅出现在每个 tab。

修复：模型目录接口不再要求已有当前选择；`setting_choice` 仅用于标记 `isCurrent`，
未配置时返回空目录 200。真实服务验证：
`GET /lingxi/v1/models` → `{"models":[]}` HTTP 200。重新构建 service/app 并重启。

### 桌面启动 smoke（2026-10-27）

在隔离 `LINGXI_HOME`（`/tmp/lingxi-r11-launch2/home`）启动最终 `.app`：

- `chromium-ready`、`node-runtime-ready`（Node/LSP/ast-grep 路径正确）；
- `service-ready` = `local-owned`（pid 29931）；
- `page-load` = `tauri://localhost`；
- `app-ready` = `windowVisible:true`，窗口标题为 `Lingxi`（已移除旧“Rust 预览”标题与“测试版”权限文案）；
- SIGTERM 后 host exit 0，无 service 残留；真实 `~/.lingxi` 未触碰。

启动时发现并修复：host 早于 About 更新链构建了 reqwest Client，而
`rustls-no-provider` 需要先安装 crypto provider，否则 release 包在 tokio worker
panic。已在 `lib.rs::run()` 最前面统一安装 ring provider。

删除旧测试后 `npm test` 为 **511 个 renderer 测试 / 8 个失败文件 / 10 条失败**，
失败均为 live/env 类（R06/R07 live 环境变量、composer、R02 WS），不是旧内核
测试；旧内核测试已随源码删除，不能复用旧结果。R11-T06 仍需补齐新测试并重跑最终 SHA。

## 4. 后续

1. 为删除的旧 Node 测试补 Rust/新契约回归（R11-T06 的覆盖缺口）；
2. 旧数据导入/回滚与升级兼容验证；
3. 清理 `scripts/` 中引用已删除目录的历史工具；
4. 最终 SHA 全量验证、SBOM、摘要；
5. 提交/推送/发布/真实迁移仍需单独授权。
