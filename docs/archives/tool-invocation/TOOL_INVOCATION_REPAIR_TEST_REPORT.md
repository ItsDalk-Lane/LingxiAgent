# 契约执行路径不变量测试报告

> 归档于 2026-09-06：以下源码路径、命令和状态按原任务的仓库根目录与日期理解；档案目录不是命令运行目录。参见[档案索引](../README.md)。

## 证据口径

所有带 `tee` 的命令均使用 `set -o pipefail` 保留真实退出码。`PASS` 只表示对应命令在当时工作树上 exit `0`；失败、环境限制和未执行项不会写成通过。原始输出保存在 `/tmp/lingxi-tool-contract-*.log`，逐项命令、统计、日志和提交坐标保存在 `TOOL_INVOCATION_REPAIR_PROGRESS.md`。

## v0.1.34 固定基线

| 门禁 | 原始结果 | 日志 |
| --- | --- | --- |
| typecheck | exit `0` | `/tmp/lingxi-tool-contract-v0134-p001-typecheck.log` |
| lint | exit `0`，`0 errors / 9194 warnings` | `/tmp/lingxi-tool-contract-v0134-p001-lint.log` |
| 11 文件定向测试 | exit `0`，`11 files / 253 tests passed` | `/tmp/lingxi-tool-contract-v0134-p001-targeted.log` |
| 全量测试 | exit `1`，`1359 passed / 1 failed / 1 skipped` files，`13763 passed / 1 failed / 7 skipped` tests | `/tmp/lingxi-tool-contract-v0134-p001-full-test.log` |
| 服务构建 | 首次 exit `1`（缺少签名变量）；匹配公钥的抛弃式签名诊断 exit `0` | `/tmp/lingxi-tool-contract-v0134-p001-build-server.log`、`/tmp/lingxi-tool-contract-v0134-p001-build-server-diagnostic.log` |

基线全量测试唯一失败来自任务记录位于旧审计封印之后；没有放宽封印门禁，也没有把该失败记为通过。最终状态必须以 P12 在冻结源码候选上的新结果为准。

## 分阶段回归

| 阶段 | 最终原始统计 | 代表日志 |
| --- | --- | --- |
| P1 规范化内核 | 身份/错误 `2 files / 6 tests`，权限 `5 / 187`，参数校验 `1 / 9`；类型检查均 exit `0` | `/tmp/lingxi-tool-contract-v0134-p101-green.log`、`/tmp/lingxi-tool-contract-v0134-p102-green.log`、`/tmp/lingxi-tool-contract-v0134-p103-green.log` |
| P2 目标表与统一网关 | 目标表 `1 / 6`，网关扩展 `5 / 197`；类型检查均 exit `0` | `/tmp/lingxi-tool-contract-v0134-p201-green.log`、`/tmp/lingxi-tool-contract-v0134-p202-green.log` |
| P3 插件装配 | 元数据/可用性 `4 / 181`，引擎装配 `5 / 115`；类型检查均 exit `0` | `/tmp/lingxi-tool-contract-v0134-p301-green.log`、`/tmp/lingxi-tool-contract-v0134-p302-green.log` |
| P4 目录、桥接与连接器 | 目录 `6 / 97`，桥接 `6 / 146`，阶段 `6 / 155`，扩展 `4 / 180` | `/tmp/lingxi-tool-contract-v0134-p401-green.log`、`/tmp/lingxi-tool-contract-v0134-p402-stage-final.log`、`/tmp/lingxi-tool-contract-v0134-p403-gate-final.log` |
| P5 生命周期 | 插件 `6 / 151`，连接器 `7 / 278`，旧会话撤销 `9 / 312`；类型检查均 exit `0` | `/tmp/lingxi-tool-contract-v0134-p501-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p502-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p503-gate-final.log` |
| P6 插件开发入口 | 聊天 `6 / 107`，HTTP 开发主体 `10 / 289`；类型检查均 exit `0` | `/tmp/lingxi-tool-contract-v0134-p601-stage-final.log`、`/tmp/lingxi-tool-contract-v0134-p602-gate-final.log` |
| P7 媒体 | 指定 `2 files / 15 tests`，含 Hub `3 / 23`，全媒体 `57 / 482` | `/tmp/lingxi-tool-contract-v0134-p702-media-all-final.log` |
| P8 知识重排 | 任务书门禁 `5 / 96`，相关知识回归 `12 / 48` | `/tmp/lingxi-tool-contract-v0134-p801-gate-attempt1.log`、`/tmp/lingxi-tool-contract-v0134-p801-related-attempt1.log` |
| P9 错误与边界 | 错误扩展 `19 / 349`；语法树扫描 `2129` 个生产源码文件、`0` 违规 | `/tmp/lingxi-tool-contract-v0134-p901-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p902-boundary-final.log` |
| P10 路径与配置 | 路径扩展 `11 / 202`；完整配置 `13 / 373` | `/tmp/lingxi-tool-contract-v0134-p1001-gate-final.log`、`/tmp/lingxi-tool-contract-v0134-p1002-gate.log` |
| P11 文档事实 | 架构说明 `1 / 4`；报告事实门禁在本项执行 | `/tmp/lingxi-tool-contract-v0134-p1101-gate.log`、`/tmp/lingxi-tool-contract-v0134-p1102-gate.log` |

## 红灯证据

P1–P9、P10-01 和 P11 都先取得与缺陷或缺失产物一致的失败，再实现并复跑。P8 第一次实现复跑只剩一项旧分支测试仍要求 v0.1.34 已删除的知识路线，校正时只更新测试去核对正式路线，没有恢复退役生产代码。P10-02 的新增配置矩阵首次即为 `12/12`，因为前序生命周期、错误映射和 P10-01 路径修复已覆盖这些组合；该项标记为 `completed_with_red_not_reproduced`，没有人为破坏实现或测试来制造失败。

## P12 最终验证

P12-01 至 P12-05 在本源码候选提交之后执行。为遵守“源码候选后不得修改非审计 allowlist 文件”，最终定向测试、静态检查、全量测试、构建、边界扫描和封印统计只回填到 `PROGRESS.md`，不回写本文件。最终执行报告同时提供源码候选 SHA、封印 SHA 和原始统计。

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
