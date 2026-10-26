# R08 最终报告（R08_REPORT.md）

- 阶段：R08 旧 Electron 客户端接入与数据迁移演练
- 基线 git HEAD：`86840d1700397d3dd620a41f2b884df4cddd5cc2`（分支 `codex/rust-tauri-migration`；全部 R08 成果在工作区未提交，工作区为唯一副本）
- 日期：2026-10-23（T01–T06 前段 + 本续接会话 T06 收口/T07/T08）
- 台账：`docs/rust-tauri/R08/PROGRESS.md` 与各 PROGRESS-tXX.md

## 一、结论（如实）

**R08 未全绿放行；达到"实现到位项 READY_FOR_REVIEW + 未竟项如实 FAIL/BLOCKED/PENDING"的诚实关闭态。**

- T01–T06：已完成并经独立验证/评审（T05 评审 PASS；T06 评审 PASS-with-MINORS 且移交项 N1/N2/N3 已落地重绑）。**注意 §三-F 的治理修正**：T04–T06 的迁移绿证据原本只对夹具自造格式成立，T07 的 F-T07-1 修复后链条对真实 Pi 格式亦闭合（夹具路径零扰动，双锚保持）。
- T07：A/B 线束建成并跑出绑定绿证（15 任务：parity=12 / differs=0 / recorded=2 / B-failed=1），两个真修复（F-T07-1 阻断级、F-T07-2）落地；**126 个补充叶全部 required_behavior_pending（其中 9 叶在 evidenceRequired 注记已实测正路径份额图钉，非门禁）**——未完成全产品回归，如实留债（门规 R05 RR1 F25：末位阶段无 later stage，份额拆分不合法，须 full_original_behavior 或 pending）。
- T08：xtask R08 注册（18 命令 × 16 场景 × 126 叶阶段图）；最终门 `verify-stage R08` 运行结果见 §四（117 pending 叶决定 overall 非 PASS——这是诚实结果，不是门故障）。

## 二、交付物清单

| 交付物 | 位置 |
|---|---|
| 迁移命令（migrate，T05 已评审） | `lingxi-service migrate`（本阶段零改动） |
| 回滚/再前滚命令（T06，评审 PASS） | `lingxi-service rollback` / `reforward` |
| 回滚操作手册 | `docs/rust-tauri/R08/T06_ROLLBACK_RUNBOOK.md`（含 manifest 双哈希披露、queued 续投语义、印章条款①、恢复根语义） |
| T06 独立评审 | `T06_REVIEW.md`（PASS-with-MINORS；rev-t06-* 8 组复跑） |
| A/B 线束与探针 | `scripts/rust-tauri/r08_t07_ab_drill.mjs`、`r08_t07_real_format_probe.mjs`、`desktop/src/react/__tests__/r08/r08-t07-kernel-ab.test.ts` |
| UI 差异报告（A13/A14） | `T07_UI_DIFFERENCE_REPORT.md` |
| FEATURE_PARITY_RESULTS 更新 | `FEATURE_PARITY_RESULTS.json`（R08 增量） |
| 阶段图与注册 | `rust/crates/xtask/src/stage_maps/R08.json` + `R08_INPUTS.json` + xtask 注册（main.rs/runner_identity.rs） |
| A15 隔离证据 | `r08_t08_isolation_check.mjs` → a15-isolation.json |
| A16 版本清单 | `r08_t08_version_manifest.mjs` → a16-versions.json |
| 交接 | `R08_HANDOFF.json`、本报告 |

## 三、关键发现与治理修正（诚实账）

- **F-T07-1（阻断级，已修复）**：T04 夹具会话用自造 `entryType` 格式，真实旧栈是 Pi SDK 格式（`{type:"session"}` 头 + `{type:"message",message:{role,…}}`）；修复前真实数据导入全部落 legacy-unknown-block（原字节隔离、对话不可用）。**治理修正：T04–T06 时期"迁移链已验证"的表述对真实数据不成立（修复前）；修复（加性）后经真实格式探针红→绿闭合，夹具证据不受影响（t07-pi-fix-amend-proof-01：435/435/5206 + 双锚保持）**。
- **F-T07-2（已修复）**：客户端边界缺 10 条会话管理写端口旧→Rust 翻译；修复含连接门（无门时旧腿被误译 404，红证据在案）。
- **D-4（未决）**：Rust 侧 rename 标题跨重载回落 + 任务序列会话数变化——post-R08 定位。
- **D-1/D-2/D-3/D-5**：标题投影差异 / 迁移根无 agent 名册（Register 不随迁；补偿=正式入口补建同 id agent，D1 已证）/ 旧栈无模型拒切换（环境前置）。
- **门工具修复**：xtask 候选快照对工作树已删除 tracked 条目（任务前遗留 8 万脏态的 D 项）尝试哈希而失败——修复为跳过（候选=工作树现状）。

## 四、最终门（verify-stage R08）

- 命令：`cargo run --manifest-path rust/Cargo.toml -p xtask -- verify-stage R08 --evidence artifacts/rust-tauri/R08/verify-r08-final-01`
- 阶段图：18 真实命令 / 16 基础场景（R00 镜像）/ 126 补充叶（9 份额绑定 + 117 required_behavior_pending）。
- **预期且如实的结果：overall 非 PASS**——117 个 pending 叶按门规不可判完成（R13-F01：登记≠通过）。最终门结果快照与逐命令回执见证据目录（本报告落笔时门在运行；结果以 `verify-stage-result.json` 为准，若有个别命令红，逐条根因在 PROGRESS-t08.md）。
- 意义：门的非绿是**覆盖缺口的真实表达**，不是门失效；R09+ 逐叶补证据后同门可转绿。

## 五、PPG-1/2/3 终裁（T08 检查单必裁项）

| 项 | 事实（T03 证据） | R08 终裁 |
|---|---|---|
| PPG-1（预览写能力缺失） | 桌面预览链路 workspace_root=None → write/edit 工具不注册；drill-06/08 帧级红证据 | **接受为预览限定 + post-R08 修复**：修复=桌面预览 workspace 策略（产品决策），非本阶段授权范围；如实标注，不隐藏 |
| PPG-2（预览读授权零根） | read 工具已注册但 authorizedFolders 永不生效（零根策略） | 同上：预览限定接受 + post-R08 |
| PPG-3（远端 LAN 端到端不可认证） | loopback token 非 Local 恒拒 + 设备凭证无呈现通道 + ALF 拦截 | 同上：回环退路已裁（T03）；真 LAN 属产品/平台工作 |

## 六、A15/A16 验收

- **A15 试用仍隔离生产**：真实 `~/.lingxi` 未被任何 R08 命令触碰（全部证据回执的 home/work/scratch 均在 /private/tmp）；Rust 预览由环境变量显式门控、默认启动=旧内核（源码事实）；真实 home 只读摘要已存（a15-isolation.json；开工基线未采集如实披露，本摘要为 R09+ 首基线）。
- **A16 对照基线固定**：HEAD/二进制 sha/渲染层树摘要/契约文件摘要/双锚/阶段图 digest 全在 a16-versions.json；R09 不得重改模型协议或数据架构。

## 七、移交 R09+

1. 117 个 pending 叶逐叶补真实证据（同门转绿）；2. D-4 定位修复；3. PPG-1/2/3 产品修复；4. D-1 标题投影裁决；5. T07 D-3 扩腿（旧栈模型替身注入后的含聊天回合全量 A/B）；6. 迁移根 agent 名册补偿步骤入正式切换 runbook（已入 T06 手册语义域）；7. post-R08 卫生清单（#5/#7/#8、manifest 双哈希修复+锚重封、backup 残渣、/api/health 鉴权标注）。

## 八、收尾轮（2026-10-25，closeout 会话）——R09 准入结论

**受测候选**：HEAD `aa99d2ec1dae7f0aef5971bab049734f22fcf7d8` + 本会话未提交工作树（门回执 `worktreeDirty=true`）。收尾文档提交（若有）与受测候选分离，不冒称已测。

**正式门**：`cargo run --manifest-path rust/Cargo.toml -p xtask -- verify-stage R08 --evidence artifacts/rust-tauri/R08/verify-r08-closeout-02`
- 命令 **20/20 PASS**（含此前从未执行的 A04 两套件、新增 C③ 旁证套件；必需命令全部真实执行，vitest 验收套件 skip=0）
- 场景 **16/16 PASS**（A04 以测试锚点+UI 案例合同判定；A13/A14 以 42 任务逐条平价合同判定）
- 候选绑定 **stable=true**（前后 17.7k 文件摘要一致；final-15 的不稳定根因为上一会话在门运行期间编辑了 3 个文件）
- 补充叶 **47 PASS / 79 BLOCKED（required_behavior_pending）/ 0 FAIL**
- **overall=FAIL**——唯一原因：79 叶仍无逐断言证据。
- 首轮 `verify-r08-closeout-01` 保留为真实红（18/20：a04_ui 为本会话日志解析 bug〔3 测试实际全过〕、a05 为已知宿主自动化间歇红；单跑复验绿后重跑得 02）。

**两条硬指标**：① 126 叶全有证据——**未满足**（47/126）；② 门 overall=PASS、绑定稳定、必需命令实际执行、关键 skip=0、相关旧能力无回归——除 overall 外全部满足（overall 受①制约）。**结论：R08 未完成，不可进入 R09；本会话不授予 R09 准入。**

### 本轮闭合（均有红→绿或实跑证据，见 CLOSEOUT_PROGRESS.md）
- 验收基础设施 A①–④（A04 接入、命令引用完整性与整图字节自检、A13/A14 差异判定、隔离硬校验）。
- 恢复链 B①–④（附件路径重基、Register 类文件进差集、旧会话可变更新）：真实 d3 演习旧二进制 8 失败 → 修复二进制 0 失败。
- 客户端语义 C①–③（pinnedAt、续接/处置翻译与 path 适配、D-10 查找 + Bridge 脱敏 + 绑定表 14 条误分类纠正）。
- 另修 4 个 Rust 缺陷：幽灵 agentId 建独立会话；默认 `auto` 权限建会话 400；**迁移真实会话因 `legacy-unknown-block` 无法压缩/续聊**；迁移消息行全字段透传致模型上下文解码失败。
- A/B 线束：42 任务 41 平价 + 1 已登记 recorded（夹具前提）+ 0 差异；D-2/3/5/7/9/10 关闭。
- 新绑定 27 叶（20→47；按域合计 D02 42 / D04 4 / D03 1）：D02 projects×10、会话地图×5、检查点×4、文件回退开关×2、建会话（焦点/独立）×2、会话内查找×1；D04 权限模式读/按作用域切换/设置×3。

### 回归入口（同一候选）
- `npm run typecheck` → 0 错误；`cargo build -p lingxi-service/-p xtask`（--locked, 1.98.1）通过；触及 Rust 模块 lib 单测 50/50、rollback_delta 8/8。
- `npm test`：20 文件/121 测试失败——在纯净基线 aa99d2ec1 worktree 上同 20 文件逐数相同（121），**全部为无关历史红**（证据 `closeout-proofs-01/regression/`），本轮不扩修。

### 剩余（如实，不转 R09 职责；续跑断点见 CLOSEOUT_PROGRESS.md）
D02 24 叶、D03 18 叶（需 MCP 替身）、D04 8 叶（确认/拒绝需待批动作；plan-mode 需 Rust 路由；默认 ASK 需产品裁决）、D01 23 叶（WS 聊天线束）、D13 3 叶、D20 3 叶。需产品裁决：P-ASK-DEFAULT、D-11、D-12（CLOSEOUT_BLOCKED.md）。

## 九、续跑第二轮（2026-10-26，业主裁决后）——R09 准入终态

**受测候选**：HEAD `aa99d2ec1` + 未提交工作树（门回执 worktreeDirty=true；收尾文档与受测候选分离）。

**正式门**：`verify-r08-closeout-03`
- 命令 **19/20 PASS**（唯一红 = a05 row1-hide 宿主自动化焦点回退，连续 3 次同签名；环境阻塞 ENV-A05-FOCUS 见 CLOSEOUT_BLOCKED.md，产品代码自上轮绿证未改）
- 场景 **15/16**（A05 连带）· 候选绑定 **stable=true**
- 补充叶 **61 PASS / 65 BLOCKED（pending）/ 0 FAIL** · overall=**FAIL**（唯一原因：65 叶无证据 + A05 环境红）

**本轮增量（相对 §八 的 closeout-02：叶 47→61，命令面因 a05 环境回退 20→19）**：
- 业主五项裁决（CLOSEOUT_ADJUDICATIONS.md）落实：/new、/reset、默认权限 3 叶按裁定绑定（Rust-authoritative 口径，A 腿观测留档）。
- 新增 Rust `/lingxi/v1/plan-mode` GET/POST（旧 /api/plan-mode 同语义）+ 客户端映射；plan-mode、快捷聊天偏好、输入草稿、WS compact 四族新任务双内核实跑平价。
- 修复客户端缺陷：旧内核专用 wire 帧（context_usage/terminal_snapshot_request/run-end 补发）不再打到 Rust 连接（此前会触发 close 1008 打断会话）。
- A/B 演习终态：**51 任务 = 48 平价 + 3 已登记 recorded + 0 差异**；131 图钉案例 130 绿（bridge-redaction 旁证由门内 cargo 套件补齐）。

**回归入口（同一候选）**：typecheck 0 错；cargo --locked 通过；rollback_delta 8/8、触及模块 lib 单测 11/11；npm test 失败集与纯净基线逐文件逐数相同（无关历史红）。

**结论：R08 未完成，不可进入 R09。** 硬指标① 61/126；硬指标②受 A05 环境红（19/20）与 65 pending 制约。剩余 65 叶续跑断点见 CLOSEOUT_PROGRESS.md；a05 解锁条件见 CLOSEOUT_BLOCKED.md。本会话不 commit/push。

## 十、收尾终态（2026-10-26，DeepSeek Harness 接手第六轮）——R09 准入结论

> 本节为**最终权威结论**。§八/§九 为历史快照（closeout-02、closeout-03 时点的数字），
> 其中「未完成/不可进入 R09」的判断已被本节取代；历史段保留以便追溯，不再作为现行结论。

**受测候选**：HEAD `aa99d2ec1dae7f0aef5971bab049734f22fcf7d8` + 未提交工作树
（门回执 `worktreeDirty=true`；收尾文档与受测候选分离，不冒称已测）。全程未 commit/push。

**正式门**：`cargo run --manifest-path rust/Cargo.toml -p xtask -- verify-stage R08 --evidence artifacts/rust-tauri/R08/verify-r08-closeout-08`
- 命令 **22/22 PASS**（全部必需命令真实执行；vitest 验收套件 skipped=0）
- 场景 **16/16 PASS**
- 补充叶 **126/126 PASS，0 BLOCKED，0 FAIL**（`full_original_behavior` 全覆盖；域：D01 23 / D02 66 / D03 19 / D04 12 / D13 3 / D20 3）
- 候选绑定 **stable=true**（before/after 摘要一致，逐命令 checkpoints 全 stable）· runnerSourceBinding 一致
- A15 `ISOLATED violations=0`；A16 版本基线 `head=aa99d2ec rust=b855e5d6`；in-gate A/B **71 任务 = 68 parity + 3 recorded（ADJ-4/5 + 终端帧族传输面旧任务）+ 0 differs**
- **overall=PASS** → **R08 完成，可信 R09 准入**（提交/推送/发布仍按业主另行指令；R09 未在本会话开展）

**本轮红→绿（详见 CLOSEOUT_PROGRESS.md）**
1. `leaf.lazy-content`：B 腿无 workspace → file/exec 工具未注册 → 直调 `exec_command` 触发 provider_error（线束误用）；
   修线束（B 腿 workspace + 隔离工作区）后暴露真实可达性缺口：Rust 历史大内容以工具产物资源
   （`exec-output:<file>`）落史，内容路由只解析 `res_` 会话文件资源 → 404。补最小实现（只读 data_home 内
   被历史引用的本地产物，canonical 边界 + 8MiB 上限，locator 非授权凭证）→ 双腿全绿。
2. `leaf.mcp.face`：两处线束误用——两内核 resources 读取都只接受 `ui://` 资源；Rust 无 REST
   `/mcp/connectors/{id}/start`（启动走 settings-action）。按真实机制修测试，未放宽断言 → 双腿全绿。
3. 正式门 07 为真实红：A15 V4 发现 `r08_ui_pages` 套件 argv 指向仓库内证据目录（正确判定）；
   改为生产者输出落 `/private/tmp` 合成 scratch 再拷入证据 → 门 08 PASS。07 红证据保留。

**本轮新增实现/线束（均有证据）**
- Rust 真实终端子系统（D13）：`terminals.rs`（WS 三帧 + 身份/所有权链 + close 收口，绝不谎报终止）+
  `procsupervisor.rs` 只读 `TerminalLog`/终端视图/tail（与 write_stdin 交付游标独立）；独立 A/B 套件
  真实 PTY + write_stdin + 心跳文件证 close 真实终止，双腿 14/14 case 真。
- A/B 演习：`--suite-files` 多套件合并、单任务过滤、B 腿 workspace；主+终端+交互三套件门内合并。
- UI 5 叶客户端份额：`r08_ui_pages` 生产者 87 case（jsdom 真实组件投影 + 既有专属单测 + 真 Electron live
  session-map 19 checks / quickchat 9 checks）；覆盖矩阵与未覆盖子句如实披露。
- D01 交互两叶：ask_user 29 case / task-abort POST 9 case（双腿真；abort 旧栈真实挂载点为
  `POST /api/task/:taskId/abort`——T01 绑定表该行 "unmounted?" 标注经活体实测不准确，本报告修正）；
  产品代码零改动。
- D-16 翻案：线束硬编码 session id 所致；改用服务端真 id + 活 Agent 400 负例后绑定叶 3818ADEAFA7A，撤 recorded。

**回归入口（同一候选）**：`npm run typecheck` 三配置 0 错；`cargo build --locked`（rustup 1.98.1）通过；
门内 cargo 套件全绿；本轮触及 `history_content` 内容回退安全边界单测 6/6、terminal 5/5、
prosupervisor 23/23、ws 13/13。`npm test` 仍有与前序基线逐数相同的历史红（与 R08 无关；未扩修）。

**如实披露（不阻塞）**：旧栈工具结果有内核侧输出保留窗口（延迟内容叶按「真实大正文、非预览」断言，
不要求两内核同字节集）；ask_user A4「ConfirmStore 不可用」无活体开关，由两内核既有专属测试覆盖；
UI 个别子句无活体构造面（MCP 轮询超时、Web window.open、子组件错误态），列于覆盖矩阵。

**独立上下文验收（2026-10-26 20:30，实施/独审分离）**：独立审查者结论 **ADMIT_R09** —— 独立脚本复核
126 叶逐断言覆盖（281 原断言 / 487 断言→case 实例 / 449 不同 case，叶内 0 复用、每 case 原始证据 ok=1，
findings=[]）、A/B 原始观测反算、UI 与 terminal 独立复跑、无门禁放水、隔离与文档一致性均成立；
报告 `artifacts/rust-tauri/R08/closeout-proofs-02/independent-review/`。保留记录项 6 条（F1 slash
接管态/无历史子句未直接取证、F4 UI 两处负向子句按现役实现验收需 R09 对账 R00 文本等）已写入
CLOSEOUT_PROGRESS/R08_HANDOFF，均不阻断门级判定；F1/F4 建议作为 R09 交接记录项。本报告 §八/§九 的历史
数字已由本节取代。
