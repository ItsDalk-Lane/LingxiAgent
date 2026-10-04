# R05-T02 独立验收报告（REVIEWER-R05-T02 · 第 R01 轮）

- **任务**：R05-T02 —— 凭证、刷新、撤销与秘密保护（credentials / refresh / revocation / secret hygiene）
- **验收者**：独立验收子代理 REVIEWER-R05-T02（与实现者不同上下文；只做审查与隔离验证，**未改任何产品代码**）
- **验收日期**：2026-10-02（本地）
- **基线提交**：`c549ff654508ab951e2cf39cf9d309fc9c6b8656`（`git rev-parse HEAD` 亲跑确认，与研究基线一致）
- **被验对象**：T01+T02 的**未提交工作树**（`git status --porcelain` 亲跑：45 个修改 + 7 组新增，含 `rust/crates/lingxi-service/src/credentials/`、`rust/crates/lingxi-adapters/src/models/`、`tests/r05_t02_*.rs` 等）
- **环境**：macOS arm64；`rustc/cargo 1.98.1 (48a229cea 2026-09-01)`（rustup）；PATH 需含 `~/.cargo/bin` 与 `/opt/homebrew/bin`

## 0. 证据分类约定

本报告每条证据标注其一：

- **[亲跑]**：验收者本人在本会话中真实执行的命令/测试/探针，日志在 `artifacts/rust-tauri/R05/REVIEW-T02/`。
- **[源码推断]**：验收者亲自阅读当前工作树源码得出的结论（给出行号），未对该行为单独执行。
- **[未验证]**：未独立复验的项目，如实标注并说明采信依据或影响。

**重要边界声明**：全程未持有、未使用任何真实供应商凭证；所有 OAuth/凭证验证均为 loopback stub + 注入式假材料。本报告**不声称**任何真实供应商（xAI / OpenAI-Codex 等）链路已通过。真实供应商验证状态为 **BLOCKED_NOT_AUTHORIZED**（见 §9）。

## 1. 基线与工作树绑定核验

| 项 | 结果 | 方法 |
|---|---|---|
| HEAD == 研究基线 | ✅ `c549ff65…` | [亲跑] `git rev-parse HEAD` |
| 实现者工作树清单 sha256 | ✅ `e9856c2fc9a8594435746bc377b16905a2023ef13972a90e61ae2836c868df8c` | [亲跑] `shasum -a 256 artifacts/rust-tauri/R05/T02/working-tree-manifest.txt`，与台账每行 `workingTreeDigest` 一致 |
| 清单内容逐文件复算 | ✅ 57/57 匹配 | [亲跑] 对当前树按清单逐文件复算 sha256，全部一致（本会话早段完成；验收中我曾 `touch` rust 源文件强制 clippy 重跑，仅改 mtime 未改内容，`git status` 无新增 diff） |
| `redaction.rs` 实际 diff | ✅ 只含声称改动 | [亲跑] `git diff HEAD -- …/redaction.rs`：152 行 +/-，仅 ① `is_token_char` 增 `/`/`=`；② 5 处 `replace_each_ci` 调用点改传小写 haystack；③ 文档注释改写；④ 3 个新 pin 测试。无夹带 |

## 2. 静态门禁（全部亲跑）

| 门禁 | 命令（摘要） | 结果 | 日志 |
|---|---|---|---|
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | ✅ exit 0。首跑命中缓存（0.19s），随后 touch 强制对 7 个本地 crate 真实重 lint 32.42s，零警告 | `REVIEW-T02/gate-clippy.log`、`gate-clippy-forced.log` |
| 契约漂移 | `cargo run -p xtask -- check-contracts` | ✅ exit 0（56 个生成文件无漂移；API_COMPAT_MATRIX 626 条一致） | `REVIEW-T02/gate-check-contracts.log` |
| 边界/归属 | `cargo run -p xtask -- check-boundaries` | ✅ exit 0（736 features / 69 stores / DEP-01..09 / D5 全 PASS） | `REVIEW-T02/gate-check-boundaries.log` |
| 格式 | `cargo fmt --all -- --check` | ✅ exit 0 | `REVIEW-T02/gate-fmt.log` |

## 3. 测试亲跑（聚焦 + 回归）

| 套件 | 结果 | 耗时 | 日志 |
|---|---|---|---|
| `lingxi-service --test r05_t02_credentials` | ✅ 16/16 | 3.92s | `REVIEW-T02/test-t02-focused.log` |
| `lingxi-adapters --test r05_t02_oauth_flows` | ✅ 19/19 | 3.37s | 同上 |
| `lingxi-service --lib`（含 credentials 单元 6、store 4、redaction 22，均点名核对在日志中） | ✅ 330/330 | 7.94s | 同上 |
| `lingxi-adapters --lib` | ✅ 45/45 | 0.16s | 同上 |
| 回归：`r05_t01_model_plane` | ✅ 7/7 | 3.61s | `REVIEW-T02/test-regressions.log` |
| 回归：`r05_t01_binary_wiring` | ✅ 2/2 | 0.99s | 同上 |
| 回归：`r04_t08_tool_matrix` | ✅ 10/10 | 300.70s（真实执行） | 同上 |

## 4. 高风险链亲验（自建独立探针）

探针为独立 crate `artifacts/rust-tauri/R05/REVIEW-T02/probe/`（`[workspace]` 隔离、非 workspace 成员，path 依赖指向 `rust/crates/*`；产品树零改动）。运行日志 `REVIEW-T02/probe-run.log`：**7 个探针全绿**（`full_chain_c09.rs` 1 个 + `probes.rs` 6 个）。

| 高风险链 | 探针/方法 | 亲验结果 |
|---|---|---|
| A03/C02 并发 401 单 flight | `probe_c02_six_way_concurrent_401_merge`：6 路并发 401 对同一 provider | ✅ 驱动调用**恰好 1 次**，6 路全部拿到刷新结果 |
| A04/C05 刷新途中撤销 | `probe_c05_revoke_midflight_fence_and_restart`：flight 在途时 revoke，随后放行迟到 token | ✅ 迟到写回被代次 fence 丢弃、**未持久化**；重启后 `not_logged_in` |
| C04 多等待者取消 | `probe_c04_all_waiters_cancelled_flight_still_lands`：**全部**等待者取消（实现者只测了 2 取 1，本探针为更严形态） | ✅ flight 不被 abort，仍落地并持久化，驱动调用 1 次 |
| C06 持久化失败诚实性 | `probe_c06_persist_failure_is_honest_and_recoverable`：注入 store 写失败 | ✅ `persisted:false`、`lastPersistFailure` 含注入标记 "REVIEW-INJECTED"；重启后回滚旧 token，不假装持久 |
| C09 自选新形态秘密 | `probe_c09_novel_base64_secret_forms_never_survive_redaction`：**我自选** marker `RvwXQ7+k/3mQ2pLmN8+vPqRsTuVwXyZaBcDeFgHiJ=`（42 字符，含 `/` 与 `=`） | ✅ 裸文本 / `key=` 赋值 / 混合大小写键 / Bearer 头 / URL query / `=` 粘连 全部脱敏；38+8 分裂逃逸两半均被捕获；良性长句不误伤；`scrub_materials` 对我的 marker 精确擦除有效 |
| C09 全链（真实启动链路） | `probe_c09_full_chain_my_marker_never_persists_or_echoes`：marker `x9Rvw/Q7+kFullChain=Marker0123456789+/==` 走真实 service 启动链路，stub 确认收到 `Authorization: Bearer <marker>`（扫描非空洞），stub 401 echo marker | ✅ run 以 failed 终结；对 service home 递归字节扫描 **0 文件**含 marker；`terminal_reason` 与 SQLite `key_events.payload_json` 汇总无泄漏（表名核实为 `key_events`）。会话种子 `sess_local_alpha` |
| C10 重定向凭证剥离 | [亲跑] 19 个 OAuth 测试中含重定向用例全绿；[源码推断] `oauth.rs:155` `redirect::Policy::none()` + `oauth.rs:195-203` 3xx → 响亮 `Protocol` 错误，凭证请求绝不跟随 | ✅ |

另：C08 OAuth 双形态 19 测试亲跑全绿，并抽查核对语义：device-code pending/slow_down(+5s)/期限/拒绝/取消/echo 擦除/不可信 URI 拒绝；PKCE S256 绑定（重算 challenge==verifier 哈希）、错误 state 400 且流程存活、过期 state 双检、重复回调被已关 listener 拒绝、每次 begin 新 state；refresh 分类 invalid_grant/401→Reauth、429/5xx/超时→Transient、JWT exp 回退、无过期→响亮拒。与现役 TS 对齐抽查：`lib/auth/xai-oauth.ts`（30s 超时、slow_down）、`core/mcp/clients/oauth.ts`（S256）一致。

## 5. C01–C12 逐项判定

| 项 | 判定 | 依据（验收者） |
|---|---|---|
| C01 单一材料出口 | **PASS** | [亲跑] `r05_t02_credentials` 相关用例绿；[源码推断] `lib.rs:988-994` plane 有而 credential_service 无 → 响亮启动错；`ApplicableAuth` 只存在 adapters+service、不进 kernel |
| C02 401 单 flight 合并 | **PASS** | [亲跑] 探针六路并发=1 次驱动调用；台账测试亲跑绿 |
| C03 本地过期先刷新 | **PASS** | [亲跑] 聚焦测试绿；[源码推断] `mod.rs:418-436` 过期腿与 401 共用同一单 flight |
| C04 等待者取消不杀 flight | **PASS** | [亲跑] 全等待者取消探针绿（严于实现者用例） |
| C05 刷新途中撤销 fence | **PASS** | [亲跑] 探针绿：迟到 token 被弃、未持久化、重启 not_logged_in |
| C06 持久化失败诚实 | **PASS** | [亲跑] 探针绿：`persisted:false`+失败详情+重启回滚 |
| C07 无无限 401 循环 | **PASS（有保留）** | [亲跑] 列明各腿（invalid_grant/401/5xx/超时/二次401）全有界且测试绿；401 恰好重试一次（provider.rs）、`DEFAULT_MAX_ATTEMPTS=2`（runs.rs）兜住 Transient。**但发现另一触发点的无界循环 → 见 F-01** |
| C08 OAuth 双形态 | **PASS** | [亲跑] 19 测试绿 + 语义抽查（§4） |
| C09 秘密保护/脱敏 | **PASS** | [亲跑] 自选新形态 marker 探针 + 全链探针均绿；redaction diff 复核只含声称改动 |
| C10 重定向不携凭证 | **PASS** | [亲跑] 测试绿；[源码推断] `oauth.rs:155,195-203` |
| C11 撤销跨重启存活 | **PASS** | [亲跑] C05 探针含重启腿；[源码推断] revoke 先删 store 行再翻内存+代次 bump；静态 key 重启重播种已在演进文档如实披露 |
| C12 句柄不可猜/绑定 | **PASS** | [亲跑] 测试绿（伪造/跨 provider/代次不符均拒）；[源码推断] 128-bit 随机、绑定 (provider, generation, expiry)。补充：时间过期腿无测试 → F-04 |

## 6. 台账抽查（12 行中抽 6：C02/C04/C05/C06/C08/C09）

- 台账文件 `docs/rust-tauri/R05/R05_ACCEPTANCE_LEDGER.json`，T02 行 12/12 标 PASS，`baseline` 如实声明"未提交工作树"。
- 抽查 6 行逐行核对：命令真实可重跑（我已重跑对应套件）、`testNames` 与我亲跑日志中的测试名双向核对存在、`evidence` 文件存在、`workingTreeDigest` 与我复算的清单 sha256 一致（以 C09 行为例逐字段核对，`runnerDigest`/`lockfileHashes`/`platform`/`toolchain` 字段齐全）。
- **结论：抽查 6/6 属实**，未发现虚报；C09 行所用 marker 与我自选 marker 不同（实现者用 `MARKER-key-0123456789abcdef`，我用含 `/`/`=` 的新形态），两路独立证据互证。

## 7. 语义审查（源码级）

- **凭证不经 kernel**：[源码推断] `ApplicableAuth`/`ProviderCredentialPort` 仅在 `lingxi-adapters::models::credentials` 与 `lingxi-service::credentials`；kernel 无材料类型。
- **遗留 trait 死代码**：kernel `ports.rs:1164` `CredentialPort` / `ports.rs:1174` `CredentialHandle` 全 workspace 零实现零调用（grep 亲验）→ F-03。
- **无全局锁/锁序**：锁序为 cell → store 一律一致（`mod.rs:909-912` 注释与代码一致）；providers map 用 std RwLock 快照后即释放（`reload` 注释 `mod.rs:570`），未观测到跨 await 持 std 锁。
- **401 重试有界**：provider.rs 401 恰好一次重试，403 不刷新；Transient 由 `DEFAULT_MAX_ATTEMPTS=2` 兜底。
- **管理面**：LocalUser 限定、无材料出口、revoke 审计入 management.json；reload 顺序先 `credentials.reload` 后 `gateway.reload`（`management.rs:653-656` 亲验）。
- **worker 环境**：env 白名单 + `env_clear`（workerrpc.rs），凭证不进 worker 环境。
- **store**：v1 未知字段 `serde(flatten)` 保留、版本冲突响亮、`atomic_write_private`（0600+双 fsync+rename，paths.rs:209）。
- **redaction**：`is_token_char` 已含 `/`/`=`（与现役 `[A-Za-z0-9+/_=-]` 对齐）；`replace_each_ci` 大小写修复有 pin 测试；误伤面由 21 个既有测试 + 我的良性长句探针覆盖。

## 8. 发现

### F-01（中）`resolve_impl` 的"刷新后重读"循环无次数上限 —— 持续签发立即过期 token 时无限循环

- **位置**：`rust/crates/lingxi-service/src/credentials/mod.rs:401`（`loop`）→ `:441` `RefreshVerdict::Refreshed { .. } => continue` 无计数。
- **亲验复现**（探针 `probe_resolve_refreshes_again_when_mints_are_still_expired`，`probe-run.log:45-46`）：驱动持续铸造已过期 token 时，`resolve` **3 秒不终止、驱动调用 265,137 次**。生产驱动下每轮 = 一次真实 token 端点 HTTP + 一次 store 0600 双 fsync 原子写。
- **生产可达路径**（[源码推断]）：
  1. `oauth.rs:248-258` `positive_seconds` 对 `(0,1)` 秒小数值 `as u64` 截断为 0（如 token 端返回 `expires_in: 0.4`）→ `oauth.rs:314-315` 得 `expires_at == now` → 新铸 token 立即过期；
  2. `expires_in: 1` 而一次 resolve 往返 >1s（慢网络/排队）：安装后重读时已过期；
  3. JWT exp 铸造时刻有效、重读时刻已过期（时钟推移/偏移）。
- **后果**：未取消的 run 在凭证解析阶段无限循环，对 token 端形成刷新风暴并反复烧写磁盘；可取消性本身完好（`wait_flight` cancel-safe，已被 C04 探针证实），但不取消则不终止。C07 列明的各腿（invalid_grant/401/5xx/超时/二次 401）全部有界且已验证——这是**另一触发点**，违反"循环有界"的验收精神。
- **修复建议**：每次 `resolve` 至多一次刷新——`Refreshed` 后重读仍过期 → 响亮错误（建议归入 `Transient` 或新增 `StillExpiredAfterRefresh`，绝不静默再刷）；附回归测试（驱动连续铸造已过期 token，断言 resolve 在 ≤N 次驱动调用内报错返回）。
- **影响面**：仅 OAuth provider 且 token 端行为病态时；静态 key 路径不受影响（`mod.rs:406` 直接返回）。

### F-02（低）`install_tokens` fence-1 对 reload 换 cell 存在 TOCTOU

- **位置**：`mod.rs:898-908` fence-1（map 现势检查，`Arc::ptr_eq`）在 **std 读锁内完成即释放**，随后 `:912` 才获取 cell 异步锁。两锁之间存在窗口：reload 恰在此时为该 provider 换入新 cell，则 fence-1 已通过、而本 flight 的 cell 已非现势。
- **fence-2 为何兜不住**：`:913` 的代次检查作用在**被换下的旧 cell** 上，其代次未被 bump（reload 是换新 cell 而非改旧 cell），检查通过。
- **后果**：迟到刷新仍按 provider 名写入 store（`:927`）并翻旧 cell 内存 → 新 cell 内存（早前从 store 播种）与磁盘分歧；最坏情形下后续 invalid_grant 响亮失败（无静默降级，危害有界）。
- **现状**：无 reload-mid-flight 交织测试；`mod.rs:566-568` 注释声称 map-currency 检查 fence 住该情形，与实现存在上述缝隙。
- **修复建议**：在 cell 锁内复查 map 现势（锁内重读 map `ptr_eq`），或 reload 换 cell 时对旧 cell 做代次 bump/标记 orphan；附 reload 与 flight 交织的回归测试。

### F-03（轻微/文档）kernel 遗留 `CredentialPort`/`CredentialHandle` 零实现零调用

- **位置**：`rust/crates/lingxi-kernel/src/ports.rs:1164`、`:1174`。
- **现状**：全 workspace grep 亲验无任何实现/调用；演进文档 §12 与台账 follow-up 已如实披露其为历史遗留。但 trait 现场无 deprecation/指针注释，文档注释读起来仍像现行解析路径，对后续读者是误导源。
- **建议**：加 `#[deprecated]` 或注释指针（指向 `lingxi-adapters::models::credentials::ProviderCredentialPort`），或在治理允许时移除。

### F-04（轻微）`CredentialError::Cancelled` 从未被构造；`resolve_handle` 时间过期腿无测试

- **位置**：变体定义 `rust/crates/lingxi-adapters/src/models/credentials.rs:75`；全 workspace 仅有匹配点（`provider.rs:57`、`credentials.rs:90,130` Display），grep 构造点为零。等待者取消实际走 watch 关闭→`Transient`（语义如实，但变体是死代码）。
- **测试缺口**：C12 句柄的伪造/跨 provider/代次不符均有测试，**时间过期腿**（`mod.rs:731` 附近）无直接测试。
- **建议**：删除 `Cancelled` 变体或在取消路径真实构造之；补过期句柄 pin 测试。

## 9. 未验证 / 受限项（如实声明）

- **真实供应商链路**：**BLOCKED_NOT_AUTHORIZED**。全程 loopback stub + 注入假材料；未持有任何真实凭证，不声称 xAI/OpenAI-Codex 等真实端点通过。
- **全 workspace `cargo test` 全套件**：**未独立复跑**。实现者声明环境项 R05-ENV-ALF-UNSIGNED-TEST-BINARY（macOS ALF 致 `r00_management_leaves` 失败）属环境限制；我采信其声明，且我亲跑的聚焦/回归套件全部 loopback、全绿，未见 ALF 影响。
- **Windows/Linux 平台行为**：未验证（本机仅 macOS arm64）。
- **F-02 的 TOCTOU 窗口**：源码级论证成立，未构造确定性交织复现（窗口为调度竞态，建议以锁内复查根除而非依赖复现）。

## 10. 总结论（R01 轮，2026-10-02 白日）

**PENDING（条件通过）** —— 已被 §11 的 fix-r1 复验取代（最终裁定见 §11.7）。

- C01–C12 各项及高风险链 A03/A04 按规格**全部验证通过**；静态门禁、聚焦测试、回归测试全部亲跑为绿；台账抽查 6/6 属实；工作树与基线绑定一致。
- 但 **F-01 属 T02 范围内真实的循环有界性缺陷**（验收精神层面的硬要求），需修复并附回归测试后**复验**方可达 PASS。建议同修 F-02（TOCTOU，低风险但修复廉价）；F-03/F-04 为建议性清理，不阻塞。
- 真实供应商验证仍为 BLOCKED_NOT_AUTHORIZED，不在本轮范围。

### 证据索引（本验收产出，全部位于 `artifacts/rust-tauri/R05/REVIEW-T02/`）

- `R01_REVIEW.md`（本报告）
- `gate-clippy.log` / `gate-clippy-forced.log` / `gate-check-contracts.log` / `gate-check-boundaries.log` / `gate-fmt.log`
- `test-t02-focused.log`（16+19+330+45 全绿）
- `test-regressions.log`（7+2+10 全绿，含 300.7s 真实执行）
- `probe/`（独立探针 crate：`tests/probes.rs` 6 探针 + `tests/full_chain_c09.rs` 1 探针）
- `probe-run.log`（7/7 绿；含 F-01 观测行：`resolve DID NOT TERMINATE within 3s; driver calls so far: 265137`）

---

## 11. fix-r1 修复轮复验（2026-10-02，R01 之后）

实现者修复 F-01—F-04（证据 `artifacts/rust-tauri/R05/T02/fix-r1/`，台账受影响行已更新）。本段全部为验收者**亲跑/亲读**结论；基线提交不变（`c549ff65…`），工作树清单已换代为 `sha256:edccced4a8e3…`（我对当前树逐文件复算 **57/57 匹配**；12 行 T02 台账的 `workingTreeDigest` 均已指向新清单）。

### 11.1 F-01（resolve 刷新循环无界）——已修复，亲验通过

- **源码** [亲读]：`credentials/mod.rs:407` 引入 `flight_awaited` 守卫——每次 resolve 至多等待一个刷新 flight；刷新后重读仍过期 → `:434-442` 响亮 `CredentialError::Transient`（"the token endpoint minted an already-expired token; …single-flight bound (F-01)"），绝不静默再刷。Transient 经 `provider.rs:55` 映射为可重试，由 run 侧 `DEFAULT_MAX_ATTEMPTS=2` 兜底（R01 已验该策略有界）→ 单次 run 最多 2 次刷新调用，完全有界。
- **探针** [亲跑]：我的 `probe_resolve_refreshes_again_when_mints_are_still_expired` 由观测型升级为评分型（断言快速响亮报错 + 恰好 1 次驱动调用）。**修复前实测**：3 秒不终止、265,137 次驱动调用；**修复后实测**：`resolve errored after 1 driver calls: … did not converge within this resolve's single-flight bound (F-1)`（`probe-run-fixr1.log`）。
- **回归测试** [亲跑]：三个新测试真实存在且由我点名亲跑通过（`--exact`，4 passed/330 filtered，含 F-04 的 pin）：
  - `credentials::tests::resolve_refreshes_at_most_once_when_every_mint_is_already_expired`（mod.rs:1316，expires_at==now 形态，断言 Err(Transient) + 驱动调用==1）
  - `credentials::tests::resolve_refreshes_at_most_once_when_a_one_second_grant_dies_in_transit`（mod.rs:1383，驱动内推钟确定性构造"往返超期"形态）
  - `r05_t02_oauth_flows::sub_second_expires_in_mints_an_immediately_expired_token`（oauth_flows.rs:853，mint 侧 pin：`expires_in: 0.4` 截断为 0 的语义被锁定——界限放在 service 侧是正确的责任划分）
- **全 workspace 同类循环排查**（实现者声称）：抽查属实——device 轮询有 `expires_in` 期限（oauth.rs:557/567）、PKCE accept 有 state 过期收束（oauth.rs:729 起），均有界。

### 11.2 F-02（install_tokens fence-1 TOCTOU）——已修复，亲验通过

- **源码** [亲读]：`install_tokens`（mod.rs:908-978）现在**先取 cell 锁**（:918）再在锁内复查 map 现势（:930-938），原 TOCTOU 窗口关闭。死锁安全性我独立论证核实：全 workspace 对 providers map 的写锁仅 `reload` 写段（:620-624），该段不碰任何 cell 锁；`reload` 循环内 cell 锁逐次获取即释放、不持 map 锁跨越——锁图无环。
- **实现者回归测试** [亲跑]：`credentials::tests::a_reload_replacing_the_cell_discards_the_late_writeback`（mod.rs:1413）由我点名亲跑通过——确定性终态 pin：换 cell 后迟到写回被 Discarded、store 保持旧行、新 cell 无刷新痕迹。
- **我的独立探针** [亲跑]：新增 `probe_f02_reload_swapped_cell_fences_my_late_writeback`（REVIEW 自選 marker：`at-REVIEW-late`/`rt-REVIEW-1` 等；可拆闸门的专用驱动），断言等待者收到诚实 `Revoked`、store 旧行不变、新 cell `last_refresh_at_unix_ms==None`、且换 cell 后第二次 resolve 经新 cell 正常落地——**通过**（`probe-run-fixr1.log`）。

### 11.3 F-03 / F-04 ——已修复，亲验通过

- **F-03** [亲读]：kernel `ports.rs:1172-1179` `CredentialPort` 加 `#[deprecated(note = …ProviderCredentialPort…R05-T08 ruling)]` + 归属指针文档；伴随 `CredentialHandle` 加指针注释（本体不标 deprecated，避免 trait 定义体自告警，`allow(deprecated)` 注释说明了取舍）。零实现零调用的现状不变，移除留待 T08 裁定——符合我"加 deprecation 或指针"的建议。
- **F-04** [亲读+亲跑]：`CredentialError::Cancelled` 变体及其 `provider()`/`Display`/`credential_failure` 三条匹配腿全部删除，全 workspace `grep CredentialError::Cancelled` **零命中**；枚举现为 7 变体；`credential_failure` 注释同步改为"取消由 future drop 表达"（如实）。时间过期腿新 pin `credentials::tests::an_expired_handle_is_refused`（mod.rs:1476）亲跑通过。
- **clippy** [亲跑]：`cargo clippy --workspace --all-targets --locked -- -D warnings` 我先跑命中缓存（0.19s），随后 `touch` 7 个涉事源文件**强制真实重 lint 26.29s，exit 0、零警告**（`gate-clippy-fixr1-forced.log`）——deprecation 标记在 `-D warnings` 下通过。

### 11.4 回归（全部亲跑）

| 套件 | 结果 | 耗时 | 日志 |
|---|---|---|---|
| `r05_t02_credentials` | ✅ 16/16 | 4.40s | `test-fixr1-focused.log` |
| `r05_t02_oauth_flows` | ✅ 20/20（19→20，新增 sub_second pin） | 0.55s | 同上 |
| `lingxi-service --lib` | ✅ 334/334（330→334） | 7.71s | 同上 |
| `lingxi-adapters --lib` / `lingxi-kernel --lib` | ✅ 45/45、82/82 | 0.16s/0.01s | 同上 |
| 4 个指名修复回归（F-01×2 + F-02 + F-04） | ✅ 4/4（`--exact` 点名） | 0.01s | `test-fixr1-named-tests.log` |
| `r05_t01_model_plane` / `r05_t01_binary_wiring` | ✅ 7/7、2/2 | 3.70s/1.24s | `test-fixr1-t01-regressions.log` |
| `r04_t08_tool_matrix` | ✅ 10/10 | 300.77s 真实执行 | `test-fixr1-r04t08.log` |
| 探针套件（8 个，含升级后的 F-01 评分探针与新增 F-02 探针） | ✅ 8/8 | — | `probe-run-fixr1.log` |

### 11.5 台账受影响行核对（C02/C05/C07/C12）

- 四行 `testNames` 共 **19 条，逐名与实现者 fix-r1 日志核对全部存在且为 ok**（19/19；首轮 9 个 MISS 系我正则未计 unit 测试全路径前缀，修正后全中）。
- 四行 `workingTreeDigest` 已换代为 `edccced4a8e3…` 且与当前树一致（57/57 复算）；`evidence` 增补 fix-r1 日志路径真实存在；`finishedAt` 已更新至修复轮。
- 实现者全 workspace `--no-fail-fast` 跑（`gates-test-workspace-nofailfast.log`）：91 个 "test result: ok"，**唯一失败仍为 `r00_management_leaves`**（panic 信息自证 macOS ALF 拦截未签名测试二进制拨号非 loopback 自地址 192.168.3.5）——与 R01 已备案的环境项 R05-ENV-ALF-UNSIGNED-TEST-BINARY 一致，与凭证面无关。我仍未独立复跑全套件，但该失败模式与凭证改动无交集，且我的聚焦/回归套件全 loopback 全绿。

### 11.6 新观察（不阻塞，建议性跟进）

- **F-02b（轻微，源码论证）**：`reload` 的"逐 cell 播种 → 最后一次性换 map"两段式仍非原子——新 cell 在写段之前从 store 播种（`seed_cell` mod.rs:611-617 读 store 在 :825）。若迟到 flight 的 `install_tokens` 临界区（持旧 cell 锁：fence→persist→翻内存）恰好物理穿插在"新 cell 播种之后、写段换图之前"（多 provider 配置下循环内有 await 让出点；单 provider 下仅剩无线程让出的物理抢占窗口），则终态为 store 领先新 cell 内存一代。后果有界且诚实：新 cell 持旧 refresh_token 再刷一次，最坏遇 rotation 得 invalid_grant → 响亮 Reauth；重启/下次 reload 即从 store 收敛。无秘密泄漏、无静默损坏。此窗口远窄于 F-02 原窗（原为异步间隙，现仅剩物理抢占/多 provider 让出点），可选加固：写段内二次播种，或换 cell 时给旧 cell 打 orphan 标记。**裁定为信息性 follow-up，不构成阻塞**（F-02 原报缺陷机制已按我建议根除）。
- **文档 nit**：`credentials/mod.rs` 头注释 :31-34 仍只述 401 侧界限；resolve 侧单 flight 界限已在修复点注释（:401-406），建议头注释补一句（不改不阻塞）。

### 11.7 fix-r1 复验结论

**PASS。R05-T02 裁定为验收通过。**

- F-01/F-02/F-03/F-04 四项全部修复并经我亲跑/亲读核实；修复附带的三（+2）个回归测试真实存在、非空洞、由我点名重跑通过。
- 全部聚焦/回归套件亲跑为绿；clippy 强制重 lint 零警告；台账受影响四行 19 个 testNames 逐名核实、树绑定一致。
- 遗留边界（如实保持）：真实供应商链路 **BLOCKED_NOT_AUTHORIZED**（全程无真实凭证，不声称通过）；`r00_management_leaves` 为已备案的 macOS ALF 环境失败，与凭证面无关；Windows/Linux 未验；F-02b 与头注释 nit 为建议性跟进。
