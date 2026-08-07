# Contributing to 灵犀 Lingxi

感谢你对 灵犀 Lingxi 的关注！

## 开发环境

### 前置条件

- Node.js >= 24.12 (see package.json engines)
- npm (latest compatible with your Node.js version)
- C/C++ 编译工具链（编译 `better-sqlite3` native module 需要）：
  - **macOS**：`xcode-select --install`（安装 Command Line Tools）
  - **Linux**：`sudo apt install build-essential python3`（Debian/Ubuntu）
  - **Windows**：安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（选择 "Desktop development with C++" 工作负载）

### 本地运行

```bash
# 安装依赖
npm install

# 启动 Electron（自动构建前端）
npm start

# 或者用 Vite HMR 开发前端
npm run dev:renderer
# 另一个终端
npm run start:vite
```

### 常用命令

| 命令 | 说明 |
|------|------|
| `npm start` | 构建前端 + 启动 Electron |
| `npm run start:vite` | Vite HMR 模式启动 |
| `npm test` | 运行测试（Vitest） |
| `npm run typecheck` | TypeScript 类型检查 |
| `npm run build:renderer` | 单独构建前端 |

### Native Module 注意事项

Server 以独立 Node.js 进程运行（`spawn`，非 `fork`），不在 Electron 主进程内，因此 `better-sqlite3` **不需要 `electron-rebuild`**。`build-server.mjs` 在目标 Node.js runtime 下执行 `npm install --omit=dev`，native addon 的 ABI 自动匹配。

## 持久化 schema 改动 → 重钉指纹

仓库有一份钉死的持久化 schema 指纹 `build/persistence-schema-fingerprint.json`，它把约 149 个"受护源文件"（持久化存储定义、site 映射源、schemaSource 声明的模块等）的解析树哈希和 SQLite runtime schema 固化下来，防止持久化形态在不知不觉中漂移。

**改了这些文件就必须重钉指纹，并在同一次提交里带上 `build/persistence-schema-fingerprint.json` 的更新。** 不知道一个文件算不算受护？改完直接提交，CI 会告诉你。

CI 的 `persistence-schema-guard` job 会在 test 矩阵之前做一次快速 diff 防呆：只要你碰了任一受护源文件、而指纹文件没有在同一次 diff 里变动，就 fail 并打印重钉命令。权威校验仍是 `npm test` 里的 `tests/persistence-schema-tripwire.test.ts`（它跑完整的 `assertCommittedPersistenceSchemaFingerprint`），guard job 是它的快前哨。

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

> 注：指纹的源码哈希走的是 TypeScript **解析树**，所以纯注释 / 空白变更不会移动哈希。但 guard job 比权威校验更严格——注释改动也会被要求重钉。重钉后若哈希未变，`npm test` 会确认指纹确实一致，不会误报。

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
hub/            # 后台任务（调度器、频道路由、Agent 通信、DM 路由）
desktop/        # Electron 应用 + React 前端
shared/         # 跨层共享工具（config schema、error bus、模型引用等）
plugins/        # 内置系统插件（随应用打包）
skills2set/     # 内置技能定义
scripts/        # 构建工具（server 打包、启动器、签名）
tests/          # Vitest 测试
```

## License

提交贡献即表示你同意你的代码以 [Apache License 2.0](LICENSE) 授权。
