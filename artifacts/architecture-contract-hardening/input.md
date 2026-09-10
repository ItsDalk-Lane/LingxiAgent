# 输入接受与取消契约补强

基线：`e0afc0dc3b94bc9763d715f981d6f948d1c39c27`。修改与下述验证发生在独立 detached 工作区；Node `24.16.0`。仅使用临时合成会话、实际 SDK 和可控流，不调用真实供应商，不读取真实用户历史。没有提交、推送或发布。

## 当前事实与修改

- 客户端原有「接受 + 对应运行结束」双证据、会话身份检查和迟到事件门禁仍保留；本次缺口位于上游证明：缺少 canonical 关联曾被错误解释为确定未接受。
- 提交层分别记录尚未移交、已经移交、无法观察，以及真实 user append。关联记录写入失败、业务身份变化、运行实例更换和完成末端抛错均不能倒退为未接受；只有确定发生在移交前的失败才标记拒绝。
- SDK 底层观察器在移交时记录事实，即使形状或关联校验失败也不否认移交；真实落盘事实先于关联保存和广播。业务会话编号与 SDK 文件头编号继续分别验证。
- 准备阶段取消以及 SDK 异步预检期间取消现在抛出明确的接受前取消，并经实际聊天路由发送结算；接受收据与跨会话投递不能把提前返回当成功。
- 两个协调器插入入口与所有实际调用方等待 SDK 异步完成。SDK 同步入队后立即保存展示与来源记录，保证仍位于对应 user 前；测试证明下一条普通输入不继承前一条插入的元数据。
- 跨会话忙态换路重试要求明确的未接受证明，移交之后碰巧名为 `session_busy` 的异常不得触发重复发送。

## 先失败证据

| 日志 | 旧行为实测 |
| --- | --- |
| `input-red.log` | 8 项失败、23 项通过；真实 SDK 关联失败/身份变化后误拒绝、取消假成功、异步插入提前返回以及实际路由结算缺失 |
| `input-steer-order-red.log` | 1 项失败；等待 SDK Promise 后写入元数据已晚于对应 user |
| `input-runtime-preflight-red.log` | 2 项失败；运行实例更换后误拒绝、SDK 预检等待期间取消仍接受 |
| `input-delivery-retry-red.log` | 1 项失败；移交后 `session_busy` 导致实际会话出现两条相同 user |

## 验证结果

- `input-final-tests.log`：12 文件、292 项通过，退出码 0。
- `input-history-projection.log`：加强真实历史重建与后一条输入隔离断言后，新增契约文件 12 项通过，退出码 0。
- `input-final-test-types.log`、`input-node-types.log`：测试和 Node TypeScript 检查退出码 0。
- `git diff --check`：退出码 0。

执行命令：

```sh
npx vitest run tests/desktop-input-lifecycle-contract.test.ts tests/round3-c01-input-rejection.test.ts tests/desktop-session-submit.test.ts tests/desktop-session-submit-knowledge-routing.test.ts tests/desktop-session-submit-detailed-research.test.ts tests/session-coordinator.test.ts tests/session-collab-delivery.test.ts desktop/src/react/__tests__/services/composer-send-coordinator.test.ts tests/round2-desktop-input-history.test.ts tests/desktop-input-run-wire.test.ts desktop/src/react/__tests__/services/composer-reconciliation.round2.test.ts tests/round2-composer-reconciliation-route.test.ts --reporter=dot
npx vitest run tests/desktop-input-lifecycle-contract.test.ts --reporter=verbose
npx tsc --noEmit -p tsconfig.test.json
npx tsc --noEmit -p tsconfig.node.json
git diff --check
```

本记录是未提交工作区的定向验证证据。完整集成门禁由主任务最终执行；其他平台、真实供应商、桌面打包及发布物均未执行，旧审计封印不作为本次工作区通过证明。
