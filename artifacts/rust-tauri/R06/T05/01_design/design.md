# R06-T05 设计稿 — 会话文件、资源与产物身份

- TASK_ID: R06-T05
- TASK_BASE_SHA: `b27dc5e4be9aed4763dda60519f0a04042fdfade`（分支 `codex/rust-tauri-migration`，= HEAD）
- 验收: R06-A09（旧附件迁移后可读）、R06-A10（缓存回收不误删交付）
- 任务书原文锚点: `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R06_上下文、会话语义、记忆与知识库.md` T05 节（4 条怎么做 + 必须交付 ResourceService / SessionFile迁移适配 / 文件引用检查器）
- 工具链: `/Users/study_superior/.cargo/bin/cargo` 1.98.1（证据 `../00_toolchain/toolchain.txt`）

## 0. 现役生产链勘察结论（RC-1，行号亲验于 base 工作区）

T05 在现役 TS 侧消费的是一条完整链：

```
上传/登记: server/routes/upload.ts（/api/upload、/api/upload-blob）
  → lib/session-files/session-file-registry.ts（SessionFileRegistry）
      sf_ 身份: buildSessionFileId :868-874
        = "sf_" + sha256hex(JSON.stringify([ownerKey, sourceKey || identityKey(realPath)]))[0..16]
      ownerKey: sessionFileOwnerKey :876-888（id:{sessionId} 优先，否则 path:{sessionPath}）
      sourceKey: buildSessionFileSourceKey :24-33（"{ns}:{sha256hex(JSON.stringify(parts))}"）
      托管缓存目录: sessionFilesCacheDir :17-22（sha256hex(ownerKey)[0..24] 于 {lingxiHome}/session-files/）
      sidecar: {sessionPath}.files.json（version 1, files map + refs）
stage/materialize: lib/tools/output-file-tool.ts（stage_files, origin=stage_files）
  → lib/resource-io/materialize-tool.ts + providers/session-file-resolver.ts
资源读面: core/resource-service.ts（envelope/resolveContent/etag=mtime36-size36）
  → lib/resources/resource-envelope.ts（res_ 前缀, schemaVersion 1）
  → core/resource-ticket-service.ts（HMAC-SHA256 base64url, TTL 5min, action resources.content）
  → server/routes/resources.ts（GET /resources/:id、POST ticket、GET|HEAD content, Range/ETag）
ResourceIO: lib/resource-io/（kernel + local-fs/session-file/resource/url/mount providers
  + resource-event-bus.ts + resource-watch-registry.ts + resource-access-policy.ts）
  → server/routes/resource-io.ts（stat/read/list/search/write/write-expected-version/
     rename/move/trash + subscribe/subscriptions/watch-diagnostics/events?since=/watch）
预览: server/routes/html-preview.ts（POST /api/preview/html, pv_ id, 32B token, 10min TTL, CSP）
桥媒体: lib/bridge/media-publisher.ts + server/routes/bridge.ts:676-701（token 读, 50MB→413）
文件历史: lib/file-history/（service 275 + store 202 + policy 62 + watcher 67 行）
  → server/routes/file-history.ts（files/versions/snapshot GET + restore POST）
fs 直读: server/routes/fs.ts（/fs/read、/fs/read-base64、/fs/docx-html、/fs/xlsx-html）
清理: engine.ts cleanupColdSessionFiles（72h 冷会话, jsonl mtime 判定;
  managed_cache→删载荷+expired, sidecar 保留; external 永不删）
  + server/index.ts:563-570（启动清扫 + 24h setInterval）
  + upload.ts:185-189（无会话 uploads/ 根 24h 清理）
会话删除保护: server/routes/sessions.ts:380-600（归档 sidecar 随行;
  永久删除 .deleting 暂存→sidecar 随行→一并 unlink→purgeSessionArtifacts）
引用收集: session-file-registry.ts collectSessionFileReferenceIdentities
  （[SessionFile] {json} 标记 + [attached_image|video|audio: ...] 标记 + typed 对象）
```

Rust 侧现状（base）：只有 T03 的 checkpoint 文件版本哈希（`session_tree.rs:list_session_file_hashes`，rewind 见证，与 sf_ 注册表是不同概念）。**sf_ 注册表在 Rust 尚不存在，T05 新建**。可复用：`resourceaccess.rs`（R02/R04 ResourceAccess 授权闸，R04-A07/A08 已验）、`filetools.rs`（R04-T04 原子写/指纹/TOCTOU）、`sessions.rs:get_for`（归属闸 404/403）、`session_admin`（归档/删除路由已存在）、v1-v8 迁移框架（`migrations.rs`，fingerprint 收据 + user_version 双记录）、`sessions.last_activity_unix_ms` 列（v8 已加，冷度判定的 DB 映射）。

## 1. 验收映射（Requirement → Entry → Owner → Implementation → +Test → -Test → Evidence）

### R06-A09 旧附件迁移后可读

| 项 | 内容 |
|---|---|
| Requirement | 旧消息引用 sf_ 与原始附件；导入后各客户端读取路径身份/文件对应正确、无断链；未授权客户端拒绝 |
| Entry | `POST /lingxi/v1/sessions/{id}/files/import-legacy`（sidecar 导入适配器入口）+ 三条读取路径 |
| Owner | T05（本任务） |
| Implementation | ① `adapters/storage/session_files.rs` v9 表 + `SessionFileStore`；② `service/sessionfiles.rs` 导入适配器（读 sidecar v1 JSON → 逐文件保留 sf_ 原 id 落库；external 不复制，real_path 指向原文件；managed_cache 载荷复制到 Rust 托管目录并记 legacy_file_paths）；③ 引用检查器 `collect_reference_identities`（从 T04 权威消息读 [SessionFile]/attached 标记）+ `check_session_integrity`（每个被引用 sf_ 可解析、状态 available、文件存在，否则列入断链清单）；④ 读取路径：`GET /lingxi/v1/sessions/{id}/files`（清单）、`GET /lingxi/v1/resources/{res_sf_...}` + `/content`（ResourceService）、`POST /lingxi/v1/resource-io/read`（session-file ref） |
| +Test | `a09_legacy_import_read_paths_preserve_identity`（现役算法金样 sf_ id 逐字节相等；导入后三条读路径字节一致；消息标记引用全部可解析）；`sf_id_formula_matches_incumbent_golden`（`sf_`+sha256(JSON.stringify 序列）[:16]，用按现役公式手工计算的期望向量）；`fork_rewrites_ids_and_keeps_legacy_aliases`（fork 后旧 id 经 alias 可解析） |
| -Test | `unauthorized_principal_gets_404_or_403_on_all_read_paths`（跨主体读清单/envelope/content/resource-io 一律 404/403）；`broken_reference_is_reported_not_hidden`（删掉底层文件后引用检查器报断链，读取 404/410，绝不静默成功） |
| Evidence | `05_targeted_tests/` 对应套件输出 |

### R06-A10 缓存回收不误删交付

| 项 | 内容 |
|---|---|
| Requirement | 附件已交付并被历史引用；运行缓存清理后权威文件仍可用，临时副本按策略清理 |
| Entry | `SessionFileService::cleanup_cold_sessions`（bootstrap 清扫 + 测试直调） |
| Owner | T05（本任务） |
| Implementation | 冷度 = `sessions.last_activity_unix_ms` 距今 > 72h（现役 jsonl mtime 的 DB 映射，差异 D3）；仅 `storage_kind=managed_cache` 删载荷文件 + status→expired + expires_at_ms；`external`（权威/用户文件）永不删；清理前过引用检查器闸：仍被**非冷**会话引用的文件跳过（现役以冷度为保护，候选增加跨会话引用闸——引用仍有效的产物不可误删，任务书第 4 条） |
| +Test | `a10_cleanup_keeps_delivered_external_and_cleans_cold_managed`（交付且被引用的 external 文件清理后仍可读；冷会话 managed 载荷删除且状态 expired）；`cleanup_skips_managed_file_still_referenced_by_live_session`（跨会话引用闸） |
| -Test | `cleanup_never_touches_external_or_warm_files`（external + 热会话 managed 均不动）；`expired_file_read_returns_410_not_bytes`（expired 后 content 读 410，不返回陈旧字节） |
| Evidence | `05_targeted_tests/` 对应套件输出 + 文件清单断言 |

## 2. 交付物与模块划分

| 交付物 | 模块 | 说明 |
|---|---|---|
| SessionFile 迁移适配 | `lingxi-adapters/src/storage/session_files.rs`（新建，v9 SQL 存取）+ `lingxi-service/src/sessionfiles.rs`（新建，注册/导入/fork/清理/引用检查/stage + 会话文件路由） | sf_ 身份公式逐字节镜像现役；sidecar→DB 导入 |
| ResourceService | `lingxi-service/src/resources.rs`（新建：envelope + resolveContent + ticket + 路由） | res_ 前缀、etag mtime36-size36、六类错误码镜像 |
| 文件引用检查器 | `sessionfiles.rs` 内 `filerefs` 子模块 | 标记解析 + 完整性报告 + 清理闸 |
| ResourceIO 面（叶 #31-45 消费） | `lingxi-service/src/resourceio.rs`（kernel/provider/event bus/watch registry + 路由） | local_fs provider 复用 `ResourceAccess`；session_file/resource provider 经注册表/ResourceService；mount/url 缺席但显式拒绝（D9） |
| 附件上传 | `sessionfiles.rs` 路由：local 导入 + blob 上传 | MAX_FILES=9、mime 白名单、sourceKey 去重复用 |
| HTML 预览 | `lingxi-service/src/preview.rs` | pv_ id、32B token、10min TTL、CSP、素材根限制 |
| 桥媒体 | `lingxi-service/src/bridgemedia.rs` | publish（内部 API）+ token 读路由，50MB→413 |
| 文件历史 | `lingxi-service/src/filehistory.rs` + v9 `file_history_snapshots` 表 | 4 路由；捕获源 = ResourceIO 写路径（D8） |
| fs 直读 | `lingxi-service/src/fsread.rs` | /fs/read、/fs/read-base64（20MB 上限，授权根） |

### v9 迁移（`migrations.rs` V9，fingerprint 固定）

```sql
CREATE TABLE session_files (
  file_id TEXT PRIMARY KEY,                 -- sf_...（导入保留原 id）
  owner_session_id TEXT NOT NULL,           -- 候选统一 id 键（D2）
  owner_key TEXT NOT NULL,                  -- 现役兼容 "id:{sessionId}"
  source_key TEXT,                          -- 去重键
  identity_key TEXT NOT NULL,
  file_path TEXT NOT NULL,
  real_path TEXT NOT NULL,
  storage_kind TEXT NOT NULL,               -- external | managed_cache
  status TEXT NOT NULL,                     -- available | missing | expired
  label TEXT, filename TEXT NOT NULL, mime TEXT NOT NULL,
  size_bytes INTEGER NOT NULL, mtime_ms INTEGER NOT NULL,
  is_directory INTEGER NOT NULL, file_kind TEXT NOT NULL,
  origin TEXT NOT NULL,
  registered_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
  expires_at_ms INTEGER,
  legacy_file_ids_json TEXT NOT NULL DEFAULT '[]',
  legacy_file_paths_json TEXT NOT NULL DEFAULT '[]'
);
CREATE UNIQUE INDEX idx_session_files_source
  ON session_files(owner_session_id, source_key) WHERE source_key IS NOT NULL;
CREATE TABLE session_file_aliases (          -- fork/导入旧 id → 现行 id
  alias_file_id TEXT PRIMARY KEY,
  canonical_file_id TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL
);
CREATE TABLE session_file_refs (             -- 显式引用记录（导入 refs + 登记/交付）
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  file_id TEXT NOT NULL, session_id TEXT NOT NULL,
  message_id TEXT, ref_kind TEXT NOT NULL, created_at_ms INTEGER NOT NULL
);
CREATE TABLE file_history_snapshots (        -- 文件历史（#11/#19-21）
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_hash TEXT NOT NULL,              -- sha256(normalized root)[..16]，镜像现役
  rel_path TEXT NOT NULL, captured_at_ms INTEGER NOT NULL,
  origin TEXT NOT NULL, size_bytes INTEGER NOT NULL,
  sha256 TEXT NOT NULL, content BLOB NOT NULL  -- 仅文本策略文件，上限镜像现役
);
```

迁移保真测试覆盖 v7→v8→v9 链（在 v7 库造数据 → 顺序升级 → 断言旧表数据无损 + 新表存在 + fingerprint 收据 + user_version=9）。

### 路由与 auth 分类（`auth.rs` classify_route 增补 + route_policy_table 断言）

| 路由 | 分类 | 依据 |
|---|---|---|
| `GET /lingxi/v1/sessions/{id}/files`、`POST .../files/import-legacy`、`POST .../attachments/local`、`POST .../attachments/blob` | Scope("chat") | 会话子树后缀，同 T04 /history |
| `GET /lingxi/v1/resources/{id}`、`POST .../ticket` | Authenticated + 处理器内归属复核 | 镜像现役 resources.ts |
| `GET|HEAD /lingxi/v1/resources/{id}/content` | Public + 处理器内双凭证（有效 ticket 或会话主体） | 现役 ticket 即凭证；远端客户端只拿 ticket URL，绝不发未授权绝对路径（任务书第 2 条） |
| `POST /lingxi/v1/resource-io/{op}`、`GET .../events` | LocalOnly | 桌面本地文件操作面，失败闭合 |
| `POST /lingxi/v1/preview/html` | Scope("chat") | 建立预览 |
| `GET|HEAD /lingxi/v1/preview/html/{id}[/assets/...]` | Public + token 即凭证（无效/过期 404） | 镜像现役 html-preview.ts |
| `GET /lingxi/v1/bridge/media/{token}` | Public + token 即凭证 | 镜像现役 bridge.ts:676 |
| `GET /lingxi/v1/fs/read`、`/fs/read-base64` | LocalOnly | 桌面直读本地文件 |
| `GET /lingxi/v1/file-history/{files,versions,snapshot}`、`POST .../restore` | LocalOnly | 桌面查看器 |

## 3. 57 叶逐叶归类

标注写回 `R06_LEAF_MAP.json`（`t05_status`/`t05_anchor`），口径同 T04。

### 真实实现 implemented（37 叶）

- **#11** ROUTE_BEHAVIOR file-history restore POST → filehistory.rs restore 路由（ResourceIO 写回 + captureNow "restore" 快照）。
- **#12-17** ATTACHMENT×6（blob media/reuse/voice、local 目录引用/文件快照/复用）→ attachments 路由 + 注册表（A09 核心）。
- **#18** BRIDGE media token 读 → bridgemedia.rs（publish 为服务内 API，桥外发调用方属后续阶段，叶断言只要求 token 读路径）。
- **#19-21** FILE-HISTORY×3（files/versions/snapshot GET）→ filehistory.rs。
- **#23-24** FS read-base64 / read → fsread.rs。
- **#26-28** HTML-PREVIEW×3 → preview.rs。
- **#29-30** RESOURCE-CONTENT GET/HEAD → resources.rs（Range 206/416、ETag 304）。
- **#31-45** RESOURCE-IO×15（events/list/move/read/rename/search/stat/subscribe×2/trash/watch/watch-diagnostics/unwatch/write/write-expected-version）→ resourceio.rs。叶断言模板中"mount 成功"份额因 mount provider 缺席改为显式拒绝（见待裁决 J2）；"url/session_file 不支持即明确拒绝"与"跨 provider 移动必须拒绝"全额实现并测试。
- **#46-47** RESOURCES 元数据 GET + ticket POST → resources.rs。
- **#53** TOOL-MATERIALIZE → resourceio materialize（本地直接回原路径；session_file/resource 经 resolver；越权/缺目标明确错误，不返回假路径）。
- **#55** TOOL-STAGE-FILES → sessionfiles stage_files 服务原语（登记 origin=stage_files；交付清单与实存文件一致；文件不存在/非绝对/无会话即失败，不宣称已交付）。R07 接线进工具层在台账申报。

### 合法份额 share（14 叶）

- **#3** DESKTOP file-edit（write-if-unchanged）：T05 份额 = resource-io write-expected-version（版本吻合写、冲突 409 不覆盖）；桌面 IPC 派发属 R09。
- **#6** DESKTOP file-read：份额 = resource-io read + fsread（绝对路径、20MB base64、版本元数据）。
- **#7** DESKTOP file-watch（identity=SUPPORT）：份额 = watch registry + resource.changed 事件总线（事件语义）；旧 IPC 监听注册属 R09。
- **#8/#9** DESKTOP write-binary / write-legacy：份额 = resource-io write（base64 解码落盘、父目录创建）。
- **#10** DESKTOP trash-item：份额 = resource-io trash（回收区 + 元数据，绝不混永久删除）。
- **#48/#50/#51/#52/#54/#56** TOOL edit/find/grep/ls/read/write：核心语义（命中替换、并发拒绝、符号链接逃逸拒绝、列表/匹配准确）由 R04-T04 filetools 已交付并验收（锚其既有测试）；T05 份额 = 同一授权闸 `ResourceAccess` 上的 resource-io 读写照；agent 工具层经 ResourceIO 目标包装（agent-tools.ts wrapResourceIoFileTools 的迁移）属 R07 运行时接线。
- **#49** TOOL-FILE：份额 = 副本字节一致 + 默认不覆盖（write-expected-version/materialize）；文档文本提取份额与 #22/#25 同因依赖冻结顺延（J3）。
- **#57** UI file-preview：份额 = resource 内容读 + resource.changed 重读信号（服务端数据源）；渲染与 viewer 属 R09（叶 stage_ids 已含 R09）。

### 后续阶段 deferred（6 叶）

- **#1** DESKTOP edit-command：纯桌面 webContents 派发（cut/copy/paste/selectAll 白名单），服务端无对应物 → R09（Tauri host）。见 J1。
- **#2** DESKTOP file-copy：现役实现就在 desktop/main.cjs（fs.copyFileSync + 普通文件/软链检查），服务端现役亦无 copy 路由 → R09。见 J1。
- **#4** DESKTOP file-open（shell.openPath）→ R09。见 J1。
- **#5** DESKTOP file-picker（系统对话框）→ R09。见 J1。
- **#22** FS docx-html：现役 mammoth 转换；Rust 锁定依赖树无等价物，新增依赖属治理决策 → 后续阶段。见 J3。
- **#25** FS xlsx-html：现役 ExcelJS 首表转换；同因 → 后续阶段。见 J3。

**计数：implemented 37 / share 14 / deferred 6 = 57。**

### 待裁决项（申报，不自行选边）

- **J1**：DESKTOP_BEHAVIOR 10 叶 stage_ids=['R04','R06']，R00 未标 R09；但 Electron IPC 处理器的物理归属是 Tauri host（R09）。T05 消费了服务端份额（6 叶 share），#1/#2/#4/#5 纯桌面份额无 R06 内后继任务可挂。请总控裁决：叶图补标 R09，或接受 T05 share/申报即闭环。
- **J2**：mount/url provider。叶断言模板含"获授权 mount 引用成功"；studio mounts 迁移在 R06 内无任何任务承接（T06 记忆/技能、T07 知识库、T08 评测）。T05 交付 mount/url 引用的**显式拒绝**（unsupported_provider，响亮不静默），成功路径需后续阶段认领（建议 R07 资源/工作室面）。
- **J3**：docx/xlsx HTML 转换需新依赖（mammoth/ExcelJS 的 Rust 等价物不在锁定树）。新增第三方包是否允许进 Cargo.lock 属阶段治理（R02 以来纪律：只允许复用已锁定版本）。T05 不新增任何新包/新版本（hmac/uuid/rand 边复用已锁版本，见 D11）。

## 4. 差异清单预登记（07_diff_ledger 骨架，实现后逐条锚定 file:line）

- **D1** 存储：sidecar JSON → SQLite v9 三表；sf_ id 逐字节保留；旧 id 经 aliases 表解析（现役 legacyFileIds 数组的结构化）。
- **D2** 属主键：候选新数据只用 `id:{session_id}`；现役 `path:{sessionPath}` 键在导入时转换并留 legacy_file_paths 证据（现役因归档 rename 失效的问题在候选侧结构性不存在）。
- **D3** 冷度信号：jsonl mtime → `sessions.last_activity_unix_ms`（72h 阈值不变）。
- **D4** ticket 密钥：securityDir/resource-ticket-key → `{runtime_dir}/resource-ticket-key`（0600，首用生成持久化）。
- **D5** 路由现代化：/api/upload* → /lingxi/v1/sessions/{id}/attachments/*；/api/preview/html → /lingxi/v1/preview/html；/resources、/resource-io、/file-history、/fs、/bridge/media 同理进 /lingxi/v1。
- **D6** uploads/ 无会话根 24h 清理不适用：候选附件路由一律会话scoped（归属闸强制），无无主上传面。
- **D7** watch 机制：fs.watch → mtime/size 轮询 watcher（tokio interval；80ms 去抖语义保留为合并窗口；refcount/UUID 订阅/诊断保留）。机制差异，语义等价。
- **D8** 文件历史触发源：现役三源（engine 事件 tap + 递归 watcher + 基线扫描）→ 候选一源（ResourceIO 写路径捕获，含 restore origin）。外部改动 watcher 与基线扫描顺延（叶的 4 条路由断言不含触发源覆盖）。
- **D9** mount/url provider 缺席 → `unsupported_provider` 显式 400（J2）。
- **D10** docx/xlsx 转换顺延（J3）。
- **D11** Cargo.lock 增加依赖**边**（hmac 0.13.0、uuid 1.26.1、rand 0.10.3——全部为已锁定包版本，无新包/新版本进入；R02 sha1/base64/sha2 先例）。lock 哈希随之变化，Cargo.lock 入候选清单。
- **D12** 查询严格化：未知/重复查询键、畸形游标/参数 → 400（T04 D1 先例），不镜像现役 Number()||默认 的静默吞。
- **D13** 会话删除保护映射：归档不动 session_files 行（候选无路径键，无失效问题）；永久删除 → external 行删除但**用户文件保留**、managed_cache 载荷删除 + 行删除；与现役"sidecar 随行/unlink + purgeSessionArtifacts"对齐。
- **D14** 清理调度：现役启动清扫 + 24h setInterval → 候选 bootstrap 清扫一次 + 服务 API（幂等、冷度闸）；常驻定时器不引入（R02-T06 关停纪律），后续如需由服务编排层加。

## 5. T05/T06+ 边界

- T05 拥有：文件附件/资源身份（sf_/res_）、会话文件生命周期（登记/fork/清理/删除保护）、引用检查、ResourceIO 面、预览、桥媒体读、文件历史、fs 直读。
- T06 拥有：记忆/人格/技能内容加载（技能包材料若以文件出现，经 T05 的授权闸读取，但发现/审核/启停归 T06）。
- T07 拥有：知识库导入/索引/检索（知识文档原文引用边界归 T07；T05 不管 knowledge store）。
- R07 拥有：agent 运行时把工具目标解析接进 ResourceIO（share 叶的工具包装份额）、stage_files 工具面接线。
- R09 拥有：桌面 IPC/渲染（#1/#2/#4/#5/#57 的桌面份额，J1）。
- 语音链路：T05 交付 upload-blob 的 voice-input 登记（#14）；speech-recognition 服务本体不属 T05（叶图未指派给 T05）。

## 6. 实现顺序（TDD）

1. v9 迁移 + SessionFileStore（RED: 迁移保真/CRUD/去重/alias/refs）→ GREEN。
2. sessionfiles.rs 注册/身份公式/导入/fork/引用检查/清理（RED: A09/A10 金样）→ GREEN。
3. resources.rs envelope/ticket/content（RED: 错误码/Range/ETag/越权）→ GREEN。
4. resourceio.rs kernel/providers/events/watch + 路由（RED: 15 叶面 + 跨 provider 拒绝 + 409）→ GREEN。
5. attachments/preview/bridgemedia/filehistory/fsread（RED: 各叶面）→ GREEN。
6. auth 分类断言 → 全量回归 → fmt/clippy → 台账 → 报告 → digest。
