# Upstream 0.444.1 同步审计

## 审计坐标

| 角色 | 提交 | 说明 |
| --- | --- | --- |
| A：同步前 Lingxi | `a5d1e5415c28b55074ba9ae81a6429d57ff5a934` | 已包含模型设置、Ollama、审批与语义槽位等下游改造 |
| B：上游目标 | `cc19cb49b0786d61ed723764e0a83baf87887270` | `upstream` remote 的 `v0.444.1` |
| C：同步结果 | `97595264ead8735a04559507ddaade25db8a4e15` | 按文件三方合并后的提交 |
| 当前 main | `b5d009d8b0ae1ab8b7df31edb0a102c98ad51b02` | C 之后的 `0.1.3` 发布准备 |

`upstream` remote 实测为 `https://github.com/liliMozi/openhanako.git`。A 与 B 没有共同 Git 祖先，不能用一次普通 merge 是否存在来判断同步完成度；本次使用 B 的真实标签内容、B 前一标签到 B 的 52 个变更路径、C 的文件内容和运行测试做对账。

## 三方结果

| 领域 | 上游变化 | Lingxi 原修改 | 当前状态 | 是否完整 | 证据 | 修复 |
| --- | --- | --- | --- | --- | --- | --- |
| 轻量本地压缩 | 新增本地快速压缩、会话调用与实验开关 | 保留 Lingxi 会话编排、网络请求和品牌边界 | 上游新增核心文件与共享模式逐字节进入 C，合并点测试通过 | 是 | `core/lossy-local-compaction.ts`、`shared/compaction-mode.ts`；`lossy-local-compaction`、`session-compactor`、`experiments-*`、ContextRing 测试通过 | 无 |
| 内部情绪块 | 新增共享解析器并接入消息格式化 | 默认人格必须仍是 Lingxi，不回退旧品牌 | 共享解析器逐字节进入 C，前端两个解析入口均接入 | 是 | `shared/internal-mood-block.ts`、`desktop/src/react/utils/{format,message-parser}.ts`；`mood-parser` 测试 12/12 | 无 |
| 可见文本累计 | 工具边界后仍正确累计用户可见文本 | Lingxi 桥接、桌面提交与会话路由都有自己的消费路径 | 累计器进入当前树，四类消费入口均接入 | 是 | `lib/bridge/visible-text-accumulator.ts`；`visible-text-accumulator` 7/7、`bridge-handle-message` 54/54 | 无 |
| 输入区与文件提及 | 输入区文件提及与状态细化 | 保留 Lingxi 请求封装和现有输入行为 | 当前文件为合并版本，新增回归测试存在 | 是 | `desktop/src/react/components/InputArea.tsx`、`InputArea.file-mention.test.tsx`；同步提交文件差异与全量后续验证 | 无 |
| 会话列表与消息事件 | 流式状态点、事件与格式改进 | 保留 Lingxi 会话状态和桌面事件处理 | 多个低冲突模块与 B 逐字节一致，其余合并测试通过 | 是 | `core/events.ts`、`server/ws-protocol.ts`、`websocket.ts`、`ws-message-handler.ts`；相关同步测试通过 | 无 |
| 实验、用量、通知 | 实验开关、用量与通知细化 | 保留 Lingxi 设置页与模型体系 | 核心实验、用量和通知模块进入 C，定向测试通过 | 是 | `lib/experiments/registry.ts`、`lib/llm/usage-observer.ts`、`lib/notifications/notification-service.ts`；实验与用量测试 | 无 |
| pi 0.84.1 适配 | OAuth 刷新签名、存储接口与包内容变化 | 保留 SDK 隔离层、下游认证清理与流式守卫 | 声明、锁文件、安装目录均解析到 0.84.1；OAuth 取消与认证清理测试通过 | 是 | `lib/auth/xai-oauth.ts`、`core/model-manager.ts`、`scripts/patch-pi-sdk.cjs`；`xai-oauth` 15/15、`model-manager-auth-storage` 29/29、`pi-sdk-import-boundary` 3/3 | 无 |
| 生成边界 | 新模块需要进入开放导出、运行时闭包和持久化指纹 | Lingxi 有自己的开放/闭合集合及数据纪元约束 | 生成文件已更新；数据版本未被无依据抬升 | 是 | `export-manifest.json`、`build/cli-runtime-closure.json`、`build/persistence-schema-fingerprint.json`；边界与迁移测试 | 无 |
| 下游模型与审批 | 上游同步触及相邻的会话、设置和模型调用区域 | 六类语义槽位、独立 approval、Gateway、Provider/Ollama 必须保留 | 当前跨层入口存在，36 文件定向矩阵 661 项全绿；发布包也包含对应标记 | 是 | 见 `FUNCTIONAL_REGRESSION_AUDIT.md` 和 packaged-state 抽检 | 无 |
| release digest | 上游标签线有自己的摘要变化 | Lingxi 产品发布摘要不能被上游文件直接覆盖 | C 保留 A 的两份摘要，未照搬 B | 是，属有意差异 | A=C、B≠C 的对象比较；同步提交说明明确本任务不代发 Release | 无 |

## 路径级结论

- `git diff --name-status v0.443.54..v0.444.1`：52 个变更路径。
- 对 A/B/C 逐对象重跑后，这 52 个路径在 C 中全部存在。
- 不能要求所有文件 B=C：Lingxi 是下游产品，输入区、设置、消息解析、服务端聊天、生成清单等文件必须保留下游差异。
- 对 B≠C 的高风险区域，已用当前源码链路和 Node 24 定向测试验证；没有发现“整段采用上游导致下游功能静默丢失”或“文件移动后下游补丁未迁移”的证据。

## 结论

`upstreamVersion=0.444.1` 不作为完成证明。基于标签路径、三方文件内容、跨层源码和 661 项定向测试，本轮未发现 0.444.1 行为遗漏，也未发现 Lingxi 模型、辅助模型、审批、Provider/Ollama、skills 或持久化行为被同步覆盖。后续 Artifact 修复不会重做上游同步。
