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

## 待 P0-01 采集

- typecheck、lint、定向测试、全量测试、服务端构建和补丁空白检查的原始统计。

