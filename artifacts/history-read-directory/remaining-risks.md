# 剩余风险与已知边界（F06）

更新：2026-09-11。仅列**当前仍存在**的边界/风险；曾出现已消除项见 PROGRESS.md 各批次「遇到的问题」。

## 一、平台/环境未验证（留待指定环境）

| 项 | 状态 | 说明 |
|---|---|---|
| 真实浏览器跨源（Y14） | 已实现但指定环境未验证 | CORS/预检/Expose 头经 Node HTTP loopback 真实链路验证（browser-smoke-report.md）；真实浏览器/Electron 网络栈（凭据自动附带、预检缓存、渲染进程头可见性、HTTP cache 交互）未验证，留待 F 后续指定环境 |
| Electron 打包后可启动性 | 未在本任务验证 | 未做 electron-builder 打包冒烟 |

## 二、已知有界边界（设计内，如实记录）

| 项 | 影响 | 缓解/理由 |
|---|---|---|
| retired 旧版本释放依赖 LIMIT=2/invalidate | 追加中会话瞬时驻留最高 3×目录估算（10k ≈ 50MB） | 有界且计入 residentBytes 预算（I11）；`cache.release()`（在途归还）全仓无调用方，属 B04 既有行为，未在本任务改动 |
| 外部进程 in-place 改写未读前缀 | 与纯追加不可区分（无权威通知） | C02 信任边界：承诺单进程写入模型+已插桩重写路径+dev/ino 捕获；双内核冷启动秒级竞态窗口已在 writer-invalidation-map 记录 |
| 冷首页 10k 延迟运行方差 | 73.0–93.2ms 波动（阈值 123.84 内） | 一次性构建成本，按会话版本摊销；OS page cache 未清空口径 |
| 服务器重启换盐 | 旧 ETag 全部失配 → 200 回退 | 设计内（盐不落盘）；不要求跨重启命中 |

## 三、既有失败（非本任务引入，保持原状）

全量 vitest 7 项失败与 A01 纯净树基线逐条一致（known-failures-comparison.json）：ObservabilityDateLine×2（图表几何）、packaged-desktop-cleanup（90s 超时）、audit-seal（封印坐标，独立授权流程）、round2×2 + round3×1（交付证据 manifest 复算，工作区含任务改动即红）。

## 三A、教训：可写夹具被污染（F-e 事件，已修复）

- **事件**：F-e 补丁验证曾把 A01 快照 clone `/tmp/lingxi-baseline-1d42b740` 当作 patch apply 目标，clone 工作区被写成任务内容（git status 与主工作区相同），且未跟踪的 `tests/compat-old-server-runner.mjs` 丢失——兼容测试报 MODULE_NOT_FOUND，且即便补回 runner，跑的也是新代码，旧+旧/旧+新组合即告无效。
- **根因**：夹具目录可写且无防护，补丁验证与夹具共用同一树；runner 以「clone 内临时文件」存在，不属于任何 tracked 来源，丢失后无法从仓库恢复。
- **防护（现已落地）**：
  1. runner 模板 tracked 于主仓库 `tests/compat-old-server-runner.template.mjs`，两个兼容测试在 beforeAll **幂等覆盖写入** clone（`fs.copyFileSync`），clone 内副本随时可重建、不作状态载体；
  2. runner 内 Hono 改裸导入（ESM 自 clone/tests/ 向上解析，保证用 clone 自身依赖；原硬编码 `hono/dist/hono.mjs` 路径在依赖重装后已失效）；
  3. clone 缺席时套件 skip 且 **console 明示**「A01 clone 缺席……四组合中旧服务端侧指定环境未验证」（describe.skipIf + 模块级 warn，不静默 skip）；
  4. 旧代码真实性由断言固定：旧+旧/旧+新用例断言响应无 `lingxi-history-protocol` 头、无 ETag（tests/history-protocol-compat.test.ts），新代码混入即红；
  5. patch apply 验证改用独立副本树（/tmp/a01-tree 派生），**不再触碰 A01 夹具本体**。
- **恢复记录**：clone 已按 initial-status.txt（A01=porcelain=0）恢复：`git reset --hard 1d42b740` + 清理 18 项任务引入未跟踪路径（`git clean -nd` 先核单后执行）；round2 patch 本地修改非 A01 既有状态（系后续全量测试在位再生成），按权威记录恢复为 HEAD 一致。

## 四、不掩盖声明

- 概览/条件校验不判定对账 complete；all/reconciliation/content/find 的既有语义与 limit 策略未改。
- 「逻辑轮次」不表述为「已完成任务数」；无可靠已加载计数时只显示总量，不扫描归并气泡。
- 未实现任何未授权产品功能；无新持久化文件/表；无 SDK 核心改动；无发布操作。
