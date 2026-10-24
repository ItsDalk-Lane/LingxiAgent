# T05 独立评审报告（ws-ui-state）

- 评审人：ws-ui-state（与实现者 t02-core、P1 实现者 t02-a04 均无交集，资格成立）
- 评审日期：2026-10-23
- 评审对象：T05 迁移协调器全部实现（工作区未提交）+ P1 客户端纪元对账 + 证据链
- 评审方式：设计文档逐节对读实现 + 证据链抽查 + 独立复跑（rev-t05-*，r08_run.py 包裹，目录全新不复用）
- 结论：**PASS** —— BLOCKER 0 项、MAJOR 0 项、MINOR 4 项（全部为文档/整洁级，无代码修正要求）

---

## 1. 评审范围与方法

| 层面 | 对象 | 方法 |
|---|---|---|
| 切割核心 | `cutover/`（journal/probe/preflight/stillpoint/backup/pointer/selfcheck 七子模块）+ `cutover_orchestrator.rs` | 全文通读 vs T05_DESIGN.md 16 节 |
| 纪元侧 | `epoch.rs`（采用执行器 + 启动门）+ `lingxi-protocol` R08_CUTOVER 常量 | 全文通读 vs 设计 §6/§7、P8 裁决 |
| 动词与启动面 | `main.rs` migrate 动词、`legacy_import_runner.rs` 提取、`lib.rs` --self-check 面 | 全文/切片通读 vs 设计 §8/§9、P3/P5 裁决 |
| α1 种子守卫 | `lingxi-adapters/src/storage/run_store.rs` `ensure_session_seed_if_empty` + lib.rs:1335 调用点 | 全文通读 vs P9/α1 裁决 |
| P1 客户端 | `desktop/src/react/services/websocket.ts` | 全文通读 vs P1 裁决 |
| 证据链 | artifacts/rust-tauri/R08/ t05-*（~90 组）+ p1-*（5 组）+ 台账三节 | 收据抽查 + 红腿留档诚实性核对 |
| 独立复跑 | rev-t05-* ×8 组 | r08_run.py 包裹、全新证据目录 |

评审期间未修改任何被评审文件（评审人不修问题）。

---

## 2. 逐项评审：设计 vs 实现

### 2.1 journal.rs（654 行）— PASS

- 10 相前进序列 + `refused` 终态，相位是恢复唯一权威；全文无 mtime 参与裁决（grep 证实）。
- `rewind_to` 是唯一审计回退路径；非法回退响亮 Err。
- 腐化 journal = 响亮 Err（拒绝修复/猜测）；`atomic_write_private` 落盘 + 0600。
- 独立复跑 rev-t05-cutover-unit-01 含 journal 测试，40/40 绿。

### 2.2 probe.rs（453 行）— PASS

- 旧栈 server-info 四态与 `shared/server-info-probe.cjs` 字节同形；corrupt = 响亮错误（不猜 state）。
- 锁探针 `create(false)` + `truncate(false)`、try_lock 立即释放、不删文件 —— 源根零写红线在代码层成立。
- 双探针任一异常即拒迁，无降级为警告路径。

### 2.3 preflight.rs（564 行）— PASS

- 六项检查全部执行后聚合出报告（非首错即停），refused → `{work-dir}/preflight-report.json` + Refused 终态。
- 源根永不写探针（仅元数据读取）；statfs 真实余量 + 256MiB slack + `--min-avail-bytes-override` 测试门。
- backup 嵌套 source = 拒迁；持有 Rust 锁 = 拒迁。测试 6 个全部命中响亮分支。

### 2.4 stillpoint.rs（323 行）— PASS

- EXPECTED_TOTAL 69 / REGISTER 35 / REGENERABLE 13 / CONVERT 11 / EPOCH 4 / SHELL 6 计数硬钉，RegistryMismatch 即 refuse（P7 红线）。
- 三遍 manifest 漂移检测（切割前基线 + 静止窗口双遍相等 + backup 后第三遍复核）；任一漂移 = 拒迁。

### 2.5 backup.rs（541 行）— PASS

- SQLite：copy-set（db+wal+shm）→ scratch → scratch 副本上 Online Backup；**源 DB 从不被 sqlite 打开**（P6 红线成立）。
- 普通文件：字节拷贝 + sha256 + 写后回读重哈希对拍；不匹配 = 响亮失败。与设计 §5.4「字节拷贝 + sha256」一致（设计无重试约定，实现亦无静默重试）。
- 未分类文件（69 类之外）= 响亮拒迁；hash 命中时幂等复用。

### 2.6 pointer.rs（246 行）— PASS

- `symlink_metadata` 拒 symlink（P4）；腐化指针 = 响亮 Err。
- `atomic_write_private` + 写后回读逐项比对（activeDataRoot/cutoverId/schemaVersion），0600。

### 2.7 selfcheck.rs（744 行）— PASS

- SELF_CHECK_LISTEN 解析：malformed 响亮拒绝；期望值 camelCase 反序列化；strict_u64 拒非整数。
- 计数对拍逐字段精确比较（零容差，P9 裁决语义）；业务面抽样：任何非 503 + `self_check_surface_closed` = SurfaceLeak 拒迁。
- SIGTERM → 预算内等待 → 退出码校验 → 非变异锁释放探针；错误路径先 terminate 子进程再上报，无僵尸。
- 独立复跑 rev-t05-selfcheck-unit-01 七项具名全绿（见 §4）。

### 2.8 cutover_orchestrator.rs（1237 行）— PASS

- `reverify_prerequisites` + `rewind_to` 是唯一审计后退路径；`refuse!` 宏 = journal.refuse + exit 2，无第二出口。
- 双跑守卫（journal 已存在且无 --resume = exit 2）、错误 cutoverId resume = exit 2、无 journal resume = exit 2 —— 全部响亮。
- import 相：TargetLock 持有 + 书签幂等 + inflight 歧义拒迁；import_verify 逐字段对拍 + 收据 sha256 钉住。
- epoch_adopt：证据链全钉（manifest id sha256 / 回执 id / store 计数）；pointer_switch：canonical 路径比对；committed 写裁决书双副本（work-dir + target/legacy-import/）。
- 独立复跑 rev-t05-orchestrator-unit-01（bin 目标）7/7 绿。

### 2.9 epoch.rs 采用执行器 + 启动门 — PASS

- 注册路径 1→2；journal `data-epoch-transition.json` schema v1 与 `shared/data-epoch.cjs` 字节兼容（key 序、2 空格缩进、尾换行），由 `adoption_fresh_run_commits_with_incumbent_byte_form` 测试钉死 —— 独立复跑 28/28 绿含此测试。
- 证据校验：全字段非空 + receipt.id == checkpoint_id；transitionId/migrationId/epoch 任一不匹配 = 响亮拒绝。
- 启动门 `coordinate_data_epoch_startup`：journal 优先；corrupt journal/stamp 拒启（PROD-DEFECT-1 修复在账）；unstamped-with-data 拒启；ProvablyNew 才盖章；相位+印章一致性矩阵，矛盾 = 响亮拒启；committed 尾部留存供门消费。
- R08_CUTOVER{preload_api:1, server_protocol:1, data_epoch:2}（P8）；R00_BASELINE 保留锚点，测试钉住其 data_epoch==1。

### 2.10 main.rs migrate 动词 + self-check 启动面 — PASS

- migrate arm：flags 带值解析，未知/缺值 exit 2；成功打 `CUTOVER_RESULT cutoverId=… phase=… outcome=… detail=…`；失败 ExitCode(2)。
- `--network-mode lan` + `--self-check` = 启动拒绝（exit 2）；默认绑定 127.0.0.1:0（loopback + 临时端口）；非 loopback 绑定必须 lan 而被拒 → loopback-only 由构造保证。
- SELF_CHECK_LISTEN k=v 行替代 READY 行（与设计 `<port>` 的偏差已在注释 + PROGRESS-t05 如实披露，评审追认）。

### 2.11 lib.rs --self-check 面 — PASS

- grep 证实恰好 5 处门：knowledge worker skip / recover_at_boot skip / cold-session cleanup skip / 后台写者 skip（dream ticker/automation/bridge/conversations/browsers，停路径幂等 no-op）/ router 替换 —— 对应台账「8 项结构性禁启」口径（8 个子系统宿主）。
- `build_self_check_router` 仅挂 health + self-check/probe；fallback = 503 `self_check_surface_closed`（**永不静默 404**）；中间件栈与 build_router 相同（auth parity）。
- `self_check_probe`：is_local_owner 403 双保险；计数经 storage `query_one_text` 只读检视缝（绝不走写路径）；行缺失或非数值 = 响亮 500 `self_check_probe_failed`，**绝不猜零**。

### 2.12 legacy_import_runner.rs 提取（P3）— PASS

- `acquire_target_lock`：create_dir_all → canonicalize → prepare_layout → instance::acquire，prepare 失败 code 2、锁占用 code 3，stderr 行逐字节保持。
- `run_import` 专用 OS 线程（importer current_thread runtime），五相位链不变，任一失败 = VerbFailure{code 2, 行} 响亮。
- 零行为对拍已由 t05-refactor-parity-01/-04 闭合（红→绿留档见 §5）。

### 2.13 P1 客户端对账（websocket.ts）— PASS

- L58 `RUST_ACCEPTED_DATA_EPOCHS = [1, 2]` 显式枚举常量；无 contract-versions import（grep 证实），shared/ 树零改动（git status 证实）。
- `rustWireHelloDataEpochDirection` → null/'too_old'/'too_new'（非整数 → null → 通用拒绝），方向可区分。
- 拒绝路径 close code 4409 + reasonKey 'status.wsHandshakeFailed' 不变；too_old/too_new 文案分流；**无任何静默接受**。
- 独立复跑 rev-t05-p1-epoch-01：13/13 绿（含 0→too_old、3→too_new、2→connected、1→connected、4409 双方向、理由可区分性钉）。

### 2.14 α1 种子守卫（run_store.rs:643-694 + lib.rs:1335）— PASS

- MOOD_STREAM_PREFIX 守卫保留响亮拒绝；tx 内 `SELECT COUNT(*) FROM sessions`；非空 = tracing::info no-op + commit（零写）；逐行 identity_retired 跳过；INSERT OR IGNORE。
- 文档注释引 P9 裁决原文；原 `ensure_session_seed` 零改动（37 调用点零爆破半径，r07_bridge_notice 连播契约不破）。
- 生产唯一调用点 lib.rs:1335 已换守卫变体。独立复跑 rev-t05-seed-guard-01：4/4 绿（含非空库 no-op 钉）。

---

## 3. 评审重点逐项裁决

| # | 重点 | 裁决 | 依据 |
|---|---|---|---|
| 1 | fail-closed 是否真 fail-closed | **PASS** | 全部失败路径为响亮 Err/exit 2/refuse；自检面 fallback 是响亮 503 而非 404；计数探针失败响亮 500 不猜零；import 歧义/未分类文件/腐化 journal/stamp/pointer 全部拒迁拒启；grep 未发现 catch-and-continue |
| 2 | journal 相位权威可否绕过 | **PASS** | 相位为唯一恢复权威，mtime 零参与；rewind_to 唯一回退路径；refused 终态门；双跑/错 id resume 响亮 exit 2；rev-t05-migrate-e2e-01 复跑双守卫断言绿 |
| 3 | 源零改动红线 | **PASS** | probe 不开不写不删；backup 源库绝不直开（scratch 副本上 Online Backup）；preflight 源根零写探针；rev-t05-migrate-e2e-01 与 rev-t05-d2-full-01 均含「legacy root byte-equal」断言且绿 |
| 4 | 单写者三层证据 | **PASS** | 裁决书仅 committed 后写（e2e 钉住 committed 前不存在）；work-dir + target/legacy-import/ 双副本字节相等（rev-t05-d2-full-01 复跑断言绿）；enforcement×3 字段钉住 |
| 5 | epoch 1→2 字节兼容与拒启牙齿 | **PASS** | incumbent byte-form 测试钉住（28/28 含之）；启动门 corrupt/unstamped-with-data 拒启；receipt 身份三元组不符拒 committed；R08_CUTOVER=2 与 R00=1 锚点共存 |
| 6 | α1 是否零爆破半径 | **PASS** | 非空表 logged no-op（单 tx）；原函数零改动；唯一生产调用点；4/4 独立复跑绿；d2-full counts 对拍绿（无 +2 增量） |
| 7 | 红证据是否如实留档 | **PASS** | 见 §5 逐项核对：红腿 exit 非零原样归档、失败签名与台账家族登记一致、红→绿闭合链完整、无任何粉饰迹象 |
| 8 | P1 无静默降格 | **PASS** | 显式 [1,2] 枚举、方向可区分、4409+reasonKey 不变、shared/ 零改动、13/13 独立复跑绿 |

---

## 4. 独立复跑证据（rev-t05-*，r08_run.py 包裹，目录全新）

| 证据目录 | 内容 | 结果 |
|---|---|---|
| rev-t05-cutover-unit-01 | `cargo test -p lingxi-service --lib cutover::` | 40/40 绿，exit 0，stable |
| rev-t05-epoch-unit-01 | `cargo test -p lingxi-service --lib epoch::` | 28/28 绿（含 byte-form 钉），exit 0，stable |
| rev-t05-orchestrator-unit-01 | `cargo test -p lingxi-service --bin lingxi-service` | 7/7 绿，exit 0，stable |
| rev-t05-selfcheck-unit-01 | `--lib cutover::selfcheck` 具名 | 7/7 绿（malformed 响亮/strict_u64/逐字段对拍/token 响亮失败），exit 0 |
| rev-t05-seed-guard-01 | `--test r08_t05_seed_guard` | 4/4 绿（非空库 no-op 钉），exit 0 |
| rev-t05-migrate-e2e-01 | r08_t05_migrate_e2e.mjs happy | 23 断言全 ok，E2E_RESULT PASS，exit 0 |
| rev-t05-d2-full-01 | r08_t05_d2_post_switch.mjs --leg full | 13 断言全 ok，committed，epoch 2 印章、裁决书双副本字节相等、旧根字节相等，D2_RESULT PASS |
| rev-t05-p1-epoch-01 | vitest r08-p1-data-epoch-hello | 13/13 绿，exit 0 |

全部 sourceBindingsStable=true。串行执行，未复用任何既有证据目录。
（过程披露：orchestrator 首次复跑误用 `--lib` 目标——`mod cutover_orchestrator` 声明在 main.rs，属 bin 目标——0 测试命中；该证据目录已删除并以 bin 目标在全新目录重跑，非实现者问题。）

## 5. 红证据留档诚实性抽查

| 红腿 | exit | 失败签名 | 闭合 | 核对结论 |
|---|---|---|---|---|
| t05-epoch-adopt-01 | 101 | 4 个 adoption 测试真实断言失败（prepared 相位误带 checkpoint） | epoch-adopt-02 exit 0 | **如实**：真 TDD 红，修复后绿 |
| t05-epoch-adopt-03 | 101 | 3 红 = 家族 #5（TMPDIR /var 符号链接）签名 | 改判环境受限在账 | **如实**：失败签名与家族登记一致 |
| t05-refactor-parity-02/03 | 1 | 3 项 parity 失败（17 项 PASS 在案） | parity-04 exit 0 | **如实**：对拍红未粉饰 |
| t05-migrate-unit-02 | 101 | r00_management_leaves 红 = 家族 #6 账目残留 | 登记表同步后 unit-03..09 绿 | **如实** |
| p1-baseline-01 vs p1-r02-regression-01 | 1 / 1 | 同一测试同一根因逐行一致（1 failed \| 5 passed 前后不变） | 预存红声明成立 | **如实**：「根因未变」声明经逐行核对 |
| t05-d2-01-full（counts diverge） | 红 | α1 裁决的直接触发证据 | α1 后 d2-03-full 绿 | **如实**：红→绿链与设计裁决互锁 |

未发现任何红腿被删改、重跑覆盖或描述美化。

## 6. 发现清单（分级）

**BLOCKER：0 项。**

**MAJOR：0 项。**

**MINOR：4 项（均无代码修正要求，是否采纳由 Lead/实现者决定）：**

- **N1（账目/文档）**：主 PROGRESS.md L594 记「t05-migrate-e2e-04 17 断言」，实际 e2e-04 日志含 24 条 ok 断言（e2e-05 为 23 条，与本人复跑 23 条一致）。台账低估了断言数，疑按早期脚本版本记录。建议订正数字或注明口径。
- **N2（整洁）**：backup.rs 文件拷贝采用 `*.tmp-<pid>-<ts>` + rename；若进程在拷贝中段被 kill，备份存储目录可能留 tmp 残渣。正常/拒迁路径已由 e2e「无 .tmp-* 残渣」断言钉净；残渣仅存在于崩溃窗口，且 tmp 命名可识别、永不会进入 manifest（先哈希校验后 rename）。整洁级备注，供 T06/T07 清理例程知晓。
- **N3（CLI UX 健壮性，设计范围外）**：main.rs migrate flag 解析将下一 token 无条件作为 flag 值，若用户写 `--source --target x` 会把 `--target` 吞为 source 值；下游 canonicalize 仍响亮 exit 2（无静默路径），仅报错文案不直观。设计未要求，记为 UX nit。
- **N4（T06 感知备注）**：epoch_adopt 在证据打包时向解析后的 checkpoint_receipt JSON 注入 `id` 字段再哈希；原始 manifest 字节经 `backup_manifest_id` sha256 独立钉住，篡改证据链完整。现状可接受，提请 T06 保持双钉不变量。

## 7. 结论

T05 迁移协调器实现与 T05_DESIGN.md（16 节 + P1–P9/α1 裁决）**一致**；fail-closed、相位权威、源零改动、单写者三层证据、纪元字节兼容、α1 零爆破、红证据诚实性七项红线全部经代码通读 + 独立复跑双重验证成立。**评审结论：PASS。** 四项 MINOR 均为文档/整洁级，不阻塞 T05 关闭与 T06 启动。

---

评审人：ws-ui-state ｜ 2026-10-23 ｜ 证据：artifacts/rust-tauri/R08/rev-t05-*（8 组）
