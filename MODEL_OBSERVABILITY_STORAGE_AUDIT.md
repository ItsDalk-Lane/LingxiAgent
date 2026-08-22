# MODEL_OBSERVABILITY_STORAGE_AUDIT.md — Phase 7 Storage Architecture Audit

> Step 1 交付（编码前审计）。基线：`feature/model-call-observability` @ `f7d0fee5`
> （Phase 6 功能树 `7374e0d6`，seal `f7d0fee5`）。
> 本文回答任务书 §五 的 10 个问题，并固化 Phase 7 存储架构决策。
> 代码事实以本审计时的当前树为准（file:line 引用）。

## Q1 — LINGXI_HOME canonical root

- 唯一 canonical 实现：`shared/hana-runtime-paths.cjs` `resolveLingxiHome()`
  （默认 `~/.lingxi`，`LINGXI_HOME` 环境变量覆盖，`~` 展开后 `path.resolve`）。
  TS 薄封装 `shared/hana-runtime-paths.ts` 仅 re-export。
- Server：`server/index.ts:254-255` 启动最早处 resolve 并回写 env。
- Desktop：`desktop/bootstrap.cjs:129-131` resolve 后经 env 传给 spawn 的 server
  （`desktop/main.cjs:1722`）；Electron userData 按其隔离。
- CLI：`cli/local-server.ts:5-17` 有重复实现，但只用于读 `server-info.json` /
  拉起 server 子进程；CLI 进程本身不建 engine、不产生模型调用。
- Engine 不解析 home，只接收 `lingxiHome` 构造参数（`core/engine.ts:348`）。

**决策**：observability store 的根 = `path.join(lingxiHome, "model-observability")`，
由 coordinator 从 engine 传入的 `lingxiHome` 派生，不自行 resolve（§七）。

## Q2 — 哪些进程可能写模型观测数据

模型调用只发生在 **server 进程**（唯一构造 `LingxiEngine` 的地方，
`server/index.ts:438-443`）。Desktop 把 server 作为子进程拉起；CLI 是纯 HTTP
客户端（`cli/` 无 engine import，`cli/chat.ts` 只透传 health.model 字符串）。
因此当前生产拓扑下 observability 的写者 = server 进程（单写者）。

## Q3 — 多进程同宅可能性

- Server 启动有同宅互斥闸（`server/index.ts:275-293` probe `server-info.json`，
  token 认证），阻止两个 server 长期并发。
- 但存在短暂重叠窗口：旧 server 退出中 / crash 残留 / CLI 拉起 detached server
  与手工 `hana serve` 竞争。mac/Linux 上 WAL 模式 SQLite 支持同主机多进程
  文件锁并发访问，是安全基线。

**决策**：不依赖「永远只有一个 Node process」。SQLite 配置
`journal_mode = WAL` + `busy_timeout = 5000` + `synchronous = NORMAL`，
可安全应对同 LINGXI_HOME 多进程先后或短暂并发访问（§六）。

## Q4 — 当前 SQLite Store 统一模式（照抄基线）

以 `core/session-manifest/store.ts` 为代表：

- better-sqlite3 经 `createRequire` 懒加载（`:14-23`），支持测试注入 Database。
- 构造：`fs.mkdirSync(dirname)` → `new Database(dbPath)` → pragmas
  （WAL / synchronous=NORMAL / cache_size / temp_store=MEMORY / mmap_size）
  → `_initSchema()`（CREATE TABLE IF NOT EXISTS）→ `_migrate()`
  （`PRAGMA user_version` 单调迁移，`:307-337`，migration 在一个 transaction 内）
  → `_prepareStatements()`。任一步失败：close db 后重抛（`:207-215`）。
- WAL/SHM sidecar 文件与 DB 同目录（registry pathPatterns 含 `-wal/-shm`）。

**决策**：observability DB 沿用同一模式；额外差异：
`auto_vacuum = INCREMENTAL`（建表前设置，retention 删除后
`PRAGMA incremental_vacuum` 收缩）、`secure_delete = ON`（敏感正文删除语义，
§八十九，性能取舍见下）、`busy_timeout`（多进程窗口）。

## Q5 — Data Epoch 分类

Observability DB 既不是业务 authoritative store（Agent/Session 正确性不依赖它），
也不是完全可再生成数据（历史模型调用无法重录）。checkpoint 若包含它只会在
epoch 迁移时复制可能很大的观测库。

**决策**（§三十一/三十二）：

- `epochPolicy = "compatible"`，`affectedByEpochMigration = false`。
- `checkpointPolicy`：显式写明「不进入业务 Data Epoch migration checkpoint；
  它不是恢复 Agent/Session 正确性所必需，且可能非常大」。
- `restorePolicy`：显式写明「不参与 epoch restore；业务 rollback 后指向已不
  存在 sessionId 的 observability 记录是允许的历史事实（§三十三）」。
- 内部 schema 用 SQLite `PRAGMA user_version` 自管（SCHEMA_VERSION=1 起）。
- 结构性保证：`core/data-epoch-coordinator.ts:239-261` validateAffectedStores
  对 `affectedByEpochMigration=false` 的 store fail-closed 拒绝进入迁移批次；
  checkpoint provider 只捕获 migration 批次引用的 store id——双保险。

## Q6 — 跨 Desktop/Server/CLI 的真实 secure key management

**结论：不存在。** 全仓 core/lib/shared 无 keytar / Electron safeStorage /
libsecret / keychain 集成（grep 实证）。现有保护只有 `shared/secret-fs.ts`
的 owner-only 文件权限（Unix 0600/0700；Windows 显式承认 NTFS 不实现 POSIX
mode，依赖 profile 目录继承 ACL，不假装等价）。

**决策**（§七十六/七十七）：不实现伪加密。at-rest protection =
dedicated private directory（Unix 0700）+ DB/blob 文件 0600 + payload
persistence 显式 opt-in + bounded retention。文档明确「At-rest protection:
filesystem permissions, not cryptographic encryption」，Known Gap 诚实记录。

## Q7 — private-directory/private-file helper

存在：`shared/secret-fs.ts`（`SECRET_FILE_MODE=0o600` / `SECRET_DIR_MODE=0o700` /
`writeSecretFileSync` / `ensureSecretFileModeSync` / `ensureSecretDirModeSync`）。
DB/WAL/SHM 与 blob 文件在目录收紧后创建即可继承（Unix 默认 umask 修正）；
打开时与每次 maintenance 时 `ensureSecretDirModeSync` + `ensureSecretFileModeSync`
回紧（§七十八：不能只 chmod DB 漏掉 -wal/-shm，目录权限先收紧）。

## Q8 — graceful shutdown 生命周期

- Server：`gracefulShutdown()` `server/index.ts:1310-1360`（SIGINT/SIGTERM/
  SIGBREAK 注册 `:1362-1364`；HTTP close → browser 挂起 → bridge stop →
  deferred flush → `hub.dispose()` → 删 server-info → exit）。
- Hub：`hub/index.ts:400-406` `dispose()` → `engine.dispose()`。
- Engine：`core/engine.ts:2669-2697` `dispose()`，finally 中
  `_sessionManifestStore.close()`。
- CLI：无 engine、无 store，无需 flush。

**决策**：engine 在 `dispose()` 中以 **bounded timeout**（默认 5s）
`await coordinator.close()`（close = final flush + stop timers + close DB +
uninstall observer/sink，§四十五）。coordinator 的 close 幂等、超时后强制关闭。

## Q9 — startup 哪个阶段打开 observability store

模型调用最早发生在 engine.init() 装载 agent 之后（memory ticker / probe /
用户 turn）。因此 store 必须在 **engine_construct** 阶段（构造函数内）打开并
安装 observer/sink——与 SessionManifestStore/FileHistoryStore 同相位
（registry `firstPossibleOpenPhase: "engine_construct"`）。

- 打开失败（损坏/未知高版本/迁移失败）**不抛进 engine 构造**：禁用 persistence
  （coordinator 处于 disabled 态，记录 reasonCode），engine 正常继续（§二十七/
  二十八/二十九）。
- wiring：`LingxiEngine` 新增可选构造参数 `modelObservability`（policy 形状，
  默认 absent = disabled，生产行为与 Phase 6 完全一致）；server `startServer`
  透传该 option（Phase 8 再接 UI/settings；不新增隐藏环境变量，§八十一）。

## Q10 — scanner / fingerprint 登记

新增 DB + blob tree 必须完成（§三十/一百二十五/一百二十六）：

1. `shared/persistence/store-registry.ts` 两个新 descriptor：
   `model-observability-db`（sqlite，schemaSource=sqlite-runtime，pathPatterns
   含 `-wal`/`-shm`）与 `model-observability-blobs`（tree）。`siteRules` 按
   sourceFile 覆盖新模块的全部 `database-open`/`mkdir`/`write-file`/`rename`/
   `remove-path` 站点（scanner 只按 sourceFile+kinds+excerpt 正则归类，
   pathPatterns 不参与源码扫描）。
2. `scripts/generate-persistence-schema-fingerprint.mjs`
   `introspectSqliteStore`：为 `model-observability-db` 添加 runtime
   introspector（temp 目录开真实 store 读 sqlite_master + user_version）。
3. `export-manifest.json` paths 收录新 lib/llm 模块（open 边界自洽）。
4. 引擎 wiring 改动 core/engine.ts（siteMappings 哈希源）→ fingerprint repin
   `--classification compatible`（无既有 store 形状变化，纯新增 store）。
5. `node scripts/scan-persistent-stores.mjs` 全绿。

## 附：进程拓扑与写作权总结

```text
Desktop (Electron) ──spawn──► Server (唯一 engine/模型调用者, 唯一 observability writer)
CLI (HTTP client, 无 engine, 非写者；可 spawn server)
同宅互斥闸防长期并发；WAL+busy_timeout 兜短暂重叠
```

## 附：SQLite pragma 决策与理由

| pragma | 值 | 理由 |
|---|---|---|
| journal_mode | WAL | 多进程短暂并发安全；读写不互斥 |
| synchronous | NORMAL | WAL 下公认安全点；观测数据非业务权威 |
| busy_timeout | 5000 | 短暂多进程窗口内排队而非立刻失败 |
| secure_delete | ON | 敏感正文删除时 freelist 页清零（§八十九）；删除只发生在 maintenance 路径，不进模型热路径；代价记录在案 |
| auto_vacuum | INCREMENTAL | retention 删除后 `incremental_vacuum` 收缩文件（§八十八）；建表前设置 |
| foreign_keys | OFF | 显式容忍 out-of-order persistence/partial crash（§二十四）；关联完整性由 call-shell upsert + 读侧解释承担 |
| trusted_schema | OFF | 不使用 schema 中的自定义函数，收紧默认面 |

## 附：At-Rest Security 明确结论（§一百三十七）

**Is observability content cryptographically encrypted at rest? NO.**
- filesystem protection：`model-observability/` 目录 Unix 0700，DB/WAL/SHM/blob
  文件 0600，Windows 依赖 profile 目录继承 ACL（不假装 POSIX 等价）。
- opt-in：默认 persistence disabled；payload 持久化需要显式 policy。
- retention：payload 可比 trace 更早过期（独立 policy 维度）。
- remaining threat：同用户进程可直接读取文件；备份/同步渠道会复制明文；
  SSD wear-leveling 使「删除后物理不可恢复」不可承诺（§九十）。
