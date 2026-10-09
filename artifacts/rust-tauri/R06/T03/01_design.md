# R06-T03 设计定稿（执行前）

本文件是 EXECUTOR-R06-T03 的工作稿：逆向追踪结论、验收映射、74 叶归类、T03/T04 边界、
差异清单预登记。报告 `docs/rust-tauri/R06/R06-T03_REPORT.md` 以其为数据源（正式版在报告中，
此处留执行痕迹）。所有「现役锚点」为文件:行号，已亲验（RC-1）。

## 一、本 Task 对用户必须产生的可观察行为（逆向追踪）

以「用户在产品里能观察到什么」为起点：

1. **fork（从某一轮分支会话）**：用户在会话 A 中段点「分支」，得到一个新会话 B。B 里能看到
   A 在分叉点之前的全部消息（共同历史），分叉点之后 A 与 B 各自的消息互不可见、互不影响；
   B 的授权目录/权限档与 A 在分叉时刻相同（继承快照），但 fork 操作本身绝不给 B 追加任何
   A 没有的授权；A 正在运行时 fork 被拒（409）。— 对应 R06-A05。
2. **重试某一 轮（retry）**：用户对某个已完成的回合点「重试」，该回合的输入（可选改字）
   被重新提交为一个**全新的 Run**（新 run_id），旧回合的消息与运行记录全部保留可读
   （审计不丢），当前分支头回移到重试点并追加一条分支重置标记。— 对应 retry 语义叶。
3. **回退到检查点（rewind）**：用户/模型请求回到某个具名存档点。若开了「回退同时恢复文件」
   偏好，回退会按存档点记录的版本恢复工作区文件；**若某个文件在存档之后被外部（用户手动）
   改过，回退必须检测到冲突、拒绝覆盖该文件并如实报告，绝不伪称已全部撤销**；外部副作用
   （已发网络请求、已执行 bash）绝不伪称回滚。— 对应 R06-A06。
4. **会话树/分支索引**：用户能看到会话之间的父子（谱系）关系，分支深度最多 2 层
   （主=0，子=1，孙=2），第 3 层被拒（409）。
5. **归档/恢复/删除、置顶/重命名/搜索/列表、具名检查点 CRUD**：现役管理面入口在 Rust 侧
   有可用的 SessionService 与持久化（本 Task 按任务书「按 R00 叶子迁移实际入口」的最小真实
   实现集交付；完整 REST 面 — 含 conversation-map、session-projects、todos、switch 等 —
   归 R08，见第三节归类）。

逆向追踪链：用户入口（HTTP 路由/工具）→ 服务入口（lingxi-service 路由 handler）→
SessionService（`rust/crates/lingxi-service/src/session_tree.rs`）→ Run（重试/回退后经
`SessionStore::execute_*` 提交新 run，`drive_run` 驱动）→ 存储（`RunDatabase` +
schema v8 新表：分支/谱系/消息树/检查点/文件版本）→ 事件输出（key_events 提交后经
EventHub 发布 `session_branch_reset` 等 Unknown 透传事件）。

## 二、验收映射（Requirement → Entry → Owner → Implementation → +Test → -Test → Evidence）

### R06-A05 fork 不串写
- Requirement：会话 A 中段 fork 为 B，两分支各自追加，共同历史可读、新消息独立、资源权限正确。
- Production Entry：`POST /lingxi/v1/sessions/{id}/fork`（新路由）→
  `SessionService::fork_session` → v8 表（sessions 谱系列 + messages 复制 + branch_heads）。
- Owner：lingxi-service（编排）/ lingxi-adapters（存储）/ lingxi-kernel（纯逻辑：深度闸、
  保留段规划）。
- Implementation：`fork_session`（busy 闸 + 深度闸 MAX=2 + 复制 root→boundary 消息且
  **保留 message_id**、parent 重链 + 新 session 行携 parent_session_id/fork_point/
  lineage_depth + 继承 permission_mode/authorized_folders 快照 + `session_created`
  Unknown 事件发布）。
- Positive Test：`fork_copies_shared_history_with_stable_ids_and_independent_writes`
  （共同历史 id 相同、各自追加互不可见、权限快照一致且不扩大）。
- Negative Test：`fork_rejects_depth_beyond_two` / `fork_rejects_while_session_busy` /
  `fork_does_not_widen_authorizations`。
- Evidence：树与消息查询（SQL 直查 + 服务查询）+ 事件回执。

### R06-A06 rewind 不覆盖外部修改
- Requirement：checkpoint 后用户手动改文件，执行回退，检测冲突并保护用户修改、不伪称全部撤销。
- Production Entry：`POST /lingxi/v1/sessions/{id}/rewind`（新路由）→
  `SessionService::rewind_to_checkpoint` → 文件版本表（checkpoint_file_versions）冲突检测
  → 分支头回移 + reset 标记 + 回退收据。
- Owner：lingxi-service / lingxi-adapters（文件版本表）/ lingxi-kernel（冲突判定纯逻辑）。
- Implementation：checkpoint 创建时记录工作区文件 sha256 版本清单；rewind 时逐文件比对
  当前 sha256 与记录版本：一致→写回存档版本（经真实文件版本，绝不盲写）；**不一致→
  冲突拒绝，该文件不覆盖**，其余文件照恢复；收据如实列出 restored/conflicted/skipped，
  外部副作用（网络/bash）字段恒为 `not_rolled_back`。
- Positive Test：`rewind_restores_unchanged_files_and_reports_receipt`。
- Negative Test：`rewind_detects_external_modification_and_refuses_to_overwrite`
  （A06 主场景：checkpoint 后外部改文件 → rewind → 该文件保留用户修改，收据标 conflicted，
  会话分支仍回移但绝不声称全部撤销）。
- Evidence：文件内容与回退收据（JSON）。

### 其余入口（最小真实实现集，同属本 Task）
- 创建 `POST /sessions`（新建会话行）、列表增强（含谱系/置顶/生命周期）、重命名、
  置顶/重排、归档/恢复/删除（lifecycle 列驱动）、搜索（标题+消息内容）、
  具名检查点 CRUD（latest 覆盖/其余冲突拒绝/上限 200）、分支树查询。
- 用户消息落盘钩子：drive_run 在 `record_run_started` 提交之后，仅对 `RunOrigin::User`
  把用户输入写为 messages 行（`user:{run_id}`，parent=当前分支头），失败走 adjudicated
  Failed finalize（镜像 T01 context_compiler 的失败可见模式）。

## 三、74 叶归类（真实实现 / 合法份额 / 后续阶段）

口径：真实实现=本 Task 在 Rust 侧交付生产链；合法份额=本 Task 交付其语义子集（如谱系/锁定），
完整面归后续；后续阶段=R07（Bridge/cron）/R08（桌面 REST 面/工具面）+依据。

逐条归类（叶 id 简写为尾部哈希）：

**真实实现（14 叶）**：
- 会话-FORK-960668（fork）、会话-TURNS-RETRY-4CEBEA（重试）、
  会话-TURNS-ROLLBACK-PREVIE-477DA3（回退预览：只读、不写文件）、
  会话-LATEST-USER-ME-4DD3F7（重放最近用户消息：retry 的 latest-user-only 兼容形态）、
  TOOL-REWIND-E1625F（rewind 语义：冲突检测/拒绝不回退/外部改动保留——REST 形态，
  工具形态归 R08）、TOOL-CHECKPOINT-6207F7（具名检查点创建/列出/删除语义）、
  CHECKPOINTS-3FAFA3 / CHECKPOINTS-ID-1AEDB2 / CHECKPOINTS-ID-RESTO-3562B5 /
  CHECKPOINTS-USER-EDI-BB0A16（文件级检查点列表/删除/恢复/用户编辑创建——存储与服务层）、
  会话-RENAME-FDB0FE、会话-PIN-0D4F0E、会话-PIN-ORDER-2ED065、
  会话-ARCHIVE-ARCHIV-5235D8（detach 子会话后归档：谱系解除语义）。

**合法份额（18 叶）**：
- 会话-38EEA9（列表：本 Task 交付谱系/置顶/生命周期字段子集；修订点/运行徽章归 R08）、
  会话-NEW-B9DA1A（创建：会话行+元数据子集；「切换焦点/浏览器恢复」归 R08）、
  会话-NEW-DETACHED-9D9E72（独立会话+谱系+深度闸子集；项目归属归 R08）、
  会话-ARCHIVED-43FFB7（归档列表子集）、会话-ARCHIVE-ARCHIV-3F9516（连同子会话归档：
  谱系遍历子集；「跳过项及错误计数」完整面归 R08）、
  会话-ARCHIVED-DELETE-POST-52226F（永久删除：行级删除子集；归档文件清理归 R08）、
  会话-RESTORE-POST-31CDA2（恢复：lifecycle 翻转子集；sidecar 移动归 R08）、
  会话-CLEANUP-POST-B0F05B（过期清理：按 archived_at+期限子集）、
  会话-SEARCH-A18187（跨会话搜索：标题+消息 LIKE 子集；phase 过滤归 R08）、
  会话-FIND-4F1F69（会话内查找：消息内容命中子集；Bridge 脱敏归 R07）、
  会话-AUTHORIZED-FOL-98DDC9/604C64/1E0D47/73B631（授权目录：作为会话属性存取与继承
  快照子集；审批链归 R08）、会话-MEMORY-51905C/961659（记忆开关存取子集；记忆物化归 T05）、
  会话-SUMMARY-F0319F（摘要读取子集——读已存记录，不生成；生成归 T05）。

**后续阶段（42 叶）**：
- R08 桌面/REST 面：conversation-map 4 叶（94B9BE/F28086/F6A1A5/5370DA/527121 实为 5 叶）、
  session-projects 9 叶（059066/20E9C8/3ECCF7/60A580/AFA722/D43836/D58B65/E16729/EEDFB7/
  EF5DD6 实为 10 叶）、会话-SWITCH-FBDEED（焦点切换+浏览器）、会话-TODOS 3 叶
  （5CD35D/45C5CE/17FCFE）、会话-MESSAGES-AA9EB3（分页投影，T04 统一历史投影+R08 路由）、
  会话-HISTORY-OVERVI-CDBFE6（历史概览，T04）、会话-CONTENT-CONTEN-ED0891（延迟内容，
  T04）、会话-PROMPT-SNAPSHO-FF4F06（提示词快照读取，T01 产物读取面归 R08）、
  会话-FRESH-COMPACT-D62A81（压缩重建，T02 能力+R08 路由）、
  会话-SWEEP-ORPHANED-21A99D（工作台清扫，R08）、会话-WORKSPACE-DISP-2D4EBE/6658E9
  （工作台归档/删除，R08）、CONTINUE-DELETED-AGEN-3818AD（续接已删 Agent，R08）、
  SLASH_COMMAND-NEW-485B2D/RESET-3FCB25（slash 命令，bridge 面，R07/R08）、
  UI-PAGE-MAP-8E2D44（地图 UI，R08）。
- R07/R03 共享叶：EXPERIMENT 3 叶（980C09/C99A43/A5DE44，实验面，R07/R08）、
  SUBAGENT 4 叶（00ECC9/207DF5/5F8791/857747，子代理工具，R03 已验+R08）、
  CHECK-PENDING-TASKS-7244E7 / STOP-TASK-84E22C（任务工具，R08）、
  TOOL-SESSION-5DCB03（跨会话协作工具，R08）、TOOL-SESSION-FOLDERS-7C959E
  （授权目录工具审批面，R08）、UI-SETTINGS-EXPERIMENTS-D8D47C（设置 UI，R08）。

（合计 14+18+42=74；分类计数在报告映射表逐叶落定，此处为预登记。）

## 四、T03/T04 边界（写进报告）

- T03（本 Task）：会话**树/分支/重试/回退**语义与存储——谱系、分支头、fork 复制、
  reset 标记、检查点与文件版本、用户消息落盘（作为消息树的写路径）。
- T04：统一**历史投影/分页/导出**——如何把 append-only 消息树投影成 UI 历史页
  （/messages 分页、history-overview、content 延迟加载、compact transcript）。
  T03 交付的 messages.parent_message_id/branch_heads 是 T04 投影的**数据源**；
  T03 不实现分页游标与投影排序策略。

## 五、差异清单预登记（报告 §十 正式版）

- D1（语义加固，A06 要求）：现役文件恢复**无外部修改冲突检测**（盲写回，
  锚点 `core/workspace-snapshots.ts:584-620` `_restoreChange`、
  `lib/checkpoint-store.ts:94-108` `restore`）。Rust 侧 rewind 恢复前逐文件 sha256 比对，
  冲突文件拒绝覆盖并在收据如实标注。此为对现役缺口的**有意加固**，非对等复刻。
- D2（语义对齐声明）：fork 共享历史**保留消息 ID**（与现役
  `session-manager.js:1113 createBranchedSession` 一致），仅 parent 重链。
- D3（存储形态差异）：现役为 JSONL 文件树 + session-manifest.db；Rust 侧统一入
  runs.db（v8），谱系/分支头/消息树/检查点/文件版本为关系表。语义等价，载体不同。
- D4（rewind 可达面差异）：现役 rewind 仅经 TOOL-REWIND（无 REST 路由，
  `lib/tools/rewind-tool.ts`）；Rust 侧本 Task 交付 REST 入口（R08 再补工具面），
  冲突检测语义先行落地。
- D5（确认卡差异）：现役 rewind 有 confirm 卡（5min 超时，拒绝/超时如实不回退）；
  Rust 侧 REST 入口由调用方（桌面 UI，R08）持有确认交互，服务端执行入口本身
  幂等且冲突拒绝，无需服务端确认卡。

## 六、schema v8 设计（一次性迁移，fingerprint 固定）

- `sessions` 增列：`parent_session_id TEXT`、`fork_point_message_id TEXT`、
  `lineage_depth INTEGER NOT NULL DEFAULT 0`、`lifecycle TEXT NOT NULL DEFAULT 'active'`、
  `pinned_at_unix_ms INTEGER`、`pin_order INTEGER`、`memory_enabled INTEGER NOT NULL DEFAULT 1`、
  `authorized_folders_json TEXT NOT NULL DEFAULT '[]'`、`permission_mode TEXT`、
  `archived_at_unix_ms INTEGER`、`last_activity_unix_ms INTEGER`。
- 新表 `session_branch_heads(session_id PK, head_message_id, observed_tail_message_id,
  revision INTEGER, head_resolution TEXT, updated_at_unix_ms)`。
- 新表 `session_checkpoints(session_id, name, target_message_id, turn_input_message_id,
  created_at_unix_ms, message_count, PK(session_id,name))`（latest 覆盖/其余冲突拒绝/上限 200）。
- 新表 `checkpoint_file_versions(checkpoint_id, file_path, sha256, size_bytes,
  recorded_at_unix_ms, PK(checkpoint_id,file_path))`；`checkpoints(checkpoint_id PK,
  session_id, name, kind, created_at_unix_ms)`。
- 新表 `session_files_checkpoint`（文件级改前存档，对应 checkpoint-store.ts：
  `file_checkpoints(checkpoint_id PK, session_id, file_path, content BLOB, encoding TEXT,
  reason TEXT, created_at_unix_ms)`）。
- `messages` 表重建（参照 v7 先例）：PK 改 `(session_id, message_id)`，增
  `parent_message_id TEXT`、`branch_id TEXT`、`entry_type TEXT NOT NULL DEFAULT 'message'`。
  理由：fork 复制保留 message_id → 全局唯一 PK 必须改为会话内唯一。
