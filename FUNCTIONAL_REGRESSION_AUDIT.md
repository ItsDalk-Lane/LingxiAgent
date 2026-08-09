# Lingxi 下游功能完整性审计

## 结论矩阵

| 功能 | Git 中存在 | server | renderer | persistence | test | 实际结果 |
| --- | --- | --- | --- | --- | --- | --- |
| 模型列表与当前模型 | `core/model-manager.ts`、`core/model-sync.ts` | `/api/models` 与切换接口 | 模型选择器与设置组件读取同一接口 | 当前选择按 agent 配置保存 | `model-sync*`、`ModelWidget` | 通过；列表、分组与当前选择链路存在 |
| Provider 添加、编辑、删除 | `core/provider-registry.ts` | `/api/providers/summary`、凭证、模型增删接口 | Providers 设置页、详情与模型编辑面板 | 全局 provider 数据写入 `added-models.yaml`，配置层不混写 agent 文件 | `provider-registry-crud` 64/64、`ProvidersTab`、`ProviderModelList` | 通过；前后端协议一致 |
| 自定义模型 | `core/model-sync.ts` | provider 模型增删与健康检查 | 其他模型区和模型编辑面板 | provider 模型条目可重载 | `model-sync` 73/73、`OtherModelsSection` | 通过 |
| Ollama 增强 | `core/provider-compat/ollama.ts`、`shared/ollama-model-metadata.ts` | 原生详情探测补上下文和能力 | 复用 Provider 模型管理界面 | 探测结果投影到模型元数据 | `ollama` 14/14、metadata 19/19、路由测试 | 通过；结构化输出与上下文参数进入真实请求桥接 |
| title slot | `core/auxiliary-slots.ts` | 统一辅助模型解析器 | 设置模型组件按独立键读写 | `title_model` 独立保存 | 槽位契约与解析器测试 | 通过；fallback 为 chat |
| summarize slot | 同上 | 会话摘要走统一解析 | 设置模型组件 | `summarize_model` 独立保存 | 槽位、压缩测试 | 通过；不与其他槽位覆盖 |
| memory slot | 同上 | 记忆路由与写入器走 memory 解析 | Agent/记忆设置保留 | `memory_model` 独立保存 | `channel-router-memory-master` 7/7、槽位测试 | 通过 |
| vision slot | `core/vision-auxiliary-policy.ts` | `/api/models/auxiliary-vision` 与能力校验 | 对话页读取视觉辅助状态 | `vision_model` 独立保存 | 槽位解析器与模型执行测试 | 通过；只允许图像能力模型 |
| approval slot | `core/auxiliary-slots.ts`、`lib/approval-gateway.ts` | Gateway 只读取 approval，未配置即不可用 | 设置文案与模型选择仍在 | `approval_model` 独立保存 | `approval-gateway` 31/31、槽位契约 28/28 | 通过；不回退 chat，不受其他辅助模型修改影响 |
| guard slot | 语义槽位定义与解析器 | 安全消费方按无 fallback 处理 | 共用辅助模型选择协议 | `guard_model` 独立保存 | 槽位解析器 18/18 | 通过 |
| 审批上下文与本机终裁 | `lib/permission/approval-review-context.ts`、Gateway | 审查输出不能越过本机权限终裁 | 用户确认交互仍走既有通道 | 不把审批材料中的敏感值写入模型凭证 | Gateway 与 review-context 测试 | 通过 |
| skills / tool choice | `core/skill-manager.ts`、聊天路由 | 纯 skill 消息进入执行，运行时技能按 agent/workspace 解析 | skillBadge 与技能管理页存在 | agent 选择和技能包存储保留 | `chat-route-skill-message`、`skill-manager` 32/32、`skills-route` 21/21 | 通过 |
| renderer ↔ server 配置协议 | 共享槽位标识、配置范围与服务端路由 | 配置变更触发 provider/model 刷新 | 设置页通过现有请求封装调用 | 全局字段与 agent 字段分开归属 | `config-scope` 16/16、`config-route-ownership` 9/9 | 通过 |
| 旧用户配置迁移 | `core/migrations.ts` 与偏好管理 | 启动期执行兼容迁移 | 读取迁移后的统一结构 | 损坏数据先保留原字节，不静默覆盖 | `migrations` 4/4、`preferences-migration-resilience` 3/3 | 通过；没有清空用户数据 |
| upstream 0.444.1 行为 | 轻量压缩、情绪块、可见文本等模块 | 聊天与桥接入口已接入 | ContextRing、实验页与格式化已接入 | 不要求数据纪元升级 | 见 `UPSTREAM_SYNC_AUDIT.md` | 通过 |
| pi SDK 0.84.1 | 三包声明和锁文件 | OAuth、认证存储与模型运行时适配 | 间接通过服务端协议消费 | 认证清理兼容新存储接口 | pi 边界、xAI、model-manager auth 测试 | 通过 |

## 发布包抽检

- GitHub `v0.1.3` renderer 归档 SHA-256：`5806dad27c9f84eb08ed8a4c80845f923d5e38c36fdfacbb60e54e757bd3f083`。
- GitHub `v0.1.3` darwin-arm64 server 归档 SHA-256：`8891b8872fbf8274de0919bcf50117fdf7d1c9dfe6721d2bb80c09129a42a8db`。
- 两个值均与当前 signed stable/beta Train 一致。
- renderer 包内命中新版供应商汇总接口、辅助视觉与 skillBadge；server bundle 命中六类辅助模型键、供应商路由、Ollama 上下文、skills 和轻量压缩。

这证明 Git 源码和 `0.1.3` 发布包都包含下游功能。用户看不到它们的直接原因是 Active runtime 仍从 `0.1.22` current 目录启动，而不是功能没有进入仓库或安装包。

## 定向验证命令

在 Node 24.15.0 下运行 36 个功能测试文件，结果：`36 passed / 661 tests passed / 0 failed`。修复完成后又执行全量测试，结果为 `1088 passed / 1 skipped` 个文件、`11016 passed / 7 skipped / 0 failed` 项测试。完整命令与阶段结果记录在 `PROGRESS.md`。
