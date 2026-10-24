# R08-T05 迁移协调器与切换状态机 — 台账

- owner：t02-core；阶段：design-first（Lead 指令：批准前零实现）
- 开账：2026-10-23（机器时间）

## 流水

| 时间 | 事项 | 证据/坐标 |
|---|---|---|
| 2026-10-23 | T04 移交项落地：receipts.rs:159 phase 文案修正（已批），`cargo build -p lingxi-adapters -p lingxi-service` 绿（57.79s，本地快速编译未包裹） | 工作区 diff |
| 2026-10-23 | T05 设计取证完成（接缝清单见下） | 本文件「取证坐标」节 |
| 2026-10-23 | **T05_DESIGN.md v1 提交 Lead 审批**，含裁决点 P1-P8 | docs/rust-tauri/R08/T05_DESIGN.md |
| 2026-10-23 | **P1-P8 全部批复**；P1 客户端涟漪归 t02-a04、常量切换排在最后 | Lead 裁决记录 |
| 2026-10-23 | **Phase A（P3 动词体提取）落地**：main.rs legacy-import 动词体 5 函数 + sha256_file → 新建 `legacy_import_runner.rs`；python 函数级 diff 证明逐字节一致（仅 use 路径别名归一）；main.rs 变薄壳（锁获取 + ImportArgs 组装 + 打印/退出码不变） | rust/crates/lingxi-service/src/legacy_import_runner.rs |
| 2026-10-23 | **P1 绿灯**：t02-a04 客户端对账验收通过（双 exit 1 红根因逐行一致、+34/-3 显式枚举 [1,2]、contract-versions 未碰）→ 排序约束解除，常量切换可在最后一步执行 | Lead 消息（本台账上方流水时点） |
| 2026-10-23 | Phase A 套件：adapters lib 165 过/**1 红=kept 家族 #4**（stash 至 HEAD 同红，台账早有定性）；service lib 647 过/**3 红**——HEAD 基线 worktree（/tmp/t05-head-baseline，用后已拆）复跑同红同签名，**新定性为家族 #5**（环境/基线红，非 R08 引入；三测试文件相对 HEAD 零改动） | 见下「kept-red 台账」#5 |
| 2026-10-23 | **P3 附加条件履行：零行为变化对拍绿**。重构后二进制对同一 bulk 夹具全新导入：435/435、issues 5206、complete；对拍脚本 20 断言全过（outcome 归一相等 / 文件集相等 / 非 run-bound 逐字节相等 / 收据归一深等（phase 键豁免见注）/ runs.db 逻辑 dump 摘要相等 8e8d6616…） | t05-refactor-parity-01（导入）/ **t05-refactor-parity-04（绿）**；脚本 scripts/rust-tauri/r08_t05_refactor_parity.mjs |
| 2026-10-23 | 对拍红证据保留：-02（3 红：WAL 残留文件集/SUMMARY 未归一/phase 标签）与 -03（1 红：dump 摘要缺 target 路径归一）——均为对拍脚本归一化规则补全过程，非产品行为差异 | t05-refactor-parity-02 / -03（红，留档） |
| 2026-10-23 | **Phase B-1**：adapters `legacy_import/manifest.rs`+`classes.rs` pub 化（纯可见性提升），新建 `storage/statfs.rs`（statvfs/GetDiskFreeSpaceExW + parent-walk 锚定，3/3 单测绿，含 `df -k` 1% 交叉对拍）；P2 零新包规则已履行 | rust/crates/lingxi-adapters/src/storage/statfs.rs |
| 2026-10-23 | **Phase B-2/3/5/7**：cutover 四模块落地——`journal.rs`（10 相 + refused 终态、atomic_write_private + 回读校验、advance 拒跳相/拒倒退、rewind_to 唯一倒退入口留审计痕、9 单测）、`probe.rs`（旧栈四态 + 新栈 try_lock 双锁探针，分类词汇与 server-info-probe.cjs 逐字一致，源根零创建，7 单测）、`pointer.rs`（JSON 指针 + symlink 拒读拒写 + 落盘回读语义对拍，5 单测）、`stillpoint.rs`（69 类枚举对拍 R01 账本 35/13/11/4/6、A/B 双程相等 + 备后第三程、3 单测） | rust/crates/lingxi-service/src/cutover/ |
| 2026-10-23 | **Phase B-4/6**：`preflight.rs` 六项检查全落地（版本身份/磁盘空间 statfs 双卷/writability 三目录探针-源根永不探/双锁旧实例态/copy-set 副本 quick_check 数据一致性/备份落点嵌套拒绝；报告落 `{work-dir}/preflight-report.json`，A10 override 拒绝时 target 未创建 + 源零变化由断言钉死，6 单测）；`backup.rs`（69 类外文件响亮拒绝、db -wal/-shm 归并 copy-set、scratch→Online Backup、file-copy 单遍 sha256 + 写后复读 + 幂等复用、backup-manifest.json 落盘，3 单测） | rust/crates/lingxi-service/src/cutover/{preflight,backup}.rs |
| 2026-10-23 | **Phase B 收口**：`lib.rs` 挂 `pub mod cutover;`；`cargo test -p lingxi-service --lib cutover::` **33/33 绿**；service 全量 680 过/3 红 = kept 家族 #5 原签名复现（红证据留档 t05-cutover-unit-01） | t05-cutover-unit-01（红=家族 #5，留档） |
| 2026-10-23 | adapters 套件复跑：t05-cutover-unit-02 暴露集成测试需 `LINGXI_HOME` 合成 HOME 门禁（环境受限，非红）；-03 补合成 HOME 后 **3 红 = 家族 #6 候选**：`r06_ar_f14_migration_receipts` userVersion 28 vs 独立登记表 26——根因=工作区 T04 未提交迁移 27/28 而 R02-T04_STORAGE_REGISTRY.json 未同步；纯净 HEAD worktree（用后已拆）3/3 绿，Phase B 文件不在其依赖路径 | t05-cutover-unit-02（环境受限留档）/ t05-cutover-unit-03（红，留档） |
| 2026-10-23 | **#6 裁决落地：T04 账目残留已清**（Lead 裁决：不立 kept-red，修登记表）。V27/V28 经既有 parser 路径从 MIGRATIONS SQL 字节追加进登记表，指纹经 lingxi-storage-inspect 对真实导入产物 runs.db（userVersion=28，receipts==compiledIn）实测提取、码侧/盘侧/编译表三方一致；`r02_registry_consistency.py` PASS（28 条全等）；`r06_ar_f14_migration_receipts` **3/3 绿**，测试文件零改动 | t05-registry-sync-01（红前）/ -03（指纹取证）/ -04（绿后） |
| 2026-10-23 | **Phase C（epoch 采用执行器）落地**：epoch.rs 新增注册表（`EpochMigrationPath` + `REGISTERED_MIGRATIONS` 单条 `r08-legacy-cutover` 1→2 resume-idempotent + `find_migration`）、`EpochAdoptionEvidence`（校验：checkpoint_receipt.id==checkpoint_id 等旧栈同款规则）、字节兼容 journal 写器 `write_transition_journal`（serde 结构体声明序=旧栈 createDataEpochJournal 键序，2 空格缩进+结尾换行，写后 read_journal 回读校验）、采用收据写器/重验器（`legacy-import/adoption-receipt.json`，migrating 相唯一数据变异，幂等重写无害）、`run_epoch_transition` 七相执行器（journal 唯一续跑权威；barrier_raised/committed 先 stamp 后 journal；validated 相调注入的 verify；committed 收尾留启动门既有 committed-tail-cleaned）；启动门拒启文案改诚实版（点名注册路径、永不自动采用） | rust/crates/lingxi-service/src/epoch.rs |
| 2026-10-23 | Phase C 测试 **12/12 新 + epoch 模块 28/28 绿**：首轮红 4 个——read_journal 旧栈校验矩阵拒绝 prepared 相携带 checkpoint（「prepared must not claim a completed checkpoint」），按矩阵修正写器：prepared 落 null、checkpoint_complete 起登记证据指针（红→绿全过程留档 -01）；字节钉测试逐字节钉死 committed 日志 JSON 形状；续跑矩阵六相位 createdAt 保留/updatedAt 刷新；verify 失败停 migrated 后可换证据续跑；收据篡改/corrupt journal/stamp 矛盾/缺源 stamp/未知 migration id 全响亮拒 | t05-epoch-adopt-01（红，留档）/ **t05-epoch-adopt-02（28/28 绿）** |
| 2026-10-23 | Phase C 回归：service lib 全量 **692 过/3 红**——红集与 kept 家族 #5 逐名同签名（owned_media/github_real_driver/concurrent_atomic_receipts），零新红；过数 +12 = 本相新测试 | t05-epoch-adopt-03（红=家族 #5，留档） |
| 2026-10-23 | **Phase D（自检面 + 协调器驱动）落地**：①`config.rs` `--self-check` 旗标（重复/带值均响亮拒，1 单测）；②`main.rs` 用法块 + 非回环拒启（exit 2）+ 5 参 bootstrap 穿线 + 自检分支打印 `SELF_CHECK_LISTEN addr=… home=… source=…`（**偏差披露**：设计 §9.1 原文为 `SELF_CHECK_LISTEN <port>`，实现沿用 READY 行同款 k=v 形——信息严格更多、可发现性等价、仍恰好一行，已内联注释钉死）；③`lib.rs` `self_check` 字段 + 8 项结构性禁启（见下方诚实清单表）+ `build_self_check_router`（health + `/lingxi/v1/self-check/probe` + fallback 响亮 503 `self_check_surface_closed`，同套 4 中间件 + DefaultBodyLimit，LocalOnly 分类免显式登记）；④`cutover/selfcheck.rs` 协调器驱动：监听行解析（非命中行放过/命中畸形响亮拒）、本地令牌读取（`lingxi-service/local-token.json` camelCase，0600）、probe 严格 u64 契约取数、三计数逐字段对拍（差异响亮列出）、业务面抽样断言（GET /cron + POST /sessions 必须 503+`self_check_surface_closed`，404 吞或 2xx 漏=SurfaceLeak）、SIGTERM（libc::kill）→预算内等退出→退出码 0→`probe_rust_instance_lock` 非变异锁释放探针；7 单测（行解析矩阵/对拍差异/令牌四态/严格取数/camelCase）。进程级 e2e 依 D2 属 Phase E/post-switch | rust/crates/lingxi-service/src/{config,main,lib}.rs、cutover/{selfcheck,mod}.rs；调用点顺带修正：tests/service_health.rs ×2 + examples/r06_gui_service_runner.rs（签名涟漪，行为不变 `false`） |
| 2026-10-23 | Phase D 验证：t05-selfcheck-01 = lib 700 过/3 红 + service_health 3 过/**4 新红**——追查：4 红与 lib trio 同签名 `MCP 原始只读目录不可用`，逐层取证锁定环境根因=**macOS 系统 TMPDIR `/var/folders` 经 `/var`→`/private/var` 符号链接，受限 ReadDirectory 拒开符号链接路径 → `read_home=None`**（mcp_settings.rs:389/541；T05 全程未触该路径）；换真实 TMPDIR 复跑 service_health **7/7 绿**、家族 #5 trio **3/3 绿** → t05-selfcheck-02 全量重录：**lib 703/703 全绿 + service_health 7/7 全绿**（703 = 692 + Phase D 新增 8 + trio 3）。**kept-red 家族 #5 根因已锁定为环境受限（非代码红），是否改判/修复 TMPDIR 处置属 Lead 裁决域，T05 未触碰** | t05-selfcheck-01（红留档+根因取证链）/ **t05-selfcheck-02（703+7 全绿）** |

| 2026-10-23 | **Phase E（migrate 动词编排）落地**：新建 `cutover_orchestrator.rs`——`run_migrate` 10 相编排循环（journal 开卷：create/reopen 续跑/双跑守卫 exit 2/committed 幂等/refused 终态）、`--stop-after-phase` 每相边界干净停（advance 后判停，journal 相位=下一待执行相）、§3.3 续跑前置重验 `reverify_prerequisites`（首个失效相 rewind 至其产出相）、`write_single_writer_ruling`（裁决书 §10 形状钉死：separated-roots/forbidden/三执行机制/探针摘要/残余风险，写 work-dir + 拷 target/legacy-import/）；相执行器：preflight→refuse! 包装、stop_write_probe 双锁、quiesce establish、backup、import（TargetLock 作用域、interrupted→书签+mark_stopped）、import_verify（收据 complete + 计数 + user_version==28）、epoch_adopt（spawn_blocking + record_adopt）、pointer_switch（stamp {2,2} 门 + canonicalize + commit_pointer）、selfcheck（60s/30s 预算）、committed（裁决书→Committed）；main.rs 增 `migrate` 动词臂（旗标解析缺值/未知旗标 exit 2，`CUTOVER_RESULT` 单行 stdout）；`VerbFailure`/`MigrateOutcome` 补 Debug derive（加性）；rewind 目标修正为产出相 idx-1（测试钉死 "quiesce_snapshot"）；编排器单测 **7/7 绿**（新鲜跑/双跑守卫/错 id/无 journal 续跑/停相穿线/裁决书形状/重验 rewind） | rust/crates/lingxi-service/src/cutover_orchestrator.rs、main.rs |
| 2026-10-23 | **fmt 漂移披露（如实）**：`cargo fmt -p lingxi-service` 归一化超出本任务编辑面（main/legacy_import_runner/cutover_orchestrator/workspace_ui_state/userprefs/sessionfiles/projects/lib/epoch/env_deps/cutover/* 均被重写）。语义中性（rustfmt 纯格式化），但漂移源疑为前序会话遗留未格式化区，特此披露由 Lead 知悉 | 工作区 diff |
| 2026-10-23 | Phase E 验证链三轮留档：unit-01（命令 `test \| tail` 吞退出码，包装 exit 0 但 lib 3 红=家族 #5 TMPDIR 签名，教训已吸收：PIPESTATUS/全量重定向）；unit-02（真实 TMPDIR：lib+bin 全绿，唯 1 红 `r00_management_positive_and_negative_branches_on_real_service`=LAN 自址 192.168.3.96 被本机防火墙/TUN 拦未签名测试二进制，测试 panic 文案自证环境失败）；**unit-03 全量日志：826 过/1 红（唯 LAN 环境红）**，家族 #5 trio 在真实 TMPDIR 下不再复现 | t05-migrate-unit-01/02（红留档）/ t05-migrate-unit-03 |
| 2026-10-23 | **kept-red 家族 #7 候选（提请 Lead 定性）**：`r00_management_leaves` LAN 正/负分支测试——历史证据同签名间歇（R07 t08c/t08g/t10-regression 红，t08d/e/f/h 绿），同主机防火墙态决定；测试文件相对 HEAD 零改动，T05 diff 未触网络/绑定路径；panic 文案「environment failure surfaced honestly」。建议立 #7=环境受限-间歇（LAN 自址+macOS 防火墙），与 TMPDIR 处置同列 post-R08 卫生 | 见台账 #7 |
| 2026-10-23 | **Phase E e2e 绿**：`r08_t05_migrate_e2e.mjs` 17 断言全过（e2e-04）——停相边界 CUTOVER_RESULT outcome=stopped（journal 相位=selfcheck、detail 点名 pointer_switch）；journal 八相 preflight..pointer_switch history 全 ok 顺序单调；import 书签 completed + adopt 书签 committed；指针文件 activeDataRoot==realpath(target)、cutoverId 回绑、schemaVersion=1；**旧根迁移前后全树 sha256 逐字节相等（零写红线）**；目标 runs.db user_version==28；backup-manifest 落盘；work/backup 双目录无 .tmp-* 残渣；裁决书 committed 前缺席（相位门正确）；双跑守卫/错 id 续跑均 exit 2 且现场不劣化。前置红证据全留档：e2e-01（preflight data-consistency 对夹具假 knowledge.db 如实 refuse——检查行为正确，暴露夹具保真缺陷）、e2e-02（backup 对 T04 故意埋入的 mystery-unmapped.bin fail-closed refuse——69-store 枚举契约正确）、e2e-03（断言词汇与 CUTOVER_RESULT 实际相位语义对齐修正） | **t05-migrate-e2e-04（绿）**；t05-migrate-e2e-01/02/03（红留档）；scripts/rust-tauri/r08_t05_migrate_e2e.mjs |
| 2026-10-23 | **夹具保真缺陷 + 显式垫片（提请 Lead 裁决）**：T04 生成器将 `knowledge/knowledge.db` 写成 ASCII 占位，而 store-registry.ts:989-997 声明 knowledge-database format="sqlite"（真实旧布局即 SQLite）——preflight 检查 #5 正确拒绝假库。e2e 脚本内施**显式垫片**：生成后把 knowledge.db 换成真实 SQLite（定序 16 行）+ 移除 mystery-unmapped.bin（happy-path 需全分类旧根；负路径已由 e2e-02 封存），仅作用合成夹具、不动 T04 生成器与已封锚点 40f42dc1。正式修复（生成器写真 SQLite + 锚点重绑 + parity 脚本 pin r08_t05_refactor_parity.mjs:228-229 更新）跨 T04 域，待裁决；A09/A10 驱动脚本在裁决前将沿用同款垫片并如实标注 | scripts/rust-tauri/r08_t05_migrate_e2e.mjs |
| 2026-10-23 | **编排器恢复修正验证**：rewind 目标=产出相 idx-1 链钉死后 lib 复跑 **703/703 绿**；`LINGXI_CUTOVER_DELAY_BEFORE_PHASE_MS` 测试钩子就位（相前延迟，A09/D2 kill 腿可确定性落在相内再 SIGKILL） | t05-migrate-unit-04 |
| 2026-10-23 | **真 TMPDIR 全面基线**：lib 703/703 + service_health 7/7 + 集成三组（5/5/25/0）全绿 | t05-migrate-unit-05 |
| 2026-10-23 | **环境发现①**：`ELECTRON_RUN_AS_NODE=1` 自宿主 shell 泄漏即破坏 r06_agent_settings_live；evidence wrapper 内显式 `unset` 后 **14/14 绿**（此后 R08 全部包裹命令固定 unset） | t05-migrate-unit-06 |
| 2026-10-23 | **夹具 v3 定版（上一行「待裁决」项经 Lead 批准后的正式修复落地）**：r08_t04_fixture_gen.mjs 改写真 SQLite knowledge.db（定序 16 行），锚点重绑 rootHash `80a8d2c2640e2b2974b299a10f36679bc19daf0911e51f82f0c43399cb32feaa`（32 文件/6 会话）；e2e 显式垫片移除——happy 变体直跑 **PASS**、unmapped 负路径 **PASS**（69 类外文件响亮拒，替代原 e2e-02 垫片位）；parity pin（r08_t05_refactor_parity.mjs）同步更新 | t05-fixture-v3-01 / **t05-migrate-e2e-05（happy 绿）** / **t05-migrate-e2e-neg-01（unmapped 绿）** |
| 2026-10-23 | **切常量前 pre-switch 基线封存**：lib 703 + health 7 + 集成三组全绿（unit-07）。**环境发现②**：r06_ar_f03 需 `LINGXI_HOME`+`LINGXI_W03_EVIDENCE` 双门禁，缺失时 21 全红（unit-08 红留档）；补门禁后 **21/21 绿**（unit-09） | t05-migrate-unit-07/08/09 |
| 2026-10-23 | **A09 kill 矩阵全绿**（驱动 scripts/rust-tauri/r08_t05_a09_kill_matrix.mjs，工作根 /private/tmp/t05a09-matrix，v3 夹具定种子）：baseline×4、preflight 干净停×5+相内 kill×5、backup 干净停+kill、import 干净停/kill-early/mid-early/mid-late、pointer_switch 干净停+kill、control stop-after-batch×3 全部 PASS；mark_stopped 语义（completed 相 ok、下一相 in-progress→stopped、resume 重验前置）封存。**A09 设计范围 TERMINAL=pointer_switch——selfcheck/committed 断点腿按设计顺延 D2 切换后批次** | t05-a09-* 共 30 组证据（artifacts/rust-tauri/R08/） |
| 2026-10-23 | **A10 小卷矩阵**：requiredBytes=268,635,965（夹具 v3 实测，不硬编码）；64MiB dmg 响亮拒绝（preflight 磁盘空间门，红留档）；353MiB dmg **PASS** | t05-a10-small-volume-01/02（红留档）/ **t05-a10-small-volume-03（绿）** |
| 2026-10-23 | **环境发现③**：ahash v0.8.12 cargo metadata 缺口经 `cargo fetch` 解决（用户级 ~/.cargo 缓存；已披露，Lead 接受） | Lead 消息 |
| 2026-10-23 | **常量切换落地（P8 裁决执行）**：`ContractVersions::R08_CUTOVER {preload_api:1, server_protocol:1, data_epoch:2}`（lingxi-protocol），R00_BASELINE 保留为锚；消费点全部换绑（main.rs、lib.rs×4、instance.rs×3、tests/service_health.rs）；拒启文案 `"epoch {n} or newer"` + reason `epoch-downgrade-blocked`；钉测 `r08_cutover_advances_only_the_data_epoch`。红→绿链：t05-switch-01（红留档：旧预期红）→ **t05-switch-02 绿（lib 703 + health 7）** → t05-switch-03/04 service_health 复绿 | rust/crates/lingxi-protocol/src/lib.rs |
| 2026-10-23 | **R02 脚本对齐四链全闭（§7.4 红→绿链齐，P8 追认范围内）**：①r02_t06_recovery_drill.sh v4 预期 epoch==2+注记——红 t05-switch-r02-red-01 → 绿 **t05-switch-t06-green-01（T06_EXIT=0）**；②r02_t01_service_smoke.sh seed_newer_home stamp 2→3 + grep `epoch 3 or newer` + health 字典 dataEpoch 1→2（第二红于 green-02 暴露即修，红证据旋转保留 artifacts/rust-tauri/R02/T01-red-t05-switch-01/02）→ **t05-switch-smoke-green-03：SMOKE_EXIT=0，17 用例 0 失败，A01 PASS**；③r02_t01_log_level_regression.py 同款断言对齐，绿证受阻于**家族 #5 同族**（脚本 L36 有意硬编码 TMPDIR=/tmp → macOS /tmp→/private/tmp 符号链接墙；A/B 机理实证：同一二进制 `--home /tmp/X` 拒启 exit 2 vs `--home /private/tmp/X` STARTED；错误串引入于 bfe32d107/R07，切换前已存在、与 epoch 无关）——Lead 接受：对齐证据=py_compile+文本对齐，「T08 终门若含 logreg 同墙预期内，台账已备」；④r02_cli_rust_matrix.py stamp 2→3——epoch 用例绿（t05-switch-r02-green-01），4 条预存腐红=**家族 #8**（见 kept-red 台账） | 各证据目录；scripts/rust-tauri/r02_*.sh/py |
| 2026-10-23 | **P9 全链（事实取证→裁决→α1→红绿闭合）**：D2-full 首跑红 t05-d2-01-full——selfcheck「sessions expected=6 actual=8」拒迁。**事实**：lib.rs:1335 每次启动 `ensure_session_seed(seed_rows())` 把 sess_local_alpha/beta（"Synthetic session alpha/beta"）无条件播入任何缺它们的根（含采用根）；种子谱系=95b65bd48（R02-T03，2026-09-26 16:19，内存 SessionStore 代表性端点脚手架）→dbfbc1856（R02-T04 17:41 持久化 INSERT OR IGNORE），parity 对象=Rust 自己的内存前代而非 Node；**旧栈 Node 从未在任何形态含此二合成会话**（server/shared/desktop 零命中；GET /sessions server/routes/sessions.ts:773=engine.listSessions() 纯盘背）；sessions.rs:749 注释自称「seeded into a fresh database」但实现无条件——实现违自身文档。**Lead 裁决**：A-变体批准（仅空表播种）；B（枚举 delta）无必要（§9.1 计数不变式按 A 成立）；C（污染迁移根）/D（自检对己写面撒谎）拒绝。**α1 落地**（Lead 将「guard inside ensure_session_seed + 调用点不动」修订为「编排语义不变，仅换用变体」——函数内守卫被爆炸半径实证否决：r07_bridge_notice.rs fixture L64+L112 同库双播，37 个调用点的消费契约不可由 T05 单方面收窄）：run_store.rs 新增 `ensure_session_seed_if_empty`（单事务内 `SELECT COUNT(*)`→非空则 tracing::info 跳过并提交返回 Ok；空则按原逻辑播种；MOOD_STREAM_PREFIX 守卫同款）；原 `ensure_session_seed` 与其 37 个调用点**零改动**；lib.rs:1335 换用变体。三态钉测（空库→播种/非空缺种子→不播/非空有种子→严格 no-op）+ 原函数非空表契约钉测 **4/4 绿**（rust/crates/lingxi-service/tests/r08_t05_seed_guard.rs，t05-seed-guard-01）；爆炸面回归 lib 703 + health 7 + bridge_notice 3 全绿（t05-seed-guard-02-blast）。红→绿链：t05-d2-01-full（根因红，scratch d2-full-XyHILy 留档）→ α1 → t05-d2-02-full（次级红=**D2 驱动预期错误**：journal.rs advance 语义=history 记「离开」某相事件，终态 committed 不被离开（advance out of terminal 响亮拒），设计形状=9 相 ok 依序+journal.phase==committed、无 committed 条目；驱动预期修正并注释钉死，scratch d2-full-CieUn8 留档）→ **t05-d2-03-full PASS** | 各证据目录；run_store.rs ensure_session_seed_if_empty；lib.rs:1335 |
| 2026-10-23 | **D2 切换后四腿全绿**（驱动 scripts/rust-tauri/r08_t05_d2_post_switch.mjs，v3 夹具定种子 20261023，工作根 /private/tmp/t05d2-matrix）：**full**（t05-d2-03）、**resume-half**（t05-d2-07：pointer_switch 干净停→`--resume <cutoverId>` 续跑至 committed，无重复 ok 相）、**kill-selfcheck**（t05-d2-08：延迟钩子落 selfcheck 相内 SIGKILL→续跑 committed——A09 设计顺延腿在此闭环）、**double-run**（t05-d2-09：§113 无 --resume 响亮拒 exit 2+双跑守卫文案点名 cutoverId；§114 带 --resume 幂等空跑 exit 0 outcome=already-committed；目标树双跑后逐字节不变）。红腿 04/05/06（驱动缺 --resume 门/§113-114 预期错误，非产品红）留档。公共断言：exit 0 + CUTOVER_RESULT outcome=committed、journal 终态、9 相 ok 依序、无 refused 条目、指针 activeDataRoot==realpath(target)、stamp {minimumReaderEpoch:2, committedDataEpoch:2}、裁决书 work-dir+target 双落点逐字节相等+字段钉死（schema/cutoverId/strategy/sharedWrites/enforcement×3）、**旧根全树 sha256 零写红线**。红腿 scratch 保留：d2-resume-half-RO9cHU / d2-kill-selfcheck-pL8h6O / d2-double-run-VRItEH | **t05-d2-03/07/08/09（绿）**；t05-d2-01/02/04/05/06（红留档） |

### Phase D 自检面 8 项禁启诚实清单（设计禁用清单 ↔ Rust 实态映射）

设计 §9「禁用：automation scheduler、model gateway outbound、watchers、全部写端点、后台备份定时器」。Rust 实态无独立 fs watcher / 无独立备份定时器进程，逐项映射为实际存在的 8 个后台写者/定时器禁启点：

| # | 禁启项 | 位置 | 设计条目 |
|---|--------|------|----------|
| 1 | dream ticker | run_with_tls | 后台写者 |
| 2 | automation().start | run_with_tls | automation scheduler |
| 3 | bridge().start + wait_recovery | run_with_tls | inbox/outbox 重投=写路径 |
| 4 | conversations().start + wait_recovery | run_with_tls | 同上 |
| 5 | browsers().start | run_with_tls | 浏览器宿主 |
| 6 | knowledge start_worker | bootstrap | 摄取/写 worker |
| 7 | recover_at_boot + attach_loop_controller | bootstrap | 循环恢复=写+闹钟定时器 |
| 8 | cleanup_cold_sessions spawn | bootstrap | 磁盘删除 |

**保留项**（完整启动链照常，设计 §9.1 要求）：legacy_plugins warn、auth bootstrap、management 打开、存储打开+幂等种子、bind_legacy_session_root+recover_pending、mcp_settings().restore()、instance-record 发布+OS 锁、epoch 门。**写端点关闭**由路由器结构性缺席 + fallback 响亮 503 双重保证（非 404 静默吞）。**model gateway outbound** 无常驻出站连接，自检面不发起；probe 为进程内只读 SQL。

### Phase D 偏差披露

- `SELF_CHECK_LISTEN addr=<addr> home=<home> source=<source>`（k=v 行）替代设计 §9.1 字面 `SELF_CHECK_LISTEN <port>`：与 READY 行同构、解析器同源、信息严格更多，仍恰好一行 stdout。已钉入 main.rs 用法块 + on_ready 注释 + 本台账。


### 对拍归一化例外（逐条钉死，非静默）

1. `runs.db-shm/-wal` 文件集豁免：基线目标被 rerun-refuse 证据复跑重新打开过而残留 WAL，候选干净关闭；库内容等价由 §5 dump 摘要（8e8d661695d75947）证明。
2. `SUMMARY.md` 归一化对拍：内含 target 路径/run_id/ISO 时间戳。
3. 收据 `phase` 键豁免：receipts.rs:159 标签在 import-08 取证后被有意修正（T04 收尾项）；基线=旧文案、候选=新文案，两值各自断言钉死，其余键逐一相等。
4. dump 归一化：13 位 unix-ms → `<MS>`；target 路径及 macOS `/private` 规范形 → `<TARGET>`（session_files 落盘目标路径属 run-bound）。

## 取证坐标（设计依据，全部实读）

- epoch.rs：JOURNAL_PHASES L88-96；REASON_* L101-111；classify_unstamped_home L671-714；write_legacy_import_stamp L719；write_stamp L726-749；启动门 L816-1049（升级拒启分支已存在：migration-path-unavailable L1014-1024 / epoch-downgrade-blocked L997-1013 / committed-tail-cleaned L882-911）
- main.rs：legacy-import 动词 L287-481；启动链 L630-690；epoch 门调用 L849；实例身份 L208
- instance.rs：acquire L432 / InstanceGuard L417 / probe_peer L266,277（std HTTP 探针范式）
- backup.rs：backup_database L243+（Online Backup + TRUNCATE checkpoint + owner-only）
- paths.rs：DataRootLayout L44-54；atomic_write L204+（replace 语义测试 L367-385）
- lingxi-protocol/src/lib.rs：ContractVersions + R00_BASELINE L414-429
- legacy_import/mod.rs：TARGET_DB_RELPATH L36；RECEIPT_DIR_RELPATH L38；真实 ~/.lingxi 拒收 L43/64/160；收据写盘 L813-837
- migrations.rs：V27 L1086 / V28 L1130 / registry L1285-1291（tip=28）
- 旧栈 TS：data-epoch-coordinator.ts（7 相编排 + fault 词汇 + 维护五值）；data-epoch-checkpoint-provider.ts（checkpoint 目录/metadata/verify/.tmp-*/.invalid-*）；data-epoch-restore.ts assertNoLiveServer L151-163（只探旧 probe）；server-info-probe.cjs 四态
- R00 STORES.json：69 stores（42 authoritative）；R01 DATA_COMPATIBILITY_MATRIX.json counts：preserve-format 35 / regenerable 13 / switch-to-new-authority 11 / epoch-mechanism 4 / shell-local 6
- DATA_EPOCH 消费面（设计发现 D2）：shared/contract-versions.json → server/index.ts:63,301（旧栈 ownEpoch）、desktop/main.cjs:1095,2563（旧壳对话框）、desktop/src/react/services/websocket.ts:129（新客户端严格校验）
- Cargo.lock：libc L1924 / nix L2256 已在树（statvfs 直接依赖边 = 零新 lock 条目）

## 待 Lead 裁决

P1 hello.dataEpoch=2 的客户端校验涟漪（域外文件 websocket.ts + 禁触测试）；P2 libc vs df；P3 动词体提取重构；P4 pointer=atomic_write 文件拒 symlink；P5 --self-check 面；P6 copy-set→scratch→Online Backup；P7 静止点枚举源；P8 epoch 真升 2（推荐）；P9 启动种子语义分歧（A 空表守卫变体/B 枚举 delta/C 改夹具/D 改自检）。**P1–P9 全部裁决完毕（2026-10-23）：P8 采纳推荐（常量切换+脚本对齐四链见流水）；P9 采纳 A-变体并以 α1（ensure_session_seed_if_empty，编排语义不变仅换变体）落地，红→绿链闭合（t05-d2-01→03），全链见流水行「P9 全链」。**

## 移交 T07 的钉子（Lead 录入 T07 清单）

- **真实迁移根首启后会话计数 == 导入收据计数（无种子增量）**：α1 后 ensure_session_seed_if_empty 对非空采用根严格 no-op；T07 真实数据演练验收时核对首启后 sessions 计数与 migration-receipt.json 计数逐字段相等（selfcheck 同款三计数对拍口径）。

## kept-red 台账（T05 不触碰，仅定性）

#1 vitest 2 红（composer-send.transaction / R02RustWebSocket）→ 设计 §13 已定性：静止点前置=旧服务已停，在途事务结构上不存在；R02RustWebSocket 红 = P1 涟漪落点，须对账非覆盖。
#2 persistence 三件套 10 红；#3 network-guard-negative 6/7；#4 browser::cdp——均出 T05 域。
#5 **已改判（Lead 裁决 2026-10-23）：环境受限-已定位，非代码红**。原三件套 `background_tasks::owned_media_query_and_cancel…` / `memorytools::github_registered_real_driver_requires_review…` / `sessiontools::receipt_tests::concurrent_atomic_receipts…` + service_health 4 红同族归并。根因链：macOS 系统 TMPDIR `/var/folders` 经 `/var`→`/private/var` 符号链接，受限 ReadDirectory 拒开符号链接路径 → mcp_settings.rs:389 `read_home=None` → :541 响亮报错；真实 TMPDIR 下 trio 3/3 绿 + service_health 7/7 绿 + lib 703/703 绿（t05-selfcheck-02），红证据 t05-selfcheck-01 留档。TMPDIR 处置修复出 R08 域，移交 post-R08 卫生清单。**同族扩展（2026-10-23）**：r02_t01_log_level_regression.py L36 有意硬编码 TMPDIR=/tmp → 同墙结构性红（A/B 实证：同一二进制 `--home /tmp/X` 拒启 exit 2 vs `--home /private/tmp/X` STARTED；错误串引入于 bfe32d107/R07，与 epoch 无关、与写域无关——脚本出 T05 写域）。Lead 接受：对齐证据=py_compile+文本对齐，「T08 终门若含 logreg 同墙预期内，台账已备」。
#6 **已清（Lead 裁决：非 kept-red，T04 账目残留须修）**：`r06_ar_f14_migration_receipts` 3 红根因=工作区 T04 未提交迁移 27/28 未登记进 `docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json`。已按既有 parser 路径补登 V27/V28 + generated_by 注记，指纹三方一致（码侧 sha256(SQL) / 盘侧 receipts / 编译表），`r06_ar_f14_migration_receipts` 3/3 绿（证据 t05-registry-sync-01..04）。
#7 **已定性（Lead 裁决成立 2026-10-23，以总台账为准）**：环境受限-间歇，post-R08 卫生清单。`r00_management_leaves::r00_management_positive_and_negative_branches_on_real_service`——LAN 自址（如 192.168.3.96）正/负分支被本机防火墙/TUN 拦截未签名测试二进制，54s 超时后 panic 文案自证「environment failure surfaced honestly」。历史同签名间歇：R07 t08c/t08g/t10-regression-r00mgmt1/t10-service-regression5 红，t08d/e/f/h 绿——同主机防火墙态决定。T05 diff 未触网络/绑定路径，测试文件相对 HEAD 零改动（证据 t05-migrate-unit-02/03 红留档，单测复现签名一致）。
#8 **已定性（Lead 录入总台账 2026-10-23）**：r02_cli_rust_matrix.py 4 条预存腐红——夹具裸 INSERT 5 列 vs 现行 20 列 sessions schema（缺列硬拒）+ 3 条级联缺列；切换前已存在（矩阵腐化），R08 不修，post-R08 卫生清单；epoch 用例本身绿（t05-switch-r02-green-01）。
