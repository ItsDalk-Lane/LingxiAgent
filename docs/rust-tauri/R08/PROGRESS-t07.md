# R08-T07 进度台账｜现有界面全功能验收

- 任务：task-9（R08-T07），续接会话 2026-10-23 开工并完成线束与修复
- 状态：**READY_FOR_REVIEW**（A/B 线束终版绿 + 两个真修复落地 + 差异报告/平价结果交付；叶级全量覆盖未完成，如实登记）

## 交付物

1. **A/B 线束**：`scripts/rust-tauri/r08_t07_ab_drill.mjs`（双内核准备/启动/双跑/三分判定比较器：parity/differs/recorded+归因）+ `desktop/src/react/__tests__/r08/r08-t07-kernel-ab.test.ts`（15 任务固定集：同一 React 构建源的真实 store 动作/lingxiFetch，jsdom，观测落 JSON）。
2. **真实格式探针**：`scripts/rust-tauri/r08_t07_real_format_probe.mjs`（A 腿旧栈列出+可读真实 Pi 会话；B 腿 legacy-import 对账 conversational/unknownBlocks）。
3. **修复 F-T07-1（阻断级）**：legacy_import session_jsonl.rs + convert_sessions.rs 真实 Pi SDK 格式识别（头行≡sessionMeta、message.role 嵌套、RFC3339→unix ms、toolResult→已知非消息、message.content 提升）；加性，夹具路径零变化（t07-pi-fix-amend-proof-01：435/435/5206 + 80a8d2c2/40f42dc1 双锚保持 + 双跑全等）；新单测 pi_sdk_real_format_entries_recognized；探针红→绿（01/02 红 → 03 绿 conversational=3 unknownBlocks=0）；adapters 178/178、svc-bin 16/16、D1/D2/D3 重绑绿（t07-pi-fix-*）。
4. **修复 F-T07-2**：use-hana-fetch.ts 翻译表补 10 条会话管理写端口（rename/pin/archive/restore/archived/delete/cleanup/sweep/pin-order/todos×3/fresh-compact）+ **连接门**（仅 rust 连接翻译；无门时旧腿 404 = 修复过程引入的真 bug，drill-03 红实证后修掉）。
5. **报告**：`T07_UI_DIFFERENCE_REPORT.md`（F-T07-1/F-T07-2/D-1..D-5 + A14 证据 + 覆盖诚实声明）；`FEATURE_PARITY_RESULTS.json`（R08 增量）。

## 红证据链（全保留）

t07-ab-drill-01（夹具 entryType 会话对旧栈不可见 0 行——引出 F-T07-1）→ 02（B 腿会话管理写端口全拒——引出 F-T07-2；A 腿 0 会话根因）→ 03（翻译无连接门，A 腿误译 404）→ 04（pin 平价；剩余归因）→ **final-05（绑定绿：15 任务 parity=12 differs=0 recorded=2 B-failed=1）**；t07-real-format-probe-01（缺 ⑤a 准备，旧栈崩）→ 02（决定性红：conversational=0/unknownBlocks=4）→ 03（绿）。

## 关键定性

- **夹具格式自造性**：T04 夹具会话用 `entryType` 自造格式，真实旧栈是 Pi SDK 格式（lib/session-jsonl.ts + tests 先例）。T04–T06 的绿证据原本只对自造格式成立——F-T07-1 修复 + 探针后链条对真实格式亦闭合（夹具证据不受影响，路径未动）。此事实按红线如实登记：**早前"迁移链已验证"的表述对真实数据不成立（在修复前）**。
- D-4（B 侧 rename 标题回落 + 会话数变化）未决，post-R08 定位。
- 叶覆盖：约 20 叶入口通路实测平价；其余叶（D01/D03/D13/D20 及 D02 余量）required_behavior_pending，见 stage map。

## 验证

- adapters **178/178**（t07-pi-fix-adapters-02）；svc-bin 16/16（t07-pi-fix-svcbin-01）；D1/D2/D3 重绑（t07-pi-fix-d1/d2-01、t07-pi-fix-d3-02；d3-01 瞬态 abort 根因未证、红证据在案）；amend 双跑全等（t07-pi-fix-amend-proof-01）。
- 无 commit/push；合成 HOME；真实 ~/.lingxi 未触碰。

## 移交

- D-4 定位、D-1 产品裁决、D-3 扩腿（旧格式 models.json 注入后含聊天回合的全量 A/B）、D-2 补偿步骤入切换 runbook（已入 T06_ROLLBACK_RUNBOOK 语义域）。

### 2026-10-24 叶级补证轮（验证器指令：9 叶负路径/边界 → full_original_behavior）

1. **套件 v3**（t07-ab-drill-leafcases-01..06 红链全保留）：受控顺序（只读→负路径→轻变更→破坏性→终态）+ observe 记录后继续 + 破坏性守卫 + pending 复位。过程发现并修三处线束缺陷：vite SSR 循环引用（合并 it 块触发）、serverConnections map 整体替换毒化 B 侧 native 连接属主（只换 activeServerConnection）、旧栈默认权限端点预热抖动（有界重试 ≤3 记录尝试数）。
2. **叶案例值双内核闭合**（leafcases-06）：rename 负路径（空标题 400/无效会话 403|404 双内核拒）、pin/todos 无效会话显式拒、list 投影字段（revision/归属/摘要-徽章面）、unauthorized 列表路由 401|403、permissions 读幂等同值。
3. **升级集（诚实上限）**：门规 verify.rs later_stages_of——R08 末位无 later stage → stage_share_satisfied 恒 FAIL（R05 RR1 F25）；full_original_behavior 须逐断言专属案例。**rename（保存成功+持久化 | 空标题拒绝 | 无效会话明确错）与 list（计数/平价/revision/摘要徽章 | 鉴权拒绝）两叶达成**→升级 full_original_behavior；**cleanup 不可升**：期限边界呈真差异（**D-6**：A 侧 30 天窗保留新归档 vs B 侧立即清除——疑 B 归档时间基差异，未决）→保持 pending+注记；pin/todos×3/sweep/permissions 断言含不可合成前提（已删 Agent 禁置顶/计划面板内容语义/孤儿挂载/ASK 缺省）→pending+注记。
4. **cases.json 门合同化**：lingxi.leaf-case-results.v1（{case,expect,actual,ok}），只收合同图钉集（门规整份记录须成立；D-6 差异不入合同文件、留差异报告）。gate-02 红（schema 不符）→gate-03 红（整记录规则吃进 D-6 案例）→修复。

### 2026-10-24 第二轮：PPG-2 真修复 + MCP 叶 + a06 根因定名

1. **家族#9 定性更正（重大）**：a05 读腿红**不是环境红**——根因=`lib.rs` 会话授权根接线缺陷：运行时 `replace_session_roots` 只写 workspace_access 实例，而预览（无 workspace）下 read 工具用**另一个**零根实例 `session_read_access`（lib.rs:1838 注册）→ 会话 authorized_folders 永不生效 = PPG-2 的机制成因。探针链：t09-a05-auth-probe-01..03（authorizedFolders 落库 ✓ + 读腿拒）+ t09-a05-standalone-01（确定性复现）。
2. **PPG-2 修复（真产品修复）**：`with_session_resource_access` 接 `.or_else(session_read_access)`——预览模式运行时根刷新落到 read 工具实际使用的同一实例；会话级授权目录经正规孔道（CreateSessionBody.authorizedFolders）生效，零根策略不变（不授予 workspace）。**a05 独立复跑绿**（t09-a05-standalone-02 verdict=read-pass__write-blocked-PPG-1 failures=0：读腿端到端真实往返 ✓，写腿按 PPG-1 呈 BLOCKED 诚实不变）。svc-bin 16/16。
3. **a06 环境红根因定名**：① System Events keystroke 路径失效（quit-b 键盘路径穷尽→SIGTERM 兜底，journal 实证）；② 客户端 A 渲染目标中途死亡（connInfoA2 ECONNREFUSED → 重解析 90s 超时）。线束侧加固：cdpEvaluateClient 目标回收重解析（t09-a06-standalone-05）+ finish 后迟到异步错误不翻转判定。**语义断言至 health-after-b 全过；A06 场景维持环境受阻 FAIL（如实）**。
4. **MCP 叶（D03 只读）**：leaf.mcp.state（连接器状态+延迟加载开关投影形状：enabled bool/connectors array/defer 标志/agent 作用域——双内核 200 平价）+ leaf.mcp.state.unauthorized（无效令牌 403|401 双内核显式拒）。线束缺陷链：无效令牌换装需连 serverConnections map 一起换（requireServerConnection 读 map 活动项）且必须保全量复原（整体重建 map 破坏 native 连接属主毒化后续动作，t09-ab-drill-mcp-02/03 实证）→ 定稿：换装用例全部置于套件最末（无受害者）。**查看 MCP 连接状态叶（DD47275AC5AE）升级 full_original_behavior**（终版 t09-ab-drill-mcp-04：23 任务 parity=18 differs=3〔皆已知 D-4/D-6 级联〕recorded=2）。
5. 门图：3 叶 full_original_behavior（rename/list/mcp-state）+ 123 pending；gate-05 运行中。
