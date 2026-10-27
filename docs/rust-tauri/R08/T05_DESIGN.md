# R08-T05 迁移协调器与切换状态机 — 设计（待 Lead 批准）

- 任务书：R08 L209-249（5 怎么做）；验收：A09（五断点 kill -9 恢复）/ A10（小盘预检拒绝）
- 输入：T05_SCOPING.md（六硬要求 + 待复核清单）、STORAGE_READINESS.md、T04 已验收 CLI 接缝
- 状态：**设计稿 v1，未实现**。本文每个「裁决点 Pn」需 Lead 逐条批复后才动工。
- 作者：t02-core（T04 实施者，接缝自述）；建议独立评审：ws-ui-state

---

## 0. 六硬要求 → 设计章节映射

| 任务书硬要求 | 设计落点 |
|---|---|
| ① 预检六项逐项取证、fail-closed、落盘报告；磁盘阈值真实读可用空间 | §4 |
| ② 停写 + 全局静止点；退出证据 = 进程退出 + 锁释放；Online Backup + 文件摘要；枚举 69 类 | §5 |
| ③ 隔离目录转换 + 验证（调 T04 CLI）；收据核对 + 计数/哈希 + 只读端点抽查；receipt + epoch 后原子切换，无先删后建窗口 | §6（转换验证）、§7（epoch 采用）、§8（原子切换） |
| ④ 自检通过前写入保持关闭；journal 落盘状态机，不靠 mtime | §3（状态机）、§9（自检与开放写入） |
| ⑤ 单写者按 R01 策略验证；不能保证 → 分离根 + 禁止共享写入 + 显式裁决记录 | §10 |
| A09 / A10 | §11 / §12 |

非目标：T06 回滚/前滚（任务书 L251+），本设计不含任何回滚执行器；备份产物只按 T06 契约**备齐**（§5.4），不在 T05 消费。

## 1. 现状取证摘要（全部实读，附坐标）

### 1.1 T04 已有接缝（直接复用，不重造）

- 动词：`lingxi-service legacy-import --source --target [--resume <runId>] [--stop-after-batch <k>]`（main.rs:287-481）。动词体分两半：adapters 侧 `legacy_import::run`（manifest→convert→簿记）+ service 侧四个 phase 函数（sessionfiles/trace/cron/surfaces，main.rs:413-462）。**surfaces 阶段末尾调 `write_legacy_import_stamp(home)` 落 stamp=1 候选章**（epoch.rs:719）。
- 簿记：V27（import_runs/import_batches/import_issues）+ V28（extensions registry），migrations tip=28，只追加不改旧版本。
- 收据：`{target}/legacy-import/`：migration-receipt.json（status/issues/计数/resumedFromRunId 条件字段）、source-manifest.{before,after}.json（两遍源清单）。
- 目标库：`{target}/lingxi-service/data/runs.db`（TARGET_DB_RELPATH）。
- 单写者：导入全程持有目标根 OS 实例锁（main.rs:363，acquire 失败 exit 3）。
- kill 恢复：instance_lock_held 裁决 interrupted；`--resume` 门 fail-closed（exit 2，已有 e2e 红证据）。

### 1.2 epoch 门（epoch.rs，实读 L86-1050）

- stamp `data-epoch.json` schema v2（epoch/minimumReaderEpoch/committedDataEpoch/lastVersion/updatedAt）；journal `data-epoch-transition.json` schema v1，**7 阶段 prepared→checkpoint_complete→barrier_raised→migrating→migrated→validated→committed**（JOURNAL_PHASES，L88-96，与 shared/data-epoch.cjs 同形）。
- 启动门 `coordinate_data_epoch_startup`：corrupt stamp/journal 一律拒启（PROD-DEFECT-1 已闭环）；unstamped-with-data 拒启（"adopting is an explicit R08 cutover action"，L962 注释原文）；symlink / `.tmp-*` 残留 → ambiguous 拒启。
- **关键发现 D1**：升级方向拒启分支**已存在**——`stamp.committed_data_epoch < own_epoch` → `migration-path-unavailable`（L1014-1024，"no epoch migrations are registered in this build"）；`minimum_reader_epoch > own_epoch` → `epoch-downgrade-blocked`（L997-1013）；committed journal + 达标 stamp → 自动清尾（committed-tail-cleaned，L882-911）。**缺的不是拒启，是：注册 1→2 迁移路径 + 转换执行器 + 断点续跑**。
- 当前 own epoch：`ContractVersions::R00_BASELINE.data_epoch = 1`（lingxi-protocol/src/lib.rs:424-428），main.rs 两处消费：L208（实例身份）、L849（启动门）。

### 1.3 实例锁（instance.rs，实读）

- `{home}/lingxi-service/instance.lock` OS 咨询锁（`File::try_lock`，unix flock / win LockFileEx）= **唯一活性权威**；SIGKILL 即释放；`instance.json` 仅诊断（stale → 归档 instance.stale.json）；永不信 PID。
- `probe_peer(addr) -> PeerProbe`（L266/277）：std TcpStream HTTP 探针，**零依赖的 HTTP 客户端范式已在树内**——旧 server-info 探针复刻此模式（§5.2），不引新 crate。
- 旧 Node 二进制不认识 Rust 锁 ⇒ 停写证据必须双锁语义（§5.2）。

### 1.4 备份（adapters/storage/backup.rs，实读 L243+）

- `backup_database(source: &Connection, dest_dir, file_stem, wal_bytes_before, options, checkpoint_grace_ms)`：pre-checkpoint TRUNCATE → rusqlite Online Backup → integrity + sha256 + owner-only perms；目的目录先收紧权限再落任何产物。直接复用。

### 1.5 旧栈 TS 契约（只读复用其语义，不改旧栈）

- `core/data-epoch-coordinator.ts`：7 阶段编排 + fault-event 词汇表（journal:prepared…journal:removed）+ 维护续跑五值（continue-before-migration / resume-idempotent / restore-only / commit-validated / finalize-committed-tail）——**T05 Rust 执行器逐语义镜像此词汇表**，journal 文件与 shared/data-epoch.cjs schema v1 字节兼容（旧 restore 工具可读）。
- `core/data-epoch-checkpoint-provider.ts`：checkpoint 目录 `{home}/data-epoch-checkpoints/<transitionId>/stores/<storeId>/…` + metadata.json（items: storeId/relPath/bytes/sha256）+ verify 全量重哈希 + `.tmp-*` 清理 + `.invalid-*` 保全（不删证）。R08 静止点备份清单沿用此 item 形状。
- `core/data-epoch-restore.ts` `assertNoLiveServer`（L151-163）：读 `server-info.json` → token 鉴权 HTTP 探针（shared/server-info-probe.cjs）→ alive-same-home / alive-unauthorized 阻断。**只探旧 probe，不查 Rust OS 锁**——T05 双锁探针要补齐的正是后者。
- `shared/server-info-probe.cjs` 探针语义（L80-126）：200+身份形状→alive-same-home；403+鉴权拒绝形状→alive-unauthorized；其它→not-hana；连接失败→dead。

### 1.6 R01 切换策略（DATA_COMPATIBILITY_MATRIX.json，实读 counts）

69 类 = preserve-format 35 + regenerable 13 + switch-to-new-authority 11 + epoch-mechanism 4 + shell-local 6。与 T04 落账口径一一对应（11 实转 / 35 登记 / 13 重建收据 / 10 出域）。**静止点声明的枚举源 = docs/rust-tauri/R00/STORES.json（69 stores，含逐 store path_patterns/format/epoch_checkpoint_policy）+ R01 策略分类**（裁决点 P7）。

### 1.7 DATA_EPOCH 单一来源张力（设计级发现 D2）

`shared/contract-versions.json` `DATA_EPOCH: 1` 是旧栈与新 web 客户端共同的来源：旧栈 server/index.ts:63/301 以它为 ownEpoch；旧壳 desktop/main.cjs:1095/2563 以它渲染 epoch-blocked 对话框文案；新 web 客户端 desktop/src/react/services/websocket.ts:129 以 `hello.dataEpoch === DATA_EPOCH` 严格相等校验。**bump 该 JSON = 改旧产品语义（旧构建会对存量 epoch-1 家目录拒启、壳层文案同步失真）——红线，绝不做**。Rust 侧必须持有独立常量（§7.2），由此产生客户端校验涟漪 → 裁决点 P1。

## 2. 总体架构

```
lingxi-service migrate --source <old-home> --target <new-home>
                       --work-dir <dir> --backup-dir <dir> --pointer <path>
                       [--resume <cutoverId>] [--stop-after-phase <p>]
                       [--stop-after-batch <k>]            # 透传 T04（A09 导入断点用）
                       [--min-avail-bytes-override <n>]    # 仅测试注入（A10 正控制）
```

- **协调器 = 新离线单写者 CLI 动词**，与 legacy-import 同范式：不启 tracing 服务栈、不触网络、专用 OS 线程执行引擎、失败响亮 exit 2（锁占用 exit 3）。
- **放置：lingxi-service crate 新模块 `src/cutover/`**（journal.rs / preflight.rs / probe.rs / stillpoint.rs / backup.rs / adopt.rs / pointer.rs / selfcheck.rs / mod.rs 编排）。理由：T04 的 service 侧四个 phase 函数在 main.rs，import 步骤必须复用同一条实现链 ⇒ 把 legacy-import 动词体**原样提取**为 `src/legacy_import_runner.rs`（零行为变化，纯移动），legacy-import 与 migrate 两动词共用（裁决点 P3）。可复用的纯函数下沉 adapters（statfs 可用空间、清单构建复用 legacy_import 的 manifest builder、backup_database 复用 backup.rs）。
- **三目录分离**（A10「目标根无部分产物」的结构性保证）：
  - `--work-dir`：协调器工作区——cutover journal、预检报告、静止点声明、scratch（sqlite copy-set）、最终裁决记录。**preflight 拒绝时唯一被写的位置**。
  - `--target`：新根，**import 阶段才创建**；之前零产物。
  - `--backup-dir`：静止点备份落盘处（A10 注入点：挂小卷）。
  - `--source`：旧根，**全程零写入**（探针全部只读/无创建；§5.2 详述）。
- 全程持有**目标根 OS 实例锁**（import 阶段起）+ **work-dir 协调器锁**（全程，防两个 migrate 并发同一 work-dir）。源根永不被锁文件创建动作触碰（§5.2 非变异探针）。
- 零新 lock entries：磁盘可用空间用 `libc::statvfs`——libc 已在 Cargo.lock（L1924，传递依赖）；加直接依赖边不产生新 `[[package]]` 条目（实现时以 lock diff 为空举证）。备选：`df -k` 子进程解析（裁决点 P2，推荐 libc）。

## 3. 切换状态机（journal 落盘，不靠 mtime）

### 3.1 journal

- 路径：`{work-dir}/journal.json`，schemaVersion 1，paths::atomic_write_private 链（tmp+rename+fsync，同目录原子替换，无先删后建窗口）。
- 内容：`{schemaVersion, cutoverId, createdAt, phase, attempts: n, history: [{phase, enteredAt, leftAt, outcome, detail}], inputs: {source, target, workDir, backupDir, pointer, sourceManifestHash}, import: {runId, status} | null, adopt: {transitionId, phase} | null, refusal: {check, reason, reportPath} | null}`。
- 每次换相 = 一次原子写；**phase 是恢复的唯一权威**（mtime 永不参与裁决）。

### 3.2 相位（10 相，A09 五断点为其子集并加粗）

| # | phase | A09 断点 | 进入前置（恢复时重验） | 幂等性 |
|---|---|---|---|---|
| 1 | **preflight** | 预检 | journal 不存在或同相位 | 纯检查+报告，可任意重跑 |
| 2 | stop_write_probe | （备份前置） | preflight 通过记录 | 纯探针，可重跑 |
| 3 | quiesce_snapshot | （备份前置） | 上相通过 | 两遍 manifest 重建，可重跑 |
| 4 | **backup** | 备份 | 静止点声明在盘 | 逐 store 校验已有产物（哈希重算），不符重备该 store；不删证 |
| 5 | **import** | 导入 | backup manifest verify 通过 | 内部=T04 簿记幂等；journal 记 runId，恢复时 `--resume runId` |
| 6 | import_verify | — | import status=completed | 纯只读重验（收据/计数/哈希），可重跑 |
| 7 | epoch_adopt | — | import_verify 通过 | 内嵌 7 相 epoch journal（§7.3），按 epoch journal 相位续跑 |
| 8 | **pointer_switch** | 指针更新 | stamp=2 committed 且 epoch journal committed | 原子写幂等（同内容重写无害），写后读回校验 |
| 9 | **selfcheck** | 启动 | pointer 已指向 target | 子进程模式起服务，探针只读端点，SIGTERM 退出；可重跑 |
| 10 | committed | — | selfcheck 通过 | 终态；写入开放判据（§9.3） |

另有终态 `refused`（fail-closed：phase + check + reason + reportPath，exit 2）。任一进相失败 = 写 refusal 后退出，**绝不跳相、绝不降格为警告**。

### 3.3 断点恢复语义（A09 通过判据的操作化）

同一入口重试：`migrate … --resume <cutoverId>`（cutoverId 也在 journal 里，不带 --resume 时若 journal 已存在则响亮拒绝并提示——防双跑）。
恢复算法：读 journal → 按上表「进入前置」逐相重验 → 从首个前置失效的相位续跑；前置全部有效但 phase=committed → 报告已完成并退出 0（幂等空跑）。
- import 相内被 kill：journal 相位=import 且 import.runId 已记 → 续跑即调 runner `--resume runId`（T04 门：非 interrupted 响亮 exit 2）。
- epoch_adopt 相内被 kill：按目标根 `data-epoch-transition.json` 的相位续跑（§7.3 续跑表），**不是靠 cutover journal 猜**。
- pointer_switch 被 kill：pointer 文件要么旧内容要么新内容（原子替换），读回校验决定重发或放行；绝无半指针。
- selfcheck 被 kill：检查有无遗留 self-check 子进程（其持目标根锁）→ 锁探针裁决后重跑本相。

### 3.4 kill 注入

- `--stop-after-phase <p>`：干净停在相边界（journal 记 stopped 供恢复演练）；A09 用真实 `kill -9`（定时窗口，T04 已沉淀 `sleep $((attempt+4))` 发现法）。
- import 相内中段 kill：真实 kill -9 落在 T04 convert 中段（bulk 435 批次源保证窗口足够宽）+ `--stop-after-batch` 干净停对照。

## 4. 预检（六项，逐项取证，fail-closed）

报告：`{work-dir}/preflight-report.json`（schemaVersion 1；checks 数组每项 `{check, pass, evidence, detail}`；任一 pass=false → 顶层 verdict=refuse）。**六项全部先跑完再汇总**（取证完整性），但任一失败即最终 refuse（不降格）。

| # | 检查 | 取证内容 | 失败样例 |
|---|---|---|---|
| 1 | 版本 | 源：server-info.json/fixture 基线身份（版本字段+形状）；目标：本二进制 serverVersion + ContractVersions；证据=两侧原文摘录 | 源无版本身份 → refuse（不能证明迁的是 Lingxi 家目录） |
| 2 | 磁盘空间 | **真实 statvfs**（libc::statvfs，f_bavail×f_frsize）分别测 backup-dir 卷与 target 父卷；required = 源清单实测字节数 + runs.db 估算（T04 实测比例）+ 256MiB 余量；证据={avail_bytes, required_bytes, 逐项构成} | avail < required → refuse（**A10 注入点**） |
| 3 | 可写性 | target 父目录与 backup-dir：创建+写+读回+删除探针文件（0700）；work-dir 同。**源根只读性反向验证：不以写探针触源根**（零改动红线） | 探针失败 → refuse |
| 4 | 旧实例状态 | 双锁探针（§5.2）四态证据：server-info 探针状态、Rust 锁探针状态、PID 活性交叉（仅诊断）、instance.json 诊断摘录 | alive-same-home / alive-unauthorized / 锁被持有 → refuse |
| 5 | 数据一致性 | 源全量 manifest（T04 builder，33 类 v2 fixture/bulk 通用）；逐 sqlite：copy-set（db+wal+shm）到 scratch → 副本上 integrity_check（QuickCheck）+ 表计数基线；**绝不直接打开源库**（Connection::open 有 WAL/SHM 副作用，违反源零改动；immutable URI 会漏 WAL 内容，弃用） | 副本完整性失败 / manifest 不可构建 → refuse |
| 6 | 备份目标 | backup-dir 存在性/可建性、owner-only 权限、**不在源根或目标根内**（路径包含关系检查）、卷空间（与 #2 联证） | 位于源/目标内 → refuse |

## 5. 停写与全局静止点

### 5.1 退出证据定义（任务书②原文落实）

「旧服务明确退出」= 以下**全部**成立，缺一 refuse：
1. server-info 探针 = dead 或 not-hana（token 鉴权 HTTP 探针，复刻 shared/server-info-probe.cjs 四态语义；Rust 实现走 instance.rs probe_peer 同款 std TcpStream 模式，零新依赖）；
2. Rust OS 锁探针 = 自由（见 §5.2 非变异探针）；
3. 源根两遍全量 manifest 字节相等（静止窗口，默认 2s+，测试可注入间隔）；
4. PID 活性交叉（server-info.json/instance.json 记录的 pid 做 kill(pid,0)）仅为**诊断附录**，永不作权威（instance.rs 既定语义）。

### 5.2 双锁探针（STORAGE_READINESS 待复核项的落实）

- 旧侧：读 `{source}/server-info.json` → 探针。阻断态 alive-same-home / alive-unauthorized；not-hana/dead 放行但记证据。
- 新侧（Rust 锁）：`{source}/lingxi-service/instance.lock` **存在** → 以「不创建」方式打开 + try_lock：拿不到 = 有活的新栈写者 → refuse；拿到即放。**文件不存在** = 不可能有活 Rust 实例（prepare_layout 必先建 runtime 布局）→ 放行。**全程不在源根创建/删除任何文件**（A10「旧根零改动」红线结构性覆盖）。
- 旧 Node 二进制不认识 Rust 锁、新栈不认识旧 server-info——两个探针各自覆盖一个栈，合取才是「无写者」。

### 5.3 全局静止点声明

- 产出 `{work-dir}/stillpoint-manifest.json`：由 STORES.json 69 类 × R01 策略 × T04 落账口径生成，逐 store `{storeId, pathPatterns, strategy, disposition, evidence}`；disposition ∈ `backup+import`（11 switch-to-new-authority）/ `backup+carry`（35 preserve-format）/ `backup-only-rebuild-receipt`（13 regenerable，不搬数据只出收据）/ `epoch-meta-handled-by-adoption`（4）/ `out-of-scope-shell-local`（6）。
- **枚举必须是全集 69，缺一类即 refuse**（防「悄悄漏项」——STORAGE_READINESS 明示风险）。生成器输出 counts 与 R01 counts 对拍。
- 静止点本身 = 两遍 manifest 字节相等 + sqlite copy-set 完整性 + 备份后第三遍 manifest 仍相等（备份期间无回写证明）。

### 5.4 备份（T06 契约备齐，T05 不消费）

- 落盘 `{backup-dir}/r08-cutover-<cutoverId>/`：`stores/<storeId>/…` + `backup-manifest.json`（items 形状沿用 checkpoint-provider：storeId/relPath/bytes/sha256 + method ∈ online-backup / file-copy + totalBytes + createdAt）。
- sqlite：scratch 副本上 `backup_database`（pre-checkpoint TRUNCATE → Online Backup → integrity + hash）；普通文件：字节拷贝 + sha256。
- verify pass：manifest 全量重哈希（checkpoint-provider verify 语义镜像）；`.tmp-*` 清理、`.invalid-*` 保全不删证。
- 恢复：逐 store 重验，不符重备该 store（幂等），manifest 不可信则整相重来。

## 6. 隔离转换与验证（调 T04 链）

1. 创建 target → prepare_layout → acquire 目标根锁（与 legacy-import 动词同一入口纪律）。
2. 调 `legacy_import_runner::run(args)`（提取后的共用实现链；首跑无 resume，续跑带 journal 里的 runId）。`--stop-after-batch` 透传用于 A09 注入与干净停对照。
3. import_verify 相（全只读）：
   - 收据核对：migration-receipt.json status=completed、phases 全绿、resumedFromRunId 与 journal 一致（若续跑）、issues 多重集 vs 既定基线口径（T04 归一化断言规则沿用：剥 run-bound 列、`<TARGET>` realpath 归一、identity hash 为内容身份证据）；
   - 计数/哈希：target runs.db 只读打开（目标是己方，WAL 副作用无虞）→ 逐表计数 + 归一化 identity hash 对拍 import 相出口快照；
   - V27/V28 簿记一致性：run status=completed、batch 计数=计划数、tip=28；
   - stamp=1 候选章存在且 schema 合法（epoch_adopt 的输入）。
4. 只读端点抽查**不在本相**（服务未起）——归入 selfcheck 相（§9），设计如此分层以免在切换前开放任何服务面。

## 7. 数据 epoch 采用契约（真实实现，非 fixture 印章）

### 7.1 语义

采用 = 把 T04 产出的 epoch-1 候选根经**注册迁移路径 1→2** 转为新栈权威根。epoch 2 的含义：「R08 新栈布局 + 采用收据链完备」。旧根永远保持 epoch 1、旧栈所有；新二进制自己的 own epoch 升 2（真实版本递增，非文案）。

### 7.2 版本与门改动（Rust 侧，独立常量）

- lingxi-protocol：新增 `ContractVersions::R08_CUTOVER { preload_api: 1, server_protocol: 1, data_epoch: 2 }`（R00_BASELINE 保留作历史锚）；main.rs L208/L849 切到 R08_CUTOVER。
- 直接后果（全部有意、全部有测试）：新建家目录盖 epoch=2；epoch-1 家目录启动 → 既有 `migration-path-unavailable` 拒启分支生效（**含 T04 未采用目标根**——未采用不可服务，正是采用契约的牙齿）；旧二进制（own epoch=1）对 stamp=2 新根 → 既有 `epoch-downgrade-blocked` 拒启（**单写者结构性保证，§10**）。
- **不改 shared/contract-versions.json**（发现 D2：旧栈自用源，改了=改旧产品语义）。由此 web 客户端 websocket.ts:129 `hello.dataEpoch === DATA_EPOCH(1)` 将拒绝 hello.dataEpoch=2 的新服务 → **裁决点 P1**（客户端一行校验策略改动 + R02RustWebSocket.test.ts 均为我的禁触/域外文件，需 Lead 指派 ws-ui-state 或授权扩域；T05 提供 Rust 侧与证据，绝不静默降格 hello 广告值）。

### 7.3 采用执行器（epoch.rs 新增，仅协调器调用，启动门永不自动采用）

- 注册迁移路径 `{from: 1, to: 2, id: "r08-legacy-cutover"}`；`run_epoch_transition(home, migration_id, evidence)` 公共 API。
- 7 相执行（journal 与 shared/data-epoch.cjs schema v1 字节兼容，fault-event 词汇镜像 TS 协调器）：
  - `prepared`：建 journal（transitionId=cutoverId；from 1 to 2；affectedStoreIds=静止点声明全集映射）。
  - `checkpoint_complete`：checkpoint 记录 = 切割备份 manifest 标识 + T04 收据链哈希（数据安全网已在 §5.4/§6 建立；本相登记证据指针，**显式记录而非静默跳过**）。
  - `barrier_raised`：写 stamp {minimumReaderEpoch: 2, committedDataEpoch: 1}（读者屏障：此后任何 epoch-1 内核——含旧二进制——开此根即拒）。
  - `migrating`：落 `{target}/legacy-import/adoption-receipt.json`（cutoverId、import runId、backup manifest id、源身份哈希、stillpoint manifest 哈希、UTC 时间）——**唯一数据变异，纯追加**（数据本体 T04 已建成，无破坏性变换可幂等）。
  - `migrated` → `validated`：重跑 §6.3 全套只读校验 + stamp/journal 一致性（transition_consistency 同规则）。
  - `committed`：写 stamp {2, 2}。下次正常启动由**既有** committed-tail-cleaned 分支清 journal（复用，不新造）。
- 续跑表（镜像 TS 维护五值）：prepared/checkpoint_complete → continue-before-migration（重验重登记）；barrier_raised/migrating/migrated → resume-idempotent（重写 adoption receipt 无害）；validated → commit-validated；committed → finalize-committed-tail（启动门既有路径）。corrupt journal/stamp → 响亮拒，绝不修。
- epoch_adopt 相内 kill -9 = A09 补充断点：恢复时读**目标根 epoch journal** 相位续跑，与 cutover journal 相位对拍（不一致 = 响亮拒）。

### 7.4 爆破半径（实现时逐项销账）

protocol lib.rs:522 测试字面量 data_epoch:1；epoch.rs 既有测试夹具（stamp=1 预期 steady 的用例 → 改 stamp=2 或走采用路径）；实例身份记录内 ContractVersions 字段（诊断面）；R02 e2e 预盖章脚本。全部以红→绿证据链呈现，不允许「顺手改预期」无记录通过。

## 8. 原子指针切换

- pointer = `--pointer <path>` 指定的**常规 JSON 文件**：`{schemaVersion: 1, activeDataRoot, cutoverId, committedAt}`，paths::atomic_write_private 链落盘（同目录 tmp+rename+fsync——rename 原子语义，**无先删后建窗口**；该链是 R02 起全仓跨平台验证过的唯一原子写实现，含 Windows ACL 纪律）。
- **不选 symlink**：Windows 符号链接需特权且 epoch 门对家目录内 symlink 一律 ambiguous 拒启；指针文件方案无双平台语义岔路。rename/symlink 二选一的任务书措辞以满足 rename 原子语义交差（裁决点 P4）。
- 写后读回 + 字段校验 +（unix）父目录 fsync；pointer 在 switch 前不存在 = 合法的「首次切割」态（A09 观察点：任一时刻要么无指针、要么完整指针）。
- 产品语义注释：正式产品里「活动目录指向」由壳层启动参数/配置承载；本演练以 pointer 文件为该指向的可观测替身，并在裁决记录中注明此映射。

## 9. 自检与开放写入

### 9.1 self-check 模式（main.rs 新增 `--self-check`，裁决点 P5）

`lingxi-service --home <target> --port 0 --self-check`：完整启动链（布局 → **epoch 门（stamp=2 须通过）** → 存储打开）但：
- 禁：automation 调度器、model gateway 外发、plugins/knowledge/sessionfiles watchers、一切写端点（响亮 405/503，不静默吞）、后台备份定时器（STORAGE_READINESS 明示的 automation 隐患在此结构性关闭）。
- 开：health + 只读抽查端点（session 计数、model_call_usage 计数、cron 任务计数），token 鉴权同正路径。
- 端口发现：`--port 0` OS 分配，stdout 打 `SELF_CHECK_LISTEN <port>` 行；协调器解析后探针，断言计数 == 收据期望值；随后 SIGTERM → 等退出 → 锁释放探针确认。

### 9.2 顺序保证（任务书④）

pointer_switch（8）→ selfcheck（9）→ committed（10）。**committed 之前不存在任何可服务常态入口**：演练域内唯一启动权威是协调器；self-check 进程写面结构性关闭；故「自检通过前写入保持关闭」不靠约定靠构造。

### 9.3 开放写入判据

journal phase=committed = 写入开放的唯一判据。此后操作者正常启动服务（无 --self-check）即全量面开放。T06 回滚若发生，以 committed journal + 备份链为输入（出 T05 域）。

## 10. 单写者验证与裁决（任务书⑤ / R01 策略）

按 R01 策略验证「旧发布程序不能再写新数据」，结论**可证明**，证据链三层：
1. **结构性**：旧二进制家目录路径绑定旧根，无任何代码路径指向新根；新根路径由本次切割新建。
2. **epoch 门强制**：采用后新根 stamp {minimumReaderEpoch: 2}——旧二进制（own epoch=1，含字段中所有已发布版本）启动即 `epoch-downgrade-blocked` 拒启（R01 A 系列探针已证明该行为在 HEAD 构建逐字节成立）。**即使有人手工把旧程序指向新根，也开不了**。
3. **新栈内**：Rust OS 实例锁 = 任意时刻唯一活写者（导入、自检、正式服务同一把锁）。

同时按任务书兜底条款落**显式裁决记录** `{work-dir}/single-writer-ruling.json`（并拷入 target 收据目录）：`{strategy: "separated-roots", sharedWrites: "forbidden", enforcement: [path-binding, epoch-barrier, os-instance-lock], probes: [§5.2 四态证据], residualRisk: "旧程序继续运行时写旧根——分离根语义内的合法行为；旧根本身不受新栈保护，属 T06/运营域"}`。分离根 + 禁止共享写入为默认裁决，不因「已证明」而省略记录。

## 11. A09 设计（断点恢复）

- 断点矩阵：五必测（preflight / backup / import / pointer_switch / selfcheck）+ 全覆盖补测（stop_write_probe / quiesce_snapshot / import_verify / epoch_adopt / committed 前缘）。每断点真实 kill -9 × 1 + `--stop-after-phase` 干净停 × 1；import 相加 kill -9 落 convert 中段（bulk 435 批次源）× 2 时机（早/晚）。
- 每轮断言：同一入口 `--resume` 重试后 ①journal 相位续跑记录完整（history 链无跳相）；②任一观察时刻有效数据根唯一（pointer 无半态；switch 前无指针=合法）；③旧根 manifest 与切割前基线字节相等；④无双写者（双锁探针复核）；⑤终态与干净跑对拍：target 归一化 identity hash + 收据 issues 多重集 + adoption receipt（剥时间戳/cutoverId）逐项相等——**干净跑/重跑诊断集一致**（A08 灵魂条款沿用）。
- 证据：`t05-a09-<bp>-<seq>` 逐断点恢复日志 = journal 全文 + 相位记录 + 探针证据。

## 12. A10 设计（小盘预检拒绝）

- 取证方法（写死进实施）：`hdiutil create -size 64m -fs APFS -volname r08t05 probe.dmg` → attach → 挂载点作 `--backup-dir`（负例卷）；正控制 = 常规卷同 fixture 预检通过；**macOS 上绝不真填满磁盘**。
- 流程：migrate（backup-dir=小卷）→ 预检 #2 实测 avail < required → refuse，exit 2。
- 断言集：①exit=2 且 refusal.check="disk-space"、evidence 含 avail/required 实数；②**目标根不存在**（import 相才创建——结构性零部分产物）；③旧根 manifest 与基线字节相等（零改动）；④work-dir 仅含 journal(refused)+preflight-report.json；⑤无 `.tmp-*` 残留；⑥正控制同 fixture 通过（证明阈值是实测比较而非常量拒绝）。
- 近拟合控制（avail 略大于 required → 通过）：dmg 容量粒度所限，标 best-effort，失败不阻断验收但须登记。
- 单元层：statfs 检查器纯函数化（avail/required 注入）正/负/边界三例。

## 13. 评审域定性（T05_SCOPING 待复核清单）

- **PPG-1 / PPG-2**（预览 workspace 策略写腿/读腿缺口）：切割安全域定性——自检抽查端点**显式排除** preview/resourceio 面（已知缺口面不作通过判据）；T04 对 35 类 preserve-format 是字节归档，与预览面行为无关。**结论：PPG-1/2 不阻断切割安全；维持 T08 终检清单登记，T05 不修复、不遮掩。**
- **PPG-3**（远端桌面 LAN 认证不可证）：演练为 localhost 单写者切割，auth 面不在切割路径。**出 T05 域，维持登记。**
- **T01 基线 2 红**（composer-send.transaction / R02RustWebSocket）：①composer-send = 旧栈渲染写路径——静止点前置是旧服务已停（§5.1 四条件），在途发送事务结构上不可能跨越静止点；该红为独立产品/测试缺陷，不阻断切割；②R02RustWebSocket = 正是 P1 涟漪落点文件——epoch bump 工作必须与该既有红**对账而非覆盖**；两红留 kept-red 台账并记本定性。

## 14. 测试与证据计划

- 单元/分层（cargo test -p lingxi-service -p lingxi-adapters）：journal FSM 换相/恢复/拒绝；预检六检查器（statfs 注入）；双锁探针四态；epoch 执行器 7 相 + 各相中断续跑 + corrupt 拒；pointer 原子写读回；self-check 模式禁用面；lock diff 为空举证。
- e2e（scripts/rust-tauri/r08_t05_*.mjs）：`migrate_e2e`（干净跑全绿+断言）、`a09_kill_matrix`（§11）、`a10_small_volume`（§12）、`assert`（T04 归一化规则沿用：剥 run-bound 列/`<TARGET>` 归一/issues 多重集）。
- 证据：仓库根 `r08_run.py --evidence artifacts/rust-tauri/R08/t05-<name>-<seq>` 包裹；receipt 尾 `exitCode` + `sourceBindingsStable`；单次性目录；grep 模式含 `^error|panicked|failures`；红证据全保留。
- 台账：docs/rust-tauri/R08/PROGRESS-t05.md（本设计提交即开账）。

## 15. 预估改动清单（批准后细化）

| 文件 | 改动 |
|---|---|
| rust/crates/lingxi-protocol/src/lib.rs | +R08_CUTOVER；测试字面量 |
| rust/crates/lingxi-service/src/epoch.rs | +迁移注册表（1→2）+ `run_epoch_transition` + 续跑；测试 |
| rust/crates/lingxi-service/src/legacy_import_runner.rs | 新文件：main.rs 动词体纯提取（零行为变化） |
| rust/crates/lingxi-service/src/main.rs | +migrate 动词 + --self-check 模式 + R08_CUTOVER 接线（L208/L849） |
| rust/crates/lingxi-service/src/cutover/*.rs | 新模块：journal/preflight/probe/stillpoint/backup/adopt/pointer/selfcheck/编排 |
| rust/crates/lingxi-service/src/{lib,server}.rs | self-check 模式禁用面接线 |
| rust/crates/lingxi-adapters/src/storage/… | statfs 帮助函数（libc）；manifest builder 复用导出 |
| rust/Cargo.toml（service/adapters） | libc 直接依赖边（lock 无新条目，diff 举证） |
| rust/crates/lingxi-service/tests/ + scripts/rust-tauri/r08_t05_*.mjs | §14 测试 |
| docs/rust-tauri/R08/PROGRESS-t05.md | 台账 |

## 16. 裁决点（需 Lead 逐条批复）

- **P1**（必须裁决）：hello.dataEpoch 升 2 → websocket.ts:129 严格相等校验拒绝新服务。客户端改动在我域外且 R02RustWebSocket.test.ts 为禁触文件。建议：Lead 指派 ws-ui-state 或授权扩域做客户端侧一行策略改动 + 测试对账；T05 提供 Rust 侧全部证据。
- **P2**：statvfs 经 libc 直接依赖边（Cargo.lock `[[package]]` 零新增，diff 举证）vs `df -k` 子进程解析。推荐 libc。
- **P3**：legacy-import 动词体纯提取为 legacy_import_runner.rs 供两动词共用（零行为变化重构）。
- **P4**：pointer = atomic_write JSON 文件（rename 原子语义），拒 symlink（Windows 特权 + epoch 门 symlink-ambiguous）。
- **P5**：--self-check 模式面（禁用清单 + `--port 0` + stdout 发现行）。
- **P6**：源 sqlite 一律 copy-set→scratch→Online Backup，绝不直接打开源库（含 read-only/immutable——WAL 语义不完整）。
- **P7**：静止点枚举源 = STORES.json 69 × R01 策略 × T04 口径，counts 对拍。
- **P8**：data_epoch 真实升 2（推荐，SCOPING 明示）vs 维持 1 + 采用标记（评审为语义赝品，不推荐）。爆破半径按 §7.4 销账。
