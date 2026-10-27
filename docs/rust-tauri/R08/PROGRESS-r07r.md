# R08 task-5：R07 服务套件当前树重绑回执台账

执行者：teammate `ws-ui-state`。开跑：2026-10-23 00:56（Asia/Shanghai）。
前置：T03 落定、Lead 确认 rust/ 冻结（A04 绿回执后产品源码零改动）。

## 口径

- 53 命令 = TASK_SUITES 51 套件 + `r07_stage_inputs.py --check`（R07_INPUTS）+
  `r07_behavior_matrix.py --out`（A23）。"53 套件"系 R07.json 命令总数口径，
  已经 Lead 复核接受（2026-10-23 00:5x lead 消息）。
- 每命令独立证据目录 `artifacts/rust-tauri/R08/r07r-<名>-01/`，经
  `scripts/rust-tauri/r08_run.py` 包裹绑定当前候选；套件再经
  `r07_stage_inputs.py --run-suite` 落 `SUITES/<suite>/{result.json,output.log}`。
- 严格串行（单循环后台作业，逐条 START/END 落 `/private/tmp/r07r-progress.log`），
  与 t02-core（task-4，不碰套件）全局互斥。
- 当前候选 `candidateDiffSha256=489bc80550aaac0dcd2485f0342229a4a025744b47b5e60cefb754f4e8ea4989`，
  `sourceBindingsStable=true`。凡绑定 `cc7f7052…` 的旧回执自本台账对应行落新回执起
  **superseded**（旧证据保留不删）。

## 附带复验项：persistence 三件套（已升级 Lead 裁决）

证据 `r07r-persistence-trio-01`（exit 1，绑定当前候选）：10 测试失败
（persistence-schema-tripwire 4 / persistence-store-registry 5 /
persistence-startup-receipt 1），根因同一：
`dangling persistence rule for store knowledge-indexes:
lib/knowledge/vector-search-backend-factory.ts`
（scripts/scan-persistent-stores.mjs:528）。三方取证：该文件 git-clean、最后改动停在
R06 时代提交 6f5c4b01d/4c6189e40、全文写 token 计数 0（构造注入委托
ann-index-store/usearch-vector-backend）；store-registry.ts:1116 规则组仍声明
mkdir/remove-path/rename/database-open → 扫描器找不到匹配位点。
**Lead 裁决（2026-10-23）：基线红家族 #2，HEAD 提交树自带漂移，非 R08 工作引入；
R08 不修，登记基线红台账，去向=用户/后 R08 裁决。**

## 试点（管道验证，先行 2 套件）

| 命令 | 证据目录 | 退出码 | 结果 |
| --- | --- | --- | --- |
| r07_scheduler | r07r-r07_scheduler-01 | 0 | 绿（92.1s） |
| r07_t08_interface_preferences | r07r-r07_t08_interface_preferences-01 | 0 | 绿（8 测试，含本阶段新增 2） |

## 全量登记（53 命令，2026-10-23 01:00:22–01:09:20 串行完成）

总计：**53/53 全绿定稿**（a23-matrix 初红，经 Lead 单行授权修复后 -03 转绿，见修复节）。
51 套件全绿（387 passed / 0 failed 合计）；inputs-check 绿。

候选分段说明：`candidateDiffSha256` = 工作区 git diff 的 sha256。串行期间
t02-core（task-4，经 Lead 批准并行）分批向 `desktop/src/react/__tests__/` 暂存新前端
测试文件，候选因此分 6 段（489bc805 → dcf5f343 → 65fd0fd3 → f6c73ae1 → c8d4259f →
67f5c8ea → 415cecf2）。**已逐字节核验：全部 55 份 candidate.diff 的
rust/ 与 scripts/rust-tauri/ 部分完全相同（17 段，sha256 前 12 位 16905aa29383）**，
即被测产物（rust/ 树与套件脚本）在整轮运行中逐字节恒定；分段仅来自
desktop 前端测试文件，与服务套件的被测面零交集。
**Lead 裁决（2026-10-23）：接受分段，不要求静默窗口重跑**——被测产物整轮未变 +
T08 终检本就将在最终树上单候选全量复验、届时回执天然单候选。

### 红项根因与修复（a23-matrix 已转绿）

**初态（红，确定性）**：`r07_behavior_matrix.py` 退出 1，
`A23 map FAILED: 2 routes have no requirement binding`（408 leaves / 416 routes /
2 unmapped）。未映射路由 = `/lingxi/v1/system/env-deps` 与
`/lingxi/v1/system/env-deps/refresh`（source lib.rs）——R08-T01 env-deps 叶新增的
两条路由未在脚本 `PREFIX_TASK` 绑定表登记（绑定优先级：INFRASTRUCTURE_PREFIXES →
MODULE_TASK → PREFIX_TASK，`/lingxi/v1/system/` 前缀无条目）。
本叶新增路由 `/lingxi/v1/preferences/workspace-ui-state` 经前缀
`/lingxi/v1/preferences`（r07_behavior_matrix.py:91）正常绑定 R07-T08(偏好设置)，
**非红因**。隔离复测 r07r-a23-matrix-02 同签名复现（exit 1，同 2 条路由），非 flake。

**修复（Lead 单行授权，2026-10-23）**：经核实全部 `/lingxi/v1/system` 路由仅此 2 条、
无前缀误捕后，PREFIX_TASK 字典序就近插入一行
`"/lingxi/v1/system/env-deps": "R08-T01(env-deps)"`（settings 与 user-profile 之间；
git diff = 1 insertion，脚本其余字节不动）。复跑 **r07r-a23-matrix-03 转绿**
（exit 0，`416 routes (0 unmapped)`）。r07r-a23-matrix-01/-02 红证据保留，
标记 superseded。**task-5 至此 53/53 全绿定稿。**

### 明细表

| 命令 | 证据目录 | 退出码 | 结果 | 测试 过/失 | 时长 | 候选段 |
| --- | --- | --- | --- | --- | --- | --- |
| `inputs-check` | r07r-inputs-check-01 | 0 | 绿 | - | - | `dcf5f343` |
| `a23-matrix` | **r07r-a23-matrix-03** | 0 | 绿（-01/-02 红已修复 superseded） | - | - | 见修复节 |
| `r07_scheduler` | r07r-r07_scheduler-01 | 0 | 绿 | 7/0 | 92.1s | `489bc805` |
| `r07_t08_interface_preferences` | r07r-r07_t08_interface_preferences-01 | 0 | 绿 | 8/0 | 27.2s | `489bc805` |
| `r07_heartbeat_plan` | r07r-r07_heartbeat_plan-01 | 0 | 绿 | 10/0 | 1.1s | `dcf5f343` |
| `r07_heartbeat_runtime` | r07r-r07_heartbeat_runtime-01 | 0 | 绿 | 6/0 | 5.3s | `dcf5f343` |
| `r07_daily_fresh_compact` | r07r-r07_daily_fresh_compact-01 | 0 | 绿 | 6/0 | 5.2s | `dcf5f343` |
| `r07_bridge_flow` | r07r-r07_bridge_flow-01 | 0 | 绿 | 13/0 | 6.8s | `dcf5f343` |
| `r07_bridge_native_flow` | r07r-r07_bridge_native_flow-01 | 0 | 绿 | 4/0 | 7.5s | `dcf5f343` |
| `r07_bridge_permission` | r07r-r07_bridge_permission-01 | 0 | 绿 | 2/0 | 1.2s | `dcf5f343` |
| `r07_bridge_notice` | r07r-r07_bridge_notice-01 | 0 | 绿 | 3/0 | 1.5s | `dcf5f343` |
| `r07_bridge_sdk_platforms` | r07r-r07_bridge_sdk_platforms-01 | 0 | 绿 | 11/0 | 10.9s | `dcf5f343` |
| `r07_bridge_session_routes` | r07r-r07_bridge_session_routes-01 | 0 | 绿 | 4/0 | 4.8s | `dcf5f343` |
| `r07_bridge_worker` | r07r-r07_bridge_worker-01 | 0 | 绿 | 5/0 | 1.4s | `dcf5f343` |
| `r07_bridge_diagnostic` | r07r-r07_bridge_diagnostic-01 | 0 | 绿 | 2/0 | 22.3s | `dcf5f343` |
| `r07_bridge_guest_context` | r07r-r07_bridge_guest_context-01 | 0 | 绿 | 2/0 | 1.2s | `dcf5f343` |
| `r07_session_collab` | r07r-r07_session_collab-01 | 0 | 绿 | 9/0 | 6.7s | `dcf5f343` |
| `r07_phone_runtime` | r07r-r07_phone_runtime-01 | 0 | 绿 | 15/0 | 9.8s | `dcf5f343` |
| `r07_browser_routes` | r07r-r07_browser_routes-01 | 0 | 绿 | 10/0 | 15.8s | `dcf5f343` |
| `r07_browser_tools` | r07r-r07_browser_tools-01 | 0 | 绿 | 13/0 | 6.2s | `dcf5f343` |
| `r07_office_tools` | r07r-r07_office_tools-01 | 0 | 绿 | 15/0 | 9.5s | `dcf5f343` |
| `r07_desk_files` | r07r-r07_desk_files-01 | 0 | 绿 | 8/0 | 11.1s | `dcf5f343` |
| `r07_t05_media` | r07r-r07_t05_media-01 | 0 | 绿 | 13/0 | 7.4s | `dcf5f343` |
| `r07_media_admin` | r07r-r07_media_admin-01 | 0 | 绿 | 10/0 | 6.7s | `dcf5f343` |
| `r07_workbench` | r07r-r07_workbench-01 | 0 | 绿 | 14/0 | 20.7s | `dcf5f343` |
| `r07_workflow` | r07r-r07_workflow-01 | 0 | 绿 | 20/0 | 14.0s | `65fd0fd3` |
| `r07_goal_loop` | r07r-r07_goal_loop-01 | 0 | 绿 | 11/0 | 68.9s | `65fd0fd3` |
| `r07_git_panel` | r07r-r07_git_panel-01 | 0 | 绿 | 25/0 | 18.8s | `65fd0fd3` |
| `r07_conversation_tools` | r07r-r07_conversation_tools-01 | 0 | 绿 | 7/0 | 4.2s | `f6c73ae1` |
| `r07_diary` | r07r-r07_diary-01 | 0 | 绿 | 16/0 | 9.4s | `f6c73ae1` |
| `r07_web_tools` | r07r-r07_web_tools-01 | 0 | 绿 | 8/0 | 6.2s | `f6c73ae1` |
| `r07_t08_settings` | r07r-r07_t08_settings-01 | 0 | 绿 | 15/0 | 9.2s | `f6c73ae1` |
| `r07_t08_provider_matrix` | r07r-r07_t08_provider_matrix-01 | 0 | 绿 | 36/0 | 10.3s | `f6c73ae1` |
| `r07_t08_provider_http` | r07r-r07_t08_provider_http-01 | 0 | 绿 | 7/0 | 19.1s | `f6c73ae1` |
| `r07_t08_provider_capabilities` | r07r-r07_t08_provider_capabilities-01 | 0 | 绿 | 1/0 | 1.1s | `f6c73ae1` |
| `r07_t08_speech_bindings` | r07r-r07_t08_speech_bindings-01 | 0 | 绿 | 2/0 | 5.9s | `f6c73ae1` |
| `r07_t08_model_configuration` | r07r-r07_t08_model_configuration-01 | 0 | 绿 | 10/0 | 5.8s | `f6c73ae1` |
| `r07_t08_model_authority` | r07r-r07_t08_model_authority-01 | 0 | 绿 | 5/0 | 10.8s | `f6c73ae1` |
| `r07_t08_oauth_alias` | r07r-r07_t08_oauth_alias-01 | 0 | 绿 | 1/0 | 13.0s | `c8d4259f` |
| `r07_t08_mcp_settings` | r07r-r07_t08_mcp_settings-01 | 0 | 绿 | 22/0 | 11.2s | `c8d4259f` |
| `r07_t08_mcp_process` | r07r-r07_t08_mcp_process-01 | 0 | 绿 | 2/0 | 1.7s | `c8d4259f` |
| `r07_t08_mcp_remote` | r07r-r07_t08_mcp_remote-01 | 0 | 绿 | 7/0 | 14.2s | `c8d4259f` |
| `r07_t08_mcp_session_permissions` | r07r-r07_t08_mcp_session_permissions-01 | 0 | 绿 | 5/0 | 38.0s | `67f5c8ea` |
| `r07_t08_metadata_redaction` | r07r-r07_t08_metadata_redaction-01 | 0 | 绿 | 3/0 | 18.8s | `67f5c8ea` |
| `r07_t08_search_clear` | r07r-r07_t08_search_clear-01 | 0 | 绿 | 5/0 | 5.6s | `415cecf2` |
| `r07_t08_skills_cards` | r07r-r07_t08_skills_cards-01 | 0 | 绿 | 5/0 | 7.7s | `415cecf2` |
| `r07_t08_card_previews` | r07r-r07_t08_card_previews-01 | 0 | 绿 | 3/0 | 7.3s | `415cecf2` |
| `r07_t08_agent_target` | r07r-r07_t08_agent_target-01 | 0 | 绿 | 4/0 | 5.4s | `415cecf2` |
| `r07_t08_media_source` | r07r-r07_t08_media_source-01 | 0 | 绿 | 2/0 | 8.1s | `415cecf2` |
| `r07_t09_client_context` | r07r-r07_t09_client_context-01 | 0 | 绿 | 4/0 | 16.2s | `415cecf2` |
| `r07_t10_observability` | r07r-r07_t10_observability-01 | 0 | 绿 | 2/0 | 4.8s | `415cecf2` |
| `r07_t11_dev_entry_and_unload` | r07r-r07_t11_dev_entry_and_unload-01 | 0 | 绿 | 3/0 | 9.3s | `415cecf2` |
| `r07_t12_closure` | r07r-r07_t12_closure-01 | 0 | 绿 | 3/0 | 4.8s | `415cecf2` |

> persistence-trio 为附带复验项（TS 侧 vitest，非 R07 套件），不计入上表 53 命令，
> 登记见上文「附带复验项」节（红，Lead 裁决=基线红家族 #2）。

## superseded 声明

凡证据绑定 `cc7f7052…` 候选的旧 R07 回执（R07 时代 verify 证据），自本台账对应行
落新回执之时起全部 **superseded**；旧证据保留不删。本台账 53 行回执即为当前树
（rust/ 恒定段 16905aa29383）的唯一现行绑定。

