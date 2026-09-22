# recheck 轮证据（R1/R2/R3）

复审提示词指出的三个缺口在本仓真实环境的处理证据。红测为修复前工作区直接运行
（见 commands.jsonl 的 source_state 与覆盖说明）；绿侧全部经 `../run-logged.mjs`
规范化执行（`../main/commands.jsonl` 的 `recheck-*` 与 `engineering-full-tests-recheck`）。

| 文件 | 内容 |
|---|---|
| r1-red.out/.err | R1 红基线：3 例 `isRunActive` 无法收尾（缺口 run_end 提前退休 runId） |
| r2-red.out/.err | R2 红基线：2 例旧恢复/迟到响应覆盖新流权威；2 个既有守卫回归钉通过 |
| F03_RECHECK_RESULT.json | R1/R2 逐项发现、修复、旧路径去向、红绿映射 |
| commands.jsonl | 本目录命令记录（红侧直接运行 + 绿侧指向 main 索引） |
| redaction-audit.md | R3：123 个未跟踪证据日志的脱敏审计（0 密钥） |
| clean-checkout-verify.out | R3：干净 checkout（write-tree+archive）引用存在与哈希一致验证 |
| green-route.out/.err | 修复后全文件 11/11（现场首验，规范登记见 recheck-f03-regression） |
| f03-regression-green.out/.err | F03 定向回归 19 文件 283 测试（现场首验） |
| affected-surface-green.out | 受影响面 19 文件 249 测试（现场首验） |
| typecheck.out / lint.out / strict-contracts.out / ws-protocol-green.out | 现场首验；规范重跑在 main/ |

本轮修改的生产文件：`desktop/src/react/services/stream-admission.ts`、
`stream-resume.ts`、`ws-message-handler.ts`、`server/ws-protocol.ts`、
`server/routes/chat.ts`、`tests/stream-route-consumer-isolation.test.ts`、
`desktop/src/react/__tests__/services/stream-resume.test.ts`（两处协议断言补
resumeToken，强度不减）。协议变更为可选 `resumeToken` 透传，旧客户端/旧服务器不受影响。
