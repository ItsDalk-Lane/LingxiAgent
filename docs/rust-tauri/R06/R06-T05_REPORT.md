# R06-T05 会话文件、资源与产物身份 — 实施报告

- TASK_ID: R06-T05
- TASK_BASE_SHA: `b27dc5e4be9aed4763dda60519f0a04042fdfade`（分支 `codex/rust-tauri-migration`）
- 验收 ID: R06-A09（导入后旧附件经一切客户端读路径可读、身份/文件对应正确、未授权客户端被拒）、R06-A10（附件已交付且被历史引用后，缓存清理保留权威文件可用、临时副本按策略清除）
- 工具链: `/Users/study_superior/.cargo/bin/cargo`（rustup shim → cargo/rustc 1.98.1，仓库根 `rust-toolchain.toml` 锁定）；全程未使用 homebrew cargo 1.93.0（证据 `artifacts/rust-tauri/R06/T05/00_toolchain/toolchain.txt`）
- 候选 digest：由 `scripts/rust-tauri/r06_candidate_digest.sh` 在全部文件写完后计算，值见 handback，**本报告不内嵌**（报告自身在 digest 范围内）

## 一、已完成的 Steps（任务书步骤逐条核对）

1. **v9 迁移 + SessionFileStore**（RED: `session_file_store` 迁移保真/CRUD/去重/alias/refs → GREEN）：v8 冻结未动，新增 v9 迁移建 `session_files`/`session_file_aliases`/`session_file_refs`/`file_history_snapshots` 四表；`v7_to_v9_chain_preserves_prior_data_and_creates_new_tables` 钉死 v7→v8→v9 链保真；fingerprint 由 `migration_idempotency` 既有闸覆盖。
2. **sessionfiles.rs 注册/身份公式/导入/fork/引用检查/清理**（RED→GREEN）：sf_ 身份公式逐字节镜像现役 `"sf_" + sha256hex(JSON.stringify([ownerKey, sourceKey || identityKey]))[0..16]`（session-file-registry.ts :868-874）；import-legacy 保旧 id；fork 重写 id 并保留 legacy alias；引用检查器收集 marker 身份并报告断引用；清理走冷度闸（72h TTL 同现役 :9）+ 引用保护。
3. **resources.rs envelope/ticket/content**（RED→GREEN）：res_ 信封、HMAC-SHA256 票据（`{runtime_dir}/resource-ticket-key` 0600 首用生成持久化）、内容 GET/HEAD（Range 206/416、ETag 304、Content-Disposition）。
4. **resourceio.rs kernel/providers/events/watch + 15 路由面**（RED→GREEN）：stat/read/list/search/write/write-expected-version/rename/move/trash/subscribe/subscriptions/watch/watch/{id}/watch-diagnostics/events；跨 provider 移动响亮拒绝；mount/url → `unsupported_provider` 400；watch 为 mtime/size 轮询（机制差异 D7）。
5. **attachments/preview/bridgemedia/filehistory/fsread**（RED→GREEN）：attachments local/blob 路由（归属闸先于一切实体读取）；preview POST + id HTML + 资产面；桥媒体 token 读（TTL 5min、下载预算 5、canonical 白名单）；file-history 四面（files/versions/snapshot/restore，merge 窗镜像 history-store.ts :84-116）；fs read/read-base64（20MB 上限、双根授权闸）。
6. **auth 分类断言 → 全量回归 → fmt/clippy → 台账 → 报告 → digest**：file-history 四面 + fs 两面全动词 LocalOnly（auth.rs 分类块 + route_policy_table 循环断言）；preview POST Scope("chat")、GET/HEAD Public；桥媒体 GET/HEAD Public。fmt exit 0、clippy `--locked --workspace --all-targets -- -D warnings` exit 0。台账 D1-D16/R1-R7/F1-F9 完成。digest 最后执行。

## 二、已交付的 Deliverables

- **ResourceService** → `rust/crates/lingxi-service/src/resources.rs`（1033 行）：res_ 元数据 GET、ticket POST、content GET/HEAD。
- **SessionFile 迁移适配** → `rust/crates/lingxi-adapters/src/storage/session_files.rs`（1204 行，v9 四表 store）+ `migrations.rs` V9_SQL（+87 行）+ `rust/crates/lingxi-service/src/sessionfiles.rs`（1897 行，注册/导入/fork/清理/删除保护/路由）。
- **文件引用检查器** → sessionfiles.rs `reference_checker_collects_marker_identities_and_reports_broken` 对应服务原语：收集消息历史中的 sf_/res_ marker 身份，缓存清理前核对引用表，被引用交付物绝不被回收（A10 核心）。
- 配套面：`resourceio.rs`（3167 行，15 路由 + watch + events + materialize）、`preview.rs`（968 行）、`bridgemedia.rs`（444 行）、`filehistory.rs`（701 行）、`fsread.rs`（238 行）、`filemeta.rs`（556 行）。

## 三、实际生产调用链（无平行构造者）

1. bootstrap（lib.rs）：storage（runs.db，v9 迁移后）→ `SessionFileStore` 适配器 → `SessionFileService`（sf_ 注册/导入/清理）→ `ResourceService`（ticket 密钥首用生成）→ `ResourceIoService`（共享 `ResourceAccess` 授权闸一次构建；workspace 缺席时 local_fs provider 501 hoist）→ `set_file_history` 接线（ResourceIO 写路径成功后 `capture_history`，D8）→ `PreviewService`/`BridgeMediaService`/`FileHistoryService`/`FsReadService`（data_home + workspace 双根）。
2. 路由注册（lib.rs :5444-5579）：会话文件 5 面（:5444-5464）→ 资源 3 面（:5466-5479）→ ResourceIO 15 面（:5481-5533）→ preview 3 面（:5535-5547）→ 桥媒体 1 面（:5549-5552）→ file-history 4 面（:5554-5572）→ fs 2 面（:5574-5579）。
3. 请求路径：auth 分类（auth.rs :755-770 file-history/fs LocalOnly 块）→ 归属闸（handler 内先于实体读取）→ 服务 → store。 accessors：lib.rs :1900-1935（session_files/resources/resource_io/preview/bridge_media/file_history/fs_read）。

## 四、代码修改清单

**修改（8 文件）：**
- `rust/crates/lingxi-adapters/src/storage/migrations.rs` +87：V9_SQL 四表。
- `rust/crates/lingxi-adapters/src/storage/mod.rs` +1：session_files 模块导出。
- `rust/crates/lingxi-adapters/src/storage/session_files.rs`（Round 5 追加）：文件历史三查询的 ORDER BY 镜像现役（:149/:158）+ `latest_file_snapshot_meta`/`update_file_snapshot`（:87-89/:104-106 镜像）。
- `rust/crates/lingxi-service/Cargo.toml` +13：hmac 0.13.0 / regex 1.13.1 / percent-encoding 2.3.2 三边（全部已锁定版本，D11）。
- `rust/Cargo.lock` +3：上述三边引用行，无新包/新版本。
- `rust/crates/lingxi-service/src/auth.rs` +272：T05 各面分类块 + route_policy_table 循环断言。
- `rust/crates/lingxi-service/src/lib.rs` +391：模块声明、ServiceState 字段、bootstrap 接线、路由注册、accessors。
- `rust/crates/lingxi-service/src/resourceio.rs`（Round 5 追加）：`OpContext.capture_origin` + `file_history` OnceLock + `capture_history`（D8）。
- `docs/rust-tauri/R06/R06_LEAF_MAP.json` +171/-57：57 叶 `t05_status`/`t05_anchor`（T01-T04 叶零触碰，diff 亲验）。

**新增（9 源 + 8 测试）：** service 侧 sessionfiles/resources/resourceio/preview/bridgemedia/filehistory/fsread/filemeta 八模块；adapters 侧 storage/session_files.rs；测试 `session_file_store`（12）+ `r06_t05_{session_files,resources,resourceio,preview,bridgemedia,filehistory,fsread}`（13/5/12/5/3/7/2）。

**未触碰的既有收口义务（按 spawn 授权不归本任务修）：** T03 R2-F-01~04、T04 F-01/F-02 原样保留；`.vscode/` 未动、未入候选。

## 五、现役语义逐条锚定（RC-2，行号亲验于 base 工作区）

全部差异逐条锚定在 `artifacts/rust-tauri/R06/T05/07_diff_ledger/diff_ledger.md`（D1-D16），关键锚点：sf_ 身份公式 session-file-registry.ts :868-874；属主键 :876-888；冷度 72h :9 + mtime 判冷 :517/:574；ticket 密钥 resource-ticket-service.ts :7/:110；uploads 24h 清理 upload.ts :11/:186；会话删除保护 sessions.ts :489/:516/:550；清理调度 server/index.ts:563-570；merge 窗 history-store.ts :84-116；files/versions 排序 :149/:158；workspace hash file-history-service.ts :19-22；策略表 text-file-policy.ts 全文；fs.ts :25-54/:77-86/:89-115；bridge.ts :676-701 + media-publisher.ts :57/:59-60/:64/:113-120；html-preview.ts 素材改写/资产根闸；resource-io.ts :130/:157-162。file-history/fs 的现役锚点在本轮（Round 5）逐行复读，其余在 RC-1 勘察期亲验。

## 六、新增测试（59 个集成 + lib 断言增量，全部通过；RC-3 现役对照式）

- `session_file_store`（adapters，12）：v9 tip 命名、v7→v9 链保真、CRUD 往返、source_key 去重（owner scoped）、alias 解析、refs 会话级删除、列表排序、删除联行。
- `r06_t05_session_files`（13）：sf_ 身份金样、import-legacy 保 id（A09）、fork 重写、attachments local/blob 路由（归属闸/白名单/限额）、inspect 安全闸映射、清理套件（冷度闸、引用保护、删除保护）、bootstrap 启动清扫。
- `r06_t05_resources`（5）：envelope 形状、ticket 签发/篡改/过期、content Range/ETag/越权错误序。
- `r06_t05_resourceio`（12）：15 路由面词汇、跨 provider 移动拒绝、mount/url `unsupported_provider`、写捕获历史带 origin（D8）、事件总线去重/since、session_file provider 链。
- `r06_t05_preview`（5）：创建/TTL/错误、资产改写与 base 注入、token 闸/CSP、50MB 拒、面分类。
- `r06_t05_bridgemedia`（3）：publish/resolve/下载预算/过期、token 读 Public 面、publish 响亮拒绝。
- `r06_t05_filehistory`（7）：策略金样表、workspace hash 金样（shasum 对验）、merge 窗/restore 语义、capture 策略闸、写捕获 origin、路由全链 + 错误词汇、未跟踪工作区 404。
- `r06_t05_fsread`（2）：resolve_allowed_path 镜像表（symlink/`..` 逃逸/ENOENT lexical 落回）、路由面与错误词汇（401/400/403/404/413 + base64 金样）。
- lib.rs 单测增量：auth route_policy_table 对 T05 全部新路径 × {GET,HEAD,POST,DELETE} 的分类断言（lib 单测总数 384 全绿）。

## 七、现有回归测试（真实命令与退出码）

- `cargo fmt --all --check`：exit 0（`03_fmt/fmt_check.txt`）。
- `cargo clippy --locked --workspace --all-targets -- -D warnings`：exit 0（`04_clippy/clippy.txt`）。
- 定向：`cargo test -p lingxi-service -p lingxi-adapters --locked`（`05_targeted_tests/targeted_suites.txt`）：857 passed / 1 failed，唯一失败 = `r00_management_leaves` LAN 阶段（macOS ALF 环境阻塞，见下）。
- 全量终局：`cargo test --workspace --locked --no-fail-fast`（`06_workspace_regression/full_workspace_final.log`）：**1749 passed / 1 failed，exit 101**，唯一失败 = `r00_management_leaves` LAN 阶段（macOS ALF 环境阻塞，签名：`192.168.3.5 … stalled during write/read exchange … 0 bytes read`，五点证据链见同目录 `r00_firewall_alf_blocked.txt`）。任务起点水位 1673/0 → 1749（只增不减）；相对 Round 4 后回归 1729 的 +20 增量逐套件对账一致（bridgemedia 3 + filehistory 7 + fsread 2 + preview 5 + lib 单测 3）。

## 八、Acceptance 证据

### R06-A09：旧附件导入后可经一切客户端读路径读取，身份/文件对应正确，未授权被拒

- `a09_import_legacy_sidecar_preserves_ids_and_readability`：旧 sidecar 导入后 sf_ id 逐字节保留（aliases 表解析 legacy id → canonical）；经 get/list/content 读路径均可读。
- `sf_identity_formulas_match_incumbent_golden`：身份公式金样与现役 :868-874 逐字节一致。
- `attachments_routes_register_list_and_enforce_ownership` / `attachments_local_route_maps_security_gate_errors`：归属闸先于实体读取，跨会话/未授权请求 404/403，绝不把未授权绝对路径发给远端客户端。
- `alias_resolution_maps_legacy_ids_to_canonical`（store 层）。

### R06-A10：交付且被历史引用后，缓存清理保留权威文件、临时副本按策略清除

- `a10_cleanup_keeps_delivered_external_and_cleans_cold_managed`：external 行删除但用户文件保留在盘；冷 managed_cache 载荷删除 + 行删除。
- `cleanup_skips_cold_managed_file_still_referenced_by_live_session`：引用检查器命中 → 跳过回收（被引用交付物绝不删）。
- `cleanup_never_touches_warm_sessions`：72h 冷度闸。
- `bootstrap_cleanup_reaps_cold_managed_payloads_once_at_startup`：bootstrap 清扫一次（D14，无常驻定时器）。

## 九、R00 原始断言对应结果（57 叶）

`docs/rust-tauri/R06/R06_LEAF_MAP.json` tasks["R06-T05"].leaves 全部 57 叶已标注 `t05_status`/`t05_anchor`（T01-T04 叶零触碰）：**implemented 37**（#11-21 中 11/12-17/18/19-21、#23-24、#26-47 中 26-28/29-30/31-45/46-47、#53、#55）、**share 14**（#3/#6-10、#48-52 中 48/49/50/51/52、#54/#56、#57）、**deferred 6**（#1/#2/#4/#5 → R09 桌面份额，#22/#25 → docx/xlsx 依赖顺延）。逐叶锚点见叶图与设计稿 §3；三个待裁决项 J1（桌面叶归属 R09）/J2（mount/url provider 成功路径无人承接）/J3（mammoth/ExcelJS 等价物依赖治理）如实申报，未自行选边。

## 十、记录在案差异

台账 D1-D16：D1 sidecar→v9 表（sf_ id 逐字节保留）；D2 属主键恒 `id:{session_id}`；D3 冷度信号 jsonl mtime→`sessions.last_activity_unix_ms`；D4 ticket 密钥路径；D5 路由前缀现代化；D6 无主 uploads 面结构性缺席；D7 fs.watch→轮询（语义等价）；D8 文件历史三触发源→ResourceIO 写路径一源（watcher/sweep/删除标记顺延）；D9 mount/url 响亮 400；D10 docx/xlsx 顺延；D11 Cargo.lock 净 +3 行边（hmac/regex/percent-encoding，uuid/rand 经 getrandom 手写未加边）；D12 查询严格化；D13 删除保护映射；D14 启动清扫一次无常驻定时器；D15 ResourceIO 面细化九条；D16 预览/桥媒体/fs 收窄与加固六条。R1-R7 实现细化、F1-F9 RED→GREEN 修复留痕（含测试期望误算自纠两条）。

**环境污染申报**：Round 5 中途后台并行回归（b85j0b85r）与编辑窗口重叠，报出 1 例假 doctest 失败（半编辑树瞬时编译），不作为有效证据；`cargo test -p lingxi-service --doc` 复跑 0 doctest 通过。终局回归在全部候选文件定型后干净树上执行，为唯一权威证据。

## 十一、接口兼容结果（R02–R05 / T01–T04）

- 全部新增面在 `/lingxi/v1/` 前缀下，无前缀路由表零改动；既有套件（r04/r05/r06_t01-t04、auth_matrix、session_tree、canonical_message_store 等）在终局回归全绿。
- v9 迁移只加表不改列，v7→v8→v9 链保真测试通过；既有 v8 行为不变。
- `OpContext` 新增 `capture_origin` 字段默认 None，T04 既有调用点行为不变（lib 单测 384 全绿含历史面）。
- T03/T04 收口义务项未顺手修（按授权保留原状）。

## 十二、未验证事项

- `r00_management_leaves` LAN 阶段在本机被 macOS 应用防火墙（ALF，cdhash 维度）阻塞：192.168.3.5 自检地址 connect+write 成功、0 字节读回，五点证据链见 `06_workspace_regression/r00_firewall_alf_blocked.txt`；判定为环境失败而非回归，需非阻塞环境复跑。
- 桌面 IPC 份额（share 叶 #3/#6-10、#57 的 desktop 面）未在本任务验证——物理归属 R09（Tauri host），见 J1。
- mount/url provider 成功路径未实现（显式拒绝已测），见 J2。
- docx/xlsx HTML 转换未移植（路由面缺席走 404，非降级），见 J3。
- D8 外部改动 watcher 与基线扫描顺延：非 ResourceIO 写路径的磁盘改动当前不产生历史快照（叶断言未覆盖触发源）。
- 桥媒体 baseUrl 只取环境变量回退（engine 配置面对应物缺席），差异 D16 已申报待裁决。

## 十三、已知风险

- J1/J2/J3 待总控裁决，结论可能要求补标叶图 stage_ids、补 mount/url 成功面、或引入新依赖（任一落定都需按纪律走对应阶段）。
- D8 一源捕获在外部工具直写磁盘场景下历史覆盖弱于现役三源；如需等价覆盖，后续阶段补 watcher/sweep。
- D14 无常驻 24h 清扫定时器：长运行实例的冷载荷回收依赖重启或服务 API 被编排层调用。
- ticket 鉴权豁免差异（现役 server/index.ts:642 豁免 vs 候选中间件统一鉴权 + handler 内票据校验）已在 lib.rs 路由注释与台账申报。
