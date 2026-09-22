# STARTUP_CRITICAL_PATH — P07-T02 启动关键路径分析与决策

日期：2026-09-22｜结论：**UNCHANGED_VERIFIED + 测试补强（A03 行为测试新增），零生产代码改动**。

## 1. 关键路径事实（全部实测）

### 1.1 阶段计时（隔离 HOME 冷启动，源码形态 `server/main-full.ts`）

证据：`logs/P07-T01-startup-phases-prof.out` / `-r2` / `-r3`（带时间戳逐行，三次独立运行）；`samples/startup-stdout.log`（隔离重跑）。总量与 W1 十次批量（median 1301ms）互证。

| 阶段 | 累计时刻 | 增量 | 内容 |
|---|---|---|---|
| spawn → ① ensureFirstRun 开始 | +1131ms | **1131ms** | Node 启动、**模块加载/编译（ESM type-strip + CJS）**、传输 bind、外置 server 探测、数据 epoch 协调、网络配置 |
| ① ensureFirstRun | +1139ms | 8ms | 首次运行播种 |
| ① 身份注册 | +1141ms | 1ms | 本地身份 registry |
| ② engine 构造 | +1163ms | 22ms | LingxiEngine 实例化 |
| ② engine.init（5 阶段） | +1228ms | **65ms** | Pi SDK 23ms→agents 14ms→ResourceLoader 11ms→模型发现 7ms→收尾 ~10ms |
| init 后 → ready（server-info.json 写入） | +1266ms | 38ms | 扩展注册、插件、hub、路由挂载 |

### 1.2 成本构成（V8 --cpu-prof）

见 HOTSPOT_REPORT §4：模块加载/编译与文件 I/O 占主导；**应用业务初始化（core+lib+shared self time）< 3%**。启动慢的不是业务逻辑，是源码形态模块编译与 I/O。

### 1.3 产物（bundle）形态

生产实际启动路径是打包产物（`dist-server/<plat>/bootstrap.js` → `bundle/index.js`）。本阶段重建 bundle（`logs/P07-T02-build-server.out`：esbuild 打包与运行时冒烟完成；**安装器 seed 签名步骤因无 LINGXI_SIGN_KEY 以 exit 1 拒绝——按设计拒绝未签名 seed，属发布凭据缺失，不影响已产出的 bundle/index.js 用于启动计时**）后实测：

| 形态 | median（×10） | p95 | ready |
|---|---|---|---|
| 源码（`server/main-full.ts`，W1 口径） | 1301ms | 1302ms | 10/10 |
| bundle（`bootstrap.js`） | **1072ms** | 1174ms | 10/10 |

bundle 形态消除约 229ms（-18%）模块编译成本。样本：`samples/startup-bundle.json`。

## 2. 「启动必需 / 首次使用必需 / 可后台延后」分类（沿现有组合入口逐项核对）

入口链：`desktop/bootstrap.cjs` / `scripts/launch.js` → spawn `server/bootstrap.ts`（bundle 形态含独立 keepalive worker，防 native 加载阻塞误判 #719/#736）→ `server/main-full.ts`（`startServer`）。

| 类别 | 模块/步骤 | 依据 | 状态 |
|---|---|---|---|
| **启动必需（不可延后）** | 数据 epoch 协调、外置 server 探测/接管保护、ensureFirstRun、身份 registry、engine.init 全链（迁移/凭证权限自愈/人格改名/置顶并入/Pi SDK/agents/频道游标修复/ResourceLoader） | 安全、数据恢复与身份准备（任务书 T02.1 明令不得随意延后） | 保持顺序前置（UNCHANGED_VERIFIED） |
| **HTTP 可用但未就绪保护** | 传输 bind 在 engine.init **之前**完成，期间 `activeFetch` 返回 503 `server_starting` | `server/index.ts:406`（bind）vs `:445`（init）、`activeFetch` 门 | **就绪不虚报**：server-info.json 只在 engine.init+路由挂载完成后写入（`server/index.ts:1277-1306`），桌面端轮询该文件，不会把 503 窗口当可用 |
| **可后台延后（已实现）** | Bridge 平台 adapter | `setImmediate(startBridgeManager)` 在 server-info.json 写入之后（`server/index.ts:1312`）；**结构测试锁定**：tests/server-startup-diagnostics-contract.test.ts:491（断言 ready 写入 < setImmediate < startBridge 调用） | UNCHANGED_VERIFIED |
| 可后台延后（已实现） | 环境依赖自检 | setImmediate + 失败静默降级，永不清空阻塞启动（`server/index.ts:1335`） | UNCHANGED_VERIFIED |
| 可后台延后（已实现） | checkpoint 清理、冷会话文件清理 | fire-and-forget `engine.cleanupCheckpoints().catch(...)` 等 | UNCHANGED_VERIFIED |
| 惰性初始化（首次使用） | BridgeManager 动态 `import()` + **并发去重 single-flight** | `server/index.ts:860-882`：`bridgeManagerInitPromise` 共享、失败捕获进 `bridgeManagerInitError`、`getState()` 暴露 ready/initializing/error 三态 | UNCHANGED_VERIFIED + **本阶段新增行为测试**（见 §3） |
| 桌面端可选跳过 | 启动期会话创建 | `LINGXI_CREATE_STARTUP_SESSION=0`（结构测试同文件:475 锁定） | UNCHANGED_VERIFIED |

## 3. 本阶段补强（唯一新增：A03 场景行为测试）

`tests/p07-startup-lazy-init.test.ts`（3 例，全绿）：从 `server/index.ts` 提取生产闭包 `startBridgeManager`（逐字源码 + vm 执行；提取失败即测试失败，防静默漂移），行为验证：

1. **并发首次调用 ×2 → 动态加载恰好一次**，两调用拿到同一实例（single-flight 生效）；
2. 初始化失败 → 收口 null 不抛出、`getState().error` 可观察（**不虚报就绪**）、promise 清空后可重试；
3. 成功后 5 次后续调用直接返回缓存实例，加载总数仍为 1（**首次使用只初始化一次**）。

命令：`P07-T02-lazy-init`（vitest run，exit 0）。

## 4. 冷热启动分测与「计时终点」

- 冷启动：W1 每次全新隔离 HOME（确定性冷启动；OS page cache 不清空，协议如实标注为暖物理缓存）。
- 热启动：同输入重跑批（P00 B1 vs B2 已建立 COMPARABLE 波动区 -3.2%；本阶段复测 B1 同批口径）。
- **计时终点 = server-info.json 出现**：该文件在 engine.init 与全部路由挂载完成后写入（§2 第 2 行），即「可接受任务且必要策略已就绪」——不是 window.show，也不是传输 bind。A02 场景由此满足：可选能力（Bridge）未就绪时 `bridgeManagerRef.getState()` 明确暴露三态，启动不等待它。

## 5. 决策与不做的事

- **不改生产代码**：剩余启动成本中 ~83%（源码形态）/~76%（bundle 形态）为模块加载+I/O+native 装载，业务初始化仅 65ms；没有任何「实测耗时且非启动必需」的模块剩余——可延后项均已延后且有测试锁定。按任务书 T02 与 T01.4，交付验证结论，不制造重构需求。
- 不引入启动并行化改造（迁移→身份→模型的顺序是安全顺序）；不把「先显示窗口」当启动变快（桌面端就绪以 server-info.json 为准，已有轮询与进度保持逻辑）。
- 开发形态 compile cache 建议见 HOTSPOT_REPORT §7.3（不纳入本阶段）。

## 6. 回退

本任务零生产代码改动；新增测试与工具独立可回退（删除 `tests/p07-startup-lazy-init.test.ts` 即可，不影响任何生产路径）。
