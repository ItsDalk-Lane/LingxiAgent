# R06-T05 独立对抗性审查报告（REVIEWER-R06-T05-R1）

- 审查对象：R06-T05 会话文件、资源与产物身份（候选 = 未提交工作树 @ HEAD b27dc5e4be9aed4763dda60519f0a04042fdfade）
- 分支：codex/rust-tauri-migration
- 审查者与实现者/修复者零关联；执行者报告仅作线索，全部结论以本人重算/重跑/读码为准
- 审查期间未修改任何产品代码与测试

## 0. 绑定复核

- 候选 digest 复算（`bash scripts/rust-tauri/r06_candidate_digest.sh`）：
  **e7c1e05e2b9910fd0c0d2e9317069e49a0ca60b99c52d2a2f860e7df6966be50** —— 与执行者声称值逐字一致。
  脚本口径（头注释亲读）= `git diff HEAD` 全部已跟踪改动 + 未跟踪文件 sha256（排除
  `artifacts/rust-tauri/R06/` 与 `candidate_digest*.txt`）；`R06_PROGRESS.json` 在口径内（总控账本，
  属脚本明示范围）；`.vscode/` 未被脚本排除但在 `git ls-files --others --exclude-standard` 中出现——
  按 T04 先例以剔除 IDE 噪声口径复核：剔除 `.vscode/` 后应得执行者辅助值 f493ff86…（同口径唯一归因，
  IDE 噪声不构成候选内容变化）。判定绑定成立。
- 工具链：本人复跑全部使用 `/Users/study_superior/.cargo/bin/cargo`，
  `cargo 1.98.1 (797e8a9bc)` / `rustc 1.98.1 (48a229cea)`（rust-toolchain.toml 锁定 1.98.1 经 rustup 生效）。
  未使用 homebrew cargo（/opt/homebrew/bin/cargo 存在于 PATH 但未使用）。

## 1. 本人复跑的命令与退出码（证据同目录）

| 命令（均在 rust/ workspace 或仓库根） | 退出码 | 结果 | 日志 |
|---|---|---|---|
| `cargo fmt --all --check` | 0 | 通过 | rv_fmt_check.txt |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | 0 | 通过 | rv_clippy.txt |
| `cargo test -p lingxi-service -p lingxi-adapters --locked` | 101 | 809 passed / 1 failed | rv_targeted_tests.txt |
| `cargo test --workspace --locked --no-fail-fast` | 101 | **1749 passed / 1 failed** | rv_full_workspace.log |

定向唯一失败：`r03_a06_cancel_parent_spares_unrelated_background`（cancellation_tree.rs:1337，
"the unrelated background task survived the parent cancellation"）。该失败使 fail-fast 在其后中断，
故我的定向轮未跑到 r00_management_leaves（与执行者轮次互补）。单独重跑 3 次全部通过（0.13s 级）——
负载敏感 flaky：background 完成全部 20 ticks 后 exit 非空使断言窗关闭。R03 取消树面，T05 diff 零触碰。

全量逐套件对账（对账脚本内嵌于审查过程，结果如下）：
- 我的全量：1749 passed / 1 failed（FAILED = `transport_drain_timeout_abandons_a_stuck_partial_body_connection`，
  shutdown_coordinator.rs:445，drain 预算窗 0.3s 在并行负载下错过；套件单独重跑 7/7 全过）。
- 执行者全量：1749 passed / 1 failed（FAILED = r00_management_leaves LAN 阶段）。
- 两轮 passed 总数与分布逐套件完全一致，唯一差异是两个 flaky/环境失败项互换：
  r00_management_leaves 在我的环境**直接通过**（82.76s，rv_r00_management_leaves_solo.txt），
  shutdown_coordinator 在执行者环境通过。两轮互证：**无 T05 引入的回归**。
- 执行者 ALF 五点证据链（r00_firewall_alf_blocked.txt）核验：测试源码自带「环境失败如实上报」panic 文案
  （r00_management_leaves.rs:52/:204），LAN 阶段绑定 0.0.0.0 并经真实网卡地址自连（:2304-2314）；
  cdhash 维度放行机制与 R05-T01 先例一致；本人在同一环境跑通，支持「环境阻塞非回归」归因。
- +20 增量（bridgemedia 3 + filehistory 7 + fsread 2 + preview 5 + lib 单测 3）与两轮 1749 水位吻合
  （起点 1673 → Round1-4 1729 → 终局 1749，链自洽）。

## 2. 六部分审查结论

### 第一部分：任务完整性 —— 无遗漏
- Goal/Steps/Deliverables/Acceptance 与任务书及 task-catalog.json 逐字核对一致
  （deliverables: ResourceService / SessionFile迁移适配 / 文件引用检查器；acceptance_ids: R06-A09, R06-A10）。
- Depends On = R06-T04（T04 已收口，交接可追溯）；Stage Scope = R06。
- R00 原有功能断言：R06_LEAF_MAP.json tasks["R06-T05"] 57 叶全部有 t05_status/t05_anchor，
  计数 implemented 37 / share 14 / deferred 6 与执行者声称一致；deferred 叶 = #1/#2/#4/#5/#22/#25 与声称一致。
- 三个 Deliverables 实体核验：resources.rs（res_ 信封/ticket/content）、
  adapters/storage/session_files.rs（v9 四表 store）+ sessionfiles.rs（迁移适配/注册/导入/fork/清理）、
  sessionfiles.rs collect_reference_identities + collect_from_text/collect_identities（引用检查器，非仅测试名）。

### 第二部分：生产路径真实性 —— 主链真实，三个原语无生产消费者（F-01）
- lib.rs 路由注册实测 :5444-5579，与执行者声称逐面吻合，共 33 面：
  会话文件 5（files GET / import-legacy POST / files/{id} GET / attachments/local POST / attachments/blob POST）
  + 资源 3 + ResourceIO 15 + preview 3 + 桥媒体 1 + file-history 4 + fs 2。
- auth.rs 分类块实测：会话文件/附件 Scope("chat")；resources GET|HEAD → Scope("resources.read")、
  ticket POST → Scope("resources.write")、错误动词/形状 fail-closed LocalOnly；preview POST Scope("chat")、
  GET/HEAD Public+token；桥媒体 GET|HEAD Public（token 无斜杠约束）；file-history 四面 + fs 两面 LocalOnly。
  route_policy_table 循环断言（auth.rs:2633 起）真实覆盖全部 T05 新路径 × fail-closed 形状。
- 接线：bootstrap（lib.rs:1703 SessionFileService::new → resources/resource_io/preview/bridge_media/
  file_history/fs_read accessors :1900-1935）真实；归属闸 gate_session（sessionfiles.rs:1520）在
  attachments 两路由中先于 body 解析与一切实体读取。
- v9 迁移：migrations.rs diff 纯追加（V9_SQL 四表 + Migration{version:9}），v1–v8 逐字未动；
  `v7_to_v9_chain_preserves_prior_data_and_creates_new_tables` 用真实 SQL 顺序重建 v7 库 + 收据 +
  user_version 后 apply_all，断言旧数据零损耗——非恒真。
- **F-01（MEDIUM）**：`mark_session_activity`/`touch_last_activity`、`fork_session_files`、
  `collect_reference_identities`/`insert_reference` 三组原语在整个 workspace 生产代码中零调用点
  （仅测试引用）。详见 FINDING 清单。执行者报告第三部分「实际生产调用链（无平行构造者）」
  对 bootstrap→路由主链属实，但未申报这三个原语的接线缺口。

### 第三部分：对抗性验证 —— 未发现越权/污染/损坏路径
- 权限越界：attachments 归属闸先于实体读取（404/403 区分）；get_file 跨会话直接命中不返回、
  只沿 alias 解析到本会话 owned 行（fork 合法共享），无存在性泄露；fsread 双根闸（data_home+workspace）
  + symlink 一律拒绝 + `..` 词法折叠 + realpath 复核 + ENOENT 父目录 lexical 落回，逐行映射现役 fs.ts:25-54；
  preview 资产 resolve_asset_path 拒绝空段/`.`/`..`/绝对/反斜杠/NUL/symlink/realpath 逃逸，
  asset_root 请求根必须包含 source_dir 否则回落，50MB 上限，nosniff+CORP same-origin；
  桥媒体 token TTL 5min + 下载预算 5（成功才计数）+ publish/resolve 双重 canonical 白名单 +
  源文件漂移重探，与现役 media-publisher.ts :57-120 逐行等价，50MB→413 在读侧（bridge.ts:690 同位）。
- 资源票据：HMAC-SHA256 + timing_safe_eq 常量时间比较 + resourceId 绑定 + 过期 + action/schemaVersion 校验；
  密钥文件丢失→重生成（旧票据全部失效，响亮）、读取错误→internal error 响亮、空文件→重生成；
  0600 原子写有断言。票据测试（r06_t05_resources）非恒真：篡改用条件替换避免自等，真 TCP HTTP 客户端，
  Range 206/416、ETag 304、坏 ticket 403、无凭证 401、好 ticket 无凭证 401 全部断言具体值。
- write-expected-version：metadata 版本比对不符→409 冲突结果（不覆盖）、文件消失→conflict（不创建）；
  check-then-write 非原子与现役 local-fs-provider.ts 同形态（非候选弱化）。
- 不可信内容：blob mime 白名单 + video 字节兼容校验 + 空 blob 拒绝 + base64 按 mime 族限量；
  preview 素材改写 + head 注入 + file: 源根闸；Content-Disposition ASCII fallback + RFC5987 编码。
- 边界：0 字节 Range（size==0 → length 0）有显式处理；ETag 形状 `"{mtime36}-{size36}"` 镜像。
- 配置代次：ticket 密钥行为见上，无静默降级。

### 第四部分：R05/T01–T04 回归 —— 通过
- v8 冻结零变化（diff 亲验）；auth.rs diff 纯追加（0 删除行）；lib.rs diff 16 删除行全部为
  ResourceAccess 构造上提为 bootstrap 一次构建的等价重构（workspace_access Option 共享给
  filetools 与 resourceio；行为差异 = workspace root 无效时提前响亮启动失败，fail-closed，台账已申报）。
- OpContext.capture_origin: Option<String> 默认 None（resourceio.rs:445/:459），T04 调用点行为不变。
- T03/T04 已登记收口义务文件未被触碰（T05 修改清单 8 文件不含 checkpoint/retry 相关文件）。
- 四组复跑命令见第 1 节；r00_management_leaves 本人环境通过；两 flaky 均非 T05 面。

### 第五部分：虚假完成防御与三个待裁决项
- 机制测试与用户可观察行为：A09 主链（旧 sidecar 导入 → sf_ 逐字节保留 → alias 解析 → HTTP 读面 +
  resources content 面授权读）有真实路由与真实测试（真 TCP）；A10（external 权威文件永不清、
  被非冷会话引用的 managed 文件跳过、72h 冷度闸、bootstrap 清扫一次）测试断言具体行为非恒真。
  现役对照：现役 cleanupColdSessionFiles（:517-571）**无**跨会话引用保护——候选引用保护是任务书
  第 4 条要求的强化（超出现役），非差异错误。
- J1（桌面 IPC 叶 #1/#2/#4/#5 → R09）：**合法延期**。R09-T03「桌面基础能力等价实现」明确覆盖
  文件对话框/openPath/桌面命令映射（R09 任务书 :124-131）；T05 Steps 全为服务端语义。
  叶图 stage_ids 未更新为含 R09——执行者以 J1 申报待总控裁决而非自行改账，处理程序正确。
- J2（mount/url）：**部分成立为现阶段缺口**。现役生产真实注册 mount/url provider
  （core/engine.ts:1802 createSandboxResourceIO → lib/resource-io/sandbox-resource-io.ts:66-86）。
  R00 叶 #44 断言原文要求「获授权 local_fs/**mount** 引用……核对成功」，候选 mount → 400
  unsupported_provider，与断言的 mount 成功半边不符；url/session_file 拒绝与断言一致。
  叶 #44 标 implemented 与断言矛盾 → F-02（MEDIUM）。响亮拒绝 + J2 申报诚实，无静默降级。
- J3（docx/xlsx 叶 #22/#25 → 顺延）：**合法延期**。mammoth/ExcelJS 无 Rust 等价物，依赖治理属
  阶段决策；候选路由面缺席走 404（响亮缺席而非假转换）；执行者未自行引入依赖，申报 J3 待裁决。
  LOW：deferred 锚未标具体目标阶段。

### 第六部分：Findings（见下节）

## 3. FINDINGS

### F-01
- FINDING_ID: R06-T05-R1-F01
- SEVERITY: MEDIUM
- REQUIREMENT_ID: 任务书 R06-T05 Steps 1/4、R06-A09/A10 的现役行为迁移完整性；R06_LEAF_MAP #12（fork 锚）
- FILE_AND_LINE:
  - rust/crates/lingxi-service/src/sessionfiles.rs:1098（mark_session_activity）、:923（fork_session_files）、:1153（collect_reference_identities）、:1107（insert_reference）
  - rust/crates/lingxi-service/src/lib.rs:1853-1858（bootstrap 启动清扫消费 cleanup_cold_sessions）
  - rust/crates/lingxi-adapters/src/storage/session_files.rs:887-905（list_cold_session_ids 仅认 last_activity_unix_ms IS NOT NULL）
  - rust/crates/lingxi-adapters/src/storage/session_admin.rs:643（touch_last_activity 同样零调用者）
- OBSERVED_BEHAVIOR: 三组服务原语在生产代码中零调用点（grep 全 workspace 仅测试引用）：
  (a) sessions.last_activity_unix_ms 无任何生产写入者 → list_cold_session_ids 恒空 →
  bootstrap 启动清扫恒为 no-op；(b) session_tree.rs fork 流程 0 处引用 session_files
  （现役 core/session-coordinator.ts:3189/:3837 fork 真实调用 forkSessionFiles）→ fork 后新会话
  旧 sf_ id 无 alias 可解析，GET files/{id} 404；(c) 消息路径无引用登记 → session_file_refs 生产
  来源仅 sidecar 导入（fork 原语本身无人调用）。
- EXPECTED_BEHAVIOR: 现役行为——启动清扫真实回收冷载荷（server/index.ts:563-570 + jsonl mtime 判冷）；
  fork 真实复制文件并登记 alias（A09 fork 场景无断链）；消息历史中的 marker 身份被收集用于引用保护
  （server/history-read/incremental.ts:480、directory.ts:343）。
- REPRODUCTION: `grep -rn "mark_session_activity\|fork_session_files\|collect_reference_identities\|touch_last_activity" rust/crates --include="*.rs" | grep -v test` → 全部命中仅定义与测试。
- ROOT_CAUSE: T05 交付服务原语（真实实现+真实测试），但编排层接线（会话活动时间戳写入、fork 文件迁移调用、
  消息引用收集）落在 T05 叶图未覆盖的缝上；执行者未在报告「未验证事项/已知风险」申报。
- SAME_ROOT_CAUSE_PATHS: (a) 冷度信号断供→启动清扫空转；(b) fork 接线缺失→A09 fork 分场景断链；
  (c) 引用登记缺失→A10 引用保护生产覆盖仅剩导入 refs。
- IMPACT: 无越权、无数据损坏、无静默假成功——三个缺口全部朝保守方向失败（宁不清不误删、
  fork 后响亮 404、保护偏少但清理同样偏少）。但「迁移现役行为」不完整：闲置缓存回收在生产中永不触发，
  fork 场景旧附件可读性回退为 404。报告「实际生产调用链（无平行构造者）」表述会使总控高估接线完成度。
- REQUIRED_FIX: 由总控裁决接线归属（会话活动写入可能属消息编排任务；fork 文件迁移调用属 session_tree
  fork 编排；引用收集属历史读取/消息写入路径），并在 R06_PROGRESS/交接账本登记为显式跨任务缝；
  若裁定属 T05，则补三处接线 + 生产链测试。
- REGRESSION_TESTS: 生产链级测试——真实 fork 路由调用后旧 id 可读；真实消息追加后 last_activity 非空
  且冷会话清扫可触发；消息含 sf_ marker 后 cleanup 跳过被引用文件。

### F-02
- FINDING_ID: R06-T05-R1-F02
- SEVERITY: MEDIUM
- REQUIREMENT_ID: R06_LEAF_MAP #44 断言（F-D09-…-RESOURCE-IO-WRITE-0C）；执行者申报 J2
- FILE_AND_LINE: rust/crates/lingxi-service/src/resourceio.rs（refuse_if_mount_or_url → 400 unsupported_provider）；
  docs/rust-tauri/R06/R06_LEAF_MAP.json tasks["R06-T05"].leaves[43]（t05_status="implemented"）
- OBSERVED_BEHAVIOR: mount 引用写入/读取 → 400 unsupported_provider；叶 #44 标 implemented。
- EXPECTED_BEHAVIOR: 叶 #44 断言原文「分别用获授权 local_fs/**mount** 引用和不支持该操作的
  url/session_file 引用核对成功或明确拒绝」——mount 属成功侧；现役生产真实注册 mount provider
  （lib/resource-io/sandbox-resource-io.ts:66-86，经 core/engine.ts:1802）。
- REPRODUCTION: 读叶 #44 断言原文 + resourceio.rs mount 拒绝分支。
- ROOT_CAUSE: mount 成功路径未实现（J2 如实申报待裁决），但叶状态归类未反映该缺口。
- SAME_ROOT_CAUSE_PATHS: url provider 同拒绝（url 在断言中属「明确拒绝」侧，一致，不受影响）。
- IMPACT: 无安全影响（响亮拒绝）；功能等价缺口，依赖 mount 配置面在候选阶段的形态。
- REQUIRED_FIX: 总控裁决 J2：或补 mount 成功面（含测试），或将叶 #44 改标部分完成并登记后续阶段归属。
- REGRESSION_TESTS: 若补面：mount 引用成功写入/读取 + 越权 mount 拒绝。

### F-03
- FINDING_ID: R06-T05-R1-F03
- SEVERITY: LOW
- REQUIREMENT_ID: 无直接条款（回归稳定性）
- FILE_AND_LINE: rust/crates/lingxi-service/tests/cancellation_tree.rs:1337；rust/crates/lingxi-service/tests/shutdown_coordinator.rs:445
- OBSERVED_BEHAVIOR: 两测试在并行全量/定向负载下各偶发失败一次（本轮审查各复现一次）；
  单独重跑均稳定通过（cancellation_tree 3/3、shutdown_coordinator 7/7）。
- EXPECTED_BEHAVIOR: 时序窗口断言应在任意合法调度下成立（放宽窗口或注入时钟）。
- REPRODUCTION: 高负载下 `cargo test --workspace`；两测试分别断言 background exit 仍为 None /
  stuck 连接存活超出 0.3s drain 预算。
- ROOT_CAUSE: 测试自身时间窗紧（20 ticks 自然完成 / 0.3s 预算），非产品缺陷。
- SAME_ROOT_CAUSE_PATHS: 两例同属「真实并发 + 固定短窗」模式。
- IMPACT: CI 偶发红灯；与 T05 改动无关（R03/R02 面，T05 diff 零触碰），不影响本轮裁决。
- REQUIRED_FIX: 后续测试治理（窗口放宽或 deterministic clock），不属 T05。
- REGRESSION_TESTS: 不适用。

### F-04
- FINDING_ID: R06-T05-R1-F04
- SEVERITY: LOW
- REQUIREMENT_ID: 叶图治理一致性
- FILE_AND_LINE: docs/rust-tauri/R06/R06_LEAF_MAP.json tasks["R06-T05"].leaves（#22/#25 t05_anchor「后续阶段」未标具体阶段；stage_ids 仍为 ['R04','R06']）
- OBSERVED_BEHAVIOR: deferred 叶未标延期目标阶段，叶图 stage_ids 未随 deferred 更新。
- EXPECTED_BEHAVIOR: 延期目标阶段显式化（或由总控裁决 J1/J3 后统一补标——执行者选择后者，程序正确）。
- REPRODUCTION: 读叶图。
- ROOT_CAUSE: 改 stage_ids 属治理变更，执行者不自行选边，申报 J1/J3 待裁决。
- SAME_ROOT_CAUSE_PATHS: 与 F-02 的叶状态归类同属叶图账本精度问题。
- IMPACT: 账本读者需读 J 申报才能理解归属。
- REQUIRED_FIX: 总控落定 J1/J2/J3 后统一补标叶图。
- REGRESSION_TESTS: 不适用。

### F-05（OBS）
- FINDING_ID: R06-T05-R1-F05
- SEVERITY: OBS
- REQUIREMENT_ID: 台账 perf note 已申报
- FILE_AND_LINE: rust/crates/lingxi-service/src/resources.rs:741-757
- OBSERVED_BEHAVIOR: resources content 全量内存读（现役 createReadStream 流式）。
- EXPECTED_BEHAVIOR/IMPACT: 语义/头/状态码保真；大文件内存峰值。已申报，留后续任务流式化。

### F-06（OBS）
- FINDING_ID: R06-T05-R1-F06
- SEVERITY: OBS
- REQUIREMENT_ID: 台账已申报（报告 §十三）
- FILE_AND_LINE: rust/crates/lingxi-service/src/auth.rs:699-718；resources.rs:679-683
- OBSERVED_BEHAVIOR: 现役 server/index.ts:642 对 ?ticket= 豁免鉴权中间件；候选统一鉴权 +
  handler 内票据校验（双闸，更严格）。外部无头客户端仅持 ticket 将 401。
- IMPACT: 候选更严格（fail-closed）；外部消费方场景由桥媒体 Public+token 面承接。

### F-07（OBS）
- FINDING_ID: R06-T05-R1-F07
- SEVERITY: OBS
- REQUIREMENT_ID: 无（威胁模型注记）
- FILE_AND_LINE: bridgemedia.rs:389-399；preview.rs serve_asset；fsread.rs size_gate
- OBSERVED_BEHAVIOR: meta/stat 与 read 之间存在 TOCTOU 窗口（文件可被本地进程替换）。
- IMPACT: 与现役 statSync+readFileSync 形态等价；单用户本地服务模型下低危；不要求本任务修复。

## 4. RC-2 现役锚点抽验（12 条，全部真实且语义匹配）

1. lib/session-files/session-file-registry.ts:868-874 sf_ 身份公式（sha256(JSON.stringify([ownerKey, sourceKey||identityKey]))[0..16]）✓
2. 同文件 :9 冷度 TTL 72h ✓
3. 同文件 :517-571 cleanupColdSessionFiles（现役无引用保护——候选保护为任务书强化的增强）✓
4. 同文件 :997-1021 collectSessionFileReferenceIdentities ✓（生产调用点 server/history-read/incremental.ts:480、directory.ts:343）
5. core/resource-ticket-service.ts:7（KEY_FILE）/ :108-132（keyPath + readOrCreateTicketKey 0600 原子写）✓
6. lib/file-history/history-store.ts:84-116 merge 窗（同 sha unchanged / 窗内且双侧非 restore merged / 否则 inserted）✓
7. 同文件 :149 files 排序 lastCapturedAt DESC ✓
8. 同文件 :158 versions 排序 captured_at DESC, id DESC ✓
9. server/routes/fs.ts:25-54 resolveAllowedPath（词法 resolve→首根定生死→lstat symlink 拒→realpath 根内→ENOENT 父 realpath 落回）✓
10. server/routes/bridge.ts:676-701 桥媒体读面（404 词汇/50MB 413/no-store/nosniff/RFC5987）✓
11. lib/bridge/media-publisher.ts:57-120（publish 校验链/resolve 过期+预算+漂移重探/_assertAllowed 白名单）✓
12. server/routes/sessions.ts:489/:516/:550 区间（会话删除时 sidecar 移动/删除保护）✓

## 5. 最终裁决

R06-A09（旧附件迁移后可读）与 R06-A10（缓存回收不误删交付）的 REQUIRED 验收在其前置/操作/通过条件
范围内确实成立：主链真实接线（33 面 HTTP 路由 + auth 分类 + 归属闸 + v9 store）、测试真实非恒真、
未授权拒绝全程 fail-closed、v8 冻结零变化、回归水位与执行者声称完全吻合（两轮 1749/1 互证，
唯一失败项均为环境/flaky 且与 T05 无交集）。未发现 BLOCKING finding。

两个 MEDIUM（F-01 生产接线三缝、F-02 叶 #44 状态与 mount 断言矛盾）不推翻验收本身，
但必须随本审查移交总控：F-01 的三处接线缺口执行者未申报，需裁决归属并登记跨任务缝；
F-02 需在「补 mount 成功面」与「叶改标部分完成」之间落定。

**VERDICT: PASS**

（附条件：F-01/F-02 作为 MEDIUM 交接义务进入总控裁决，不因本 PASS 视为已闭合；
本审查不构成任务 DONE 宣布，不赋予 commit/push 权限。）

## 6. 审查证据文件清单（本目录）

- REVIEW.md（本文件）
- rv_fmt_check.txt（fmt exit 0）
- rv_clippy.txt（clippy exit 0）
- rv_targeted_tests.txt（定向 809/1，flaky cancellation_tree）
- rv_full_workspace.log + rv_full_workspace_exit.txt（全量 1749/1，exit 101=有失败即 101；FAILED=shutdown drain flaky）
- rv_r00_management_leaves_solo.txt（本人环境 82.76s PASS）
