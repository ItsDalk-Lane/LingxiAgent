# Model Observatory 现行入口

源码核对日期：2026-09-06。本文说明当前实现和排查入口；本次未运行模型调用、跨平台测试或发布验证。

## 调用、用量与存储

Model Observatory 把调用身份、执行轨迹、请求/响应副本和用量组织成可查询的调用记录。
Usage Ledger 仍是用量事实来源，不是第二份正文存储；观测写入故障不反向控制模型调用或账本。

| 关注点 | 现行源码入口 |
| --- | --- |
| 运行时安装、设置、健康和来源补充 | [core/engine.ts](../../core/engine.ts) |
| 观测代际切换与在途调用收口 | [model-observability-engine.ts](../../lib/llm/model-observability-engine.ts) |
| 写入策略、队列与故障状态 | [model-observability-persistence.ts](../../lib/llm/model-observability-persistence.ts) |
| 数据库结构与兼容迁移 | [model-observability-schema.ts](../../lib/llm/model-observability-schema.ts) |
| 只读查询、详情与聚合 | [model-observability-query.ts](../../lib/llm/model-observability-query.ts) |
| HTTP 查询、载荷、Blob 和导出 | [server/routes/model-observability.ts](../../server/routes/model-observability.ts) |

当前 `MODEL_OBSERVABILITY_SCHEMA_VERSION = 6`。v3 增加显式用量关联状态，v4 增加来源名称快照；
v5/v6 是轨迹数据整合，v6 按调用所属会话合并轨迹，不以此猜测或重写 `parentCallId` 因果关系。
[用量投影](../../lib/llm/model-observability-accounting-projection.ts) 按 `metadata.modelCallId` 幂等写入 `model_call_usage`；
启动回填只覆盖 bounded Usage Ledger 中仍保留的记录，不能视为完整历史恢复。

## 配置与运行状态

[产品偏好归一化](../../lib/llm/model-observability-preferences.ts) 固定启用观测、元数据、正文和媒体持久化，只允许调整保留天数。
设置 API 拒绝把这些开关设为 `false`；低层 persistence policy 仍保留开关，供显式注入及故障隔离使用。
`desired` 表示保存的偏好，`effective` 表示实际记录状态；全开偏好不能证明存储处于 `active`。
启动失败、数据库不兼容或写入故障可能产生 `disabled` / `degraded`，原因和状态由设置及 health API 返回。
记录状态与历史查询状态分别报告；停止记录不等于历史不可读。调整设置通过代际切换保留在途调用的收口机会。
存储保护为本地文件权限，`cryptographicallyEncryptedAtRest = false`，不能描述为磁盘加密。

## 身份与正文展示

[调用关联](../../lib/llm/model-call-correlation.ts) 先从运行时消息对象取得身份，再写入隐藏的 `hana-model-call-reference-v1` 条目。
桌面与 Bridge 协调器在助手消息落盘前保存该条目；[历史读取](../../core/message-utils.ts) 只把它关联到其后第一条助手消息。
关联缺失时保留缺失，不按时间接近度补造调用编号；隐藏条目不进入展示或模型上下文。
[来源解析](../../lib/llm/model-observability-source-identity.ts) 使用业务类型、实体身份和名称快照；engine 可用当前实体名称补充显示，保留底层原始来源。

[调用详情](../../desktop/src/react/settings/tabs/observability/ObservabilityCallInspector.tsx) 展示业务名称、类型、模型、状态、时间和用量，技术字段默认折叠。
打开详情即并行请求各条载荷，单条失败分别显示；正文经 [纯文本投影](../../desktop/src/react/settings/tabs/observability/trace-detail/payload-plain-text.ts) 阅读，不等于原始网络字节。
列表接口只返回元数据。正文、Blob、导出与设置修改的 `LOCAL_ONLY` 权限由 [route-security.ts](../../server/http/route-security.ts) 约束。

## 必须保留的状态语义

状态闭集以 [API 契约](../../shared/model-observability-api-contract.ts) 和查询实现为准：

- 用量的 `0` 是已观察到的零值，`null`、`unknown`、`not_correlated`、`projection_unavailable` 和 `corrupt` 不可合并为零。
- 载荷的 `present`、`expired`、`dropped`、`not_captured`、`unknown` 分别表达保存事实；未捕获的历史正文不能补造。
- `opaque` / `unavailable` 表达观测能力边界；可见度、脱敏/截断和保真度是不同维度，不能把运行时副本称为完整 raw 请求。
- 调用的 `ok`、`error`、`aborted`、`incomplete` 与数据完整性分别报告，单个调用成功不代表观测数据完整。

## 视频及证据边界

视频输入沿 [InputArea](../../desktop/src/react/components/InputArea.tsx) 的读取与 `wsMsg.videos`，进入 [desktop-session-submit](../../core/desktop-session-submit.ts) 提交链。
[传输决策](../../shared/model-capabilities.ts) 当前包含 Gemini inline data、已识别端点的 OpenAI `video_url`，以及通用 OpenAI 兼容 `video_url`。
通用档要求模型声明视频输入并使用兼容协议；未知兼容端点不再一律拒绝，实际支持由供应商响应确认，仍受 [视频校验](../../shared/video-mime.ts) 约束。
源码接线与本地测试不证明真实供应商视频调用成功。本次未检查凭据、未执行付费 smoke，也未刷新历史验收状态。

## 历史材料

[实现报告](../archives/model-observability/OBSERVABILITY_IMPLEMENTATION_NOTES.md) 与 [统一实施事实](../archives/model-observability/OBSERVABILITY_UNIFICATION_FINDINGS.md) 保留当时的设计、限制和验证证据。
其中旧 schema、默认开关、视频拒绝策略和阶段完成状态不能直接充当当前实现结论；排查现行行为先使用本文源码入口。
