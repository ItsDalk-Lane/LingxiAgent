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

待执行。

## P0-02 现状调用矩阵与原始入口

待执行。
