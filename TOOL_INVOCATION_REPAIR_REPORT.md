# 契约执行路径不变量修复报告

## 结论

本分支把工具调用收拢为“一份身份、一份权限、一份参数校验、一个网关、一个执行器”。直接展示、延迟目录、插件开发聊天和本地开发 HTTP 只是不同入口，不再各自推导权限、挑选凭证或调用原始执行器。

- 固定基线：`4fefe66ec3b4f6b23c78a09869a607886585740e`
- 执行分支：`fix/tool-contract-path-invariance`
- 源码候选和审计封印的真实 SHA：按 Git 自引用限制，记录在 `PROGRESS.md` 与最终执行报告；机器事实中固定为 `null`。
- 实施范围：严格覆盖任务书 P0-00 至 P11-02；最终验证和封印由 P12 完成。

## 核心变化

1. 注册时先给每个工具建立稳定目标身份、完整参数校验、规范权限和生命周期代次。
2. 当前 Agent 看不到或禁用的目标先被排除，再计算是否进入直接面或延迟目录。
3. 目录只保存目标引用和只读描述，不保存原始工具对象或执行器。
4. 直接、延迟、开发聊天和 MCP 都进入同一个调用网关；网关在执行前复核参数摘要、会话、Agent、代次与实时可用性。
5. 插件和 MCP 的卸载、禁用、重载、重连会让旧代次失效，旧会话不能继续执行陈旧对象。
6. 媒体四类入口统一解析执行目标和凭证来源；知识检索两条兼容路径统一消费完整重排策略。
7. AST 边界扫描把允许接触 raw executor 的位置固定为精确文件白名单。

完整不变量、时序、路径和接入规则见 `docs/architecture/tool-invocation-path-invariance.md`。

## 发现项关闭矩阵

| 发现 | 状态 | 修复位置与证据 |
| --- | --- | --- |
| V1 | fixed | `lib/permission/tool-invocation-permission.ts` 统一新旧权限方言；`core/engine.ts` 先注册规范目标再规划延迟；direct/deferred bundled 矩阵证明 7 个只读和 5 个副作用工具可达。 |
| V2 | fixed | `core/tool-invocation-gateway.ts` 和准备上下文绑定真实目标、参数、会话、Agent、调用编号与句柄；路径变形测试比较执行结果和副作用次数。 |
| V3 | fixed | identity 把 `capabilityBase` 与显示名分离；Bridge 只委托注册目标实际拥有的能力，跨来源同名不再靠前缀猜测。 |
| V4a | fixed | `core/tool-availability.ts` 与 Engine/MCP eligibility 在装配和执行前复用同一判断；Agent 禁用返回 `TARGET_DISABLED_FOR_AGENT`。 |
| V4b | fixed_by_generation_contract | Plugin 和 MCP 都维护 live generation；准备后发生卸载、禁用、重载或重连时返回 `TARGET_REVOKED`，不执行旧对象。 |
| V5a | fixed | `core/plugin-dev-tools.ts` 的聊天入口只使用宿主会话与 Agent 身份，模型参数不能覆盖；目标解析和执行均走 Gateway。 |
| V5b | fixed_with_local_developer_principal | HTTP 入口由本机 owner 派生 `LocalDeveloperPrincipal`，清空会话/Agent 冒充字段，并由 Gateway 独立准备与审批。 |
| V6 | fixed | `lib/knowledge/rerank-policy.ts` 提供共享重排执行器；compiled scope 与 notebook query 使用同一完整 policy 和摘要缓存键。 |
| V7 | fixed | `core/media/media-execution-target-resolver.ts` 是媒体凭证选路唯一门面；image、video、STT、background 只消费规范执行目标。 |
| V8 | fixed | 注册、schema、错误因果和组合矩阵全部有自动测试；缺声明、冲突、未知字段和不稳定输入均 fail-closed。 |
| V9 | fixed | MCP 的全局开关、连接器开关、Agent 开关与 model/app 可见性共用 eligibility；app-only 目标不进入模型目录。 |
| V10 | fixed | MCP direct/deferred 共用 Manager target adapter、Gateway 和结果规范化；结构化内容、应用卡片与来源信息保持一致。 |
| V11 | fixed | `core/tool-catalog.ts` 以 TargetId 和来源限定索引；未限定来源的同名命中多个目标时返回 `TARGET_AMBIGUOUS`。 |
| V12 | fixed | 完整 TypeBox schema 验证覆盖嵌套对象、联合、枚举、格式与额外字段；错误带稳定 issue paths，Bridge 不把异常改成成功文本。 |

## 安全与诊断边界

- 目标不存在、不可见、Agent 禁用、已撤销、能力不匹配、凭证缺失、凭证解析失败、传输失败和取消分别保留稳定错误码。
- 模型可见错误会清除常见密钥、令牌和内部路径。
- Gateway 诊断只记录 route、origin、targetId、sourceId、generation 和错误码。
- `npm run check:tool-invocation-boundaries` 扫描 raw MCP、raw Plugin、Bridge 旧旁路、Engine 原始对象映射和 Gateway 外 canonical executor 调用。

## 范围边界

- 未合并 `main`，未删除分支，未强推。
- 未改动任务书之外的产品功能。
- 未恢复任何 raw plugin/MCP 直连。
- 未删除、跳过或放宽测试；P10-02 新增矩阵首次即绿的事实单独留痕，没有人为制造红灯。

