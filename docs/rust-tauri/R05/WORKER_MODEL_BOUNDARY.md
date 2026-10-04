# R05-T06 — Worker 模型回调边界（WORKER_MODEL_BOUNDARY）

版本：2026-10-03／1.0。状态：T06 实现登记（工作树候选，未提交）。
本文件是 T06 交付物之一，固定 worker 子进程与宿主模型平面之间的信任边界、
异步桥形状、预算键与秘密规则。引用规格：R05 总控 §2.3、§4-T06、附录B
T06-C04..C12；实现裁决 R05_INTERFACE_EVOLUTION.md §29。

## 1. 边界总图

```text
worker 子进程（不可信载荷来源）
  │  行协议: kind=model.complete + cb_id + purpose + prompt + max_output_tokens
  ▼
workerrpc.rs 读循环（每 invocation 一个宿主铸造 id；deadline 约束 timeout_at）
  ▼
BoundedWorkerModel（宿主预算前置：每调用回调数、token 上限、prompt 字节上限）
  ▼
GatewayWorkerModel（service/src/workermodel.rs）
  │  1) purpose → AuxiliarySlot 仅经宿主白名单映射，未知即拒（C09）
  │  2) QuotaManager Model permit（与主循环/操作平面同一配额管理器，C06）
  │  3) 关联 id = aux-{slot}-{invocation}-{cb_id}（trace 父子关联，C04）
  ▼
AuxiliaryExecutor（adapters/src/models/auxiliary.rs）
  │  turn=1 / 无工具 / 无 prior 的最小交换；槽位自有路由绑定
  ▼
GatewayedProvider → 真实五族 chat 适配器（openai-completions 等）
  ▼
CredentialService（唯一凭证材料出口；错误串过 scrub_materials）
```

## 2. 信任规则（载荷不是权限）

| 规则 | 实现 | 验证 |
|---|---|---|
| worker 不能选择 provider/model/endpoint/凭证 | 路由一律由 ModelGateway 按槽位解析；线格式出现身份主张字段（provider/model/endpoint/auth/run_id/parent_run_id）即 `worker_identity_not_negotiable` 响亮拒绝 | r05_t06_worker_model `c09` |
| purpose 必须命中注册时宿主批准的白名单 | `WorkerToolSpec.allowed_model_purposes`；未授权用途 `model_purpose_not_granted` | `c09` |
| 预算不能由载荷扩大 | max_output_tokens==0 / 超过 4096、prompt 超 64 KiB → `BudgetExceeded`，在 inner 端口被咨询之前拒绝 | `c08` |
| 预算键 = invocation id | `begin_invocation` 在 execute() 铸 id 后调用；`end_invocation` 每条结算路径（含 future drop 的取消路径，RAII）回收计数 | `c07` |
| 同 invocation 内重复 cb_id 绝不二次外发 | invocation 级回执缓存应答重放，不消耗回调预算 | `c07` |
| 凭证材料不进 worker | env/参数/输出零密钥；宿主环境植入秘密的对照为负 | `c11` |

## 3. 异步桥（§29.1 裁决的落地）

- `WorkerModelPort::complete` 是 boxed-future 异步端口（与
  `TurnProviderPort::next_turn` 同形）；签名携带宿主铸造的 `invocation` 与
  worker 侧 `cb_id`。
- workerrpc 读循环以 `timeout_at(deadline, port.complete(..)).await` 调用：
  callback 等待与行读取共享同一 invocation deadline——到期即协议取消 +
  进程组有界 kill + Unknown 结算。
- async 读循环内无 `block_on`、无持锁等网络。单 worker-thread 运行时下
  并发 health/取消/另一请求仍可服务（C05 `c05`，真实延迟回调用 stub 钉死）。
- 真实子进程：测试 worker 是 `r04_t07_fixture` 子进程，不是进程内函数调用。

## 4. 配额互锁（C06）

- callback 与主模型循环共用同一 `QuotaManager` 的 Model 资源；agent lane =
  `worker:{plugin_id}`，session lane = run 的 session。
- 结构性前提：runs.rs 在主模型 HTTP 轮次解出后、工具执行前释放模型
  permit——callback 的 acquire 才能成功。全局并发=1 时「主模型→worker 工具
  →宿主回调」完整链不死锁（`c06` 正向腿）。
- 负向腿：人为持有唯一 permit 时，callback 的有界等待以 `BudgetExceeded`
  超时退出并说明原因，绝不静默死锁（`c06` 负向腿）。
- 挂起回调：模型端一直不答时，deadline 到 → worker 进程按预算结束、结算
  Unknown（`c10`）。

## 5. 操作平面共享同一配额（T06 新增）

`OperationService`（service/src/operations.rs）的每个操作入口
（embedding/rerank/image/video/speech/transcribe 及各 query）经同一
`QuotaManager` Model permit 准入（lane `operations:{op}`），与主循环、worker
回调共用全局并发预算——不存在绕过模型配额的第二条通道。

## 6. 已登记边界与后续归属

| 事项 | 归属 |
|---|---|
| usage 聚合账本 | R05-T07（T06 仅在回执/结果上携带 usage 原始事实） |
| 媒体任务持久化 + 管理 HTTP 面（D12 tasks READ/RETRY 叶） | R07 业务面 |
| worker 回调的 trace 持久化（`WorkerCallbackTracePort` 生产端为 Noop） | T07（测试以记录型端口钉住父子关联） |
| 嵌套 subagent 的回调链 | 现有 subagent 运行时（R03-T06），不在 T06 改动 |
| LIVE 供应商验证 | BLOCKED_NOT_AUTHORIZED（RR-BLK-CREDENTIALS，最迟 R10） |
