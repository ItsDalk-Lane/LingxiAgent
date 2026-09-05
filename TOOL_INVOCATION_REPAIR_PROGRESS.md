# 工具契约执行路径不变量修复校正版进度

## 固定事实

- 校正基线：`60d910b84572c525a7c9c49216fb9206623bf7a4`（`v0.1.34^{commit}`）
- 校正版分支：`fix/tool-contract-path-invariance-v0134`
- 原实现来源：`fix/tool-contract-path-invariance` @ `c723410c8ebcd95f6330f7a4a85c325698d3960b`
- 执行原则：迁移既有实现和测试，只重做基线事实、冲突适配与新基线验证；不强推或改写原分支。

## 状态总览

| 项目 | 状态 | 提交 SHA | 说明 |
|---|---|---|---|
| P0-00 | `completed` | `6de275e2cf1e0391a466a9f95f2c499455a07d97` | 已从正式 v0.1.34 创建分支、完成环境准备并推送 |
| P0-01 | `completed_pending_commit` | 待提交 | 原始结果已完整保留；封印顺序失败与签名环境失败均已归因 |
| P0-02 | `pending` | — | 新旧基线差异与迁移矩阵 |
| P1–P7 | `pending_migration` | — | 优先复用原实现 |
| P8 | `pending_v0134_adaptation` | — | 知识正式架构发生变化，需重点适配 |
| P9–P11 | `pending_migration` | — | 迁移错误、边界、组合测试与文档 |
| P12 | `pending` | — | 最终验证、构建和封印 |

## P0-00 固定 Git 基线并创建校正版分支

- 状态：`completed`
- 提交：`6de275e2cf1e0391a466a9f95f2c499455a07d97`，远端分支已核对同 SHA。
- 固定提交：`60d910b84572c525a7c9c49216fb9206623bf7a4`
- 分支：`fix/tool-contract-path-invariance-v0134`
- 环境：Node `v24.16.0`、npm `11.13.0`、`Darwin 25.6.0 arm64`。
- `git fetch` 等价 SSH 同步 exit `0`；`npm ci` exit `0`，安装 `1286` 个包。
- 改动文件：本基线、正式进度文件，以及三个本任务专用文件化计划文件。
- 测试：本项不运行产品测试；P0-01 单独采集。
- 日志：`/tmp/lingxi-tool-contract-v0134-p000-fetch.log`、`/tmp/lingxi-tool-contract-v0134-p000-npm-ci.log`。
- 偏差：为保留已推送旧分支且禁止强推，校正版使用带 `-v0134` 后缀的新分支；用户已批准该迁移方案。

## P0-01 运行校正版基线门禁

- 状态：`completed_pending_commit`。
- typecheck exit `0`；lint exit `0`，`0 errors / 9194 warnings`。
- 指定 11 文件定向测试 exit `0`，`11` 文件、`253` 测试全部通过。
- 全量测试 exit `1`：`1359 passed / 1 failed / 1 skipped` 文件，`13763 passed / 1 failed / 7 skipped` 测试；唯一失败是 P0 新增记录尚未进入旧封印 allowlist。
- 原始服务端构建 exit `1`：缺少 `LINGXI_SIGN_KEY`；抛弃式匹配密钥诊断复跑 exit `0`，临时密钥已删除。
- `git diff --check` exit `0`，工作树在记录结果前干净。
- 处理边界：没有修改生产代码、封印测试或 allowlist；失败保持原始状态，P12 再推进已验证源码坐标。

## 错误记录

| 时间 | 编号 | 原始错误 | 处理 |
|---|---|---|---|
| 2026-09-05 | 基线审计 | 原任务固定在 v0.1.34 发布前阶段提交 | 保留旧分支，以正式发布提交建立校正版分支 |
| 2026-09-05 | `P12_SEQUENCE_SEAL_GATE_CYCLE` | P0 新增任务记录使全量套件中的旧封印测试失败 | 保留 fail-closed 与原始失败；P12 按任务书固定新源码坐标 |
| 2026-09-05 | `BUILD_SIGN_KEY_MISSING` | 原始 `build:server` 缺少签名密钥，exit `1` | 抛弃式匹配密钥诊断复跑 exit `0`；临时材料已删除 |
