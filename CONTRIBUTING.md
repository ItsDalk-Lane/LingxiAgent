# Contributing to 灵犀 Lingxi

感谢你对 灵犀 Lingxi 的关注！

使用、架构、发布流程及历史资料入口见[文档索引](docs/README.md)。

## 开发环境

### 前置条件

- Node.js `>=24.12.0 <25`（以 `package.json` 的 `engines` 为准）
- 与该 Node.js 版本兼容的 npm
- C/C++ 编译工具链（编译 `better-sqlite3` native module 需要）：
  - **macOS**：`xcode-select --install`（安装 Command Line Tools）
  - **Linux**：`sudo apt install build-essential python3`（Debian/Ubuntu）
  - **Windows**：安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（选择 "Desktop development with C++" 工作负载）

### 本地运行

```bash
# 安装依赖并执行安装后脚本
npm install

# 构建 workspace 包（新 checkout 或包源码变更后）
npm run build:packages

# 启动 Electron（自动构建前端）
npm start

# 或者用 Vite HMR 开发前端
npm run dev:renderer
# 另一个终端
npm run start:vite
```

上述 Electron 启动命令以及 `npm run server`、`npm run cli` 都使用 `scripts/launch.js`；它会将 `LINGXI_HOME` 设为 `~/.lingxi-dev`，覆盖传入的同名环境变量。开发数据与生产默认目录 `~/.lingxi` 分开。

### 常用命令

| 命令 | 说明 |
|------|------|
| `npm start` | 构建前端 + 启动 Electron |
| `npm run dev:renderer` | 启动 Vite HMR 服务 |
| `npm run start:vite` | 连接已运行的 Vite HMR 服务并启动 Electron |
| `npm run server` | 仅启动 Server |
| `npm run cli` | 运行 server-first CLI |
| `npm run build:packages` | 构建 `packages/*` workspace 包 |
| `npm test` | 运行测试（Vitest） |
| `npm run test:watch` | 监听模式运行测试 |
| `npm run typecheck` | TypeScript 类型检查 |
| `npm run lint` | ESLint 检查 |
| `npm run build:renderer` | 单独构建前端 |

### Native Module 注意事项

Server 以独立 Node.js 进程运行（`spawn`，非 `fork`），不在 Electron 主进程内，因此 `better-sqlite3` **不需要 `electron-rebuild`**。`build-server.mjs` 在目标 Node.js runtime 下执行 `npm install --omit=dev`，native addon 的 ABI 自动匹配。

### 打包环境

`npm run pack` 构建本地应用目录；`npm run dist`、`npm run dist:win`、`npm run dist:linux` 分别生成 macOS、Windows、Linux 安装包。完整打包会生成并校验签名 seed，需要 `LINGXI_SIGN_KEY` 指向签名私钥；缺少密钥时 `build:server` 会明确失败。

本地验证可用 `scripts/artifact-keygen.mjs` 在仓库外生成临时密钥，并通过 `LINGXI_SIGN_KEYSET` 指向对应公钥条目组成的 JSON 数组；构建与校验使用同一组环境变量。不要为本地验证修改 `shared/artifact-core/pinned-keyset.json`，临时签名产物也不能作为正式发布证据。

macOS 的 `scripts/notarize.cjs` 是唯一 `afterSign` 入口：有 `CSC_LINK` 时保留 Developer ID 签名，无证书时调用 ad-hoc 重签。本地不执行公证需显式设置 `SKIP_NOTARIZE=true`；执行公证则需准备 Apple 凭据。CI 的证书和公证选择见 [.github/workflows/build.yml](.github/workflows/build.yml)。

## 持久化 schema 改动 → 重钉指纹

仓库有一份钉死的持久化 schema 指纹 `build/persistence-schema-fingerprint.json`，它把受护源文件（持久化存储定义、site 映射源、schemaSource 声明的模块等）的解析树哈希和 SQLite runtime schema 固化下来，防止持久化形态在不知不觉中漂移。受护文件集合由指纹文件派生，不在本文维护固定数量。

**改了这些文件就必须重钉指纹，并在同一变更里带上 `build/persistence-schema-fingerprint.json` 的更新。** 本地可先运行只读检查，无需先提交：

```bash
node scripts/check-persistence-schema-fingerprint.mjs
```

本地检查比较 `HEAD` 与已跟踪文件的暂存及未暂存改动，不覆盖未跟踪文件。CI 的 `persistence-schema-guard` job 与 test 矩阵并行运行，比较对应事件的 Git diff：只要任一受护源文件变动、而指纹文件没有在同一 diff 里变动，就 fail 并打印重钉命令。权威校验仍是 `npm test` 里的 `tests/persistence-schema-tripwire.test.ts`（它运行完整的 `assertCommittedPersistenceSchemaFingerprint`）；快速 guard 通过不能替代它。

重钉命令（选一种）：

```bash
# 兼容性变更（持久化形态不破坏既有数据，如纯新增字段/表）
node scripts/generate-persistence-schema-fingerprint.mjs \
  --classification compatible \
  --compatibility-reason "<说明为什么这次源码改动是 schema 兼容的>"

# 破坏性变更（既有数据无法满足新形态，需要迁移 + DATA_EPOCH 递增）
node scripts/generate-persistence-schema-fingerprint.mjs \
  --classification breaking \
  --source-data-epoch <当前> --target-data-epoch <当前+1> \
  --affected-store <storeId> \
  --checkpoint-policy "<迁移前如何 checkpoint>" \
  --restore-policy "<迁移后如何 restore>"
```

> 注：指纹的源码哈希走的是 TypeScript **解析树**，所以纯注释 / 空白变更不会移动源码哈希。但 guard 按文件 diff 判断，仍要求同一变更更新指纹；应通过生成命令记录本次真实的兼容性说明，并检查生成差异。仅重复生成完全相同的内容不会使 guard 通过；完整 tripwire 另行验证实际形态。

## 验证范围

根据变更影响选择检查，测试取舍见 [tests/README.md](tests/README.md)。文档或提示词变更先核对事实、链接和差异；涉及代码行为时运行受影响测试及适用的类型、lint、构建或平台检查。提交和发布仍需满足相应 CI 门禁。未执行的跨平台、打包、外部服务检查不能写成通过。

## Pull Request

项目目前处于早期阶段，**不接受 Pull Request**。如果你有想法或发现了问题，欢迎开 issue 讨论。

## 报告问题

提交 issue 时请包含：

- 操作系统和版本
- Node.js 版本
- 复现步骤
- 期望行为 vs 实际行为
- 相关日志或截图

## 项目结构

```
core/           # Engine 编排层 + Manager
lib/            # 核心库（bridge、sandbox、memory、tools、providers）
server/         # Hono HTTP + WebSocket 服务
cli/            # 连接 Server 的命令行入口
hub/            # 后台任务（调度器、频道路由、Agent 通信、DM 路由）
desktop/        # Electron 应用 + React 前端
shared/         # 跨层共享工具（config schema、error bus、模型引用等）
packages/       # npm workspaces（插件协议、SDK、运行时和组件）
plugins/        # 内置系统插件（随应用打包）
skills2set/     # 内置技能定义
scripts/        # 构建工具（server 打包、启动器、签名）
tests/          # Vitest 测试
```

## License

提交贡献即表示你同意你的代码以 [Apache License 2.0](LICENSE) 授权。
