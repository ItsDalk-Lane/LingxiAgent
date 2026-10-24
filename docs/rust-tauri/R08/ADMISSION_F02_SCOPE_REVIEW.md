# R07 准入 F02 同因范围独立审查

结论：**F02 仍为 FAIL / 待修复，不能沿用旧的逐叶 PASS 放行完整 R08 业务依赖。** 本报告查到的是“证据没有证明所绑定的要求”，不是把全部涉及功能判定为未实现。用户已允许 F01/F02 的最小前置修复；本调查只列必要修复边界，不更改原账、业务实现或旧回执。

审查者：独立代理 `admission_evidence_review`，承接原 `r07_admission_review` 的范围调查；未参与业务实现。日期：2026-10-21。提交基点仍为 `86840d1700397d3dd620a41f2b884df4cddd5cc2`。当前工作区有并行实现改动，本报告以摘要绑定的审查输入为准，不把提交号当作全部受测候选。初次准入反例与锁定工具链实测见 `R07_ADMISSION_REVIEW.md`，本轮是源代码/原断言的独立调查，**没有重跑业务测试，没有声称修复通过**。

## 全量范围与证据层次

已逐条重新读取 R00 的 `FEATURE_STAGE_ACCEPTANCE.json`、R07 的 `R07_LEAF_BINDINGS.json`，重建全部 363 条 `full_original_behavior` 叶的原断言、阶段、任务与逐条案例绑定，和配套范围 JSON 比对：零差异。其余 45 条既有 `stage_share_satisfied` 不在本次 full-original 同因集合内；既有阶段责任不变。

| 检查集合 | 数量 | 结论边界 |
|---|---:|---|
| R07 已登记补充叶 | 408 | 363 全原行为 + 45 阶段份额 |
| full-original 使用 `r07_ui_vitest` | 40 | 38 仅该套件、2 混合；需要区分真实客户端断言、旧工具单测和实际 Rust 接线 |
| full-original 把 A23 当某条业务原断言的证据 | 102 | 共 102 条原断言；探针没有执行该业务方法/输入/状态检查 |
| 客户端叶仅绑 Rust 服务案例（上述 UI 集合外） | 1 | 技能面板 `0DDB5AA05072`，不能由服务查询证明 toast/取消/重载等客户端结果 |
| 条件成功候选 | 7 | 不是全部业务失败；逐项判断见下文 |
| 另外确认的错端点 | 4 | 两项 apply 建议、两项全局供应商保存/删除 |
| 上述集合去重 | 149 | 同因复核范围；集合交叉，不能把各组简单相加 |
| 其他 full-original | 214 | 仅完成结构重建，**未获本轮语义通过认证** |

完整 363 条记录和每条原文、现绑定、阶段及判断见 `ADMISSION_F02_SCOPE_REVIEW.json`。102 条 A23 替代集合并非抽样推算：脚本从实际案例引用逐条重算；40 条 UI 集合同理。所有原文/绑定/阶段及审查源摘要的独立重建结果在 `artifacts/rust-tauri/R08/r07-f02-scope-review-01/verification.json`，状态仅为 `PASS_SOURCE_RECONSTRUCTION_ONLY`，业务验收为 `NOT_EXECUTED`。

## 根因与不能沿用的证据

`r07_apply_leaf_bindings.py` 核对了组数、非空、叶内测试名不重复、套件存在，但没有验证测试实际操作与原断言是否相同。`full_original_behavior` 的生成还会删除原有 `stageShare/laterShare`。因此，案例名称存在、套件退出 0 和数量齐全，都不足以判断全原行为已被证明。

一个真实测试可以同时证明原文中的几个结果；反过来，一条复合原断言也可能必须由多条测试共同证明。当前“叶内案例不得重复”的机械规则，不能成为把某条断言强行绑定不相关测试的理由。修复实现时若调整这一检查，应独审其覆盖语义：仍须逐原断言列证据，禁止空组、漏条、零匹配、空日志与关键跳过，保留原要求和历史回执。

### A23 的 102 条替代

`r07_t12_a23_runtime_route_matrix` 源码第 197—270 行实际行为是：持本地凭证发送 TRACE，若 404 就记路由空洞；对受守路由再发送无凭证 GET，要求 401；公共 GET 与 token URL 直接跳过该 GET 检查。它证明路由/鉴权外围有接线，**没有执行业务 POST/PUT/DELETE、没有业务请求体、没有查持久状态、没有测字段缺省/冲突/授权身份组合**。对公共路由甚至没有无凭证 GET 结果。

这不是要删掉 A23；应保留它作为路由/鉴权附加证据，并给下列原断言增加同端点、同输入和实际结果的具名测试。102 条分布为 D04=2、D10=8、D12=14、D16=1、D17=9、D18=1、D19=13、D20=18、D21=26、D22=10；配套 JSON 精确保留每条 ID 和原文。不能仅把所有条款改写成“无 token 401”。

已核实的例子：

| 原叶后缀 | 原要求 | 现证据的缺口 | 最小补证 |
|---|---|---|---|
| `09D567E99524` / `D5514BBDBF88` | 回执/富流式字段缺省保持原值、拒绝不变 | TRACE/GET 不执行 toggle | 复用 `bridge_settings_receipt_and_rich_streaming_toggles_persist` 中真实缺省/无关字段/非法模式读回，按条款重绑；缺身份分支再补 |
| `F8935B6B0221` | 无效 level / 409 不改变默认、目标不变 | 无请求体和会话状态 | 对现有会话分别触发非法 level、真实冲突与拒绝，比较会话和默认值 |
| `066B54983F6A` | 有效/无效 cookie 的净化 principal、无 secret 泄漏 | 公共 GET 被跳过 | 真实 cookie 与连接类型组合，检查响应字段、身份及状态 |
| `0F7D128E01B1` | 不存在的 Agent / 安装失败不得误启其他 Agent | 只查无 token GET | 安装失败与未知 Agent 后读回目标/其他 Agent 与运行时列表 |
| `25C4FF66FEE5` / `8D3CEB6133E1` | 权限默认读取/保存及拒绝副作用 | PUT 也被称为“只读面401” | 当前真实 GET/PUT/非法模式400后不变已在 `session_permission_default_read_and_write` 内，按真实断言重绑并补缺省 ASK / 必需身份边界 |

### 40 条 UI-Vitest 与额外客户端叶

前端 HTTP 替身可证明真实组件的显示、点击与错误状态；它不能证明 Rust 处理器已经执行。旧 TS 工具适配器/伪造 manager/固定 execFile 结果，也不能证明真实 Rust 工具面。需要把这些层次分开陈述，按原条款保留真实有用的测试，再接对应真实服务证据；不能把全部客户端测试一概作废。

五工具与 learn_lesson 的明确错绑：

| 叶后缀 | 已确认不对应的绑定 | 必须证明的行为组 |
|---|---|---|
| `0818574ABD43` run_tools | 子调用独立授权绑定外层静态 permission | 实际多子调用逐项授权/拒绝、超时/未知工具、已执行副作用和沙盒；与 F01 真实注册修复一并核 |
| `196D50D8DD6E` ast_grep | 真实结构匹配绑定固定 execFile JSON 的分页 | 真实结构解析、同文字符串不误中、范围/错误/无匹配不写 |
| `2D896560381E` lsp | 不可写只预览绑定 applied=true 实际写回 | 真实 LSP 查找/rename、不可写预览不落盘、范围与异常；允许外部语言服务器协议替身，不假核心执行 |
| `C88F29B5114A` run_code | 重启清变量绑缺语言；取消绑假 manager 死亡 | 同会话变量保持、重启变量消失、实际取消清理进程和状态、语言缺失诚实错误 |
| `CD5D7FC02D8E` security_scan | 无越权读绑非 Git 提示；上限/引擎失败绑静态 permission | 实际扫描范围/拒绝、截断和失败不冒充全量，以及真实效果/记录 |
| `771296A194E0` learn_lesson | 未确认不写绑已确认写；索引/Agent一致绑同名拒绝 | **Rust 已有实现与真实测试，修 R07 证据即可，不重开 R06** |

其余 UI/CLI 同因项中，须按操作核对的行为组如下。每条原文和原绑定保留在 JSON；这里汇总同类修复，避免逐例反复返工。

| 行为组与相关叶后缀 | 当前不足 | 最小修复方向 |
|---|---|---|
| 访问设置 `000E6E1301C0` | 撤销只查 POST，返回仍是旧 summary；空态/错误/刷新失败未齐 | 成功删除条目、失败保留、后置重载失败提示；生成/远端连接/账号分别按原文核 |
| 自动化 `FA0FF4DC0779` | 5 条绑定频道或 phone 动作 | 真 AutomationPanel 读取/新增/启停/删除/update、失败和取消；已有组件错误测试可复用 |
| 活动 `E6F239636060` | 详情绑 DM；promote 绑加成员 | ActivityPanel 展开/加载/错误、升格点击与状态转移、心跳；真实服务升格测试不能代领点击 |
| 技能设置 `99F8CD228733`、技能面板 `0DDB5AA05072` | 11 条顺序错位：安装绑 delete、删除绑 create、查看器绑 CSS；面板全绑服务探针 | 原组件的安装/取消/重载/启停/目录/能力/预算偏好/查看器及错误，服务用对应真实技能测试配对 |
| 宿主入口 `52A5D1660088` / `9CA8A47BE208` / `0F28429F329E` / `CECACC73463E` | 技能目录/查看器绑通用资源窗；超大块/外来exportId绑桥缺方法；工作区失效绑viewer监听 | 核实际目标组件和宿主能力；保留确有对应的旧IPC兼容断言；真实文件/句柄/拒绝边界按原文 |
| 设置初始化 `32FFEC05BAA7` / `D2657E4AB5FF` / `D1BEE19A95BB` | 冷缓存/15秒/错误或实际端口/token全链不足 | 原客户端初始化确有有效测试，逐分支配对；不把已有设置控件全部判缺失 |
| 工作 `E3A8057D7156` | 初始化/助手切换/模式/心跳/选目录/保存后GET分支不齐 | 复用 WorkTab 成功与后置刷新失败测试；补操作与实际状态边界 |
| 观测/用量 `2045462C0D98` / `1850FE23DD5D` | 健康、记录开关、分页、payload、保留删除等绑子Tab/字符串/文件名 | 各真实组件动作/列表/详情/错误与服务台账结果配对 |
| 模型/供应商 `F92121640419` / `D50161CE274A` | ModelsTab 绑 ModelWidget/Usage布局；OAuth/退出/reveal绑定无关操作 | 按原界面操作重新对应测试，不能由结构或样式通过代替保存、授权/错误状态 |
| Bridge `DF1F77F2D776` / `7AD1B00D8158` / 历史 `64A517AF731B` | 初始化/二维码/owner/重置等串位；历史拒绝只接受预设500 | 同端点与真实组件，区分既有错误改善裁决和原要求，拒绝真实越权历史 |
| 频道 `51E2C5A963ED` | 成功 store 动作未齐确认/取消/失败/刷新/草稿/缓存/已读 | 实际组件操作和持久结果，保留现有真实成功动作 |
| 媒体 `22A8DCD792BE` / `125C9E21A621` / `7747A6F59130` | 系统语音边界绑Usage；旧即梦适配器假提交不能证Rust交付 | 复用真实 Rust speech / media / 监管CLI产物链，协议供应商可替身，补对应UI及必需拒绝 |
| CLI `1B09760C2B1C` / `200D4E5D52C9` / `3B86332B042C` / `B8A1AD32A8E1` / `D3710D637C19` / `EEC1A2A5CD04` | Node环境/abort reducer/parse参数/缺模型故意失败代替Rust实际前台、恢复、help入口、聊天和status成功 | 真实CLI入口、stdout/退出码/进程/模型Agent成功夹具；保留有用reducer与拒绝测试，接真实Rust链 |
| data CLI `00E57BB5F32B` / `0BE8B22DA5D5` / `1388F8EE0EBC` | 日期没断言、假invalid目录、正常只读代替corrupt；restore只绑错token | 合绑现有缺token/无checkpoint/TTY/正确恢复，补输出与损坏原状态；R08真实Rust全库往返仍由原阶段份额承担 |
| bundle CLI `289A4033D656` / `AA65EF26F65E` | 假download真/假代替签名；current/next/错误/不得联网写指针不齐 | R07 CLI契约与原定R10/R11真实产物分清，不能改口全原行为已过 |

## 条件成功的七项，不能一刀切

- 桌面凭证 `093F22C4FF63`、手机凭证 `747E0ADC941B`：测试允许 400/403 即通过，未强制证明成功登记/一次性 secret/地址。必须保留非安全传输拒绝测试，另加符合现有传输和主人授权规则的成功夹具，不能放松权限取绿。
- QR `5E4A9CF58BED`：原文确实允许 LAN 地址不可用 400；问题是签发失败时完全没有调用 QR。需强制执行该端点，核相应错误/地址，具备地址时验证 SVG。
- 批准配对 `5E1C3363A070`：现测试确实强制批准 200 并查设备/撤销；不是整个正向缺失。但没有检查 secret 一次性、作用域、安全审计，拒绝组还错绑另一个签发端点。
- `/mobile` `3291CFD5F7E2`、`/desktop` `8BC1A036AFAA`：原文允许无产物指引/错误，不把 400/404 本身判失败。现测试只核状态，没有正文/资源/错误内容，A23 又对公共 GET 跳过；按实际有/无构建夹具补证，不强迫所有环境都必须 200。
- 频道删除 `DD087FDD3244`：创建若 400 就不执行 DELETE 仍通过。需要真实两个有效助手使正向强制执行，核删除/重复404/状态；另保留成员不足的400。

## 四项错端点与现成真实证据

`095E0402EEEC`（cron apply suggestion）、`5F394AA4C9A3`（slash /apply）绑了普通 cron 修改/删除或取消自动审查；测试根本没创建/应用 suggestion。真实服务已有 `r07_automation_flow::real_tool_suggestion_requires_apply_and_replays_after_expiry` 和 `::slash_apply_uses_frozen_owned_suggestion`，可直接纠正成功证据；前者还验证已应用建议重放同 ID、任务数仍 1。**已应用后重放不等于未应用即过期拒绝**，原要求中的找不到建议/冲突/Studio身份缺失/重复防护须逐项补齐或找到真正对应测试。

`9FBBC275BD34` / `DBBDA23AB24F` 原要求是 `PUT /api/config` 供应商保存/删除、目录/插件/焦点配置/派生投影/部分成功读回。当前正向却绑定 `rejected_agent_provider_and_yuan_patches_do_not_save_global_values_first`，该测试实际对 `PUT /agents/a/config` 发无效 patch，并期待 400。全局路由和成功操作都不同。`r07_t08_provider_http` 的模型编辑/删除与投影失败测试是有用现成证据，但也不是该全局操作；不能只换成一个名称相近的模型测试。必须对原全局操作核目录/插件/偏好及焦点配置、部分成功真实状态；不要求发明原文没有的原子回滚。

## 现成测试的复用边界

配套 JSON 的 `reusableCandidates` 保存源文件、精确测试名、已核断言与未证明边界。重点：

- `r06_skills_completion::original_lesson_review_one_shot_content_binding_conflict_and_real_retrieval`：核到真实 SKILL.md、风险未确认不写、token正文绑定/一次性、经验库与Agent启用、其他Agent不变、同名/缺上下文/写失败。`memorytools.rs` 有真实 `learn_lesson` manifest 和分派。**修证据，不重做 R06。** 本轮只核源码，重新绑定后仍须按新候选运行。
- `r07_automation_activity::completed_activity_promotes_the_same_real_session_without_new_messages_or_runs` 和 `::running_activity_uses_actual_state_and_cannot_be_promoted_while_busy`：真实存储/服务升格、同一会话、无新消息Run、幂等/重启、运行中409已有；ActivityPanel 点击/缓存/提示须另外验。
- `AutomationPanel.test.tsx` 的 `shows the structured POST error and keeps the panel usable`、GET错误保留badge、blocked导入；`ActivityPanel.test.tsx` 的展开正文与列表错误保留；`SkillsPanel.test.tsx` 的安装/Agent开关等，可按同一组件条款重绑。它们 HTTP 为替身，不冒充实际 Rust 结果；SkillsPanel选Agent安装不能替代技能设置“全局安装不自动启用”。
- `WorkTab.test.tsx` 的 `keeps the optimistic desk value and the saved toast when the settings refresh fails` 是实际客户端后置失败结果；不得再把“保存失败提示”一律解释为未持久化。
- `r07_t05_media::r07_media::jimeng_image_and_video_use_real_supervised_cli_and_current_sdk_with_owned_delivery` 是 Rust监管CLI/当前SDK/产物归属；外部CLI是合成协议替身，不能宣称真实供应商已验。

以上“可复用”只表示源码链路和断言对应，不授予新候选的运行 PASS；必须保留/重新生产具名日志、运行退出码与摘要。

## 阶段责任：只沿原账分份

363 条 full-original 中只有以下 **7 条**原阶段集合含 R07 之后阶段。这是允许讨论分份的必要条件，**不是自动改份额许可，更不是把R07缺证挪走**。

| 叶后缀 | 原阶段/任务 | 分份边界 |
|---|---|---|
| `00E57BB5F32B` / `0BE8B22DA5D5` / `1388F8EE0EBC` | R07-T09、R08-T06、R10-T04 | R07 CLI契约与拒绝要闭合；R08真实数据回滚前滚和R10原份额继续，不能称单JSON测试已证明全库 |
| `289A4033D656` / `AA65EF26F65E` | R07-T09、R10-T07、R11-T04 | R07 CLI入口/错误/只读；真实签名包与后续发布责任按原账，不拉入R08 |
| `25C4FF66FEE5` / `8D3CEB6133E1` | 原阶段 R04/R07/R08；原 execution_task_ids 仅R04-T03/T06 | 原账阶段与任务字段存在不同粒度，原样保留；R07真实偏好服务不能挪给R08，R08客户端/兼容责任仍在126叶内 |

多数错绑客户端叶原阶段到 R07 为止，没有 R08 份额，必须在已授权 F02 最小修复中纠正，不能为了让 R07 绿而新增 R08/R09 分担。既有 45 条阶段份额、R07交来的27条R08后续份额及R08总126叶不因本调查改变或重复计数。不恢复退役浏览器宿主，不拉回仅R09的窗口份额，不重开已接受R06。

## 修复完成后独审的必要材料

按域一次整理“原断言→原阶段责任→实际入口→执行/真实存储→用户结果→具名测试”，先复用确有对应的既存实现/测试，只补缺少行为。F01五工具注册与执行修复可同时承接其错绑证据；F02其余组主要是逐行为重绑/补实测，不能以新报告替代行为。

冻结最终候选后，独审需要核实际链路、每条原断言的覆盖与缺口、案例确有运行/非零匹配/非空证据；对复用案例的覆盖调整核原要求，保留原失败。没有独立审查通过前，只可称实现候选或 READY_FOR_REVIEW，不能标 R07前置已闭合、R08业务已放行或R08完成。

本次只写本报告、配套 JSON 与独立范围重建证据；未读真实用户数据、未启动生产默认、未提交/推送、未改原账或既有历史证据。
