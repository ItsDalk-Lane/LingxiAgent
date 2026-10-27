# R00-T02 第四轮独立对抗性验收

**VERDICT: FAIL。R00-A03 FAIL；R00-A04 PASS（当前源码的静态分类）。** 本轮只审阅指定候选与生产调用链，不修改候选、产品、测试或总控账本。后续 490 个迁移验收场景尚未执行，属于 R00-T07 接收与各实施任务执行范围；本结论针对 R00-T02 的清单和映射质量。

## 候选身份与独立核查

- 分支 `codex/rust-tauri-migration`；HEAD/Task Base 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。指定九文件按文件名排序，每行 `文件SHA256␠␠文件名\n` 求得聚合 SHA-256 `9691faa5591e0f13607c328ce5a4107fd43ac6d5411702626aac5fb91e3e9876`，与冻结值相同。
- 已读本地 `AGENTS.md`、新任务书 01/03/05/R00 的 T02/A03/A04/T07/A14、R1—R3 独立报告、R2/R3 根因及九份候选。审前工作区已有旧任务书删除、新任务书未跟踪、总控账本修改及候选文件，均未触碰。
- 独立用 TypeScript AST 从已登记的生产 route 源文件反抽：清单 468 条 HTTP 登记，AST 直接找到 464 条；其余 `/ws`、`/task/:taskId/abort` 分别由 `wsRoute`、`restRoute` 注册，两个 `mobile-static` 客户端入口由 helper 注册，均已有单独登记。未发现 AST 中有清单未列的直接注册。此项独立于生成器的正向抽取。
- 结构复算：832 条原始登记、730 个保留 F-ID、24 域、7 个非生产分类；四项登记差集均空。旧 F01—F20 的域勾稽齐全，986 条旧 F-ID→现行 F-ID 关系没有悬空新 ID 或入口。桌面 86 个 invoke/19 个事件、核心 slash 11 个命令/6 个别名、兼容地址及内部端点的修复保留。共享原始入口 `POST /desk/cron`、`POST /desk/files`、`DELETE /agents/:id` 和确认 POST 均明确指向多叶，未再强制一入口一叶。
- 72 组同地址多方法有 147 个方法效果记录：65 组拆分、5 组 GET/HEAD 同读、2 组 OAuth 内部步骤。逐方法记录有动作、结果、源码位置和 F-ID，原 124 项“静态推断待核”文字已移除。382 个原待核项在 G1/G2/G3 分别为 128/127/127，ID 无交叉或缺口；30 个同方法拆分项使这 382 项成为 444 个叶，加 46 个单独声明的 route 叶，共 490 个 HTTP 叶。抽查 cron、文件/远端工作台、便笺空值、会话授权与处置、助手连带技能、Git unstash、媒体 capability、配置部分写入的源码与叶子，当前列出的动作方向及主要副作用相符。以上是登记及针对性语义证据，不把每条文件摘要自检当作 382 个效果的独立运行证明。
- 490/490 HTTP 叶都有唯一子场景、执行 Task、负责人和 R00-T07 正式化交接，状态均为 `SPECIFIED_NOT_EXECUTED`。旧 A-ID 关系中 520 条 `INDIRECT`、1492 条 `GAP`、0 条 `DIRECT`，没有把阶段横向场景冒充逐叶验收。44 个“外部 HTTP 已挂载但未找到内置直接调用”及 6 个已知产品表现限制被列为限制；例如助手删除的 `skillsSkipped` 未在现有弹窗显示、配置混合写入可能部分生效，清单没有宣称自动回滚。预览保持独立叶，接 R09-T05/A09/A10；`mobile-workbench` 现役保留、最终开放/闭集归属仍为 `DECISION_REQUIRED`。

## 阻断发现

### R4-F01 — 240 个非 HTTP 保留叶未得到同等的结果与验收映射

`FEATURE_STAGE_ACCEPTANCE.json.supplemental_scenarios` 只为 490 个 `route_behavior` 建子场景；另外 **240 个保留叶**仅挂阶段任务的旧 A-ID，其中有具体生产动作被泛称结果遮盖。这不是单纯要求“240 个场景必须另建”的计数问题；以下已经能从现役实现和旧场景复核实际遗漏：

| 现役行为与源码 | 当前叶子及映射 | 缺少的可核结果 |
|---|---|---|
| `lib/tools/ask-user-tool.ts:194-270` 和 `core/agent.ts:726-733`：`ask_user` 弹出结构化提问卡，按用户作答、超时推荐、暂不回答分别回传。现有 `tests/ask-user-tool.test.ts:134-224` 也区分这些结果。 | `F-D01-TOOL-TOOL-ASK-USER-94CB74` 只写“工具结果或产物进入会话”；挂 R03-A01/A02（Run 终态）、R06-A07/A08（MOOD/历史）、R08-A13/A14（固定产品集/入口回归），无对应子场景。 | 提问卡可见、答案映射、超时/拒答的真实结果及不编造答案的断言。 |
| `plugins/beautify/tools/create-cover.ts:6-90`：`create-cover` **只把已有图片应用到 Markdown**，不生成图片；目标不是 Markdown 或路径无效时返回错误文本。`tests/beautify-create-cover-tool.test.ts:18` 保护“不调用生成服务”。 | `F-D16-BUILTIN_PLUGIN_TOOL-BUILTIN-PLUGIN-TOOL-BEAUTIFY-CREATE-COVER-F5E8EA` 只写“看到执行结果或产物”；R07-A09 测的是文档导出 PDF，R07-A10 测的是危险归档拒绝，R04-A13/A14 测重试和 worker 越权；无本工具子场景。 | 已有图片写入指定 Markdown、目标文件真实变化、无图片生成、错误时文件不变的结果。 |
| `server/routes/confirm.ts:18-54` 的 `action=rejected` 与 `core/slash-commands/bridge-commands.ts:157-164` 的 `/reject` 都把待批动作拒绝并避免执行。 | `F-D04-BODY_EFFECT-CONFIRM-REJECTED-F97BFB`、`F-D04-SLASH_COMMAND-SLASH-COMMAND-REJECT-CD1524` 均无子场景。前者仅挂 R04-A05/A06：A05 验“批准文件 A 不可扩大到 B”，A06 验“禁用工具后批准旧请求仍拒绝”，两者不操作用户主动拒绝。 | 确认卡拒绝/Bridge `/reject` 的状态、流事件、待执行动作未运行，以及拒绝后的再次提交边界。 |

R00-T02 步骤 3 要求每个叶子记录可观察结果和验收场景；R00-T07 才负责正式可执行账本和运行结果。因此未来执行未开始不是失败理由，**上述叶子当前的具体结果与适用场景尚未写进本阶段交付**才是 A03 阻断。R3-F02 对 HTTP 382 项的“待复核模板”已关闭，但同类问题仍在非 HTTP 行为中。

### R4-F02 — 确认接口的同方法新动作可绕过分支复审门禁

`r00_t02_inventory.py:430-468` 只给 382 个普通 route 审查记录核 handler/关联文件摘要；`POST /confirm/:confirmId` 被特殊排除。确认接口在 `:1150-1173` 仅检查源码仍含 `['confirmed', 'rejected'].includes(action)` 的原语句，并按固定两项 `CONFIRM_EFFECTS` 生成叶子；`server/routes/confirm.ts` 不在任何 G1/G2/G3 的已绑定源码摘要中。

我只在 Python 进程内把第三种 `action=deferred` 的处理插入 `server/routes/confirm.ts` 原有校验之前，保留原批准/拒绝语句，调用 `build()` 后仍返回 **730 叶、确认接口仍只有批准/拒绝两叶，未报错**；仓库文件没有更改。作为对照，独立在内存中改变 cron 动作、`mount-aware-file-service` helper 和媒体 capability，`validate_semantic_reviews(extract())` 均非零报“签名变化”。所以六个已列反例只能证明已绑定的普通路由会被提醒，不能证明所有同方法新增行为都被发现。R3 根因要求“新增分支但无审查决定/行为映射/适用断言”失败；确认接口是现成反例。

## A03/A04 与前轮阻断裁决

| 项目 | 本轮 | 依据 |
|---|---|---|
| R00-A03 真实入口均有归属 | **FAIL** | 832 登记的双向结构差集为空，HTTP 490 叶已有可交 R00-T07 的场景；但 R4-F01 的现役非 HTTP 叶没有具体结果/适用验收，R4-F02 的确认新分支未受发现门禁约束。不能以路由集合完整代替行为集合完整。 |
| R00-A04 撤回与现役不混淆 | **PASS（静态分类）** | `core/agent.ts:936-938` 三个现役 subagent 工具、`lib/experiments/registry.ts:122-145` 默认关闭实验、`core/provider-registry.ts:436,480` Ollama 均保留；`server/routes/knowledge.ts` 仍挂现役知识库，旧 research 表/会话种类仅兼容；独立研究运行、强制子代理目录、本地模型管理不在保留叶，`core/engine.ts:3779` 只装随包插件。未据此声称真实供应商或旧数据演练通过。 |
| R1-F01/F02/F04、R2-F02 | **PASS（点名范围）** | IPC、slash、MOOD/五语言等入口登记及文件预览独立归属仍在。 |
| R1-F03、R2-F01、R3-F01 | **PASS（已点名的 HTTP 行为）** | 别名/内部端点不再冒充用户功能；72 组多方法与六组 cron/files/jian/授权/处置等同方法效果已拆叶，Git/media/助手删除等抽查有结果及边界。 |
| R3-F02 | **HTTP 部分 PASS，整体 FAIL** | 382/382 旧待核项均有审查记录及 490 个 HTTP 子场景；R4-F01 显示逐叶验收规则未覆盖所有保留功能。 |

## 已运行检查与界限

- 独立 AST 入口差集、机器清单交叉引用与数量复算；在内存中运行三个可检出分支反例及一个确认接口漏检反例，未写生产文件。
- `npx vitest run tests/confirm-route.test.ts tests/ask-user-tool.test.ts tests/beautify-create-cover-tool.test.ts tests/slash-commands/bridge-commands.test.ts`：4 文件、39 项通过，退出码 0。这证实当前产品对应路径可运行，不能替代迁移后场景的执行证明。
- 未运行四平台安装包、真实供应商、真实远端操作、旧用户数据迁移或未来 490 个迁移场景；它们不计作本轮已通过。当前 R00-T02 候选不能标完成，也不能据此更新总控账本为接受。
