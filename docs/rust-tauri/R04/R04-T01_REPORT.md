# R04-T01 报告｜建立工具目录与参数契约

执行代理：EXECUTOR-R04-T01-E01（一次性执行代理，ZCode Agent 工具派发）。
日期：2026-09-30。分支 `codex/rust-tauri-migration`，基线 `bf6450bcd722668188d1a3091681bf285875ef11`。
状态：**READY_FOR_REVIEW**（两层自查完成；不自行判独立 PASS）。

---

## 1. 目标与结论

发现、描述、权限与执行指向同一份真实工具；完整有效参数与结构化结果契约落地；schema
预算与外部 dialect fail-closed；安装/更新/禁用/卸载对 generation 与描述快照的影响有明
确语义；R04-SUP-03 在 T01 消费点逐项评估。

结论：全部当前到期义务已实现并真实执行测试（§5 验证，全部退出码记录在案）；R03 行为不
变量经工作区全套件 + 十套件钉数修复回归 + A15/A16 组合链复验无回归；一个环境级失败
（`r00_management_leaves`，macOS 应用防火墙拦截新建未签名测试二进制的入站回环，与本改动
无关，证据见 §5.3）如实记录为未验证项。

## 2. 实现摘要（关键文件:行）

### 2.1 工具目录（核心交付）

- `rust/crates/lingxi-kernel/src/toolcatalog.rs`（新文件，约 2400 行含单测）：
  - `ToolTargetId`（L37）/`ToolOrigin`（L52）/`tool_target_id()`（L119）：稳定 target 身份，
    字节格式与现役 TS registry（`lib/tools/invocation/identity.ts` 的
    `tool:first-party:{name}` / `tool:plugin:{plugin}:{name}` / `tool:mcp:{server}:{name}`）
    完全一致，`encode_fragment` 为 encodeURIComponent 兼容编码。display name
    （`manifest.display_name`）与身份分离。
  - `ToolManifest`（L522）：来源、local/display 名、别名、版本、输入/输出 schema
    （复用协议 `ToolSchemaDocument`，逐字节保留）、权限契约（`PermissionContract`，
    四种 kind 对齐现役 `FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS`）、可用性
    （`Availability`：available/deferred/disabled/**future**——未迁移能力诚实登记为
    不可调用，不伪装 available）、超时、并发上限、来源自称权限
    （`DeclaredPermission`，仅审计）与**已验证**恢复能力
    （`ToolRecoveryCapability`，默认 CONSERVATIVE）。
  - `SchemaBudget`（L626）+ `validate_schema_document`（L763）：schema 大小（默认 64KiB）、
    嵌套深度（16）、对象属性数（128）、验证节点成本（100k）四类硬预算；引用词汇
    （`$ref/$id/$defs/$anchor/$dynamic*/$recursive*/$schema`…）显式拒绝
    （`SchemaReferenceUnsupported`，永不联网解析）；无法精确执行的验证关键词
    （`pattern`、`if/then/else`、组合子、`multipleOf`、`uniqueItems`…）使 schema 本身
    不兼容（`UnsupportedSchemaFeature`，注册失败，不宽松放行）；厂商注解键（`x-*` 及
    未知非验证键）原样保留不解释；dialect 经协议 `check_schema_dialect` 校验，未知
    dialect 明确失败。
  - `EffectiveArguments`（L1012）：不可变有效参数对象。构造时强制：顶层对象、数字必须是
    ±2^53 安全整数（浮点/越界整数 `ArgumentsNotSafeInteger` 硬失败——与 TS
    canonical-json 的 hard-fail 对齐，RR-T02-F2 裁定）、深度/大小/属性数预算。
    `digest()`（L1076）由可信边界对 canonical 字节计算；`digest_matches()` 供驱动反伪造
    核对。
  - `normalize_arguments`（L1198）：原始请求→schema 校验（类型/enum/const/数值界/长度/
    required/additionalProperties:false）→**按 schema 逐层填默认值**→同一份规范化对象。
    未知键在 `additionalProperties:false` 下拒绝，schema 未声明时保留（不丢数据）。
  - `summarize_arguments`（L1159）：**仅含键名与类型标签**的形状摘要
    （`{path:str,limit:int,list:arr[3]}`），不含任何值——摘要不可反推出路径/正文/命令
    （单测 `summary_never_carries_values` 用真实路径/命令/正文断言零泄漏）。
  - `ToolRegistry`（L1572）：`register`（L1596，重复 target 拒绝；别名与任何其他目标的
    名/别名冲突=`NameCollision`，**同主名不同来源合法**——双双入索引、名单独解析显式
    `TargetAmbiguous`，不制造第二身份）、`resolve`/`describe`/`describe_full`/`search`/
    `snapshot`（冻结副本）、`update`（L1736，身份不可变，改身份必须注册新目标+卸载旧
    目标）、`set_availability`（L1824）、`uninstall`（L1853）、`prepare_invocation`
    （L1887）。**生成代次语义**：catalog generation 每次成功变更 +1；target generation
    随 update/enable/disable +1；`prepare_invocation` **先**校验调用方 pin 的 catalog
    generation——不匹配即 `StaleCatalog`（携带 held/current 代次，文案要求重取快照），
    旧描述永不指向新含义（R04-A02）；描述快照是不可变副本（单测
    `snapshots_are_frozen_copies`）。
  - `PreparedToolCall`（L1528）+ `into_tool_request()`：T02 网关/T03 批准的绑定输入
    （身份+双代次+有效参数+可信摘要+权限契约+已验证恢复能力+超时）。

### 2.2 参数与结果契约演进（R03 → R04）

- `rust/crates/lingxi-kernel/src/ports.rs`：
  - `ToolRequest`（L855）：新增 `arguments: EffectiveArguments`（完整有效参数，执行器
    消费的就是这份）；`args_digest` 仍存储但必须与 arguments 派生一致；
    `from_effective_arguments`（L887，可信边界构造：摘要由构造器派生，模型自填摘要不被
    接受）、`digest_matches_arguments()`（L920）。`delegation` 载荷不变（R03-T06 语义保持）。
  - `ToolOutcome`（L1010）：`Success` 从 `content_digest: String` 演进为
    `result: ToolSuccess`（L1021：内容块 `Vec<ContentBlock>`、资源引用
    `Vec<ResourceRef>`、截断标志、进程退出/运行状态 `ToolRunStatus`（L1044：
    `Exited{code}`/`Running{handle}`，T05 拥有完整语义）、派生 `content_digest`
    （完整性/审计值，journal 收据 dedup id 语义不变）。`success_text`（L1078）便捷构造。
- `rust/crates/lingxi-service/src/runs.rs`：
  - **反伪造参数门**（L980-1040）：驱动在写 invocation intent 前核验
    `digest_matches_arguments()`；不匹配＝提供方/适配器协议违规：以**可信摘要**（对实参
    计算）落 journal intent，收据 `Failed+dispatched=false`，工具事件结构化失败，
    **零派发**，循环继续（对抗项「伪摘要不触发副作用」在真实链上关闭）。
  - 委派成功（L1399 附近）：子运行/线程身份成为**真实内容块**（`success_text(identity)`），
    派生摘要继续充当 dedup id——R03 收据语义不变。
  - `journal_receipt_of` / `tool_result_wire`：成功收据 dedup=派生内容摘要、进程状态入
    detail；wire 工具结果携带**真实内容块/资源引用/截断标志**（不再输出
    `content-digest:` 占位文本）——R03 运行器/R05 适配器可取得可消费结果。
- `rust/crates/lingxi-service/src/invocations.rs`：`RegistryCapabilities`（L104）——R03
  交接中「R04 registry 取代恢复能力解析」的落地：按 target id/名称解析 manifest 的**已
  验证**恢复能力；未注册/歧义目标一律 CONSERVATIVE（未知≠乐观）。默认注入不变
  （`ConservativeCapabilities`）。

### 2.3 规范 JSON 修复（R04-SUP-03 / RR-T02-F1）

- `rust/crates/lingxi-protocol/src/canon.rs`：canonical 写入器改为**按 UTF-16 码元序排
  序对象键**（`cmp_utf16` L115、手写 `write_canonical` L68——标量编码与 serde_json 紧凑
  形字节一致，有等价性单测），与 TS 侧 `Object.keys().sort()` 的默认比较在全键形上一
  致；非 BMP 分叉类（星形键 vs U+E000..=U+FFFF 键）有 Rust golden 单测
  （`non_bmp_keys_sort_in_utf16_unit_order_ts_parity`）与跨语言 node 证据脚本
  （`artifacts/rust-tauri/R04/T01-E01/cross-lang-canon-nonbmp.mjs`，exit 0）。
- `docs/rust-tauri/R01/PROTOCOL_SPEC.md` §9：修正原「UTF-16 码元序与码点序等价」的错误
  论断为 UTF-16 码元序规范，并登记 F2 裁定（TS hard-fail 为正确语义，Rust 工具参数摘要
  边界对齐硬失败）。

### 2.4 依赖与工作区

- `rust/crates/lingxi-kernel/Cargo.toml`：新增 `serde_json = "1.0.151"`（与
  protocol/service 同一锁定版本，`rust/Cargo.lock` 仅新增一条依赖边，无新包版本）。
- 共享 TS/Node 消费面**未触碰**（canon 修复只改 Rust 写入器行为并补 Rust/跨语言证据；
  TS canonical-json.mjs 本就按 UTF-16 序排序，行为无需变更）。

## 3. 调用链与修改范围

真实调用链（目录层）：provider/适配器原始参数 →
`ToolRegistry::prepare_invocation`（pin 代次门→解析（别名→权威；同名多源需源限定）→
可调用性→schema 校验→默认值/规范化→`EffectiveArguments`→可信摘要/形状摘要）→
`PreparedToolCall` →（T01 最小接线）`into_tool_request()` → `ToolExecutorPort` 执行器
（T02 网关在其后接管，本 Task 不建平行网关）。运行驱动侧：`ToolRequests` 进入
`runs.rs` 工具环 → **摘要反伪造门** → R03-T05 journal 写序
（intent→authorized→started→执行→receipt）全部保持。

修改文件清单：

- 新增：`rust/crates/lingxi-kernel/src/toolcatalog.rs`；
  `rust/crates/lingxi-service/tests/r04_t01_tool_catalog.rs`；
  `artifacts/rust-tauri/R04/T01-E01/**`（证据）。
- 修改（产品）：`lingxi-kernel/src/{ports.rs,lib.rs,Cargo.toml}`、
  `lingxi-protocol/src/canon.rs`、`lingxi-service/src/{runs.rs,invocations.rs}`、
  `rust/Cargo.lock`（kernel→serde_json 一条边）。
- 修改（测试适配，断言不降级，见 §6）：`lingxi-service/tests/` 下 21 个 R03 测试文件
  （`ToolRequest` 字面量→构造器、`ToolOutcome::Success` 字面量→`success_text`、恢复
  resume 重构请求携带完整参数并把「记录内容」从摘要断言升级为**内容级断言**）。
- 修改（文档）：`docs/rust-tauri/R01/PROTOCOL_SPEC.md` §9、
  `docs/rust-tauri/R04/R04_TEST_MAP.json`（T01 条目）、本报告。
- 未触碰：生产默认入口、Node/Electron 栈、xtask stage maps（R04.json 属 T08）、
  `ORCHESTRATOR_PROGRESS.json`（总控文件）。

## 4. 验收场景与逐项结果

| ID | 要求 | 实现/测试 | 结果 |
|---|---|---|---|
| R04-A01 | 两个来源不同的同名工具：发现/描述/调用只命中正确 target | `r04_a01_catalog_and_execution_share_one_source_of_truth`（integration，计数执行器）+ kernel 单测 `same_name_two_sources_never_overwrites_and_name_only_is_ambiguous`、`target_ids_are_origin_namespaced_and_ts_compatible` | PASS（两执行各命中自身 target；执行关联记录含每次执行的 canonical 参数负载；同名单独解析显式 Ambiguous 列出双 id） |
| R04-A02 | generation1 缓存在工具升 generation2 后提交旧请求：明确拒绝/要求刷新 | `r04_a02_stale_catalog_generation_is_refused_until_refresh` + kernel 单测 `stale_catalog_generation_refuses_and_requires_refresh`、`disable_and_uninstall_change_callability_and_generation` | PASS（`StaleCatalog{held,current}`，错误文案含 re-describe 指令；拒绝期间执行计数零增加；刷新后新 schema 正常准备执行；卸载后旧 pin 同样被代次门拒绝） |
| 对抗·别名与主名权限一致 | 别名不得改变权限 | `adversarial_alias_and_primary_name_share_one_permission_contract` + kernel `aliases_resolve_to_the_authoritative_identity_and_share_permissions` | PASS（别名/主名 prepared 的 target/权限契约/摘要/恢复能力完全一致；别名与其他目标名冲突=注册拒绝） |
| 对抗·畸形 schema/过深参数/非法必填类型/伪摘要不触发副作用 | 全部 fail-closed 零副作用 | `adversarial_malformed_inputs_never_reach_the_executor`（$ref 拒绝且无半注册痕迹；深参数/类型违规/浮点全拒绝；计数执行器 0 次；合法对照执行 1 次）+ `adversarial_forged_digest_is_refused_with_zero_dispatch_on_real_chain`（**真实 ServiceState/run 驱动**：伪造摘要→执行器 0 次调用、journal 绑定可信摘要、`failed`+`dispatched=false`、运行 completed.with_final；诚实对照同链执行 1 次、succeeded） | PASS |
| 规范化碰撞明确处理，不制造第二工具身份 | 别名/名字冲突策略 | kernel `NameCollision`（别名 vs 任何其他目标名/别名；主名 vs 其他目标别名）+ 跨来源同主名显式 Ambiguous | PASS |
| 来源自称只读≠授予只读分类 | declared ≠ grant | kernel `declared_read_only_does_not_grant_verified_recovery` + integration `registry_capabilities_resolve_recovery_classification` | PASS（declared=ReadOnly 时恢复能力仍 CONSERVATIVE） |
| T01 消费点恢复能力解析 | R03 交接义务 | `RegistryCapabilities`（注册表解析，未知目标保守） | PASS |
| schema 预算/dialect/引用 | 大小/深度/成本有界、外部 dialect 明确失败、禁止默认联网解析 | kernel `schema_budget_and_depth_are_enforced`、`reference_and_unsupported_keywords_fail_loudly`（$ref/pattern/未知 dialect/厂商注解四类） | PASS |
| 安装/更新/禁用/卸载对 generation/快照/句柄的影响 | 旧描述不得指向新含义 | `update`/`set_availability`/`uninstall` 单测 + 快照冻结单测 + A02 全链 | PASS |
| 完整参数契约 | 执行器拿到的是参数本体而非摘要 | 计数执行器记录 canonical 参数负载并断言精确内容；`EffectiveArguments` 不可变 | PASS |
| 结果契约 | 内容块/错误/资源引用/截断/退出状态可消费 | `ToolSuccess` 结构 + wire 映射单测（runs.rs `tool_outcome_maps_onto_wire_status_one_to_one`、`success_receipt_keeps_digest_dedup_and_status_detail`） | PASS |

## 5. 验证（真实命令与退出码）

环境：macOS darwin 27.0.0 arm64；rustup 1.98.1（`/Users/study_superior/.cargo/bin/cargo`，
PATH 先于 `/opt/homebrew/bin`）；Node v24.16.0。全部命令在仓库根执行；原始输出归档于
`artifacts/rust-tauri/R04/T01-E01/`（`gates/`、`r03-regression/`、
`r03-repair-suites-regression/`、`workspace-test-*.log`），复跑脚本
`run_t01_validation.sh`。

| # | 命令 | 退出码 | 备注 |
|---|---|---|---|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 101（仅 1 环境失败） | 41 套件 ok＋1 环境失败套件、526 通过/0 断言失败；唯一失败 `r00_management_leaves` 见 §5.3 |
| 4 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | 0 | 56 个生成文件零漂移（canon 键序修复未影响任何既有 golden——分叉类不存在于既有样本） |
| 5 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | 0 | D1–D5+负向电池全过（kernel 新增 serde_json 不触碰任何禁用模式） |
| 6 | `cargo test -p lingxi-service --test r04_t01_tool_catalog` | 0 | 6/6（A01/A02/四对抗） |
| 7 | `node cross-lang-canon-nonbmp.mjs` | 0 | TS canonical == Rust golden（非 BMP 分叉类） |
| 8 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh <fresh dir>` | 0 | 十套件（G01–G06+RR2-F05）全绿，钉数精确（13/6/5/5/5/2/8/7…） |
| 9 | `bash scripts/rust-tauri/r03_t08_matrix.sh <abs dir>` | 0 | A15 组合矩阵 28 叶用例+11 组合计数全绿（真实链） |
| 10 | `bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh <abs dir>` | 0 | A16 种子机制四相全绿 |

npm/TS 侧未运行：本 Task 未触碰任何 TS/Node 源（§2.4），按派单「如触碰」条件不适用。

### 5.3 已知环境失败（如实登记，非本改动回归）

`r00_management_leaves::r00_management_positive_and_negative_branches_on_real_service`
失败于「request to 192.168.3.5:… stalled during write/read exchange (POST
/lingxi/v1/web-auth/login, 0 bytes read)」，其 panic 文案本身即说明为 macOS 应用防火墙/
代理 TUN 拦截**未签名测试二进制**的入站连接（环境失败，非断言失败）。取证：
- 同一测试在暂存本 Task 改动后的原树上运行通过（复用了 R03 时期已放行的旧二进制路径）；
  恢复改动后连续两次失败（kernel 变化→测试二进制哈希变化→防火墙视为新未签名 app）。
- 该测试与工具目录/参数契约无任何交集；其失败与 41 个绿套件互不掩盖。
- 影响：R04-T08 最终候选重跑完整 R03 Gate 时，若防火墙策略未变，该环境项会再现——已在
  风险 §8 登记（需要环境层处理：对本机测试二进制放行或关闭阻塞，非代码可修）。

## 6. R03 行为不变量与测试适配证明（断言不降级）

- 全部 R03 测试的生产断言未删减；适配仅限构造形状：`ToolRequest{...}` 字面量 →
  `ToolRequest::from_effective_arguments(...)`（该构造器**更强**：强制摘要与参数一致，
  R03 测试因此顺带获得一致性保障）；`ToolOutcome::Success{content_digest:X}` →
  `success_text(X)`（语义等价：内容=X，dedup=派生摘要）。
- 两处「恢复 resume」测试（`invocation_journal.rs` F04 幂等恢复、`tool_receipt_unknown.rs`
  RR2 验证链）：原从 journal 行摘要重构请求（R03 只有摘要可用）——现重构请求携带完整有
  效参数，且「核验=记录结果」的断言从**摘要相等升级为内容相等**（取出真实内容块比对，
  强度更高）；执行计数/单次执行断言原样保留。
- 十套件钉数由 `r03_g07_repair_suites.sh` 以精确计数复验（§5 #8），任何测试删除/改名/
  过滤 0 都会在该脚本变红。
- `runs.rs` 内嵌单测新增 2 条（wire 内容块映射、成功收据 dedup+退出状态 detail），原 4
  条断言保持。

## 7. R04-SUP-03（RR-T02-F1..F4）在 T01 消费点的评估

| 风险项 | T01 消费点 | 结论 | 证据 |
|---|---|---|---|
| RR-T02-F1 canonical 非 BMP 键序 Rust/TS 分叉 | 工具参数/内容摘要（canon.rs 全部 digest 路径） | **CLOSED（T01 修复）** | Rust 写入器改按 UTF-16 码元序；Rust golden `non_bmp_keys_sort_in_utf16_unit_order_ts_parity`；跨语言 node 脚本对同一分叉类样本输出字节相等（exit 0）；PROTOCOL_SPEC §9 错误论断修正。既有 56 个 golden 零漂移（check-contracts exit 0）证明对现存样本行为不变 |
| RR-T02-F2 TS canonical 浮点/>2^53 硬失败 vs §8/§9 矛盾 | 工具参数摘要边界（第三方 MCP schema 携带浮点/大整数 default） | **CLOSED（T01 消费点裁定+落地）**：TS hard-fail 是正确语义；Rust 工具参数边界（`EffectiveArguments`/`prepare_invocation`）对浮点与越界整数同样硬失败（`ArgumentsNotSafeInteger`，单测含 1.5 与 2^53+1 负例、2^53−1 正例）；`canonical_bytes<T>` 的既有序列化面（wire 类型按规范不含浮点）不动——未知事件 raw 保留语义（§8）不受影响；spec §9 补记裁定。第三方工具 schema 携带浮点 default 时该工具参数无法通过本边界＝按 fail-closed 口径不宣称兼容（与 RISK_REGISTER failure_handling 一致） |
| RR-T02-F3 EventPayload 回退吞畸形已知事件+两信封不变量未编码 | T01 **不消费**事件 schema 解析面 | **不适用/继续 CARRIED**：本 Task 未触碰事件解析/生成 schema；其消费点是 R04 事件面任务与 R08 客户端接入（resolve_by_stage 登记不变）。T01 期间无新增风险敞口 |
| RR-T02-F4 Rust Seq/u64 解析宽松（007/+5） | T01 **不消费** wire 数字解析面 | **不适用/继续 CARRIED**：`EffectiveArguments` 的安全整数硬失败方向与该修复一致（更严不更宽）；其 golden 修复留待对应 wire 解析消费任务，登记不变 |
| （关联）RR-T02-F5 WS close 4409 | — | 归 R08，非本 Task 范围（登记不动） |

## 8. 未验证项与风险

1. `r00_management_leaves` 环境失败（§5.3）——需要环境处理（防火墙对新建测试二进制放
   行），代码无对应修改；T08/最终候选重跑 R03 Gate 时将再现，届时需按环境问题处置而非
   代码回退。
2. 跨平台（Windows/Linux）未验证：本 Task 仅在 macOS arm64 实测；纯 Rust 领域逻辑无平
   台分支，但按任务书口径本地结果不替代其他平台验证。
3. schema 子集的覆盖广度：支持的验证关键词为白名单（§2.1），现役四基础工具与 MCP 常见
   schema 形态可覆盖，但真实第三方 schema（T07 接入时）可能命中
   `UnsupportedSchemaFeature`——这是**设计内的显式不兼容**（fail-closed），不是缺陷；
   T07 需要时可按同规则扩白名单并补负向测试。
4. `ToolRunStatus` 目前只在 kernel 结果与 journal detail 中承载；wire `ToolResultWire` 无
   对应字段（避免本 Task 改动生成契约）。进程工具（T05）落地时决定 wire 呈现（内容块/
   资源引用），已在代码注释与本报告登记——这是受控缺口，不是静默降级（结果内容块本
   Task 已可消费）。
5. R04-SUP-03 F3/F4 保持 CARRIED（§7）；若后续任务在 T01 边界消费事件 schema/宽松数字
   解析，需先回补对应 golden。
6. 风险（低）：`RegistryCapabilities` 以名称解析时若目标注册为多源同名会得到
   Ambiguous→保守能力（fail-closed 方向，可接受）；T02 网关统一 target id 后此路径消失。

## 9. 回退

回退范围＝本 Task 获准修改（§3 清单）：恢复 `ports.rs`/`runs.rs`/`canon.rs`/
`invocations.rs` 与 21 个测试文件、删除 `toolcatalog.rs` 与 `r04_t01_tool_catalog.rs`、
还原 `Cargo.lock`/kernel `Cargo.toml` 的 serde_json 边即可回到基线形态。原始失败证据与本
报告保留；canon 键序修复如需单独回退，会重新打开 RR-T02-F1（跨语言 golden 分叉），必须
连同 PROTOCOL_SPEC §9 修正一起回退并重新登记风险。

## 10. 替身边界声明

- `CountingExecutor`/`Gate`（`ToolExecutorPort` 替身）与 `ForgedDigestProvider`
  （`TurnProviderPort` 替身）：仅产生外部响应/外部工具输出，记录到达身份；不替代被测的
  注册表、规范化、权限契约、journal、驱动裁决或恢复分类（那些全部是真实实现）。
- 无任何策略/授权/存储/监督被替身伪造；伪造摘要场景中的「拒绝」由真实 `runs.rs` 驱动门
  产生，非测试内模拟。

## 11. 证据索引

- `artifacts/rust-tauri/R04/T01-E01/run_t01_validation.sh`——复跑脚本（绝对路径口径）。
- `gates/`：rust_fmt_check / rust_clippy / check_contracts / check_boundaries /
  r04_t01_acceptance_tests / cross_lang_canon_nonbmp 各 .log。
- `workspace-test-cargo-test-workspace-locked.log`——全工作区测试原始输出。
- `r03-repair-suites-regression/`——十套件钉数结果（repair-cases.json+summary+逐套件日志）。
- `r03-regression/A15|A16/`——R03 组合矩阵与种子机制复验。
- 新测试文件：`rust/crates/lingxi-service/tests/r04_t01_tool_catalog.rs`。
