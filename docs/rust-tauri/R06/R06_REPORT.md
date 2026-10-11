# R06 阶段报告 — 上下文、会话语义、记忆与知识库

- **阶段与结论**：R06；本报告由 EXECUTOR-R06-T08（阶段收口任务执行代理）按任务书 R06-T08 交付。阶段状态 **READY_FOR_REVIEW**——T01–T07 各任务已经其独立评审（见 §独立审查），T08 交付（行为评测 + 一处评测暴露缺陷的根因修复 + 本报告/HANDOFF/BEHAVIOR_EVAL）为**未提交工作区候选，尚未经独立复核**；执行者不自称阶段 PASS。真实模型评测项 BLOCKED（§未执行/BLOCKED）。
- **范围**：仅 R06 阶段八个任务（T01–T08）授权范围。T08 候选改动 = 评测套件（新增测试文件）+ 一处产品根因修复（`subagents.rs`，评测暴露，见 §已知缺陷/§安全与数据）+ 三份阶段交付文档 + PI 矩阵终态交接。无获准 ADR 之外的契约变更；登记在案的差异全部在各任务报告差异台账。
- **源码**：阶段基点 `c9cca6020e0…`（= T01 基点）；T08 任务基点（TASK_BASE_SHA）`ea61380577916e9fb75a4f4c2694321af7b83923`，分支 `codex/rust-tauri-migration`。T01–T07 交付均已提交推送（task_chain 内逐任务 commit SHA，origin 读回确认）；T08 候选未提交，本报告不预写其自身提交 SHA。T08 候选 `rust/Cargo.lock` 零改动。工作树含 `.vscode/`（IDE 噪声，永不入候选）。
- **环境**：macOS arm64（Darwin 27.0.0）；工具链 `/Users/study_superior/.cargo/bin/cargo` + rustc **1.98.1**（仓库 `rust-toolchain.toml` 锁定），全程 `-j 8`（T06 构建波次纪律）；测试数据 = 临时目录合成夹具；外部替身 = loopback SSE 路由替身 / 脚本化 provider 端口 / 多路媒体替身；**无真实供应商、无真实账户**。

## 一、完成项（逐 T-ID）

| 任务 | 交付与关键实现 | 生产接线 | 报告/评审 |
|---|---|---|---|
| T01 上下文编译器与短提示词 | ContextCompiler + ContextArtifact（24 段闭合注册表、总预算 25,068、render/observation 同源、子代理裁剪、digest_only 脱敏） | `runs.rs` drive_run 唯一编译点；观测端点同 artifact | T01_REPORT；R1_PASS |
| T02 token 预算、压缩与长会话 | ContextBudgeter + CompactionService（FORCE/reserve 触发、配对证明规划器、九标题摘要+一次修复、族分流 max_tokens、硬截断兜底、summarize→chat 显式回退链） | drive_run loop 顶 + compact_now | T02_REPORT；R4_PASS（3 轮修复闭环） |
| T03 会话树、重试、fork 与回退 | SessionService（fork/retry/rewind/检查点/管理面，schema v8） | HTTP 管理面 + execute 生命周期闸 | T03_REPORT；R2_PASS |
| T04 统一消息历史、重连与导出 | CanonicalMessageStore + project_history（七态权威结构、LegacyRaw 档案、/history /export + ETag + 游标分页 I/O 探针） | HTTP /history /export | T04_REPORT；R1_PASS |
| T05 会话文件、资源与产物身份 | SessionFileStore（v9 四表、sf_ 身份公式金样）+ ResourceService（HMAC 票据）+ ResourceIO 15 路由 + preview/bridgemedia/filehistory/fsread | bootstrap 装配 + 30+ 路由 | T05_REPORT；R1_PASS（2 MEDIUM 收口义务） |
| T06 人格、记忆与技能系统 | MemoryService（per-agent 五 store）+ SkillService（审核 token 协议双层）+ memorytools 五工具 + ExtensionHooks（loop guard/tool_result）+ lingxi-test-guard | bootstrap 工具注册 + runs.rs 两个 hooks 注入点 | T06_REPORT；R2_PASS |
| T07 现役知识库迁移 | KnowledgeService（摄入/分块/embedding/索引/检索/引用/删除）+ knowledgetools 五工具 + 三库（事实 v1/FTS/向量）+ 维度铁律 | bootstrap 注册 + 网关嵌入单一链 | T07_REPORT；R1_PASS |
| T08 工具行为评测与上下文交接 | 固定任务集 ×10 行为评测（三层既有生产链 harness）；A15/A16 证据；PI 矩阵阶段终态交接；本报告/HANDOFF/BEHAVIOR_EVAL | 评测复用真实二进制/真实链；无平行执行器 | 本报告；待独立复核 |

## 二、行为变化

对用户可见的行为变化全部是**迁移等价或显式申报的差异**（逐条锚定现役行号，见各任务报告差异台账）：无新增功能。T08 候选包含一处缺陷修复（`subagents.rs`），其效果是**恢复**现役既有行为（子代理获得父代理裁剪后上下文），不是新功能：修复前生产 wired 链上所有子代理 run 因代理材料身份错误而响亮失败——该缺陷由 T08 评测暴露（BE-08），根因与修复见 §安全与数据 与 `artifacts/rust-tauri/R06/T08/07_diff_ledger/diff_ledger.md` D-T08-1。

## 三、验收（逐 A-ID，R06 十六场景）

| A-ID | 场景 | 结论 | 证据 |
|---|---|---|---|
| R06-A01 | 观测和实际请求同源 | PASS（T01 独立复核） | 三层同源断言 + 真二进制字节/digest 对照（T01_REPORT §八） |
| R06-A02 | 提示词长度不暗增 | PASS（T01 独立复核） | 冻结预算 + 逐段现役实测对照 + golden 锚点 |
| R06-A03 | 工具对在压缩后完整 | PASS（T02 R4 复核） | planner/渲染/真二进制三层合取 |
| R06-A04 | 摘要失败不破坏会话 | PASS（T02 R4 复核） | 直测 + drive 腿 + 真二进制腿 |
| R06-A05 | fork 不串写 | PASS（T03 R2 复核） | adapters + service 生产链 |
| R06-A06 | rewind 不覆盖外部修改 | PASS（T03 R2 复核） | sha256 比对 + 三档恢复收据（内容级回滚） |
| R06-A07 | 实时与重开语义一致 | PASS（T04 复核） | realtime==reconnect、history==export 逐项断言 |
| R06-A08 | 长历史分页不重复全扫 | PASS（T04 复核） | 1200 消息 24 页不丢不重 + I/O 探针窗口界 |
| R06-A09 | 旧附件迁移后可读 | PASS（T05 复核） | legacy sidecar 导入保 id + 读路径 + 归属闸 |
| R06-A10 | 缓存回收不误删交付 | PASS（T05 复核） | 冷度闸 + 引用保护（生产接线义务在 §未决 T05-F-01） |
| R06-A11 | Agent 记忆不串用 | PASS（T06 R2 复核） | 存储物理隔离 + HTTP 归属闸 + 工具单 agent 绑定 |
| R06-A12 | 技能审核和执行边界仍有效 | PASS（T06 R2 复核） | token 协议 7 步全链 + 网关越权拒绝 + guard 缺席 fail-closed |
| R06-A13 | 检索引用可验证且不串来源 | PASS（T07 复核） | 双向零串源 + 引用三环字节级断言 |
| R06-A14 | embedding 变化不静默损坏 | PASS（T07 复核） | 真重启维度切换 → fts 降级 + 重建恢复 + 旧变体零参与 |
| **R06-A15** | 语法有效不等于参数正确 | **PASS（T08 交付，待独立复核）** | BE-10：替身写 b.md（参数合法）→ schemaValid=true / semanticParamsCorrect=false / taskComplete=false / overall=fail，四维不混算由机器验证。明细：`docs/rust-tauri/R06/BEHAVIOR_EVAL.json` tasks[BE-10] + `artifacts/rust-tauri/R06/T08/eval_raw/eval_records.jsonl` |
| **R06-A16** | 不具备能力不盲调 | **PASS（T08 交付，待独立复核）** | BE-04（工具禁用，正反两腿）+ BE-05（权限不满足，正反两腿）：模型可解释受限（run completed、结构化拒绝到达模型）；网关始终拒绝（正常/注入 prompt 两腿拒绝形状逐字相同、零派发、零副作用）。命令：`cargo test -j 8 -p lingxi-service --test r06_t08_behavior_eval --locked`（EVAL_EXIT=0，`05_targeted_tests/behavior_eval_final.txt`） |

T08 评测汇总：固定任务集 10 任务 = 9 pass + 1 fail-by-design（BE-10 即 A15 存在证明）；确定性契约腿 **9/9 = 100%**；四维分项（10 任务累计，BE-10 按设计为 false）：toolSelection 10/10、schemaValid 10/10、semanticParamsCorrect 9/10、taskComplete 9/10；额外模型调用合计 **0**（每任务 observed == expected，实测自 usage 台账）。

## 四、安全与数据

- **权限负向（T08 实测）**：目录外工具调用零派发且 prompt 注入不放宽（BE-04 两腿同形状）；workspace 外写零副作用、拒绝码到达模型（BE-05）；子代理越权写零派发（journal `dispatched=false`、真实文件缺席、旧通道执行器零到达）（BE-08）；未知参数在网关 prepare 层拒绝、零派发（BE-03 反腿）。
- **真实进程**：B 层全部为真 `lingxi-service` 子进程（真 argv/config/SIGTERM 优雅停机断言）；无平行 Agent Loop 或第二执行器（评测基础设施只读请求体、回脚本化响应）。
- **单写者/存储**：全部经真实存储链；无旧 Node/新 Rust 双写。
- **T08 缺陷修复（T08-FIX-01）**：`src/subagents.rs` 一处——`drive_run` 的 agent_id 实参由配额 lane 字符串改为真实服务代理身份（子代理材料解析/工具归属/台账归 identity）；子代理并发隔离由 lane 参数承载（per-thread session lane + 全局上限）。修复前 RED 证据：`02_red_tests/red_first_full_suite_run.txt`（exit 101，BE-08 child run failed.provider_error）；修复后 R03-T06 五套件 30 用例全绿（`05_targeted_tests/subagent_regression.txt` exit 0）+ workspace 全量（§七）。有意偏离（per-subagent-thread agent lane 取消，父与子共享同一代理的 agent lane）已在 D-T08-1 申报并说明现役等价理由，请独立审查复核该项。
- **迁移/回滚**：T08 无存储 schema 变更（v9 冻结）、无数据迁移；回滚范围 = 删除 T08 候选文件（评测套件/文档）+ 还原 `subagents.rs` 一处改动，不丢失任何后续数据。

## 五、完整映射（F-ID→T-ID→A-ID→测试→结果）

- 逐叶映射：`docs/rust-tauri/R06/R06_LEAF_MAP.json`（376 叶：implemented 139 / share 65 / deferred 172——T01 的 31 叶按其报告口径 0 整叶实现 / 4 读取份额 / 27 后续阶段计入）。逐任务叶数：T01=31、T03=74、T04=89、T05=57、T06=101、T07=24；T02/T08 为横切任务无直接叶子。
- PI 能力账本（阶段终态）：`docs/rust-tauri/R00/PI_REPLACEMENT_MATRIX.json` 每条 `r06_t08_handoff`——replaced_in_rust ×11（PI-01/02/03/05/06/07/08/09/10/11/14）、shared ×2（PI-04/12）、deferred ×1（PI-13 图片入参重采样→R07 依赖治理）；每条 remaining 依赖均归属 R07/R08/R10/R11，**无未归属核心依赖**。
- 未覆盖集合：真实模型行为采样（RR-BLK-CREDENTIALS→R10）；桌面 UI 断言份额（R08/R09）；旧数据割接（R08）。

## 六、已知缺陷

| ID | 根因 | 后果/严重度 | 影响域 | 修复归属 |
|---|---|---|---|---|
| T08-FIX-01 | `subagents.rs` agent 身份与配额 lane 混淆（lane 字符串被当作代理材料身份） | 生产 wired 链上所有子代理 run 响亮失败；HIGH；**已在本候选修复并回归** | 子代理派发链（R03-T06 交付面 × T01 编译器） | 本候选（RED→GREEN，见 §三/§八） |

无其他本阶段新发现缺陷。前序任务的登记缺陷/义务**原样保留**（不在本任务修）：

## 未决与收口义务清单（R05 遗留 + T01–T07 收口义务，原样保留）

- **R05 遗留**：RR-BLK-CREDENTIALS（真实供应商 LIVE，最迟 R10，发布阻塞项）。
- **T01**：F-01（TOTAL_SYSTEM_TOKEN_BUDGET 测试锚）、F-04（acceptance-map 陈旧测试名）、F-06（观测端点跨主体 403 入库测试）等 LOW——见 R06_PROGRESS task_chain。
- **T03**：R2-F-01（MEDIUM：fork/retry/rewind 会话级操作互斥——收口前修复或差异台账显式申报，二选一）、R2-F-02/03/04（LOW）、OBS×5。
- **T04**：F-01/F-02（LOW：all 参数重复键；reconciliation=1 台账补记）、OBS×6（含 R08 UI 断言份额挂起）。
- **T05**：F-01（MEDIUM：三组原语生产零调用点——总控裁决 R06 收口前接线或申报归属）、F-02（MEDIUM：叶 #44 与 mount/url 成功面随 J2 二选一）、F-03/F-04（LOW）、J1/J2/J3 待裁决。
- **T06**：OBS-R2-1（catalog 序措辞，R07 接线核对）、OBS-R2-2（LAN 环境项）。
- **T07**：F-01（LOW：第三重门补测试+收敛措辞）、F-02（LOW：缓存库 open 分类修复或台账补记）、OBS×4。
- **T08 新登记**：线体 tool_call→DelegationRequest 铸造面 + 委派实验开关生产接线归 R07（D-T08-3）；PI-13 图片入参重采样等价物归 R07（依赖治理）。

## 七、现有回归测试（真实命令与退出码，工具链 1.98.1，全程 `-j 8`）

| 命令（rust/ 目录） | 退出码 | 证据 |
|---|---|---|
| `cargo fmt --all -- --check` | **0** | `03_fmt/fmt_check.txt`（首轮非 0 → `cargo fmt --all` → 复检 0） |
| `cargo clippy -j 8 --locked --workspace --all-targets -- -D warnings` | **0** | `04_clippy/clippy.txt`（首轮 9 lint → 修复（type alias/allow 带理由/is_empty/redundant closure）→ 复检 0） |
| `cargo test -j 8 -p lingxi-service --test r06_t08_behavior_eval --locked -- --test-threads=3 --nocapture` | **0**（10 passed / 0 failed） | `05_targeted_tests/behavior_eval_final.txt` + `eval_raw/eval_records.jsonl` |
| `cargo test -j 8 -p lingxi-service --test subagent_lifecycle --test subagent_closeout --test subagent_permission_inheritance --test cancel_link_inheritance --test background_steering --locked` | **0**（3+8+4+7+8=30 passed） | `05_targeted_tests/subagent_regression.txt`（T08-FIX-01 修复的定向回归） |
| `cargo test -j 8 --workspace --locked --no-fail-fast` | **101 = 恰 1 个环境失败**：145 个 test target，**1861 passed / 1 failed**，唯一失败 = `r00_management_positive_and_negative_branches_on_real_service`（LAN 自地址入站被防火墙/TUN 间歇拦截的既有环境项）；**该用例 solo 复跑 PASS**（1 passed，142.64s，exit 0）。flaky 三项（cancellation_tree/shutdown_coordinator/PTY）本轮零失败。水位：T07 终局 1852 → 1862 = **+10 恰为 T08 评测套件**，只增不减 | `06_workspace_regression/full_workspace.log`（末行 WORKSPACE_EXIT=101）+ `06_workspace_regression/r00_management_lan_solo.txt`（SOLO_EXIT=0） |

## 八、未执行/BLOCKED

- **真实模型评测多次采样与波动报告（任务书 T08 Step 3 后半）**：BLOCKED_NOT_AUTHORIZED——真实付费凭证未授权（R05 既定边界 RR-BLK-CREDENTIALS，最迟 R10）。本评测全部使用确定性替身，**不冒称替身测试为真实供应商验证**；确定性契约腿（Step 3 前半）已 100% 完成。是否阻止当前放行：不阻止 READY_FOR_REVIEW；作为发布授权条件项在 R10 独立处理。
- Windows / Linux 平台：NOT_RUN（本机 macOS arm64；跨平台归 R09/R10 打包口径）。
- 旧 Electron 引擎对照运行：NOT_RUN（方法边界见 BEHAVIOR_EVAL.json `comparison_method`——以现役语义清单+金样为金线）。
- T08 评测的 S 层腿（BE-08）走 `bootstrap_with_deps`（真实 supervisor/网关/存储/编译器 + 脚本化 provider 端口）：线体 tool_call→DelegationRequest 铸造面未接线（归 R07，D-T08-3），二进制腿无法到达 child run——该替身边界如实申报。

## 九、回退

已演练步骤 = T08 候选范围本身可整体还原（删除新增文件 + 还原 `subagents.rs` 单处改动）；不丢失本次之后新增数据（无 schema/数据变更）。前序任务回退语义见各自报告（T03 rewind 三档收据、T05 删除保护等）。

## 十、独立审查

T01–T07 各任务已经独立评审（结论与轮次见 `docs/rust-tauri/R06/R06_PROGRESS.json` task_chain `review_state`：T01 R1_PASS、T02 R4_PASS（3 轮修复闭环）、T03 R2_PASS、T04/T05/T07 R1_PASS、T06 R2_PASS；审查者独立复跑 digest/测试/叶图，证据在 `artifacts/rust-tauri/R06/T0x-REVIEW-0x/`）。**T08 交付（本报告、HANDOFF、BEHAVIOR_EVAL、评测套件、subagents.rs 修复）尚未经独立复核**——审查者应至少：重放 `r06_t08_behavior_eval`（10/10 且 BE-10=fail 为预期）、复算候选 digest、亲核 BE-10 四维不混算与 BE-04/05 正反两腿证据、复核 D-T08-1 修复的语义等价性（尤其 agent lane 共享的有意偏离）。本报告区分：源码推断（§一表格）、实际运行（§三/§七命令与退出码）、受环境限制（§八）。

## 十一、下一阶段（R07 输入）

- **允许范围**：R07 外围能力、后台自动化与多入口接入——以 `R06_HANDOFF.json` 为唯一交接面（interfaces 全部为已实现并复核的服务面；未实现 port 已注明状态）。
- **必须输入**：HANDOFF（含未决项清单与 leaf/PI 终态归属）+ BEHAVIOR_EVAL（行为回归基线：触及上下文/工具/网关的改动应重放该固定任务集）+ T05-F-01/F-02 与 T03/T04/T07 收口义务的收口裁决结果。
- **不允许开始**：在 R06 收口裁决（§未决清单二选一落定）完成前宣称 R06 PASS；恢复研究引擎/强制子代理分发；把 BLOCKED 真实模型项写成已验证。

## 远程/发布

未获准、未执行任何 git commit/push/发布/付费请求。T08 候选以工作区形式交付（任务纪律），候选摘要（digest）在全部文件写完后由 `scripts/rust-tauri/r06_candidate_digest.sh` 计算，值见交付消息与 `artifacts/rust-tauri/R06/T08/08_candidate_digest/candidate_digest.txt`（此后候选不再改动）。

---

**STATUS: READY_FOR_REVIEW**
