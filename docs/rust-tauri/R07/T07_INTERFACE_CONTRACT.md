# T07 新增接口与现役兼容

权威类型：`rust/crates/lingxi-protocol/src/workbench.rs`；生成物在 `contracts/generated/`，不得手改。最终候选尚未冻结，以下是实际实现契约，不是验收通过记录。

| 接口 | 权限与归属 | 请求/响应 |
|---|---|---|
| GET /lingxi/v1/sessions/{session_id}/agent-activities | chat 加明确会话所有权检查 | AgentActivitySnapshot；仅工作流家族持久快照，与实时 agent_activity 使用 AgentActivityEntry |
| POST /lingxi/v1/desk/activities/summarize | 本机所有者；每次调用和输出前重新检查来源文件夹许可 | ActivitySummaryRequest / ActivitySummaryResponse；缺 id 返回现役业务错误，缺活动返回 summary:null，真实模型失败明确 error；每次长度修复另记真实调用 |
| loop_status 实时帧和会话列表 loopStatus | 原会话归属；不借用前台焦点 | LoopStatusSnapshot；runtimeError 可选，磁盘仍为 running 时不伪造 paused，明确停止自动推进 |

活动快照不合并 `conversations/{id}/agent-activities`：后者属于既有 phone 会话族，形状不同。读取失败保留已有页面记录并明确记录错误，不把失败响应当成空活动。迟到快照不能覆盖实时更新，切换连接后旧响应不应用。

AgentActivityEntry 必需但允许 null 的字段保持现役语义，包括 finishedAt、agentId、summary 等；兼容扩展通过 flatten 保留。LoopStatusSnapshot 的 pausedReason、prompt、maxTurns 也保留 null。生成检查含相应样本及必填/可空检查，避免只修本地类型而改变线上含义。

工作台写入的租约结清失败仍保留真实错误：HTTP 503，leaseState=unknown，executionState=result_produced / failed_or_partial / not_started，canRetry=false；已产生的实际结果放 result，不暗示回滚。

活动摘要端点在旧产品中没有实际 UI/CLI 调用者；本阶段接真实服务及既有路由映射，不虚构新页面入口。


## 循环内部授权与记账归属

正式循环投递签发不可伪造的原代次证明（opaque proof），沿受理、后台启动及工具调用传递；只有真实来源为 Loop 的主运行登记循环预算。普通 User 运行不认领循环预算，只在最后会话许可真正释放时参与空闲守恒检查；旧 observe_main/register_main 登记入口仅供 cfg(test)，不是正式服务路径。

原证明在启动前和 record_run_started 真实提交后分别校验：前者过期则明确拒绝，不新建 Run；后者过期时 Run 已是实际事实，必须进入统一取消与清理，不能丢下 Running，也不能重新读取当前会话代次来认领新循环。loop_control complete/schedule 等动作在同一生命周期锁内按原证明拒绝过期调用，不允许旧运行结束新一代或覆盖新闹钟。此段是当前实现语义；新增真实数据库交错检查尚待运行，不能当验收通过。
