# R07 阶段报告 —— 外围能力、后台自动化与多入口接入

> 全部结论基于 HEAD 0c86675d4 的工作区候选（含未提交实现），不声称该 HEAD 提交本身含全部实现。

## 一、阶段结论（本地候选范围）

- 分支 codex/rust-tauri-migration，基点 0c86675d4；验收范围为本机隔离环境真实服务/数据库/执行链，外部平台一律确定性替身。
- T01—T12 十二项任务全部本地验证（各任务报告 T01_REPORT.md…T08_REPORT.md、T07_REPORT_DRAFT.json、T09/T10 证据账本、T11/T12 本报告证据链）。
- 服务侧全量候选回归：t10-service-regression34 全绿（219 二进制/2363 项/0 失败），T11/T12 增量后 t10-service-regression36 复证全绿（220 二进制/2368 项/0 失败/49 分钟/sourceUnchanged=true）。
- 正式门禁 xtask verify-stage R07 已接通并真实消费义务（verify-r07-2）：24/24 基础场景 PASS、2/2 扩展边界 PASS、53 命令全绿、候选逐命令快照无漂移、runner 身份绑定通过；408/408 补充叶按设计 BLOCKED（required_behavior_pending——逐断言专属证据待立），35+43 移交透镜随叶状态，overall 诚实 FAIL。整体转绿的条件是 408 叶逐断言证据闭合，不以本轮套件绿冒充。
- 未验边界（明确保留，最迟 R10）：真实账号/付费供应商/对外发消息、非 macOS 平台、正式安装包、锁屏下真原生对话框与废纸篓（5 项环境受阻）、MCP legacy SSE 未实现、SDK 解析前分配上限未证明。

## 二、任务逐项

| 任务 | 内容 | 状态 | 证据 |
|---|---|---|---|
| T01 | 定时任务/心跳/统一调度 | LOCAL_VERIFIED | T01_REPORT.md |
| T02 | 五平台 Bridge 接入 | LOCAL_VERIFIED（真实账号保留） | T02_REPORT.md |
| T03 | 频道/DM/多 Agent 协作 | LOCAL_VERIFIED | T03_REPORT.md |
| T04 | 生产级浏览器宿主 | LOCAL_VERIFIED_FINAL_R07_PENDING | T04_REPORT.md |
| T05 | Office/PDF/美化/内置插件 | LOCAL_VERIFIED | T05_REPORT.md |
| T06 | 媒体/语音/视觉桥 | LOCAL_VERIFIED | T06_REPORT.md |
| T07 | 独立工作台 | 已验证，两浏览器失败交 T12（见 §五） | T07_REPORT_DRAFT.json |
| T08 | 设置/供应商/MCP 面 | LOCAL_VERIFIED_FINAL_R07_PENDING | T08_REPORT.md |
| T09 | CLI/mobile 共享 context 与真实执行接线 | LOCAL_VERIFIED | R07_PROGRESS.json + 回归链 |
| T10 | 可观测性（用量查询/筛选/详情/导出） | LOCAL_VERIFIED | T10_LEAF_EVIDENCE_DRAFT.json |
| T11 | 开发入口授权矩阵 + 热卸载失效 + 旧插件退役 | LOCAL_VERIFIED | t11-dev-unload3（3/3） |
| T12 | 闭卷：A23 双向路由矩阵 + A24 无旧内核闭环 + 门禁接通 | LOCAL_VERIFIED（门禁诚实 FAIL 为设计态） | t12-closure2（2/2）+ verify-stage 回执 |

## 三、验收范围核算

- 基础场景 24（R07-A01—A24）：与 R00 ACCEPTANCE_MAP 全等，specMirror 逐字段对照原账，漂移即硬失败。
- 补充叶 408（1050 条原断言）：R07 地图全量登记 required_behavior_pending——每条原断言须绑定本阶段真实专属正反行为证据后方可判完成；门禁如实整体 FAIL，不以套件绿冒充逐断言完成。
- 扩展边界 2（F-D16-E-USER-PLUGIN-LEGACY、F-D16-E-DYNAMIC-SLASH-PROTOCOL）：T11 真实测试证据闭合，PASS。
- 移交 35（R04 规划继承，29 条 R04/R07/R08 + 6 条 R04/R07，87 条原断言）+ 前阶段递延 43（R02 9、R03 13、R04 21）：两来源零重叠、全在 408 内不重复计数；门禁透镜视图随叶状态输出缺口清单。
- 阶段组合复核：R07 独属 123；与后续阶段共享的 52 叶保留原阶段集合与剩余责任，不把早期阶段当延期接收者。

## 四、用户可见语义变化（醒目）

1. **桌面内嵌浏览器宿主退役**：R07 起原生浏览器状态窗口独立创建，绝不挂载本机 WebContentsView；旧用例经用户 2026-10-19 裁决按设计退役，R08 须重立对应覆盖。
2. **dream 槽模型回退**：槽位 memory→chat 回退（混合配置语义变更，契约更新已核）。
3. **配置写入先校验后落盘**：非法配置不再先写后报。
4. **引用认领面收窄**：模型参数/工具回显不再视为附件授权；非法相对路径由导入安全闸在工具执行内明确拒绝（KNOWLEDGE_IMPORT_PATH_INVALID），Run 正常收官。
5. **网关目录收紧**：显式选择的助手/目标必须命中平面目录，不存在即响亮拒绝。
6. **渲染端点迁移**：/lingxi/v1/models→/lingxi/v1/chat/models；workspace-files→/lingxi/v1/desk/files。
7. **常驻工具面 7→18**：七模块 add_model_resident；按需两族（知识/记忆 14 件）保持按轮激活不常驻。
8. **模型投影归档统一**：直注执行器路径也归档 tool_result_model_projection（历史恢复消费此记录），持久事件链 11→12。
9. **旧用户插件生态退役**：数据根 plugins/ 残留只告知不加载；/api/plugins/dev* 开发入口任何身份响亮拒绝。
10. 服务侧「缺人格模板误标 provider_error」已知偏差（登记，未夸大）。

## 五、保留与失败史（原样）

- T07 浏览器两失败（真实 Chromium 弹窗、cookie 后 Resume 导航）：锁屏环境受阻，未改写为通过。
- T08 清账项：t08h-t12-clause-followup-actual1.json 的 17 界面分支/4 类窄负例/minimax 条款矛盾。
- 环境受阻 5 项 + persona 专属环境 2 项（单独取证绿）+ 设计退役 1 项：T10_LEAF_EVIDENCE_DRAFT.json 各段。
- 时序敏感 flake：actual_proactive_31_minutes（T05 一次 + 回归35 一次，同源码 3+3 次通过，回执全留）。
- 本轮回归链 73 条全部登记（含全部原始失败回执）。
- 提交前 JS 侧全量 vitest 清点（2026-10-19，对照 0c86675d4 干净基线）：基线已红 16 文件
  （封印族 3、r06-ar 缓存保全 3 与环境变量族、persistence 3、knowledge 2、style-discipline
  tauri 资源、R02/R06 时期桌面用例 4）为既有失败，不归咎本轮；R07 增量红 8 项中 7 项已按
  R07 现役语义对齐并复跑转绿（access-route/web-auth-route 桌面画像加 sessions.write、
  open-boundary 开放集补 cli/rust-chat.ts 与 contracts/generated/ts/lingxi-protocol.ts、
  secret-file-custody 扫描器对无初始化器声明的健壮性、浏览器宿主测试接受
  wrapIpcHandler/wrapIpcBestEffortHandler 两种注册壳、工作台面板发送体补 requestId/
  sessionKey、设置原语合同允许刷新失败提示条），1 项 TerminalCard 全量负载时序抖动
  （单独复跑 5/5 绿）；3 个新增 R07 验收测试（r07-a17-a18-cross-entry、
  r07-t09-mobile-browser、R07SkillsFailureBoundariesLive）与 r06-ar 先例同型，需
  LINGXI_SERVICE_BIN/R07_SKILLS_* 等显式环境，默认 npm test 下响亮报错而非静默跳过。

## 六、证据锚点

- 回归链账本：docs/rust-tauri/R07/T10_LEAF_EVIDENCE_DRAFT.json（regressionChainThisRound）
- 全量回归：artifacts/rust-tauri/R07/T09/t10-service-regression34.json 与 t10-service-regression36.json（增量后复证）
- T11：t11-dev-unload3.json；T12：t12-closure2.json、t12-xtask-units2.json
- 正式门禁：artifacts/rust-tauri/R07/T12/verify-r07-2/verify-stage-result.json（verify-r07-1 失败轮原样保留同目录）
- 35 项移交来源：artifacts/rust-tauri/R07/T12/35-handoff-source-resolved-20261018.json
- 旧依赖台账：docs/rust-tauri/R07/LEGACY_DEPENDENCY_LEDGER.json

## 七、2026-10-20 收口轮（审查缺陷 A/B/C/D 修复）

审查基线 bfe32d107 的四组确诊缺陷全部实现并验证，逐项状态见
R07_PROGRESS.json 的 closeout.fixedDefects。要点与证据：

- **A 观测面**（lingxi-service/src/observability.rs + adapters run_store.rs +
  desktop rust-observability.ts）：settings GET/PUT（retention 持久化、恒真面
  409、同 home 重启保留）、storage 概况（真实按日统计与库文件字节）、
  maintenance/delete（日期窗校验、会话轨迹+legacy trace+孤儿行删除、
  VACUUM 压实）、payloads/blobs 显式 404/400（本构建不捕获副本，不伪造）、
  query/traces 重写为统一轨迹面（`session:` 会话聚合 + 导入 legacy trace +
  可选 singleton；minCallCount/origin/includeSingleton 下推 SQL；keyset
  cursor 与查询形态绑定）、aggregate dateBucket（utcOffsetMinutes 分桶；
  无 tz 库的 timeZone 显式 400）、export 对齐原版请求体（query.filter/
  includePayloads/maxCalls）并补 bundle trace/usage 字段、call 详情补
  payloadRecords/attempts/parentCall/childCalls（因果链经 emitted/
  parent_tool_call_id 真实查询）。测试：r07_t10_observability 套件 2/2
  （新增 closeout 综合用例）；UI 侧 observability 135 + 路由映射 5 绿。
- **B Bridge /loop**（slash/bridge/loopsvc/loopx/model_history +
  adapters storage）：bridge_command_kind 认 /loop /stop /abort /halt；
  LoopTarget::bridge + execute_scheduled_for 真实执行；回执/通知落 V25
  bridge_loop_deliveries 台账（重启接力、delivery_id 幂等）；V26 重建
  outbox 增加 loop_delivery_id 来源列——修复循环通知因来源 CHECK 三源
  皆空被拒的真实缺陷（来源互斥校验防伪造）。测试：r07_bridge_flow 14/14
  （guest 边界、全生命周期、伪造来源响亮拒绝、重启回执），相邻
  bridge_store/effect_store/notice/message_projection 套件绿。
- **C Loop 媒体授权**（media_source_authority.rs）：RunOrigin::Loop 的
  正式持久授权 = 落盘循环记录活跃（循环根 run + 本机用户 + cause 逐字
  匹配 + 会话活跃 + 记录 Running/Paused）；stop/complete 后持续重查即拒。
  测试：r07_t05_media 13/13。
- **D 门禁证明力**：A23 双扫描器多行 `.route(` 修复（识别 141→409），
  behavior_matrix 未归因路由非零退出 + 清单量级自检（首轮 82 unmapped
  红证据留存 artifacts/.../negative-verification-closeout-1）；lib.rs
  主干 23 组前缀按 handler 模块归因（0 unmapped）；24 基础场景逐场景
  登记 39 条 suiteTestAnchors（零匹配/未通过/producer 失败均场景 FAIL；
  xtask 127 单元测试含 3 项正反向锚点测试）；A24 扩为六能力域闭环 +
  进程树 node 检测（含故意 spawn node 的检测器自证）+ 出站 HTTP 白名单。
- **反向验证**：artifacts/rust-tauri/R07/T12/negative-verification-closeout-1/
  README.md（三组红→绿：路由漏项、空/失败证据锚点、旧内核调用）。
- **回归**：lingxi-adapters 全量 546+ 通过（真实浏览器 Chromium 弹窗用例
  并发偶发、单跑 10/10 绿，属既有环境敏感）；lingxi-service 全量除
  r00_management_leaves（本机 TUN/防火墙阻塞非环回自地址入站，测试自述
  环境失败并如实呈现）外全绿；npm vitest 观测面套件绿；迁移登记表
  docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json 补登 V24–V26（fingerprint
  与真实收据逐字一致）。
- **既定缺口（不冒充完成）**：408 补充叶（1050 断言）的逐断言绑定
  authoring 未完成——本轮交付的是门禁机器与四组缺陷修复；叶整体保持
  required_behavior_pending/BLOCKED，正式门禁 overall 如实 FAIL，这是
  「逐断言证据待立」的诚实呈现而非门禁故障。
- 收口门禁：artifacts/rust-tauri/R07/T12/verify-r07-closeout-01/（本轮
  候选；结果见该目录 verify-stage-result.json）

## 八、2026-10-21 叶绑定推进（通向 R08 的剩余量化）

- 基建：r07_leaf_cases.py 生产者（真实套件日志→案例，套件失败即案例不成立）、
  R07_LEAF_BINDINGS.json 绑定表（逐叶逐断言 + rationale）、r07_apply_leaf_bindings.py
  注入器（full_original_behavior 叶内 case 不复用；stage_share_satisfied 仅限含后续
  阶段叶）、xtask 解析护栏测试 r07_registered_map_parses_with_anchors_and_leaf_bindings。
- 绑定：181/408 叶（437/1050 断言）。按域：供应商 35、MCP 19（R07 服务端份额，R08
  客户端余款）、git 21、desk 11、workbench 10、媒体/语音 32、频道/DM 13、桥 11、
  定时/心跳 8、其余 21。只读路由的权限拒绝负向以 A23 全表无凭证 401 探针为证。
- 未绑定 227 叶（613 断言）按缺口类型：候选存在待语义审核 131、客户端行为需
  UI/CLI 套件入门禁 43、候选不足需补探针 29、无候选需新探针 24。逐叶机器账：
  R07_LEAF_REMAINING.json/.md。
- 后续 gate-05~10 逐轮推进绑定（新增 r07_probe_skills/r07_probe_access 两个 Rust 探针套件、
  r07_ui_vitest UI/CLI/工具套件接入正式门禁、run_tools/ast_grep/lsp/run_code/
  security_scan/learn_lesson 由 lib 共享层真实 vitest 测试逐断言证明）。
- **最终门禁 verify-r07-closeout-11（决定性候选，47.1 分钟）：57/57 命令、24/24 场景、
  408/408 叶、2/2 扩展边界全部 PASS；35/35 移交 + 43/43 递延视图结算完成；候选与
  runner 源码绑定稳定 —— overall PASS。R07 达到放行条件，R08 准入证据取得**（独立
  审查为放行前最后人工步骤；gate-01~10 过程回执全部留存）。
