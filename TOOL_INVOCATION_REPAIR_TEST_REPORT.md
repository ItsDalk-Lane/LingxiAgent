# 契约执行路径不变量测试报告

## 证据口径

所有带 `tee` 的命令均使用 `set -o pipefail` 保留真实退出码。`PASS` 只表示对应命令在当时工作树上 exit `0`；失败、环境限制和未执行项不会写成通过。原始输出保存在 `/tmp/lingxi-tool-contract-*.log`，逐项命令、统计、日志和提交坐标保存在 `TOOL_INVOCATION_REPAIR_PROGRESS.md`。

## 固定基线

| 门禁 | 原始结果 | 日志 |
| --- | --- | --- |
| typecheck | exit `0` | `/tmp/lingxi-tool-contract-p001-typecheck.log` |
| lint | exit `0`，`0 errors / 9188 warnings` | `/tmp/lingxi-tool-contract-p001-lint.log` |
| 11 文件定向测试 | exit `0`，`11 files / 251 tests passed` | `/tmp/lingxi-tool-contract-p001-targeted.log` |
| 全量测试 | exit `1`，`1331 passed / 2 failed / 1 skipped` files，`13432 passed / 2 failed / 7 skipped` tests | `/tmp/lingxi-tool-contract-p001-full.log` |
| 服务构建 | 首次 exit `1`（无签名变量）；临时抛弃式签名诊断 exit `0` | `/tmp/lingxi-tool-contract-p001-build-server.log`、`/tmp/lingxi-tool-contract-p001-build-server-diagnostic.log` |

基线两项全量失败来自旧审计封印和固定基线落后于远端历史版本；没有把它们记为本任务通过。

## 分阶段回归

| 阶段 | 最终原始统计 | 代表日志 |
| --- | --- | --- |
| P1 规范化内核 | `8 files / 202 tests passed`；三段 typecheck exit `0` | `/tmp/lingxi-tool-contract-p1-stage.log` |
| P2 Registry 与 Gateway | `4 files / 101 tests passed`；三段 typecheck exit `0` | `/tmp/lingxi-tool-contract-p2-stage-final.log` |
| P3 插件装配 | `11 files / 267 tests passed`；三段 typecheck exit `0` | `/tmp/lingxi-tool-contract-p302-stage-final.log` |
| P4 Catalog、Bridge、MCP | `6 files / 155 tests passed`，扩展 `4 files / 180 tests passed` | `/tmp/lingxi-tool-contract-p403-gate-final4.log` |
| P5 生命周期 | 阶段矩阵 `312/312` 后完成扩展门禁；三段 typecheck exit `0` | `/tmp/lingxi-tool-contract-p503-gate-attempt3.log` |
| P6 plugin-dev | P6-02 扩展 `10 files / 314 tests passed`；三段 typecheck exit `0` | `/tmp/lingxi-tool-contract-p602-gate-final.log` |
| P7 媒体 | 指定 `15/15`，全媒体 `57 files / 482 tests passed`；三段 typecheck exit `0` | `/tmp/lingxi-tool-contract-p702-media-all-final.log` |
| P8 知识重排 | 指定 `5 files / 94 tests passed`，扩展 `10 files / 52 tests passed` | `/tmp/lingxi-tool-contract-p801-gate-final2.log` |
| P9 错误与边界 | 错误扩展 `19 files / 283 tests passed`；AST 扫描 `2121` 文件、`0` 违规 | `/tmp/lingxi-tool-contract-p901-gate-final.log`、`/tmp/lingxi-tool-contract-p902-boundary-final.log` |
| P10 路径与配置 | 路径扩展 `11 files / 191 tests passed`；完整配置 `13 files / 373 tests passed` | `/tmp/lingxi-tool-contract-p1001-gate-final.log`、`/tmp/lingxi-tool-contract-p1002-gate.log` |
| P11 文档事实 | 文档/边界测试随本项执行；结果见 P11 日志 | `/tmp/lingxi-tool-contract-p1101-gate.log`、`/tmp/lingxi-tool-contract-p1102-gate.log` |

## 红灯证据

P1–P9、P10-01、P11 均先取得与缺陷或缺失产物一致的失败，再实现并复跑。P10-02 的新增配置矩阵首次即为 `12/12`，因为前序生命周期、错误映射和 P10-01 路径修复已经覆盖这些组合；该项标记为 `completed_with_red_not_reproduced`，没有人为破坏实现或测试来制造失败。

## P12 最终验证

P12-01 至 P12-05 在本源码候选提交之后执行。为遵守“源码候选后不得修改非审计 allowlist 文件”，最终定向测试、静态检查、全量测试、构建、边界扫描和封印统计只回填到 `PROGRESS.md`，不回写本文件。最终执行报告同时提供 source candidate SHA、seal SHA 和原始统计。

P12 计划日志：

- `/tmp/lingxi-tool-contract-p1201-boundary.log`
- `/tmp/lingxi-tool-contract-p1201-targeted.log`
- `/tmp/lingxi-tool-contract-p1202-typecheck.log`
- `/tmp/lingxi-tool-contract-p1202-lint.log`
- `/tmp/lingxi-tool-contract-p1202-open-boundary.log`
- `/tmp/lingxi-tool-contract-p1202-full-tests.log`
- `/tmp/lingxi-tool-contract-p1203-build-server.log`
- `/tmp/lingxi-tool-contract-p1203-build-server-open.log`
- `/tmp/lingxi-tool-contract-p1203-build-client.log`
- `/tmp/lingxi-tool-contract-p1203-seed-kit.log`
- `/tmp/lingxi-tool-contract-p1205-matrix.log`
- `/tmp/lingxi-tool-contract-p1205-post-diff.log`
- `/tmp/lingxi-tool-contract-p1205-seal-tests.log`
