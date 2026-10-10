# R06-T05 差异台账（Design → Implementation → Incumbent）

Task Base: `b27dc5e4be9aed4763dda60519f0a04042fdfade`
本台账分三部分：D 项（对现役的行为差异，逐条锚定现役 file:line）、R 项（实现相对设计稿的细化）、F 项（RED→GREEN 过程中的修复记录）。Round 1-4 现役锚点亲验于设计稿 RC-1 勘察（01_design §0，base 工作区）；Round 5 锚点（preview/bridge/file-history/fs）与本页标注的复核点均在本轮亲自逐行复读。

## D 项：对现役（incumbent）的行为差异

### D1 存储：sidecar JSON → SQLite v9 表；sf_ id 逐字节保留
- 现役：`lib/session-files/session-file-registry.ts` sidecar `{sessionPath}.files.json`（version 1，files map + refs，01_design §0）；sf_ id = `"sf_" + sha256hex(JSON.stringify([ownerKey, sourceKey || identityKey]))[0..16]`（:868-874）。
- 候选：v9 迁移建 `session_files` / `session_file_aliases` / `session_file_refs` / `file_history_snapshots` 表（`rust/crates/lingxi-adapters/src/storage/migrations.rs` V9_SQL）；sf_ id 算法逐字节同现役（`sessionfiles.rs`）；旧 id 经 aliases 表解析（现役 legacyFileIds 数组的结构化）。
- 锁定测试：`session_file_store`（adapters 12 例）、`r06_t05_session_files` 13 例。

### D2 属主键：候选新数据只用 `id:{session_id}`
- 现役：`sessionFileOwnerKey`（session-file-registry.ts :876-888）`id:{sessionId}` 优先，否则 `path:{sessionPath}`——归档 rename 后 path 键失效。
- 候选：新写入恒 `id:{session_id}`；导入/旧数据经 legacy_file_paths 保留 path 证据。候选侧结构性不存在归档失效问题。
- 锁定测试：`r06_t05_session_files`（owner 键与 legacy 列断言）。

### D3 冷度信号：jsonl mtime → `sessions.last_activity_unix_ms`
- 现役：`cleanupColdSessions` 以会话 jsonl mtime 判冷（registry :517/:574，`core/engine.ts:1844` 委托），阈值 `SESSION_FILE_CACHE_INACTIVE_TTL_MS = 72*60*60*1000`（session-file-registry.ts :9）。
- 候选：`sessions.last_activity_unix_ms`（v8 已加列）承载同一语义；阈值 72h 不变（`SESSION_FILE_CACHE_INACTIVE_TTL_MS` 同名同值）。
- 锁定测试：session_files 冷会话清理套件。

### D4 ticket 密钥：securityDir → `{runtime_dir}/resource-ticket-key`
- 现役：`core/resource-ticket-service.ts :7/:110`（`securityDirPath(lingxiHome)/resource-ticket-key`）。
- 候选：私有运行目录 `{runtime_dir}/resource-ticket-key`（0600，首用生成持久化）。
- 锁定测试：`r06_t05_resources`（票据签发/校验/过期/动作绑定）。

### D5 路由现代化（前缀迁移）
- 现役：`/api/upload*`、`/api/preview/html`（html-preview.ts）、`/api/bridge/media/:token`（bridge.ts:676-701）、`/file-history/*`（file-history.ts）、`/fs/*`（fs.ts）、`/resource-io/*`、`/resources/*`。
- 候选：统一进 `/lingxi/v1/...`（uploads → `/lingxi/v1/sessions/{id}/attachments/*`；preview → `/lingxi/v1/preview/html*`；bridge media → `/lingxi/v1/bridge/media/{token}`；file-history → `/lingxi/v1/file-history/*`；fs → `/lingxi/v1/fs/read|read-base64`）。
- 锁定测试：各 Round 路由套件（preview/bridgemedia/filehistory/fsread/resources/resourceio/session_files）。

### D6 uploads/ 无会话根 24h 清理不适用
- 现役：`server/routes/upload.ts` :11 注释 + `cleanOldUploads` :186（无 sessionPath 的 `{lingxiHome}/uploads/` 根，24h 清理）。
- 候选：附件路由一律会话 scoped（归属闸强制），不存在无主上传面，该清理面结构性缺席。
- 锁定测试：`r06_t05_session_files` 路由归属闸 404/403。

### D7 watch 机制：fs.watch → mtime/size 轮询
- 现役：`lib/resource-io/resource-watch-registry.ts` 递归 watcher。
- 候选：tokio interval 轮询（`resourceio.rs` WatchEntry/WatchSnapshot；80ms 去抖语义保留为合并窗口；refcount/UUID 订阅/诊断保留）。机制差异，语义等价。
- 锁定测试：`r06_t05_resourceio` watch 轮询链（修改/删除/重命名 diff）。

### D8 文件历史触发源：三源 → 一源（ResourceIO 写路径捕获）
- 现役：`lib/file-history/file-history-service.ts` 三触发源——engine 事件总线 tap（handleResourceEvent :138-163，150ms 防抖 _captureSoon :215-225）+ 递归 watcher（workspace-watcher.ts）+ 基线扫描 _sweep（:243-269）；restore 路由再显式 captureNow("restore")（file-history.ts :88）。存储为每工作区独立 history.sqlite 双表（files + snapshots，gzip 内容、op_context、files.deleted_at，history-store.ts :54-74）。
- 候选：一源——ResourceIO `write`/`write_expected_version` 成功后同步捕获（`resourceio.rs` `capture_history`，emit 门控 = 现役 emit:false 无事件语义；origin 默认 "event"，restore 路由经 `OpContext.capture_origin` 传 "restore"）。存储为 runs.db 内 v9 `file_history_snapshots` 单表（无 gzip/op_context/deleted_at 列）。防抖不引入：同步捕获的净落库状态 = 现役防抖后状态（同窗合并臂一致）。watcher/sweep/删除与重命名历史标记顺延（叶的 4 条路由断言不含触发源覆盖）。
- 锁定测试：`record_snapshot_merge_window_and_restore_semantics`、`capture_from_disk_policy_gates`、`resourceio_write_captures_history_with_origin`、`file_history_routes_full_chain_and_error_vocabulary`（restore 后版本表首行 origin="restore"）。
- 合并窗语义逐行镜像 history-store.ts :84-116：同 sha → unchanged；窗内（<60s、非负、双侧非 restore）→ UPDATE merged（`update_file_snapshot`）；否则 INSERT。latest 探测 = `latest_file_snapshot_meta`（:87-89 的 captured_at DESC,id DESC LIMIT 1）。

### D9 mount/url provider 缺席 → `unsupported_provider` 显式 400（J2）
- 现役：mount/url provider 存在于内核（lib/resource-io/ providers）。
- 候选：`refuse_if_mount_or_url` → 400 `unsupported_provider`（resourceio.rs）。响亮拒绝，不静默降级。
- 锁定测试：`r06_t05_resourceio`（mount/url ref 的 write/read 拒绝断言）。

### D10 docx/xlsx 转换顺延（J3）
- 现役：`server/routes/fs.ts` :118-169（mammoth / ExcelJS 转 HTML）。
- 候选：不移植（npm 库无 Rust 对应物入候选依赖纪律）；叶图 #23-24 只含 read/read-base64。
- 影响：叶面缺席而非降级——候选路由表无这两面，请求走 404。

### D11 Cargo.lock 依赖边（全部为已锁定包版本，只加边）
- 设计稿登记 hmac/uuid/rand 三条候选边；实现期 uuid/rand 经 getrandom 手写替代（resources.rs `uuid_v4` 注释"手写，避免 uuid feature 进锁"；桥媒体 token 32B 同走 getrandom），**未加边**。
- 实际净增三边：hmac 0.13.0（resource-ticket-service.ts signBody 的 HMAC-SHA256）、regex 1.13.1（html-preview.ts 素材引用改写/`<head>` 注入正则）、percent-encoding 2.3.2（JS encodeURIComponent/decodeURIComponent/decodeURI 保留集语义）。三者均为已锁定传递成员；`cargo update --offline -p lingxi-service` 后 Cargo.lock 仅 +3 行引用，无新包/新版本（git diff rust/Cargo.lock 亲验）。
- sha2 0.11.0 为 R02-T03 既有边（filehistory 的 workspace hash 复用，无新边）。

### D12 查询严格化（T04 D1 先例的延展）
- 现役：`Number(query)||默认` 静默吞非法值；未知 query 键不报错。
- 候选：events（resourceio Round 4）起，file-history 四叶面与 fs 两叶面同样严格——未知/重复 query 键 → 400；file-history 的 id/snapshotId 非正整数 → 400（词汇与现役 :57/:79 一致，但现役 `Number("abc")→NaN` 判 invalid 是同语义，未知键放行才是差异）。
- 锁定测试：filehistory/fsread 路由套件的 unknown/duplicate query 断言。

### D13 会话删除保护映射
- 现役：归档 sidecar 随行（rename）；永久删除 = `.deleting` 暂存 → sidecar 随行 → 一并 unlink → `purgeSessionArtifacts`（server/routes/sessions.ts :489/:516/:550）。
- 候选：归档不动 session_files 行（无路径键，无失效问题）；永久删除 → external 行删除但**用户文件保留**、managed_cache 载荷删除 + 行删除（`delete_session_file` + refs 清理）。
- 锁定测试：`r06_t05_session_files` 删除保护套件。

### D14 清理调度：启动清扫 + 24h setInterval → bootstrap 清扫一次
- 现役：`server/index.ts:563-570`（启动 fire-and-forget + 24h setInterval）。
- 候选：bootstrap 清扫一次 + 服务 API（幂等、冷度闸）；常驻定时器不引入（R02-T06 关停纪律）。

### D15 ResourceIO 路由面细化（Round 4 汇总）
- 归一失败 → 400 `invalid_resource_ref`（resource-refs.ts 同义表端口；现役抛无 code 的 Error，路由同样 400——候选人给稳定 code）。
- local_fs 目录读 → 400（现役 readFileSync EISDIR → 500 系；候选响亮 400 invalid 系）。
- 写内容映射：现役 `decodeWriteContent`（resource-io.ts :157-162）`String(body?.content ?? "")`——对象静默变 `"[object Object]"`；候选非标量 string → number/bool `to_string`、对象/数组 → 400（禁止静默降级红线）。
- list 排序、mtimeMs 整数化：候选人 wire 形状字段同现役，mtimeMs 取整（现役 fs mtimeMs 浮点直传；候选 u64 毫秒，测试钉死）。
- copy op 未映射：现役路由表无 copy 叶面，候选同样无。
- trash 根：`{data_home}/trash`（现役 sandbox-resource-io.ts :64 `path.join(lingxiHome, "trash")` 的候选映射）。
- 共享 ResourceAccess 实例：bootstrap 一次构建，ResourceIO 与资源面共用同一授权闸（设计文档 :82）。
- workspace 授权闸在 bootstrap  hoist：workspace 缺席时 ResourceIO local_fs provider 501（现役 provider_not_available），不延迟到请求期。
- search 路由 query-only、路由 ctx 映射 `ctx_from_body`（operationContextFromBody :130-146 的候选简化：路由面恒本地属主，sessionId/sessionPath/reason 透传）。

### D16 预览/桥媒体/fs 的候选收窄与加固（Round 5）
- 桥媒体白名单比较：现役 `_assertAllowed` 用身份键（大小写折叠，media-publisher.ts :113-120）；候选比较 canonical 路径原样——大小写变体只会被**拒绝更多**，绝不扩大披露面。
- 桥媒体 baseUrl 配置源：现役 `engine.getBridgeMediaPublicBaseUrl?.() || env LINGXI_BRIDGE_PUBLIC_BASE_URL`（bridge-manager.ts :680-760）；候选无 engine 配置面对应物，只取环境变量回退——**待裁决项**（engine 配置面归属后续阶段接线）。
- 桥媒体 publish 为服务内 API（叶 #18 只断言 token 读路径）；桥外发调用方属后续阶段。
- fs 授权根：现役每请求现算 lingxiHome + 全体 agent desk 并集（fs.ts :77-86）；候选 = data_home + workspace 单根（单工作区实例），bootstrap 一次构建。
- fs 读面加 20MB 上限（01_design :87 登记），413 "file too large" 与现役 docx/xlsx 面（fs.ts :128/:149）词汇一致；现役 read/read-base64 无上限。
- file-history 单工作区：agentId 只做存在性必填（file-history.ts :23/:74 语义），不映射多 desk（现役 resolveWorkspaceRoot :4-8 按 agentId 解析）。
- preview 资产根：requested sourceRootPath 须含 sourceDir 否则回落 sourceDir realpath（html-preview.ts 语义镜像）；symlink/越界/空段/`..`/反斜杠/NUL 一律 404 空体。

## R 项：实现相对设计稿（01_design/design.md）的细化

- R1 filehistory `RecordOutcome` 三态枚举（Inserted/Merged/Unchanged 携带行 id）镜像现役 `RecordSnapshotResult`（history-store.ts :32-35），服务级断言可钉 id 不变性。
- R2 `capture_from_disk` 返回 bool 而非 Result：捕获失败一律吞（tracing::warn 留痕），镜像现役 `_capture` 的 try/catch log（file-history-service.ts :227-241）；写路径绝不被历史捕获阻断。
- R3 `FileHistoryService::new` 的 workspace canonical 失败 → 按未跟踪处理（镜像现役 statSync 失败跳过 :90-91），不阻断 bootstrap。
- R4 restore 路由不显式二次 captureNow：写路径已带 `capture_origin="restore"` 捕获一行；现役的显式 captureNow 在候选人语义下只会撞 unchanged（同 sha），净落库状态一致。
- R5 fsread `lexical_resolve`：手写 `..` 折叠（根处 `..` 吞掉，镜像 Node path.resolve("/..")==="/"），不引第三方 path 库。
- R6 preview `file:` URL 解析手写（不引 url  crate 新边），`decode_uri`/`decode_uri_component`/`encode_uri_component` 按 JS 保留集逐字节实现，金样测试钉死。
- R7 桥媒体 `resolve` 两段式借用（先不可变判定过期/预算 → 回收；再重探源文件 → get_mut 计下载），语义与现役 :84-107 一致。

## F 项：RED→GREEN 修复记录（诚实留痕）

- F1 preview `decode_uri` 测试期望写错：误期 JS decodeURI 解 %2F；实际 decodeURI 保留保留字集（%2F 保持编码）。实现正确，修测试。
- F2 preview 资产 Content-Type 初版 `Box::leak` 静态化 mime（每请求泄漏）→ 改为响应 headers_mut 直插。
- F3 bridgemedia `resolve` 借用冲突（get_mut 后 remove）→ 两段式（R7）。
- F4 bridgemedia 文件名断言误期 label 优先；现役序为 `filename || label || basename`（media-publisher.ts :64）——修测试并注释锚点。
- F5 lib.rs bootstrap 的 bridge_media map_err 括号结构修正；错误映射统一走 `ServiceStartupError::Storage(StorageError::InvalidRequest{..})`（workspace_access hoist 先例）。
- F6 filehistory `StorageError` 导入路径修正（`lingxi_kernel::ports::StorageError`，与 sessionfiles.rs 一致）。
- F7 filehistory `record_snapshot_merge_window_and_restore_semantics` 的 b2 断言误期窗内增生行；窗内 merged 进同一行是实现正确行为——修测试（b2 出窗 68s 后录入）。
- F8 clippy 清扫：`result_large_err` 5 处按项目既有纪律加 `#[allow]` + 理由注释（lib.rs gate_session_tree_write 先例）；bridgemedia question_mark；resources redundant_closure；sessionfiles io_other_error；bridgemedia 测试 cloned_ref_to_slice ×5；preview 测试 assertions_on_constants（改 const 块）；三个测试夹具 dead_code 字段补 `#[allow]`。
- F9 fmt 初检发现既有未格式化段（preview 测试尾部），`cargo fmt --all` 归零后复检通过。

## 环境污染申报

- Round 5 中途曾后台并行跑一次全量回归（b85j0b85r），与 Round 5 编辑窗口重叠，报告出 1 例假 doctest 失败（半编辑树的瞬时编译）——该次运行**不作为有效证据**。`cargo test -p lingxi-service --doc` 复跑通过（0 doctest）。最终全量回归（06_workspace_regression/full_workspace_final.log）在所有候选文件定型后干净树上执行，为唯一权威回归证据。
- r00_management_leaves 的 LAN 阶段失败为 macOS 应用防火墙（ALF，cdhash 维度）环境阻塞，五点证据链见 06_workspace_regression/r00_firewall_alf_blocked.txt；最终回归中复现同一签名（192.168.3.5 LAN 自检地址写读停滞），判定不变。
