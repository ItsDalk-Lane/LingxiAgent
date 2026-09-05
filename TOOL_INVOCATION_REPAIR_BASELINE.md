# 工具契约执行路径不变量修复基线

## Git 与环境坐标

- 仓库：`ItsDalk-Lane/LingxiAgent`
- 固定来源分支：`feat/knowledge-retrieval-research-p0-p3`
- 固定来源提交：`4fefe66ec3b4f6b23c78a09869a607886585740e`
- 执行分支：`fix/tool-contract-path-invariance`
- Node：`v24.16.0`
- npm：`11.13.0`
- 操作系统：`Darwin 25.6.0 arm64`
- 开始时间：`2026-09-05 13:31:56 +0800`
- 切分支前工作树：干净
- 依赖安装：`npm ci`，exit `0`；新增 `1286` 个包，审计 `1291` 个包
- 依赖审计摘要：`13 vulnerabilities (1 low, 11 moderate, 1 high)`；未运行自动修复

## P0-00 偏差记录

- `git fetch origin --prune`：exit `0`。
- 固定来源远端引用核对：exit `1`，`origin/feat/knowledge-retrieval-research-p0-p3` 已不存在。
- 按任务书既定规则继续从固定提交创建分支；固定提交经 `git cat-file -e` 验证可达。
- 目标本地分支和远端分支在创建前均不存在，没有改写已有历史。

## P0-01 基线门禁

| 门禁 | 日志 | 原始结果 | 状态 |
| --- | --- | --- | --- |
| `npm run typecheck` | `/tmp/lingxi-tool-contract-p001-typecheck.log` | exit `0`；三段 TypeScript 检查完成 | PASS |
| `npm run lint` | `/tmp/lingxi-tool-contract-p001-lint.log` | exit `0`；`0 errors`，`9188 warnings`，其中 `24` 条可自动修复 | PASS_WITH_WARNINGS |
| 11 文件定向 Vitest | `/tmp/lingxi-tool-contract-p001-targeted.log` | exit `0`；`11 passed` files；`251 passed` tests；无失败、无跳过 | PASS |
| `npm test` | `/tmp/lingxi-tool-contract-p001-full.log` | exit `1`；`1331 passed / 2 failed / 1 skipped` files；`13432 passed / 2 failed / 7 skipped` tests | FAIL_BASELINE |
| `npm run build:server` | `/tmp/lingxi-tool-contract-p001-build-server.log` | exit `1`；签名打包前明确拒绝：`LINGXI_SIGN_KEY is not set` | FAIL_ENVIRONMENT |
| `git diff --check` | 无单独日志 | exit `0` | PASS |

### 基线失败归因

1. `post-verification-audit-seal`：检测到本任务要求新增的 `TOOL_INVOCATION_REPAIR_BASELINE.md` 和 `TOOL_INVOCATION_REPAIR_PROGRESS.md` 位于旧封印 SHA 之后。该门禁按设计 fail-closed，未修改测试或 allowlist；最终在 P12 重新封印。
2. `release-preflight`：固定基线的候选版本为 `0.1.33`，本次 `git fetch --prune` 后读取到历史最大版本 `0.1.34`，因此发布预检按设计失败。未改版本、未改发布规则。
3. `build:server`：环境没有 `LINGXI_SIGN_KEY`。使用仓库自带密钥生成器，在 `/tmp/lingxi-tool-contract-p001-signing` 创建抛弃式密钥与匹配 keyset 后，诊断复跑 exit `0`，日志为 `/tmp/lingxi-tool-contract-p001-build-server-diagnostic.log`；随后精确删除临时目录内四个文件并移除空目录。首次失败仍保留为原始基线结果。

P0-01 的职责是建立真实基线而非修复这些非工具契约失败；三处红灯均保留为失败状态，没有改写成“可忽略”或 PASS。

## P0-02 现状调用矩阵与原始入口

待执行。
