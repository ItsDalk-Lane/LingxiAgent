# R03 RR2 F05 E01 执行报告 — R03-RR2-F05-01（canonical requestId 全链一致 + 旧 cause_id 兼容读取）

- 执行者：EXECUTOR-R03-RR2-F05-E01（定点修复，仅此一缺陷）。
- 基线：`c96f7cc635cf18fa83a81b0e28f33d4fed8baf9c`（分支 `codex/rust-tauri-migration`，开工时工作区干净，HEAD 与基线一致）。
- 本报告只陈述事实与运行结果，不写 PASS/验收结论；验收判定属独立审查者（C06）。
- 未执行 git commit / git push；未触碰任务书目录、AGENTS.md、`.sync-audit/`、其他阶段文档与本任务无关的 scripts。

## 1. 缺陷复现（失败原件）

证据根：`artifacts/rust-tauri/R03/repair-current/RR2-F05-E01/failure-originals/`

- `baseline-run.txt`：在**未修改任何产品代码**的工作区上（仅新增测试文件），运行
  `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test request_id_canonicalization -- --test-threads=4`
  （rustup 锁定工具链 1.98.1，`CARGO_NET_OFFLINE=true`，独立 `CARGO_TARGET_DIR`），退出码 **101**，1 通过 / 4 失败：
  - `rr2_f05_c02_padded_id_raw_and_canonical_retries_bind_one_logical_request` 失败：落库 cause_id 为 `request: req-42 `（原始串），期望 `request:req-42`。
  - `rr2_f05_c02_restart_matrix_foreground_background_confirmed_unknown` 失败：fg/确认/原样格重启后重试拿到**全新受理**（`run_count: 2`）——盲重执行实锤，非纸面推导。
  - `rr2_f05_c03_whitespace_variants_share_one_canonical_chain` 失败：`\treq-tab\t` 落库为 `request:\treq-tab\t`。
  - `rr2_f05_c05_legacy_cause_ids_stay_linkable_across_restart_shapes` 失败：旧格式行 `request: req-42 ` 被逻辑 key 查询漏掉 → 全新受理（`run_count: 2`）。
  - `rr2_f05_c01_plain_id_control_group_keeps_the_restart_contract` 通过（无空白控制组——与既有 `adv_c05_*` 一致，证明既有绿色测试没暴露该缺陷的原因）。
- C04 与 C05 歧义用例引用了修复后新增的错误变体（基线无该类型无法编译），按约束在修复后加入；缺陷本身的反例由上列 4 个失败原件证明。
- 外部计数器为真实追加文件（`external+1` 行数），重启 = `storage().close()` + drop + 同数据根重新 `boot_on`，进程内注册表随之清空。

## 2. 修复设计

### 2.1 canonical 边界放哪、为什么

- **规则本体（唯一实现）放 lingxi-kernel**：`lingxi-kernel/src/subagent.rs` 新增 `canonical_request_id(&str) -> &str`（Rust `str::trim`，完整 Unicode White_Space 集）与 `REQUEST_CAUSE_ID_PREFIX = "request:"`。理由：`RunLineage`（锚点写入方）在 kernel，跨重启读取方在 lingxi-adapters，受理策略在 lingxi-service——三层的公共底座是 kernel；规则放 kernel 使三层**复用同一实现**而非各自重写 trim，且不违反依赖方向（kernel 不依赖 service）。锚点前缀 `request:` 原先在 kernel（写入）与 adapters（查询）两处硬编码，现单一来源化。
- **策略（非空、≤128 字节）仍在受理边界**：`lingxi-service/src/dedup.rs::validate_request_id` 改为调用 kernel 的 `canonical_request_id` 后再执行策略检查，返回 canonical 串；接受集不变（不会因规范化扩大接受或截断内容，长度检查按归一后字节数）。
- **单一 canonical 事实产生点**：`lingxi-service/src/sessions.rs` 新增私有 `canonicalize_submission_request_id`，在 `execute_submission_for` 与 `execute_background_for` 两个公开受理面的**最前端**各调用一次（先于会话读取、busy gate、run-id 分配、dedup 预留、任何持久写与派发），随后以 canonical 形态重构 `ExecuteSubmission` 向下传递。下游全部消费者——内存 DedupKey（`admit_submission`）、跨重启持久查询（`find_request_binding`）、`DriveAuthorization::user_submission`→`RunLineage` cause_id、错误回显（InvalidRequestId / DuplicateRequestConflict / AdmissionInFlight / RequestIdBoundToEarlierRun / RequestIdBoundAmbiguous）——使用同一事实。
- **原始 ID 仅作审计字段**：raw 与 canonical 不同时打一条 `tracing::info`（raw_request_id / canonical_request_id），不再成为任何身份来源。
- **不是调用点临时 trim**：前台/后台/重启查询/错误补偿共用同一 canonical 入参；`admit_submission` 内部保留 `validate_request_id`（对 canonical 输入幂等）作为纵深防御；`runs.rs::DriveAuthorization::user_submission` 增加 debug 断言（非 canonical 输入在 dev/test 立即失败），防未来调用方回退。HTTP 入口（lib.rs）本就先 validate/规范化，现在与 service 面一致（幂等）。
- 上轮 F05 全部语义（Pending/Committed/Unverified 生命周期、新鲜度、主体/会话隔离、同 key 不同内容冲突、未知副作用不删绑定、两阶段受理与派发拒绝补偿）零改动：`admission_dedup_consistency`（5）与 `admission_dedup_adversarial`（5）原样通过。

### 2.2 兼容读取方案（对基线已产生的带空白 cause_id 旧行）

- `RunDatabase::find_run_id_by_request`（精确等值 `l.cause_id = ?`）替换为 `find_request_binding`，返回新的 `lingxi_kernel::ports::RequestBindingLookup`：
  - `Unbound`：该逻辑 key 无任何持久锚点；
  - `Bound { run_id }`：恰一个运行（规范锚点**或**单一旧格式变体）；
  - `Ambiguous { run_ids }`：多个不同运行绑定同一逻辑 key（修复前重复执行的冻结事实），按 created_at/run_id 降序（确定性）。
- SQL 只用 `LIKE 'request:%'` **收窄扫描**；权威匹配在 Rust 侧完成：剥 `REQUEST_CAUSE_ID_PREFIX` 后 `canonical_request_id(raw) == canonical`。明确**不用 SQLite TRIM()**（其默认字符集仅小部分 ASCII，覆盖不了 U+3000/U+00A0 等 Rust trim 空白）。
- 服务面处理：`Bound` → 既有 `RequestIdBoundToEarlierRun`（拒绝并点名旧运行）；`Ambiguous` → 新增 `SessionExecuteError::RequestIdBoundAmbiguous { request_id, run_ids }`（HTTP 映射为 409 `request_id_bound_ambiguously`，列出全部运行）——不挑最后一条、不静默当新任务执行；`Unbound` → 正常受理。旧行**永不改写**（lineage 不可变语义不变，测试断言冻结行原值）。

### 2.3 歧义判定的边界

同 key 多运行只在"历史原始值归一冲突"（或基线造成的规范+原始并存）时出现；修复后的新写入只产生规范锚点，且同 key 第二次受理会被内存 dedup 或持久查询拒绝，不会再制造新的歧义。

## 3. C-ID → 测试名 → 运行结果对照

套件：`rust/crates/lingxi-service/tests/request_id_canonicalization.rs`（真实 composition root、真实 SQLite、真实 supervisor/gate/dedup；"重启"=close+drop+同数据根重boot；竞态用 0 许可 Semaphore 停驻/ ParkingStartPort 固定窗口，无 sleep 等待）。

| C-ID | 测试名 | 基线 | 修复后 |
|---|---|---|---|
| C01 | `rr2_f05_c01_plain_id_control_group_keeps_the_restart_contract` | 通过 | 通过（E1） |
| C02 | `rr2_f05_c02_padded_id_raw_and_canonical_retries_bind_one_logical_request` | **失败** | 通过（E1） |
| C02 对抗矩阵 | `rr2_f05_c02_restart_matrix_foreground_background_confirmed_unknown`（前台/后台 × 确认/未知副作用 × 原样/规范化 = 8 格，每格独立数据根+计数器+实例对） | **失败**（fg/确认/原样格） | 通过（E1，8 格全绿） |
| C03 | `rr2_f05_c03_whitespace_variants_share_one_canonical_chain`（Tab/CRLF/U+3000/U+00A0/混合/128 字节边界/内部空白保留 + 3 类非法 ID 副作用前拒绝） | **失败**（Tab 变体） | 通过（E1） |
| C04 | `rr2_f05_c04_colliding_raw_ids_one_namespace_isolation_and_compensation`（停驻窗口并发 in-flight（回显 canonical）、不同内容冲突、跨会话/跨主体隔离、起始事务失败补偿后 canonical 重试全新受理、容量拒绝后 canonical 重试、结算后第三种原始形态 replay、全部响应 run 有 durable 行） | 修复后新增 | 通过（E1） |
| C05 | `rr2_f05_c05_legacy_cause_ids_stay_linkable_across_restart_shapes`（单一旧行正常停止 settled + 遗留 active 异常重启→恢复扫描收束→仍可关联；冻结行不改写） | **失败**（旧行漏查→全新受理） | 通过（E1） |
| C05 对抗 | `rr2_f05_c05_colliding_legacy_rows_refuse_as_explicit_ambiguity`（三个原始变体归一冲突 → `RequestIdBoundAmbiguous` 列出全部三个运行；计数不增；三行冻结原值） | 修复后新增 | 通过（E1） |
| C06 | 本执行者不写 C06 测试；生产者门禁腿见 E2（十套件逐套件固定计数机器核验） | — | 见 E2 |

另：kernel 新增单测 `canonical_request_id_trims_the_full_unicode_whitespace_set`、`request_cause_id_prefix_is_the_single_sourced_anchor_format`（workspace 内通过）。

## 4. pin 增量表（旧值 → 新值）

| 处 | 旧 | 新 |
|---|---|---|
| `scripts/rust-tauri/r03_g07_repair_suites.sh` pin 表 | 9 行（cancel_link_inheritance 7 F01 … background_steering 8 F07） | 原 9 行**零改动**，追加 `pin request_id_canonicalization 7 RR2-F05`（注释注明 RR2 增量与理由）；build `--test` 列表同步追加；"nine" 措辞更新为 ten（G01-G06 九 + RR2-F05-01 一） |
| `rust/crates/xtask/src/stage_map.rs` `R03_REPAIR_PIN_TABLE` | 9 元组 | 原 9 元组零改动，追加 `("request_id_canonicalization", 7, "RR2-F05")`（注释注明增量）；`r03_repair_producer_pin_table_matches_the_registered_suites` 保持两侧一致 |
| `rust/crates/xtask/src/stage_maps/R03.json` | R03-RP01 note 列 9 套件 | note 追加 RR2 增量句（第十套件 RR2-F05 计数 7）；supplementalCoverageNote 追加同一增量说明；命令/场景/证据路径注册零改动 |

无删除测试、无降低 pin。

## 5. 本地门禁结果（全部真实运行；命令自仓库根，rustup 锁定工具链 1.98.1，离线 + 独立 CARGO_TARGET_DIR）

| 项 | 命令 | 结果 |
|---|---|---|
| E1 新增/受影响套件 | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test request_id_canonicalization --test admission_dedup_adversarial --test admission_dedup_consistency -- --test-threads=4` | 通过：7+5+5 passed，exit 0 |
| E2 修复套件生产者 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh artifacts/rust-tauri/R03/repair-current/RR2-F05-E01/repair-suites`（全新空目录） | 通过：exit 0，十套件逐套件精确计数全绿（含 RR2-F05 7/7），gaps.txt 为空 |
| E3 fmt | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 通过（exit 0） |
| E4 clippy | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 通过（exit 0，无告警） |
| E5 workspace 全量 | `cargo test --manifest-path rust/Cargo.toml --locked --workspace` | 通过：exit 0，73 个 "ok" 结果行，合计 718 passed（RR1 基线 709 + 新套件 7 + kernel 新单测 2；含 xtask pin 测试与全部 72→73 套件） |
| E6 契约 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | 通过（exit 0，全部 PASS） |
| E7 边界 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | 通过（exit 0，全部 PASS） |

NOT_RUN 项：完整 verify-stage R03 与受影响 R02 定向链（属总控在候选提交后运行，本执行者职责外）。离线模式全程成功，未回退在线。

证据：`artifacts/rust-tauri/R03/repair-current/RR2-F05-E01/`（`failure-originals/`、`selfcheck/`、`logs/`、`repair-suites/`、`README.md` 逐文件↔命令↔C-ID 对应）。

## 6. 修改文件清单

产品代码：
- `rust/crates/lingxi-kernel/src/subagent.rs` — 新增 `REQUEST_CAUSE_ID_PREFIX`、`canonical_request_id`；`RunLineage::user_submission` 改用单一来源前缀并写明 canonical 契约；+2 单测。
- `rust/crates/lingxi-kernel/src/ports.rs` — 新增 `RequestBindingLookup`（Unbound/Bound/Ambiguous）。
- `rust/crates/lingxi-adapters/src/storage/run_store.rs` — `find_run_id_by_request`（精确等值）替换为 `find_request_binding`：LIKE 收窄 + Rust 侧剥前缀按 kernel 规则归一匹配，输出 Unbound/Bound/Ambiguous（确定性降序）。
- `rust/crates/lingxi-service/src/dedup.rs` — `validate_request_id` 的 trim 改为调用 kernel `canonical_request_id`（策略不变，文档更新）。
- `rust/crates/lingxi-service/src/sessions.rs` — 新增 `canonicalize_submission_request_id`；两个公开受理面头部各规范化一次并以 canonical 形态向下传；`SessionBackend`/erased/RunDatabase 实现/fake 后端的方法改为 `find_request_binding`；跨重启分支处理三态并新增 `RequestIdBoundAmbiguous` 错误变体。
- `rust/crates/lingxi-service/src/runs.rs` — `DriveAuthorization::user_submission` 增加 canonical debug 断言与契约文档。
- `rust/crates/lingxi-service/src/lib.rs` — 新增 `EndpointError::request_id_bound_ambiguously`（409）与路由映射。

测试：
- `rust/crates/lingxi-service/tests/request_id_canonicalization.rs`（新文件，7 测试，C01–C05 全覆盖）。

pin/门禁：
- `scripts/rust-tauri/r03_g07_repair_suites.sh`、`rust/crates/xtask/src/stage_map.rs`、`rust/crates/xtask/src/stage_maps/R03.json`（见 §4）。

证据/报告：
- `artifacts/rust-tauri/R03/repair-current/RR2-F05-E01/**`、本报告。

## 7. 疑点与未覆盖项（供审查者核对）

1. `find_request_binding` 对单个 (owner, session) 命名空间扫描其全部 user `request:%` lineage 行（LIKE 收窄 + Rust 匹配），非索引精确等值；单会话显式 ID 运行数通常很小，未做额外索引（最小修复）。
2. `Ambiguous` 的 `run_ids` 为 created_at/run_id 降序（确定性），报告/错误中同时列出全部，不做选择。
3. debug 断言仅在 dev/test 生效（release 编译剔除）；结构性保证来自两个公开受理面的唯一规范化点 + 既有调用面核对（HTTP 已先规范化；无其他生产调用方）。
4. C02 矩阵"未知副作用"格在进程 B 的重试用停驻工具作反证信号（若被错误受理，计数会 +1 被断言捕获）；未模拟"进程 B 期间旧进程驱动复活写入"的极端交错（存储已 close，队列拒绝写）。
5. R07 后台入口尚无生产调用方；其未来接线必须复用 `execute_background_for`（canonical 边界内）或在自身入口同样先规范化——已在 `RunLineage::user_submission` 契约注释与 `DriveAuthorization` 断言中设防。
6. 完整 verify-stage R03、受影响 R02 定向链、候选提交绑定与独立审查（C06）由总控/审查者执行，本报告不代其结论。
