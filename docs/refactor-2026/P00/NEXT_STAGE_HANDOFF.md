# NEXT_STAGE_HANDOFF — P01 输入（P00 → P01）

日期：2026-09-21｜P00 验收基线：见 P00_RESULT.json。

## 1. 已验收坐标与环境

- START_SHA = END_SHA = `92c6646c581883436e026c53898e8bfc34ce4d27`（P00 零生产改动、零 commit；全部产出为未跟踪文件）
- 分支：`docs/knowledge-closeout-2026-09-21`（未切换；P01 开始前由用户决定是否建 refactor 专用分支）
- lockfile sha256 `a9735825…9d3c`；Node v24.16.0；darwin 27.0 arm64
- 任务书研究 SHA `8037fae7a` = HEAD~1，docs-only 差异（BASELINE.json drift 节）——**P01 任务书中的行号引用需按当前 HEAD 复核，未核对项标待复核**

## 2. P01 唯一允许修改范围（依 P00 事实收敛）

1. `cli/local-server.ts:5-15` → 统一 LINGXI_HOME 解析到 `shared/hana-runtime-paths`（P01-2）。
2. `vitest.config.js` / `tsconfig.test.json` 死别名清退（P01-3，先核实零引用）。
3. strict 核心入口建立（P01-4，按 P01 任务书细化；不动 tsconfig.node 全局宽松区除非其任务书要求）。
4. 证据目录 lint 策略（P01-5，工程配置决策）。
5. **不得**新增重复 Agent 循环/工具网关/权限网关/凭证存储/消息语义解析器；Pi 适配仅回归（P01-1 UNCHANGED_VERIFIED）。

## 3. 必读输入（P01 开工前）

| 文件 | 用途 |
|---|---|
| docs/refactor-2026/P00/OWNERSHIP_MAP.md | 18 类事实权威+3 个🔴缺口（P01 拥有 LINGXI_HOME 统一） |
| docs/refactor-2026/P00/CALLSITE_MATRIX.json | 10 个入口/执行点/旁路登记 |
| docs/refactor-2026/P00/ENTRYPOINT_MATRIX.md §2 | 组合根与装配顺序（P01 边界工作的锚点） |
| docs/refactor-2026/P00/BASELINE_FAILURES.md | F1（治理，P01 勿动）/F2（P03 拥有，P01 勿抢修）/F3（环境） |
| artifacts/refactor-2026/P00/logs/agent-*.md | 四份原始证据（功能/测试/所有权/调用链，含全部文件:行） |

## 4. 已执行测试路径（P01 回归时的参照系）

- 基线全量：`npm test`（1446 文件/14705 用例；**预期红=4 个 F1 用例**，修复归 F1 治理流程，P01 不得为绿而动 seal）
- `npm run typecheck`（绿）、`npm run lint`（**预期红=3 个 F2 error**，修复归 P03）
- `npm run build:renderer`（绿）、`npm run check:tool-invocation-boundaries`（绿）
- 边界相关定向：tests/tool-invocation-gateway、tests/pi-sdk-import-boundary、tests/server-composition-boundary

## 5. 必保留兼容/观察分组（P01 不可改变）

- trace 口径：user_turn 会话粒度复用 mt_、后台强制新根、traceId≠sessionId（P00-A07 已核实）
- 四常驻工具+按需目录（三重启动断言保护中）
- 观测 schema v7 / DATA_EPOCH 机制 / store-registry 指纹联动

## 6. 未验证环境

- 四平台 CI 真实运行记录（gh 认证+代理故障，F3）
- Windows/Linux 实机行为（本地 darwin only）
- 真实供应商调用（无授权，零消耗）

## 7. 当前数据版本

- DATA_EPOCH stamp 机制在、迁移表为空（无已行使 epoch 边缘）；session-manifest.db user_version=5；observability schema v7；knowledge v19（隔离目录实测 data-epoch.json 正常生成）

## 8. P00 产出物索引

docs/refactor-2026/P00/{BASELINE.json, WORKTREE_SAFETY.md, FEATURE_MATRIX.md, SCOPE.json, ENTRYPOINT_MATRIX.md, OWNERSHIP_MAP.md, CALLSITE_MATRIX.json, ACCEPTANCE_MAP.json, BASELINE_TESTS.json, BASELINE_FAILURES.md, BENCHMARK_PROTOCOL.md, PROMPT_BASELINE.json, REFACTOR_BACKLOG.md, STAGE_DEPENDENCIES.json, NEXT_STAGE_HANDOFF.md, P00_RESULT.json, P00_REPORT.md}
artifacts/refactor-2026/P00/{logs/command-log.jsonl + 全部原始日志, tools/*, fixtures/*, samples/*, isolated/isolation-proof.json}
