# 本轮修复记录

起点：docs/knowledge-closeout-2026-09-21，b0e9118427e16206dcd25ecfb566d766ed476727，工作区初始干净。Node v24.16.0，npm 11.13.0。

范围仅 F01—F05/O01/C01—C32。无提交、推送、PR、发布、真实供应商调用和真实用户数据授权。外部限制单独登记，不妨碍本地实现。

分工：F01 子代理负责任务注册表、总线、真实生产方和 C01—C08；F02 子代理负责 HTTP 输入和 C09—C13；F03 子代理负责 WS/消费者和 C14—C19；主代理负责 strict 门禁、工程验证、状态重算及统一交付。

预期修改地图：F01 lib/task-registry.ts、server/task-bus-handlers.ts、实际调用方及生命周期契约；F02 server/hono-helpers.ts、server/routes/sessions.ts、真实输入校验契约；F03 use-stream-buffer、真实WS消费者及必要发送护栏；F04 tsconfig.core-contracts.json、check-core-contracts-strict.mjs、门禁自测、实际核心闭包；F05 本目录报告、映射及 artifacts/refactor-2026/independent-fix 原始证据。

成功条件：真实 Node24/锁定依赖先复现，再使必修代码反例普通正向通过；C01—C32逐项关联生产入口、测试、命令与日志；全量退出码原样保留。外部必需验证受阻时总体不得PASS。

当前：已读审查三材料、总入口和通用约束；开始核对阶段文档/CI/依赖。三个实现包并行，先红后绿证据由各包保存。严格门禁发现丢弃无文件诊断，需修复与负例验证。

收尾：F01—F04代码与真实接线已完成；额外修复跨会话取消、清理失败批次索引释放和确认卡终态复活。原始红绿/全量失败日志保留。真实GUI及最终产物已补验，资源RSS趋势与授权/平台/安装限制如实登记。最终证据生成与校验以FIX_RESULT、SOURCE_MANIFEST、EVIDENCE_SHA256为准，未提交/发布。

recheck轮（2026-09-22）：入场 8d55046d5646a008f71f57de745b3564e94a5f21，工作区干净。R1（接纳原子性）/R2（恢复代次）先红（3+2例）后绿，新增7例真实WS链路反例；R3 将123个未跟踪证据日志脱敏审计后纳入版本管理并重生成清单。绿侧经run-logged登记（recheck-*、engineering-full-tests-recheck：14874/4/15，exit 1，4例与上轮相同）。本页此前各段为F01—F05轮原始记录，保持不变。
