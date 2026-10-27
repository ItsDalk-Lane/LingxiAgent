# R05-T06 — Worker 模型回调边界（WORKER_MODEL_BOUNDARY）

版本：2026-10-07／RR3 当前源码核对。原 T06 未提交描述为历史；阶段 NOT_ACCEPTED，R06_READY=false。
本文件是 T06 交付物之一，固定 worker 子进程与宿主模型平面之间的信任边界、
异步桥形状、预算键与秘密规则。引用规格：R05 总控 §2.3、§4-T06、附录B
T06-C04..C12；实现裁决 R05_INTERFACE_EVOLUTION.md §29。

## 1. 边界总图

```text
worker 子进程（不可信载荷来源）
  │  行协议: kind=callback + op=model.complete + cb_id + purpose + prompt + max_output_tokens
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


线格式包含两个独立字段：`kind="callback"`是读循环的消息分流字段，`op="model.complete"`是模型回调操作字段，不能把操作名写进`kind`。当前`WorkerCallbackLine`声明顺序为kind、cb_id、op、purpose、prompt、max_output_tokens；前3项必需，后3项有反序列化默认值，但正常调用必须满足宿主用途白名单与预算。真实`ask_model_tagged`夹具使用summarize和64 token，对应最小正常消息：

```json
{"kind":"callback","cb_id":"cb-1","op":"model.complete","purpose":"summarize","prompt":"Summarize the granted input.","max_output_tokens":64}
```

此消息以一行JSON和换行发送；前提是宿主已批准summarize并配置对应槽位，invocation、权限与截止时间由宿主管理。`workerrpc.rs`先按kind进入callback分支，再反序列化`WorkerCallbackLine`、检查身份主张/用途/预算并调用宿主模型端口；其他kind进入协议错误分支。当前分支没有另行按`cb.op`匹配操作字符串，本文不虚称新增了op校验。依据为[真实消息声明与分流](../../../rust/crates/lingxi-service/src/workerrpc.rs)、[真实worker夹具](../../../rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs)。E-02只改文档，没有改接口、权限或预算，未重新运行worker进程。

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
| usage 聚合账本 | R05-T07 已实现 v7；成功/失败/取消事实经真实生产端持久化，当前阶段验收未通过 |
| 媒体任务持久化 + 管理 HTTP 面（D12 tasks READ/RETRY 叶） | R07 业务面 |
| worker 回调的 trace 持久化 | 生产组合根 `lib.rs` 已注入 `LedgerWorkerCallbackTrace`（storage+clock），成功/失败先落账后回话；deadline/drop经 `abandoned`，RAII脱离落账为尽力语义，非 Noop |
| 嵌套 subagent 的回调链 | 现有 subagent 运行时（R03-T06），不在 T06 改动 |
| LIVE 供应商验证 | BLOCKED_NOT_AUTHORIZED（RR-BLK-CREDENTIALS，最迟 R10） |

## 7. RR3 源码核对与交接

`rust/crates/lingxi-service/src/lib.rs` 的worker组合块实际 `Arc::new(workermodel::LedgerWorkerCallbackTrace::new(storage, clock))`；`workermodel.rs`/`workerrpc.rs` 实现成功/失败及abandoned事实。旧Noop说明已过期，不能留到R06补必需trace。主模型permit在工具执行前释放，回调仍共享配额/截止时间及宿主白名单。真实端口字段和最小调用片段见[HANDOFF](R05_HANDOFF.json) consumer_contract；本轮仅源码与已有证据核对，自检不独立签收。LIVE/平台边界不变。
