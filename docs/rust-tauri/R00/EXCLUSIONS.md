# R00-T02 撤回项、兼容项与现役边界

基线 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。R1 修复后重新核对撤回、外装和兼容边界；没有修改生产代码或旧数据。机器可读的对应 F-ID 在 `FEATURE_INVENTORY.json.classified_nonproduction`。

## 硬排除

| F-ID | 项目 | 当前调用图结论 | 迁移处置 |
|---|---|---|---|
| `F-D05-X-FORCED-SUBAGENT-ROUTING` | 专用子代理目录与强制分发改造 | `core/agent.ts:936-938` 创建现役 `subagent/subagent_reply/subagent_close`；`core/agent.ts:1228-1230` 挂入工具快照。`lib/experiments/registry.ts:122-145` 的主动委派为 beta 且默认 `false`，`core/agent.ts:1754-1769` 仅在开关生效时补充提示。没有必须通过新目录强制分发的生产注册。 | 不恢复强制目录或强制分发；现役三工具与实验开关继续保留。 |
| `F-D08-X-INDEPENDENT-RESEARCH` | 独立知识研究运行引擎 | `server/composition/open-root.ts:108` 挂载 `knowledge` 路由；`server/routes/knowledge.ts` 的注册项为 notebook/source/import/ingestion/citation/内容读取，没有 research 启动端点。`core/agent.ts:1236-1240` 当前知识工具仅 search/read/outline/grep/manage。 | 不新增研究启动入口、后台研究 Agent 或研究运行任务。 |
| `F-D10-X-LOCAL-MODEL-MANAGEMENT` | 本地模型下载、安装、生命周期管理子系统 | 当前模型供应商注册含 Ollama 接入 `core/provider-registry.ts:480`，且设置端的模型发现见 `server/routes/providers.ts:664-690`；这些路径只接入已有 Ollama 服务，不提供本地模型管理。 | 不实现本地模型管理；继续支持 Ollama 作为供应商。 |

## 必须保留的相邻能力

- 现役子代理：`lib/tools/subagent-tool.ts:253,668,944` 提供派发、续接、关闭；`core/agent.ts:898-938,1228-1230` 注入运行和工具快照；`server/routes/chat.ts:2353-2357` 可停止；运行/线程状态分别持久化。主动委派实验在 `lib/experiments/registry.ts:15,122-145` 默认关闭，设置界面消费 `desktop/src/react/settings/tabs/ExperimentsTab.tsx:444-450`。清单用 `tool:subagent*`、`ws-in:subagent_stop_request` 与 `experiment:subagent.proactive_delegation` 分别登记。
- Ollama：`core/provider-registry.ts:436,480` 注册 `lib/providers/ollama.ts`；`server/routes/providers.ts:664-690` 读取本地服务模型与能力；`shared/model-operations.ts:12,38-39` 保留 embedding 协议选择。清单中 `provider:ollama` 为保留功能。
- 现役知识库：`server/routes/knowledge.ts` 的 notebooks、来源导入/刷新/重建、内容块、引用等端点，及 `core/agent.ts:1236-1240` 的五个知识工具，均归 `D08` 保留。
- 工具扩展边界：`core/tool-catalog-bridge.ts:31,230` 的目录搜索、描述、调用以及 `server/routes/mcp.ts:245-259,381-385` 的连接器管理与旧别名均保留。用户配置的具体 MCP 实例属于外装扩展，不计作内置工具。`core/engine.ts:3750-3761,3779-3799` 当前仅加载随包 `plugins/` 目录；旧 `~/.lingxi/plugins` 用户插件目录不再加载，不把它误算成现役内置插件。
- 斜杠命令边界：`core/slash-commands/index.ts:25` 将 `bridgeCommands` 的 11 个核心命令注册进 dispatcher；`lib/bridge/bridge-manager.ts:1139` 和 `server/routes/chat.ts:2538` 消费它。`core/plugin-manager.ts:1006-1041` 可从插件 `commands/` 动态注册命令，需通过 full-access 与保留名检查。随包插件当前没有 `commands/` 目录；外装命令名不是仓库内核心命令闭集。清单保留协议边界 `F-D16-E-DYNAMIC-SLASH-PROTOCOL`，不虚构具体外装命令。

## 兼容地址与内部协议的分类

- `server/routes/mcp.ts:245-311` 的 `/servers` 和 `/connectors` 12 组路径，及 `/settings/enabled`、`/enabled`，经同一处理函数或同一动作参数进入相同行为；`/plugins/mcp` 与 `/mcp` 由同一子路由挂载。原始地址仍逐项列在 `ENTRYPOINT_COVERAGE.json`，30 条受影响旧 F-ID 在 `FEATURE_INVENTORY.json.legacy_feature_id_map` 映射到现行叶子。
- 同类兼容地址还包括 `agents` 的 `ishiki`/`agents-md` 与 `mobile-workbench` 的 `/mobile/workbench`/`/workbench`。生成器比较成对注册语句，若 handler 分叉会失败；这只是静态同源证据，不替代运行时响应测试。
- `/api/health` 负责客户端首次连接和身份/头像资料，`/api/log` 是前端诊断上报，`/api/shutdown` 是服务生命周期操作；它们归本地服务连接能力，不单列三个用户功能。MCP 与认证 OAuth callback/poll 归各自登录流程；记忆/模型观测 health 归相应查看能力。它们仍是生产入口，未从覆盖清单删除。

## 只作兼容的旧研究资料

`lib/knowledge/knowledge-store.ts:1901-2068` 仍建立 v18/v19 研究台账与完整性表，`lib/knowledge/knowledge-store.ts:3692-3697` 的来源删除保护仍引用旧记录。`core/session-manifest/knowledge-ancestry.ts:59` 识别旧研究会话种类；`server/routes/chat.ts:1955-1996` 转发已存在的研究事件；`desktop/src/react/utils/tool-label.ts:31-32,113-115` 和 `desktop/src/react/components/chat/MessageActivity.tsx:31-33` 可展示旧工具记录。这些读取、迁移与展示路径应保护旧数据，但不构成新的研究启动能力。

## 分类复核命令

```bash
rg -n 'PROACTIVE_SUBAGENT_EXPERIMENT_ID|proactive_delegation|createSubagentTool|createSubagentReplyTool|createSubagentCloseTool' core/agent.ts lib/experiments/registry.ts desktop/src/react/settings/tabs/ExperimentsTab.tsx
rg -n 'ollamaPlugin|ollama-embed|name === "ollama"' core/provider-registry.ts lib/providers/ollama.ts shared/model-operations.ts server/routes/providers.ts
rg -n 'knowledge_research|research_run_id' lib/knowledge core/session-manifest server/routes desktop/src/react/utils/tool-label.ts desktop/src/react/components/chat/MessageActivity.tsx
rg -n 'knowledge_research|research' server/routes/knowledge.ts core/agent.ts
python3 -B docs/rust-tauri/R00/r00_t02_inventory.py
```

最后一条 `rg` 若无匹配会以退出码 1 表示“未找到文本”，不是动态运行证明。这里的结论来自已挂载的路由、实际工具快照及其消费者共同核对；之后若新增注册入口，应重跑提取并重新分类。未运行真实模型或旧资料迁移，因此本文件不声称这些后续行为已验收。

## R2 修复后的入口与行为边界

- 同一地址的不同 HTTP 方法保留各自原始登记，除 GET/HEAD 同资源读取外，按效果拆成不同叶子；OAuth 回调与轮询仍是登录内部步骤。72 组逐项裁决在 `ENTRYPOINT_COVERAGE.json.multi_method_decisions`，含源码位置、每种方法的动作/结果及 F-ID。
- `POST /confirm/:confirmId` 由请求体 `action` 选择批准或拒绝。两种相反效果各有叶子，共享一个原始路由；不能把这个地址当作第三个独立用户功能。具体消费者与共享关系见 `body_effect_decisions`。
- `/api/health`、`/api/log`、`/api/shutdown` 仍附着本地服务连接能力，不独立计用户叶子；源码和内存反例检查防止它们脱离能力归属。
- 文件预览是只读展示与不可信内容隔离行为，独立于文件写回。它可与编辑共享 `read-file-snapshot` 底层桥，归 R09-T05/A09/A10；共享入口在 coverage 的 `feature_ids` 中可见。
- 原 R1 的撤回/兼容/外装分类不变。现役子代理、Ollama、知识库与随包插件继续保留；旧研究资料继续只作兼容。
