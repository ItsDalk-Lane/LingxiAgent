# R00-T02 独立对抗性验收 R1

VERDICT: FAIL

验收对象：`codex/rust-tauri-migration`，Task Base / tested HEAD `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。审阅前六份候选按“相对路径 + NUL + 文件 SHA-256 原始 32 字节”聚合，摘要为 `e8786a4926116558795e4e6361588c537cd69bacb9b2984b83094f3e4fb87142`，共 1,204,282 字节。本报告不属于该候选摘要。已独立阅读新任务书 01—06、R00 阶段、task-catalog 的 R00-T02、acceptance-catalog 的 R00-A03/A04，以及旧 `FEATURE_MATRIX.md`、本轮六份交付和相关生产源码。

## 验收结果

| REQUIRED 场景 | 结论 | 依据 |
|---|---|---|
| R00-A03 真实入口均有归属 | **FAIL** | 候选只在自身抽取的 700 项之间形成零差集；真实桌面 IPC 和斜杠命令等现役登记未入集合，且部分兼容别名、内部端点被误计为叶子。 |
| R00-A04 撤回与现役不混淆 | **PASS（本阶段静态分类范围）** | Ollama 供应商、现役三项子代理工具及默认关闭的实验被保留；研究表/旧事件只作兼容；旧用户插件目录不加载，外装 MCP 实例未当内置。此结论不代表真实供应商或用户数据已验证。 |

## Findings

### F01 — BLOCKING：桌面宿主的生产入口完全未纳入双向清单

- **位置与证据：**`r00_t02_inventory.py:190-449` 的抽取范围没有 `desktop/preload.cjs` / `desktop/main.cjs` 的 IPC 注册；只有用文本搜到 `desktop/main.cjs` 中首个 `train-update-` 才生成一个笼统的更新叶子。独立从 `desktop/main.cjs` 的 `wrapIpcHandler` / `wrapIpcBestEffortHandler` 与 `desktop/preload.cjs` 的 `ipcRenderer.invoke` 取交集，得到 **82 个**同名生产入口，`ENTRYPOINT_COVERAGE.json.registrations` 中 `ipc:` 为 **0 个**。例如 `desktop/preload.cjs:73-77` → `desktop/main.cjs:5431-5434` → `desktop/src/react/settings/tabs/GeneralTab.tsx:205-225` 是用户开关“开机启动/保持唤醒”的完整链；`desktop/preload.cjs:169-178` → `desktop/main.cjs:5479-5553` 是可见浏览器窗口和标签；`desktop/preload.cjs:121-124` → `desktop/main.cjs:5804-5863` 是模型观测导出。旧种子 `FEATURE_MATRIX.md` 的 F17 已明确指出最后一组 IPC，候选仍未追入。
- **违反要求与后果：**R00-T02 步骤 1—3、R00-A03 要求真实 UI/路由登记到叶子的双向覆盖；D09/D14/D15/D22/D23/D24 的桌面能力会在迁移规划中漏实现或漏验收。“700 入口、零差集”不能证明全部真实入口有归属。
- **根因与同类路径：**提取器把 HTTP、部分 UI、工具等选定输入当作全体生产入口，遗漏 Electron preload ↔ main 的独立组合根。文件选择、拖放、通知、快捷键、更新、资源编辑、预览和系统权限属于同一漏检类。
- **修复要求及重跑：**从 preload 暴露、main 注册与实际 renderer 调用取交集，区分内部桥接操作和用户可见叶子，补入逐项入口、所有者、数据、F-ID 和阶段/验收归属；重新做全量双向差集及 D09/D14/D15/D22/D23/D24 人工抽样。加入一个只在内存中新增 IPC handler/桥接调用但未赋功能归属的负向反例，必须非零失败；重跑清单生成、针对的宿主测试与受影响界面测试。

### F02 — BLOCKING：真实斜杠命令注册被一个传输入口遮盖

- **位置与证据：**`core/engine.ts:863` 构造 `createSlashSystem`，`core/slash-commands/index.ts:25` 将 `bridgeCommands` 逐个 `registerCommand`；`core/slash-commands/bridge-commands.ts` 包含 `/stop`、`/new`、`/reset`、`/rc`、`/exitrc`、`/apply`、`/confirm`、`/reject`、`/compact` 等不同用户动作和权限/状态效果。`lib/bridge/bridge-manager.ts:1139-1152` 与 `server/routes/chat.ts:2538` 是现役消费者。候选只保留 `ws-in:slash` 一个泛化入口，`ENTRYPOINT_COVERAGE.json` 不含任何 `slash-command:` 登记，`FEATURE_INVENTORY.json` 没有 `/rc`、`/exitrc` 等叶子。
- **违反要求与后果：**R00-T02 要求拆到真实可见子功能并从实际注册追调用；一个 WS 消息类型不能代表不同命令的行为，尤其 `/reset` 删除历史、`/rc` 接管桌面会话、`/confirm` 审批等。Bridge/聊天/会话/后台能力的迁移任务和场景因此漏账。
- **根因与同类路径：**只抽传输层消息种类，未继续展开实际 dispatcher 的静态命令闭集及插件可动态注册的边界。`core/plugin-manager.ts:1029` 注册的插件 slash 命令也需明确是随包闭集还是外装实例。
- **修复要求及重跑：**以 `createSlashSystem` 的真实注册表和两个调用方为起点，登记每个不同效果的核心命令；别名归同一行为，外装动态实例只记录扩展边界。为新增/删除核心命令而功能表未改的负向反例提供失败证据；重跑双向差集及 slash/Bridge 相关针对测试。

### F03 — BLOCKING：注册地址被机械地算成叶子，且用户行为描述失真

- **位置与证据：**`r00_t02_inventory.py:157-180` 仅按同一路径合并 HTTP 方法；`server/routes/mcp.ts:245-259,306-311` 的 `/connectors` 与旧 `/servers` 指向相同 handler，候选仍产生 **12 对**不同 F-ID，例如 `behavior:mcp:/connectors/:id/start` 和 `behavior:mcp:/servers/:id/start`。`server/routes/mcp.ts:381-385` 已明确另一组 `/plugins/mcp` 是 legacy alias，脚本只特殊处理这一组。`server/index.ts:1059` 的 `/api/log` 是前端诊断上报，候选却列为“用户执行记录界面诊断”；`server/index.ts:1027` 的 `/api/health` 和 `server/routes/model-observability.ts:85` 等健康端点也各算用户叶子。全部 **391** 个 route_behavior 的结果是同一模板“界面显示…的内容、状态或操作回执”；例如会话归档、fork、工作区回退均无各自实际可见结果。
- **违反要求与后果：**R00-T02 明确不能以 README 标题或地址数量代替真实叶子，任务要求用户动作和可观察结果；本轮验收还特别要求不能把兼容别名、内部健康接口机械算作用户功能。`566` 叶子数被别名和诊断端点抬高，真正重要的结果/数据效果则被模糊模板隐藏，后续迁移可能只验 HTTP 成功而不验正确行为。
- **根因与同类路径：**以 `(route, path)` 自动生成 F-ID/动作/结果，未按 handler 身份和真实 UI/消费者归并，也没有内部协议入口与用户能力的分类门槛。MCP、健康/日志/回调等具有同类风险。
- **修复要求及重跑：**保留原始注册地址作为入口证据，将同 handler 的兼容别名指向同一个稳定叶子；把健康、日志等内部端点归到所服务的能力/兼容边界而非独立用户功能。逐叶写出真实动作、结果、所有者与存储，抽查归档、fork、审批、MCP、模型观测等跨域链。重生三个机器清单，复核数量与双向差集，并用别名/内部端点反例验证不会重新膨胀叶子。

### F04 — BLOCKING：任务书明确要求保护的可见子功能未拆出

- **位置与证据：**`03_功能与所有权矩阵.md` 的 D06 明列“MOOD 展示与历史”，D23 明列“五语言”；旧种子 `FEATURE_MATRIX.md` F06/F20 同样列出。候选 `FEATURE_INVENTORY.json.features` 搜不到 `MOOD`、`mood`、`locale`、`i18n` 的功能叶子，只有“Agent 设置页”和“界面设置页”等页面级概括。实际 `server/routes/chat.ts:1438-1445` 产生 MOOD 流事件，`desktop/src/react/components/chat/MoodBlock.tsx:20-26` 展示，`desktop/src/react/utils/message-parser.ts:45-63` 负责历史重开；语言切换实际在 `desktop/src/react/settings/tabs/InterfaceTab.tsx:585-600` 保存，支持五个选项。这两组跨层行为未由页面或 `/config` 泛称明确追踪。
- **违反要求与后果：**R00-T02 步骤 2 和 D06/D23 的明确保护行为没有叶子、真实链、独立场景归属；迁移时可能保留页面但丢失 MOOD 历史一致性或语言切换。
- **根因与同类路径：**提取 UI 只识别页面/设置 tab，而不继续枚举页面内的现役用户操作和无独立路由的事件投影；同类风险包括快捷聊天、文件预览、通知等。
- **修复要求及重跑：**以 24 域的“必须保护行为”逐条对照清单，补齐 MOOD 实时/历史、语言切换及同类无独立 URL 的可见功能；给出当前生产调用链、存储、负责人和相关阶段/验收映射，重新运行种子逐项差集、登记双向差集与相应界面/历史测试。

## 独立复核与边界判断

- `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py` 退出 0、`CHECKS_OK`，复得 700 个被脚本选中的入口、566 个保留叶子、24 域及其内部零差集。这是“已抽取集合内部一致”，不是 R00-A03 的全体真实入口证明。
- 内存负例：临时从 `ROUTE_DOMAINS` 去掉 `knowledge`，`extract()` 抛出“新增已挂载 route 未归属”；该负例有效。反向盲点：只在内存给 `desktop/main.cjs` 增加 `wrapIpcHandler("r00-review-probe", ...)`，`extract()` 仍返回 700 且无 `ipc:r00-review-probe`，证明该类遗漏不会被门禁发现。均未改仓库文件。
- `npm test -- tests/experiments-registry.test.ts tests/subagent-tool.test.ts tests/provider-compat/ollama.test.ts tests/knowledge-store-v19-migration.test.ts`：退出 0，4 文件/80 检查通过，无 skipped。补跑 `tests/slash-commands/bridge-commands.test.ts`、`tests/slash-commands/bridge-commands-rc.test.ts`、`desktop/src/react/settings/tabs/__tests__/GeneralTab.test.tsx`：退出 0，3 文件/55 检查通过；这些测试表明遗漏路径确有现役行为，但没有替代缺失的登记验收。
- A04 人工调用图：`core/provider-registry.ts:436,480` 注册 Ollama；`core/agent.ts:936-938,1228-1230` 注入现役三工具，`lib/experiments/registry.ts:122-145` 默认 false；`server/routes/knowledge.ts` 无 research 启动注册，`lib/knowledge/knowledge-store.ts:1901-2068,3692-3697` 与 `server/routes/chat.ts:1955-1996` 留作旧数据/事件兼容；`core/engine.ts:3750-3799` 仅将随包 `plugins/` 交给 PluginManager，旧用户目录只报警不加载；外装 MCP 的实例由用户配置。这些分类合理。`mobile-workbench` 在 `server/index.ts:1022` 真正挂载，R01/R07 的开放/闭集 `DECISION_REQUIRED` 合理，现有路由叶子已有 D19/D20 任务归属，本身没有因此无主；此判断不弥补 F01—F04。
- 完整性：HEAD 等于任务基线；六文件聚合摘要匹配；只有总控账本有已存在的跟踪修改，生产代码和测试实现无本任务改动，暂存为空。T01 快照中的 33 个旧有未跟踪任务书逐文件 SHA 未变；17 个旧任务书仍为删除，旧目录 `git diff --binary` SHA 为 `55789bbdb322dc1ea6e64af683e5820238c5083ed67f23ca5d6d1f1f5876b54b`，与 T01 快照一致。没有 commit/push。

**复审门槛：**修复 F01—F04 后，以新候选摘要重新独立抽取，执行真实组合根与清单双向核对、上述负例和受影响针对测试。当前 R00-A03 必须维持 FAIL，R00-T02 不可标完成。
