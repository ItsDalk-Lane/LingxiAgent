# R08-T07｜UI 差异报告（A13 同界面换内核 A/B + A14 失败不藏）

- 证据链：t07-ab-drill-01..04（红，保留）/ **t07-ab-drill-final-05（绑定绿证，15 任务：parity=12 / differs=0 / recorded=2 / B-failed=1）**；t07-real-format-probe-01..02（红）/ 03（绿）。
- 线束口径（固定四要素）：同一 React 构建源（jsdom 载入 desktop/src/react 真实 store 动作与客户端边界，零 fork）；数据 = 夹具 v3 基座 + **会话面替换为真实 Pi SDK 格式**（3 会话，两内核同见；夹具自造 entryType 会话对旧栈不可见——见 F-T07-1）；模型替身 = 本任务集不触模型回合（模型链由 T03 a05 系与 T06 D1 另证）；步骤 = `r08-t07-kernel-ab.test.ts` 固定任务集双跑。
- 驱动层级披露：UI 绑定层（store action / lingxiFetch = 按钮背后的唯一通路），非像素级 DOM 操作；窗口形态由 T03 Electron 深度腿另证。

## 平价面（12 任务）

身份握手、会话列表（计数/置顶面）、重命名（持久化回读）、置顶/重排/取消、归档维护与孤儿清扫、能力刷新、计划余项完成/取消/收纳、新会话默认权限、**外观偏好读/编辑/回读**、失效令牌拒绝（凭证失效）、不存在会话明确拒绝（不伪装空数据）、任务后列表可读性。其中重命名/置顶/计划余项在修复 F-T07-2 前于 Rust 腿全拒（见下），修复后与旧内核逐值平价。

## 差异与发现（逐项定位：服务契约 / 兼容转换 / UI 绑定）

| # | 差异/发现 | 层 | 定性 | 处置 |
|---|---|---|---|---|
| F-T07-1 | **真实会话格式不被 T04 导入链识别**：真实旧栈会话 JSONL 是 Pi SDK 格式（首行 `{type:"session"}` + `{type:"message",message:{role,content}}`，业务 id 由 session-manifest 派生）；T04 夹具用自造 `entryType` 格式，解析器只认顶层 role/ts → 真实条目全部落 `legacy-unknown-block`（原字节隔离、非静默丢失，但对话不可用）。决定性证据：探针 02（A 腿旧栈列出+可读，B 腿 conversational=0/unknownBlocks=4）| 兼容转换（legacy_import/session_jsonl.rs + convert_sessions.rs） | **阻断级（已修复）** | 修复（加性）：解析器识别 Pi 形状（头行≡sessionMeta、message.role 嵌套、ISO 时间→unix ms、toolResult→已知非消息类）；content 规范化提升嵌套 message.content。夹具行为零变化（amend 双跑 435/5206/双锚保持，t07-pi-fix-amend-proof-01）。探针 03 绿（conversational=3/unknown=0）。**治理注记：T04–T06 全部绿证据原本只对该自造格式成立，对真实数据不构成证据——本修复 + 探针后链条对真实格式亦闭合（fixture 路径未动，夹具证据继续有效）** |
| F-T07-2 | **客户端边界缺会话管理写端口翻译**：rename/pin/archive/restore/delete/cleanup/sweep/pin-order/todos/fresh-compact 在 Rust 连接下被 `buildConnectionUrl` 响亮拒（"no migrated endpoint"），而 Rust 服务路由全部存在（T01 绑定表已判 MAPPED）| UI 绑定/兼容转换（use-hana-fetch.ts translateLegacySessionCall） | 平价缺口（已修复） | 修复（加性）：翻译表补 10 条（body `path:"native:<id>"` → `{sessionId}` + camelCase + deny_unknown_fields 对齐）；**连接门**：翻译仅对 `serverNodeKind==='lingxi-service'` 生效（无门时旧腿被误翻译 404——t07-ab-drill-03 A 腿实证，属修复过程引入又修掉的真 bug，红证据在案） |
| D-1 | 会话标题投影：旧内核对无题会话返回 `null`（UI 走回退显示），Rust 列表以 id 干（"ab-real-alpha"）为题 | UI 绑定（列表投影） | 已知差异（记录） | 差异报告披露；产品裁决（保留/对齐）移交 post-R08 |
| D-2 | B 腿 `sessions/new` 404：迁移根无 agents/ 名册（Register 类不随迁，T04 设计）→ `agent not found: a1` | 迁移设计 | 已知语义（记录） | 补偿操作=正式入口补建同 id agent（T06 D1 已证）；T07 对照：真实切换演练须含此步骤（入 runbook/交接） |
| D-3 | A 腿 `sessions/switch` 500：旧栈无模型配置时拒开会话运行时（环境前置，A13 模型替身未注入 A 腿）| 环境前置 | recorded | 完整 A/B（含聊天回合）需旧格式 models.json 注入——移交 post-R08 扩腿 |
| D-4 | B 侧 rename 标题在后续重载中回落为 id 干 + 任务序列中会话数 3→2（archive 任务因找不到重命名会话而 B-failed）| 待定位（rust 列表投影 vs rename 持久化竞态） | **未决差异（如实 B-failed）** | 未在 R08 内修复；差异证据 t07-ab-drill-final-05（B observations）；移交 post-R08 定位（首疑 studio-list 标题计算与 rename 写入字段不一致） |
| D-5 | A/B 双腿 `sessions.create` 皆 false：A=D-3 同因；B=D-2 同因 | 环境前置 | recorded | 同 D-2/D-3 |

## A14 失败不靠隐藏消失（证据）

- 修复前（t07-ab-drill-02/03）：Rust 腿对 rename/pin/todos 等**响亮报错**（UI 报错 toast + console error + 请求 4xx/异常），入口（store 动作/按钮通路）**存在且未被禁用**——没有任何 `disabled` 冒充完成。
- 修复后（drill-04/05）：同一入口真实可用并达平价。
- 错误路径任务（unauthorized/not-found）双腿均显式拒绝且语义一致——空数据伪装在两侧都被拒。

## 叶覆盖诚实声明

本线束覆盖 D02 会话管理族约 20 个叶的**入口通路**（列表/切换/重命名/置顶/重排/归档/恢复/删除/清理/清扫/能力/计划余项）+ D04 权限（默认权限）+ 设置编辑 + 两条错误路径。D01 聊天/WS、D03 MCP 连接器操作、D13 terminal WS、D20 CLI、以及 D02 其余叶（项目/文件夹/地图/搜索/内容分页等）**未在本线束执行**——如实标 required_behavior_pending（叶绑定见 stage map），不以本报告的绿推定全量。
