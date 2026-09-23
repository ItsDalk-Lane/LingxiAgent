# R00-T02 独立对抗性验收 R3

**VERDICT: FAIL。** R00-A03 **FAIL**；R00-A04 **PASS（静态分类范围）**。R00-T02 需要两项均实质通过，本候选不能标为完成。本报告只审阅清单，不改候选、产品、测试或总控账本。

## 候选与方法

- 分支 `codex/rust-tauri-migration`，Task Base / HEAD `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。审前按六文件相对路径排序，对每项写入“路径 UTF-8 + NUL + 文件 SHA-256 原始 32 字节”重算聚合 SHA-256：`daecf232ae3509a16fa0f55e532fa2e0bd47a91285bfac696f3b07d764798603`，与交接值一致；本报告不计入指纹。
- 阅读了任务书 `01/03/05/R00` 的逐叶字段及 R00-T02/A03/A04、`R09-T05/A09/A10`，R1/R2 审阅、R2 根因、旧 F01—F20 种子和六份候选。反向核查实际组合根、handler、界面消费者与文件/远端副作用。原始登记 832 项、保留叶子 668 项、非生产分类 7 项、24 域及旧 F01—F20 勾稽均能在机器清单中找到；四项内部差集为空。这些计数只是结构证据。
- 对 72 组同地址多方法裁决逐项核对清单登记和源码注册行：65 `SPLIT`、5 `SHARED_READ`、2 `INTERNAL_STEP`；所涉及 160 个原始入口的文件、行、方法和地址均匹配，F-ID 关联无断链。但其 147 项方法效果中 **124 项仍标“方法级静态推断；具体副作用尚须逐项复核”**，这一步不能充当逐 handler 语义验收。

## 场景及旧 finding 裁决

| 项目 | R3 裁决 | 独立依据 |
|---|---|---|
| R00-A03 真实入口均有归属 | **FAIL** | 地址/方法层的零差集成立；多个真实请求体动作仍归同一泛称叶子，且 382 个普通 route 叶子的动作与可见结果明确待复核。见 R3-F01/F02。 |
| R00-A04 撤回与现役不混淆 | **PASS（静态）** | `core/agent.ts:936-938,1228-1230` 的现役三项 subagent 工具、`lib/experiments/registry.ts:122-145` 默认关闭的主动委派、`core/provider-registry.ts:436,480` 的 Ollama、已挂载 `server/routes/knowledge.ts` 均保留。旧 research 表在 `lib/knowledge/knowledge-store.ts:1901-2068`，未发现当前知识路由有研究启动入口；`core/engine.ts:3779` 只装随包插件。硬排除与外装边界分类合理。此结论不替代真实供应商或旧数据演练。 |
| R1-F01 桌面 IPC 漏账 | **PASS（注册层）** | 独立从 preload/main/updater 重算 86 个 invoke = 82+4 个 handler，集合相同；另有 19 个 preload 事件，均见 coverage。共享 IPC 的预览关系另见下项。 |
| R1-F02 核心 slash 漏账 | **PASS（注册层）** | `createSlashSystem` 注册循环及 `bridge-commands.ts` 的 11 个主命令、6 个别名均登记；Bridge/Chat 消费同一 dispatcher。 |
| R1-F03 地址冒充功能、结果模板 | **FAIL（部分修复）** | MCP/Agent/workbench 别名与 health/log 内部端点已归并；但 382 个普通 route 仍以请求方法模板写动作和结果，且漏掉同一 POST/PATCH 内的相反效果。 |
| R1-F04 必保无 URL 行为 | **PASS（已点名范围）** | MOOD 实时/历史、五语言、拖放、通知、可见浏览器、快捷聊天、观测另存以及独立文件预览已有入口和叶子；24 域非空本身仍不能证明其他逐叶语义。 |
| R2-F01 多方法与高副作用 | **FAIL（方法层修复，行为层未闭合）** | 笔记本 GET/PATCH/DELETE、密码 PUT/DELETE、连接器 PUT/DELETE 已分叶；Git push/pull/fetch 与工作区回退开关的当前说明和源码一致。但单方法中请求体可选择不同动作，实际遗漏见 R3-F01。 |
| R2-F02 文件预览 | **PASS（当前清单）** | `ui-behavior:file-preview` 有独立 F-ID，`read-file-snapshot`/监听/文档桥作为共享支持入口，编辑写回仍单列；映射含 R09-T05、R09-A09/A10。源码 `file-preview.ts:110-150` 只读打开，`PreviewEditor.tsx:635-636` 写回；两个结果不同。 |

六项 UI 投影的当前归并中，拖放、通知、浏览器窗口、快捷聊天和观测另存的界面触发与宿主操作可作为同一流程；预览独立叶子合理。`body_effect_decisions` 仅登记了确认卡的批准/拒绝：`server/routes/confirm.ts:18-50` 与 `SettingsConfirmCard.tsx:93-112` 的两个结果确实相反，现已分为两叶。旧 F-ID 映射中，原确认 F-ID 指向这两叶，原文件编辑 F-ID 指向新预览叶；结构关系可追踪。以下同类请求体动作却没有这样处理。

## 阻塞 findings

### R3-F01 — BLOCKING：单一方法中的不同用户结果仍被合成一叶

真实 handler 和界面消费者给出至少六处反例，均在 `ENTRYPOINT_COVERAGE.json.pending_semantics`，而每个地址/方法在 `FEATURE_INVENTORY.json` 仅有一个泛称 F-ID：

| 当前生产链 | 不同实际效果 | 清单现状 |
|---|---|---|
| `server/routes/desk.ts:1062-1259` 的 `POST /desk/cron`；`AutomationPanel.tsx:106-160` 发起调用 | `body.action` 可应用建议、新增、删除、启停、修改任务。删除使任务消失，启停保留任务并改变调度状态。 | 单叶 `F-D18-ROUTE_BEHAVIOR-BEHAVIOR-DESK-DESK-CRON-POST-ED9D52`：“请求执行工作台定时任务”，结果仅“返回该请求的实际响应；写入、外部副作用与界面结果待源码复核”。 |
| `server/routes/desk.ts:1519-1672` 的 `POST /desk/files`；`desktop/src/react/stores/desk-actions.ts:985-1047,1242-1342` | 上传、创建、建目录、改名、移动、批量移动、移入回收。上传绝对路径另有本地主人限制。 | 单叶 `F-D19-ROUTE_BEHAVIOR-BEHAVIOR-DESK-DESK-FILES-POST-FDE2C2`，没有删除/上传范围与结果的逐叶归属。 |
| `server/routes/mobile-workbench.ts:88-117` 的 `POST /workbench/actions`（含兼容地址）；`desk-actions.ts:985-1342`、`remote-file-preview.ts:270-285` | 建目录、写文件、改名、移动、批量移动、安全删除。远端文件保存还有版本结果。 | 单叶 `F-D19-ROUTE_BEHAVIOR-BEHAVIOR-MOBILE-WORKBENCH-WORKBENCH-ACTIONS-POST-8DC0F2`。 |
| `server/routes/sessions.ts:2670-2740` 的 `POST /sessions/workspace-disposal`；`session-actions.ts:1925-1960` | `action=archive` 可恢复归档；`action=delete` 进一步永久删除归档文件和草稿，UI 明确提供二选一。 | 单叶 `F-D02-ROUTE_BEHAVIOR-BEHAVIOR-SESSIONS-SESSIONS-WORKSPACE-DISPOSAL-PO-A1174A`，未区分可恢复与不可恢复后果。 |
| `server/routes/sessions.ts:1206-1250` 的 `PATCH /sessions/authorized-folders`；`SessionStatusCard.tsx:75-100` | `action=add/remove/set` 改变会话可访问目录的范围；现役 UI 发 `add`。 | 单叶 `F-D02-ROUTE_BEHAVIOR-BEHAVIOR-SESSIONS-SESSIONS-AUTHORIZED-FOLDERS-PA-3853EA`，未写授权增加、撤销与整组替换的验收差异。 |
| `server/routes/desk.ts:1489-1513` 的 `POST /desk/jian`；`desk-actions.ts:862-879` | 非空内容写 `jian.md`；空内容调用 `safeDeleteIfExists`，文件被删除。 | 单叶 `F-D19-ROUTE_BEHAVIOR-BEHAVIOR-DESK-DESK-JIAN-POST-DA8CAB`：“请求执行工作台便笺”。 |

这些不是只有地址相同的协议步骤：它们产生不同数据状态、恢复能力或授权范围，且至少前四处有直接界面消费者。R00-T02 要求 F-ID 绑定叶子行为、用户动作、可观察结果及阶段验收；任务书 03 的 D02/D18/D19 也明确要求归档/删除、调度与工作台文件细分。当前 `side_effect_reviews` 的 34 项中，上表六组**均为零项**，因为生成器按路径里的 `delete/rollback/...` 字词选风险，识别不到 `action=remove/safeDelete/delete` 或空内容删除。`body_effect_decisions` 只含 `confirm`，不能补这些缺口。

### R3-F02 — BLOCKING：382 个待核 route 的验收条件不是实际结果

`FEATURE_INVENTORY.json.pending_route_semantics_count=382`，`ENTRYPOINT_COVERAGE.json.pending_semantics` 逐项保留待核列表。生成器 `route_semantics()` 对未声明的 POST 统一写“请求执行…/返回该请求的实际响应；写入、外部副作用与界面结果待源码复核”，`acceptance_requirement` 又直接拼接这两句；这不是 R00-T02 步骤 3 要求的“用户动作、可观察结果”，也无法判断迁移后是创建、删除、归档还是改变权限。R3-F01 证明待核集合中确有真实行为遗漏，因此不能仅把 382 视为可留到实施阶段的文字润色。

任务/场景映射也无法替代逐叶验收。例如上述 cron POST 挂 `R03-T06/R03-A11/A12` 和 `R07-T01/R07-A01/A02`；R03-A11 验子代理不能升级权限，R07-A01 验重启不重复提醒，均不检查新增、编辑、启停或删除定时任务的用户结果。`POST /desk/files` 挂 `R07-A05`（退出频道后私密内容授权）和 `R07-A13`（文件监听撤权），也不检查文件创建、改名、移动、删除或绝对路径上传的结果。旧 F-ID 和域映射存在，并不使这些对应场景变为适用的验收 oracle。

## 门禁反例与针对检查

- `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` 退出 0，报告 832 登记、四项空差集，并成功抓住已知的新增异效果 HTTP 方法、笔记本/密码错合、Git push 旧错误文案、预览错归/丢 R09、确认拒绝丢失、内部 health 独立化、MOOD 投影删除及 R1 的 IPC/slash/别名反例。这些已知回归门禁有效。
- 另做**仅在 Python 进程内**的反例：在 `server/routes/desk.ts` 的 `body.action` switch 加一个 `purge_all` 分支，随后调用当前 `build()`；调用仍成功返回 832 入口、668 叶子，cron POST 仍只有原 F-ID 和泛称结果，没有非零失败。此次替换不写仓库文件。说明现有负例只盯地址/方法及已手列的确认分支，捕不到同一方法新增动作。
- 运行针对测试 `npx vitest run`：路由、桌面文件操作、预览/编辑、Git、知识库、access/MCP、slash、MOOD、i18n、自动化共 **16 文件 / 387 项通过，退出码 0**；A04 的实验、子代理、Ollama、旧知识库兼容共 **4 文件 / 80 项通过，退出码 0**。这些证明当前产品路径存在和旧行为回归，没有验证机器清单已正确命名全部叶子。未进行真实 Git 远端 push、四平台、真实供应商或旧数据迁移。

## 结论与边界

R1 的注册全集漏账和 R2 指出的 72 组多方法、确认卡、Git 远端说明、文件预览归属等具体修补，在本候选里有可复核进展；R00-A04 的静态分类可通过。R00-A03 要求的是**每个现役用户行为有可核对的叶子及结果**。现有 382 个待核 route，尤其上述相反副作用被同一 POST/PATCH 吞掉，使这项条件失败。需要先逐 handler/消费者核实这些动作，按真实效果拆分或明确证明同一效果，再给各叶对应的可观察结果和适用场景；随后重新独立审阅。未证实的 124 项多方法效果和其余待核语义继续保持未证实状态。

审阅后只新增本报告。审前旧任务书 17 项删除的 `git diff --binary` SHA-256 为 `55789bbdb322dc1ea6e64af683e5820238c5083ed67f23ca5d6d1f1f5876b54b`；新任务书 33 项和已有总账修改均未触碰，未暂存、提交或推送。
