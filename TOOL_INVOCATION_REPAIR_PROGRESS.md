# 工具契约执行路径不变量修复校正版进度

## 固定事实

- 校正基线：`60d910b84572c525a7c9c49216fb9206623bf7a4`（`v0.1.34^{commit}`）
- 校正版分支：`fix/tool-contract-path-invariance-v0134`
- 原实现来源：`fix/tool-contract-path-invariance` @ `c723410c8ebcd95f6330f7a4a85c325698d3960b`
- 执行原则：迁移既有实现和测试，只重做基线事实、冲突适配与新基线验证；不强推或改写原分支。

## 状态总览

| 项目 | 状态 | 提交 SHA | 说明 |
|---|---|---|---|
| P0-00 | `completed_pending_commit` | 待提交 | 已从正式 v0.1.34 创建分支并完成环境准备 |
| P0-01 | `pending` | — | 校正版基线门禁 |
| P0-02 | `pending` | — | 新旧基线差异与迁移矩阵 |
| P1–P7 | `pending_migration` | — | 优先复用原实现 |
| P8 | `pending_v0134_adaptation` | — | 知识正式架构发生变化，需重点适配 |
| P9–P11 | `pending_migration` | — | 迁移错误、边界、组合测试与文档 |
| P12 | `pending` | — | 最终验证、构建和封印 |

## P0-00 固定 Git 基线并创建校正版分支

- 状态：`completed_pending_commit`
- 固定提交：`60d910b84572c525a7c9c49216fb9206623bf7a4`
- 分支：`fix/tool-contract-path-invariance-v0134`
- 环境：Node `v24.16.0`、npm `11.13.0`、`Darwin 25.6.0 arm64`。
- `git fetch` 等价 SSH 同步 exit `0`；`npm ci` exit `0`，安装 `1286` 个包。
- 改动文件：本基线、正式进度文件，以及三个本任务专用文件化计划文件。
- 测试：本项不运行产品测试；P0-01 单独采集。
- 日志：`/tmp/lingxi-tool-contract-v0134-p000-fetch.log`、`/tmp/lingxi-tool-contract-v0134-p000-npm-ci.log`。
- 偏差：为保留已推送旧分支且禁止强推，校正版使用带 `-v0134` 后缀的新分支；用户已批准该迁移方案。

## 错误记录

| 时间 | 编号 | 原始错误 | 处理 |
|---|---|---|---|
| 2026-09-05 | 基线审计 | 原任务固定在 v0.1.34 发布前阶段提交 | 保留旧分支，以正式发布提交建立校正版分支 |

