# 兼容报告（compatibility-report）

- 四组合矩阵执行：F01（真实接口/请求链）；逐格证据 `protocol/compatibility-matrix.json` fourCombos。
- 旧基线：A01 clone（/tmp/lingxi-baseline-1d42b740，HEAD=1d42b740，未提交/未建分支）。

## 一、四组合结果

| 组合 | 验证层级 | 结果 | 关键断言 |
|---|---|---|---|
| 旧+旧（控制组） | 真实 HTTP（clone 旧路由子进程，Node type-stripping 跑 A01 旧代码） | 200 旧 JSON（messages/hasMore）、无协议头/无 ETag | 原分页行为保持 |
| 新+旧 | 真实 HTTP（新路由真实服务 × 旧客户端构造：path+sessionId、省略 limit、无条件） | 默认 50、正常 200、七字段 JSON 可消费、新增头不影响旧解析 | 旧客户端零改动可用 |
| 旧+新 | 真实 HTTP（clone 旧服务端 × 主仓库新客户端代码） | loadMessages 200 无感回退不存记录；概览 404→unsupported 不卡住；翻页 nextBefore 照常推进；reconcile 正常触发 | 能力缺失无感回退 |
| 新+新 | 真实 HTTP | 条件请求 200→304 无正文；ETag 随 limit 变化；K=100 协商生效 | 条件复用+概览+推荐页生效 |

## 二、兼容边界（如实记录）

- 旧服务端无概览端点 → 新客户端 404 → epoch 记为不支持 → 回退既有翻页探底（不卡住、不弹持续错误）。
- 旧服务端消息响应无 ETag/协议头 → 新客户端不保存校验记录（无标签可复用，保持 50 无条件）。
- 新服务端对旧客户端：省略 limit 仍 50；新增头（Lingxi-*/Cache-Control）为增量信息，旧客户端只读 body 不受影响。
- 严格对账（reconciliation=1）与 all=1 不进条件快路径（回归：history-protocol-conditional『all/reconciliation 不进条件快路径』）。

## 三、未覆盖与边界

- 完整旧 renderer（Electron UI 全链）未运行——旧客户端以 clone 源码请求构造 + 真实 HTTP 验证（层级如实标注）。
- 跨请求不可变快照（TASKBOOK §5.2）未建设——外部无通知分支修改的边界在 remaining-risks.md 另列。
