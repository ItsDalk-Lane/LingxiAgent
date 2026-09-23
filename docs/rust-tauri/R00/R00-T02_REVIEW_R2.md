# R00-T02 独立对抗性验收 R2

**VERDICT: FAIL**

验收对象：`codex/rust-tauri-migration`；Task Base、当前 HEAD 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。本轮只审 R00-T02。七份审前候选按“相对路径 + NUL + 文件 SHA-256 原始 32 字节”重算，聚合 SHA-256 为 `8447cef051355cdd540e510579de95fd8aba45fe1e2e271434db2e019a535c39`，与交接值相同；本报告不计入该候选。

## REQUIRED 场景

| 场景 | 本轮结论 | 理由 |
|---|---|---|
| R00-A03 真实入口均有归属 | **FAIL** | 832 项原始登记与 598 个候选叶子的内部差集为空，但不同用户行为仍被合并为同一叶子，且若干副作用的动作和结果与生产代码不符。入口有 F-ID 不等于真正的叶子行为已有归属。 |
| R00-A04 撤回与现役不混淆 | **PASS（静态分类）** | Ollama、现役三项子代理工具和默认关闭的主动委派实验保留；旧 research 表、事件只作兼容；独立研究引擎、强制子代理目录、本地模型管理未进入开发清单。未进行真实供应商或旧用户数据验收。 |

R00-T02 的 Steps 2—3 与“逐叶动作、结果、实施及验收归属”尚未实质完成；两项 REQUIRED 必须同时通过，本轮不能放行。

## 对 R1 F01—F04 的逐项复查

- **F01 桌面 IPC：原漏账已修。** 独立从 `desktop/preload.cjs` 的 `ipcRenderer.invoke` 提取 86 个频道，从 `desktop/main.cjs` 的 wrapper 注册提取 82 个、`desktop/auto-updater.cjs` 提取 4 个；集合相同。另有 19 个 preload send/on 事件，均在 coverage 中。抽查开机启动（`GeneralTab.tsx:99,205-225` → preload → `main.cjs:5431-5434`）、可见浏览器（`BrowserViewerApp.tsx:120-237` → preload → `main.cjs:5479` 起）和观测导出（`observability-export-save.ts:69-89` → preload → `main.cjs:5804-5863`）。74 个 invoke 找到 renderer 静态调用，12 个未找到，清单标为“桥已注册、未发现直接调用”；例如 `watch-file`、`debug-open-onboarding` 没有冒充已运行的 UI 测试。IPC 新增未归属内存负例确实失败。D09/D14/D15/D22/D23/D24 的宿主入口已覆盖。
- **F02 核心 slash：原漏账已修。** 从 `createSlashSystem` 的注册循环及 `bridge-commands.ts` 重算 11 个主命令、6 个别名，均在 coverage；`bridge-manager.ts:1139` 和 `chat.ts:2538` 消费同一 dispatcher。新增主命令未归属负例失败。外装插件命令仅登记动态协议边界，未虚构随包命令。
- **F03 地址与真实叶子：仅部分修复，仍为 BLOCKING。** MCP `/servers` 与 `/connectors`、`/settings/enabled` 与 `/enabled`、Agent `ishiki` 与 `agents-md`、移动工作台两套地址均抽查为同一 handler；分叉负例失败。健康、日志、OAuth 回调/轮询保留为内部入口并归所属能力。但相同 URL 下效果不同的 HTTP 方法被合并，其他有副作用路由仍用错误的静态结果描述，见 R2-F01。
- **F04 无独立 URL 的行为：部分修复，仍为 BLOCKING。** MOOD 实时和历史、五语言切换、拖放、通知、可见浏览器、快捷聊天、观测另存均有源码链和稳定 F-ID。文件预览虽被登记为原始入口，却被并入“读取或保存工作区文件”，没有自己的叶子 F-ID，后续桌面展示验收也未挂上，见 R2-F02。24 个域均非空，但这不能代替逐行为核对。

## Findings

### R2-F01 — BLOCKING：不同动作按同一 URL 合成叶子，且副作用结果失真

**位置与证据。** `r00_t02_inventory.py:258-271` 的 `behavior_key` 丢弃 HTTP 方法，`304-350` 对整组只选一个动作/结果；有 DELETE 时优先写“删除”。独立逐项检查 `FEATURE_INVENTORY.json`，发现 **72 个** `route_behavior` 同时含不同 HTTP 方法。并非 72 项都必须拆分（GET/HEAD 可以是同一读取行为），但以下反例明确有不同用户效果：

- `server/routes/knowledge.ts:258,272,288` 对同一 `/knowledge/notebooks/:id` 分别提供查看、改名/修改、删除；候选 `FEATURE_INVENTORY.json:14007-14019` 只有一个 F-ID，动作/结果写成“删除笔记本、从列表消失”，把 GET 和 PATCH 的保护语义抹掉。
- `server/routes/access.ts:198,218` 的同一路径 `PUT /access/account/password` 与 DELETE 分别设置、移除密码；`server/routes/mcp.ts:250-252` 的连接器 PUT/DELETE 分别修改、删除。候选同样统一写“删除或撤销”。同类还有头像 GET/POST/DELETE、知识来源 GET/POST、技能包 GET/POST 等。
- 即使仅有一个方法，`/git/push` 的候选 `FEATURE_INVENTORY.json:13513-13531` 写“用户修改Git推送”“Git推送更新后可再次读取”。真实链为 `GitGraphPanel.tsx:244,279` → `git-env-api.ts:353-362` → `git-environment.ts:433-440` → `server/git/git-command.ts:663-680` 的 `git push` 或 `git push -u`，用户看到推送成功/失败提示；这里存在远端副作用，不能用“更新后可再次读取”表示。`/git/pull` 同样会 fetch 并可能快进本地工作树，不是一般设置修改。

**违反要求与后果。** R00-T02 步骤 2—3 要求真实叶子各有用户动作、可观察结果、当前路径和后续任务/验收；R00-A03 要求每个生产入口有正确叶子归属。此处的原始地址虽有 F-ID，但查看、设置和删除没有可分别追踪的验收对象，远程推送也缺少真实副作用与结果语义。迁移时容易只保留一种方法，或把“HTTP 返回成功”错当业务结果。

**根因与同类路径。** 生成器把 `route + path` 当功能身份，静态结果按方法集合的优先级或末段名称猜测。内部差集只能证明这套自建规则自洽，不能发现合并后的语义丢失。上述 72 项是需要逐项审定的同类集合；尤其有数据删除、权限、远程操作和知识导入的路径应优先。

**修复及重跑要求。** 对不同效果的方法拆成稳定叶子 F-ID；只在确为同一用户行为时合并，并记录理由。按真实 handler、消费者和持久化/远端结果修订有副作用路由的动作与结果，至少覆盖查看/修改/删除、审批、知识导入、Git push/pull 与会话回退。重生三份机器清单、旧 F-ID 映射和双向差集；增加“同路径新增另一种效果的方法不得被旧叶子吞掉”的反例，再跑路由及相关界面回归。

### R2-F02 — BLOCKING：文件预览入口被文件编辑叶子吞并，桌面验收归属缺失

**位置与证据。** `r00_t02_inventory.py:770,794-795` 仅检查 `ui-behavior:file-preview` 在抽取集合中，再经 `UI_PROJECTION_OWNER` 把它附到 `desktop-behavior:file-edit`。候选 `FEATURE_INVENTORY.json:3055-3073` 对预览和 `write-file*`、`run-edit-command` 等编辑操作只给同一个 F-ID `F-D09-DESKTOP_BEHAVIOR-DESKTOP-BEHAVIOR-FILE-EDIT-EE18A5`；用户动作是“读取或保存工作区文件”。真实预览路径在 `desktop/src/react/utils/file-preview.ts:110-150` 打开预览，`preview-file-content.ts:100` 读取快照；`PreviewEditor.tsx:635-636` 的版本写入是另一种动作。原始 `ui-behavior:file-preview` 没有独立 F-ID。该合并叶子只映射 `R04-T04/R06-T05`，没有 `R09-T05` 及 `R09-A09/A10`；R09 任务书 `R09-T05` 第 3—4 条要求在新桌面宿主验证文件、HTML/SVG 等预览和隔离。

**违反要求与后果。** R00-T02 步骤 2—3、03 矩阵 D23 的“预览”和 R1 F04 修复要求均要求无独立 URL 的用户行为成为可追踪叶子，并有实施/验收映射。预览与编辑的成功结果、权限及平台风险不同；把两者并成一项会让新桌面上的预览展示及不可信内容隔离缺乏明确接收者。清单有原始入口或泛称“预览”仍不满足叶子映射。

**根因与同类路径。** `UI_PROJECTION_OWNER` 为消除重复计数而按共享 IPC 归并，不区分同一 IPC 可服务的不同用户动作。应复核其他被归并的 UI 行为，例如快捷聊天、浏览器可见窗口和观测导出，确保只是同一动作的多入口，而非吞并不同结果。

**修复及重跑要求。** 为文件预览建立独立稳定 F-ID、真实调用链、存储、目标负责人及 `R09-T05/A09/A10` 等适用映射；文件编辑继续单列。保留 IPC 原始登记，说明共享 `readFileSnapshot` 如何由不同叶子使用，不靠强制“一入口仅一叶”抹掉功能。重跑生成器、双向关联、预览/编辑组件测试以及桌面宿主映射抽样。

## 独立证据与边界

- 审阅了任务书 01—06、R00 阶段、task-catalog `R00-T02`、acceptance-catalog `R00-A03/A04`、旧 `FEATURE_MATRIX.md`、七份候选和 R1 报告；独立按 24 域及旧 F01—F20 抽样，现役域均有候选条目。`mobile-workbench` 确由 `server/index.ts:1022` 挂载；开放/闭集责任在 `R00-T02-DR01` 标 `DECISION_REQUIRED` 合理，功能未因该决定而被删。
- 运行 `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` 退出 0：脚本报告 832 登记、24 域、内部四组差集为空；IPC 新增未归属、slash 新命令未归属、MCP 别名 handler 分叉三项内存负例均报预期错误。独立正则从真实 preload/main/updater 与 slash 注册表复算 86（82+4）invoke、19 事件和 11 主命令，coverage 对这些集合无差项。MCP 别名及内部端点还直接查了生产注册与对应 F-ID；零差集的结论只限已抽取集合。
- 两轮针对回归实跑均退出 0、无 skipped：首轮 14 文件/248 项，含 slash、MCP、MOOD 实时/历史、i18n、IPC、浏览器、观测导出、Git、知识库、Ollama、子代理；次轮 5 文件/61 项，含文件预览/刷新、PreviewPanel、access 与 MCP。`/git/push` 链只做源码、界面与现有路由测试复核，没有向真实远端推送；这不影响发现清单文字与真实动作矛盾。
- 机器清单的 `source_ref` 所指文件和行号存在，抽查均落在当前 HEAD 源码；产品源码和测试相对 HEAD 无本任务改动，暂存为空。原工作区 17 个旧任务书仍删除，旧目录 `git diff --binary` SHA-256 `55789bbdb322dc1ea6e64af683e5820238c5083ed67f23ca5d6d1f1f5876b54b` 与 T01 快照相同；33 个新任务书逐文件 SHA-256 与 T01 快照一致。总控账本已有修改未触碰。未 commit、push 或改动其他交付。

本轮静态和针对测试证明若干旧 findings 已修、已有行为仍可运行；R2-F01/F02 阻止把 R00-A03 或整个 R00-T02 判为 PASS。
