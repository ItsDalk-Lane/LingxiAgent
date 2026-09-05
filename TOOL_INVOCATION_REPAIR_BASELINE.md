# 工具契约执行路径不变量修复校正版基线

## P0-00 固定坐标

- 仓库：`ItsDalk-Lane/LingxiAgent`
- 正式来源：`main` / `v0.1.34`
- 固定源码提交：`60d910b84572c525a7c9c49216fb9206623bf7a4`
- 签名标签对象：`8c2e80e7e00b993a260a3e9273a85be1678c3b94`
- 校正版执行分支：`fix/tool-contract-path-invariance-v0134`
- 原证据分支：`fix/tool-contract-path-invariance`，HEAD `c723410c8ebcd95f6330f7a4a85c325698d3960b`
- Node：`v24.16.0`
- npm：`11.13.0`
- 操作系统：`Darwin 25.6.0 arm64`
- 开始时间：`2026-09-05T19:27:47+0800`

## 基线校正原因

原任务书固定的 `4fefe66ec3b4f6b23c78a09869a607886585740e` 是 v0.1.34 发布链的早期阶段提交。
正式 v0.1.34 在原任务开始执行前已经合入 main；从旧提交执行导致版本、知识架构和审计坐标
同时偏移。用户已明确授权以正式发布提交重新建立基线，但要求迁移既有成果，不推倒重做。

## 环境准备原始结果

- 远端同步：exit `0`；使用等价的显式 SSH 地址更新 `origin/*`，避免用户级 URL 重写造成 HTTPS 超时。
- `npm ci`：exit `0`；安装 `1286` 个包，审计报告 `13` 项依赖问题（`1 low / 11 moderate / 1 high`），未执行越权自动升级。
- 分支创建前原证据分支无已跟踪或未跟踪改动；规划技能随后创建三个本任务专用未跟踪规划文件并随校正版分支带入。
- 日志：`/tmp/lingxi-tool-contract-v0134-p000-fetch.log`、`/tmp/lingxi-tool-contract-v0134-p000-npm-ci.log`。

## P0-01 校正版基线门禁

| 门禁 | 原始日志 | 原始结果 | 判定 |
|---|---|---|---|
| `npm run typecheck` | `/tmp/lingxi-tool-contract-v0134-p001-typecheck.log` | exit `0`；三段 TypeScript 检查完成 | `PASS` |
| `npm run lint` | `/tmp/lingxi-tool-contract-v0134-p001-lint.log` | exit `0`；`0 errors / 9194 warnings`，其中 `25` 条可自动修复 | `PASS_WITH_WARNINGS` |
| 11 文件定向 Vitest | `/tmp/lingxi-tool-contract-v0134-p001-targeted.log` | exit `0`；`11 passed` files；`253 passed` tests；无失败、无跳过 | `PASS` |
| `npm test` | `/tmp/lingxi-tool-contract-v0134-p001-full-test.log` | exit `1`；`1359 passed / 1 failed / 1 skipped` files；`13763 passed / 1 failed / 7 skipped` tests | `FAIL_SEQUENCE_SEAL` |
| `npm run build:server` | `/tmp/lingxi-tool-contract-v0134-p001-build-server.log` | exit `1`；签名打包前明确拒绝：`LINGXI_SIGN_KEY is not set` | `FAIL_ENVIRONMENT` |
| 抛弃式密钥诊断构建 | `/tmp/lingxi-tool-contract-v0134-p001-build-server-diagnostic.log` | exit `0`；服务端、渲染器归档和签名清单均生成 | `PASS_DIAGNOSTIC` |
| `git diff --check` | 无单独日志 | exit `0` | `PASS` |

### 基线失败归因

1. `post-verification-audit-seal` 的唯一失败文件正是 P0-00 新增的五份任务记录；发布提交自身从旧已验证源码到 `60d910b8` 只改六份既有审计 allowlist 文件。未修改或放宽封印测试，留待 P12 重新固定已验证源码坐标。
2. 原始服务端构建在缺少真实签名密钥时按设计拒绝。随后用仓库自带生成器在 `/tmp/lingxi-tool-contract-v0134-p001-signing` 创建抛弃式密钥与匹配公开 keyset，诊断复跑 exit `0`；临时私钥和 keyset 已逐文件删除，空目录也已移除。
3. P0-01 只建立迁移前事实，不把上述两项原始失败改写为通过，也不在本项改审计门禁或生产代码。
