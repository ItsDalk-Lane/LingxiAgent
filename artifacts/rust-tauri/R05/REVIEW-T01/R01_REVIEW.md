# R05-T01 独立审查报告 — R01_REVIEW

- 审查者：REVIEW-T01（独立验收子代理，非实现者）
- 审查日期：2026-10-02
- 审查对象：EXECUTOR-R05-T01 交付的 R05-T01（ModelGateway、范围清单与模型交换契约）
- 审查方式：只审查 + 隔离环境亲验，未修改任何产品代码；本代理新增文件全部位于 `artifacts/rust-tauri/R05/REVIEW-T01/`
- **总体结论：PASS**（C01–C12 全部 PASS；3 个观察项均非 FAIL，见 §6）

---

## 1. 候选绑定（证据锚定的树与二进制）

| 项 | 值 |
|---|---|
| 基线 SHA | `c549ff654508ab951e2cf39cf9d309fc9c6b8656`（分支 `codex/rust-tauri-migration` HEAD） |
| 改动形态 | 未提交工作树 diff（51 个 rust/ 文件），无 commit/push —— 与授权边界一致 |
| 工作树清单 | `artifacts/rust-tauri/R05/T01/working-tree-manifest.txt`（逐文件 sha256） |
| 清单摘要 | `sha256:e2c0c1f50c18797d01eb44e50fb5a28e1dbfcbc7045579af9781ae61964228c9` |
| 审查者复核 | 撰写本报告前重算全部 51 文件：**51/51 与清单全等**（`ALL 51 FILES MATCH MANIFEST`），清单哈希与上值一致 → 我验证的树就是实现者证据所指的树 |
| 审查者自建二进制 | `rust/target/debug/lingxi-service` `sha256:617ed372394bff8ee06a18c0cf407d68ad6ac7be5e09f45e4310047f3db1acd6`（`cargo build --locked -p lingxi-service`，源同上树） |
| 二进制驱动结果 | `logs/review-binary-driver-rerun.log`：**50/50 PASS，exit 0**（首次运行 `logs/review-binary-driver.log` 同绿；该二进制后被全量测试重建覆盖，故以重建+复跑方式重新绑定证据） |

## 2. 证据分类声明

- **亲自运行**：本报告引用的全部门禁、测试、二进制驱动、矩阵重算、ALF 探针均为我在本次审查中亲自执行，日志在 `artifacts/rust-tauri/R05/REVIEW-T01/logs/` 与 `alf-reverify/`。实现者日志（`artifacts/rust-tauri/R05/T01/`）仅作交叉核对，未转抄为实测。
- **源码推断**：接口演进完整性、凭证不落 kernel、现役契约对齐等结构性结论，由全仓 grep + 逐文件阅读得出，文中标注「源码」。
- **未验证**：`xtask verify-stage R04` 基线重跑按任务书规定不自跑（总控统一调度），仅做实现者证据一致性复核；真实供应商凭证/生产端点未测（T01 全部使用合成 key 与 127.0.0.1 替身，符合规格 §5）；附录 C/D 的 16 项负测归 T08，不在本切片。

## 3. 门禁与测试（全部亲自运行）

| 检查 | 结果 | 日志 |
|---|---|---|
| `cargo check --workspace --all-targets --locked` | exit 0 | `logs/cargo-check.log` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0；另 touch 五个 crate 入口强制全量重跑仍 0 警告（非缓存假象） | `logs/cargo-clippy.log`、`logs/cargo-clippy-forced.log` |
| `cargo fmt --all -- --check` | exit 0（零输出） | `logs/cargo-fmt.log` |
| `xtask check-contracts` | exit 0，56 个生成文件无漂移 | `logs/xtask-check-contracts.log` |
| `xtask check-boundaries` | exit 0，DEP-01..09 全过（含新 `models` 模块适用规则） | `logs/xtask-check-boundaries.log` |
| `cargo test -p lingxi-kernel` | 82/82 | `logs/test-kernel.log` |
| `cargo test -p lingxi-adapters` | lib 39/39 + 集成全绿 | `logs/test-adapters.log` |
| `r05_t01_model_plane` + `r05_t01_binary_wiring` | 7/7 + 2/2 | `logs/test-r05-suites.log` |
| `r04_t08_tool_matrix` + `run_lifecycle`（回归） | 12/12 | `logs/test-r04-regressions.log` |
| `cargo test --workspace --locked --no-fail-fast` | 90 个套件，**992 passed / 0 failed / 0 ignored**，exit 0 | `logs/test-workspace-nofailfast.log` |

全量失败面说明：实现者终态全量为 991/992，唯一失败是 `r00_management_leaves`（ALF 环境项，§7）。我运行时该机已对当前测试二进制哈希授予 ALF 放行，故 992/992 全绿——失败面恰好只有该环境项这一事实，由实现者日志与我的 ALF 复核共同确认。

## 4. 高风险链亲验（审查者自建驱动，非实现者 harness）

驱动脚本 `review_binary_driver.py`（我从零编写）：threaded `http.server` 协议替身 + 真实 `lingxi-service` 子进程（临时 home、真实 argv、真实 `--config` 文件、经认证 HTTP 端点提交）。四场景全部 PASS（`logs/review-binary-driver-rerun.log`，50 项断言）。

- **C02 真实接线**：错误 token 得 401 → 认证提交成功 → 替身记录**恰好 2 次**外发 `POST /v1/chat/completions`，均带 `Bearer sk-review-t01-A`、`model=stub-model-review`；第 1 请求声明 `read` 工具且只含 user 消息；**运行时 nonce 链**：替身在第 1 请求到达后才把工作区内 `nonce_probe.txt` 写出（内容 `NONCE-5dc7355d…`，提交时不存在），第 2 请求的 tool 消息携带同一 nonce —— 证明真实的中途工具执行与真实结果回传（wire 落盘 `c02-stub-wire-capture.json`）。DB 断言：终态 `completed.with_final`、2× `model_call_completed`、1× `tool_call_completed`、usage 以**字符串化 u64** 持久（`"inputTokens":"31"`；我首版断言误写数字形态导致唯一一次 FAIL，修正后全过——属我的断言笔误，非产品缺陷）、持久身份 `(main, stub-model-review)`、替身谎报的 `review-stub-lies` 从未落盘、SIGTERM 干净退出。
- **C03 诚实失败**：无模型平面配置 → run 落定 `completed.no_final.no_provider_configured`，零 model call、零 tool call、零伪造消息行。
- **C06 零外发**：配置 anthropic-messages 平面（T01 未实现族）→ run `failed.provider_error`，替身请求计数**恰为 0**（拒绝发生在任何外发之前）。
- **C04 加强版**：两 provider（alpha/beta）挂同一 model id `shared-review-model`、**不同 key**（`sk-review-t01-A`/`sk-review-t01-B`）：alpha 端点**零命中**，beta 服务两个 run 且请求**只带 B key**，A key 未出现在任何 wire，两会话持久身份均为 `(beta, shared-review-model)`。

## 5. 接口演进完整性（源码核验）

- `next_turn` 全仓单一签名：声明 `rust/crates/lingxi-kernel/src/ports.rs:1033`，唯一生产实现 `rust/crates/lingxi-adapters/src/models/provider.rs:79`，唯一生产调用点 `rust/crates/lingxi-service/src/runs.rs:909`。旧签名无残留消费者。
- **无第二 Agent loop**：`lingxi-adapters/src/models/` 各适配器模块无任何 tool dispatch / journal / storage / Run 终态写入（grep 零命中）；工具执行、事件持久化、终态裁定全部留在 service/kernel 侧。
- 宿主 `ToolCallId` 仍仅由运行层铸造：`rust/crates/lingxi-kernel/src/lib.rs:506` `tool_call_id()`（`{run_id}-tc{seq:04}`）；`provider_call_id` 仅作关联值，无任何宿主状态以它为键。
- 既有测试 diff 为零断言删改的纯签名同步 codemod（0 删 0 增断言），未借重构放水。
- `redaction.rs` 修复核实为真：`replace_each_ci` 闭包形参序 `(lower, rest, from)` 与 finder 形参序 `(rest, lower, from)` 原先对调，导致混合大小写密钥词（`apiKey`/`Password`/`TOKEN`）逃出扫描；5 处调用点修复（`rust/crates/lingxi-service/src/redaction.rs:160-185` 区域），由 `r05_secret_key_words_match_case_insensitively` 与 `r05_provider_api_key_shapes_are_redacted` 两测试钉死，模块文档如实记录缺陷与修法。

## 6. 逐项裁决（C01–C12）

| C-ID | 检查点 | 裁决 | 证据类别与指针 |
|---|---|---|---|
| C01 | 现役范围无漏项 | **PASS** | 亲自运行：从 `docs/rust-tauri/R00/ACCEPTANCE_MAP.json` 独立重算 execution_stage_ids 含 R05 的叶 = 恰 130，与 `R05_SCOPE_MATRIX.json` 双向零差异；125 share / 5 deferred，**5 个 deferred 全是 R07 UI/桌面壳入口，无"缺密钥"式延期**；抽 10 叶裁定理由成立（43 种不同 share 文本，非模板复制）。provider 矩阵 39 == `lib/providers` 41 文件 − 2 helper；R00 交叉 37 match:true + 2 个非 provider 语音叶、0 冲突；callsite 69 行的文件存在性 / source_sha256 / 行锚点全部核验；knowledge 死槽确证（全仓无 `resolveAuxiliaryModel("knowledge")` 调用）。三个生成脚本我重跑 exit 0 且三矩阵**字节不变**（`logs/c01-*-regen.log`） |
| C02 | 正常二进制真正接线 | **PASS** | 亲自运行（二进制级，§4-C02）：真实外发、真实工具网关、运行时 nonce 证明真实结果回传；非 bootstrap_with_deps 冒充 |
| C03 | 无配置诚实失败 | **PASS** | 亲自运行（二进制级，§4-C03）：显式 unconfigured 终态、零副作用、零伪造 |
| C04 | 同名模型不串供应商 | **PASS** | 亲自运行（二进制级加强版，§4-C04）：端点/key/身份归属均不串。观察项 O1 见下 |
| C05 | 配置快照与变更生效点 | **PASS** | 亲自运行：`r05_t01_model_plane` 7/7 含 `c05_reload_through_management_surface_swaps_atomically`（经 management HTTP 面真实 reload、代次递增、新 run 用新平面）与 `c05_reload_without_a_configured_plane_is_a_loud_404`；源码：代次钉定在 `lingxi-kernel/src/model_exchange.rs` 快照语义 |
| C06 | 不支持能力零请求 | **PASS** | 亲自运行（二进制级，§4-C06）：外部计数器恰为 0；另进程内 `c06_unsupported_family_makes_zero_requests` 与 adapters `unsupported_capabilities_refuse_before_any_resolution` 同绿。澄清项 O2 见下 |
| C07 | 辅助槽位确实独立 | **PASS** | 亲自运行：adapters 套件含 `chat_and_auxiliary_routes_resolve_independently`（重指一路不改他路解析身份）。边界如实：辅助槽的 Rust 业务接线尚未接入（callsite 矩阵如实列 27 aux_slot + 1 死槽为现役 TS 面），归后续阶段，非 T01 缺漏 |
| C08 | 工具声明来自实际目录 | **PASS** | 亲自运行：kernel 套件含 model_exchange 快照测试（只声明钉定代次的可调用驻留目标；空快照=代次 0；wire 名净化按最严协议面）；二进制 C02 第 1 请求 tools 声明恰来自真实 registry 的 `read` |
| C09 | 工具目录变化不可误路由 | **PASS** | 亲自运行：adapters `a_history_target_missing_from_the_snapshot_fails_loudly`（模型请求的历史目标不在钉定快照 → 响亮协议违规，绝不猜测）；kernel `toolcatalog` 过期代次拒绝测试 |
| C10 | 调用身份不来自模型 | **PASS** | 亲自运行：adapters 测试证明 usage/身份取自 driver 可信上下文而非响应载荷；二进制 C02 独立证实：stub 谎报 model 不落盘、持久身份=路由身份 |
| C11 | 本地无密钥兼容不扩权 | **PASS** | 亲自运行：adapters `credential_is_explicit_and_never_defaulted`、C02 Bearer 上线、C04 `auth:none` 无凭证头。源码：现役 `shared/provider-auth.ts` 契约（authType∈{none,optional}，任意端点允许缺 key 或本地 URL 放行）与 Rust 侧（`auth` 键强制显式、空 key 装载期响拒、`kind:none` 须显式声明）语义对齐，未扩权 |
| C12 | 依赖方向与范围不倒退 | **PASS** | 亲自运行：`check-boundaries` exit 0（DEP-01..09）；`cargo tree -p lingxi-kernel --edges normal --locked` 无 tauri/reqwest/hyper/keyring/secret-store（`Cargo.toml`/lib.rs 中的字样仅为描述与注释文本）；无第二 Agent loop（§5） |

### 观察项（非 FAIL，不阻塞通过）

- **O1（C04 证据形态）**：实现者进程内 `c04_same_model_id_different_providers_never_cross_wire` 两端 provider 均为 `auth:none`（`rust/crates/lingxi-service/tests/r05_t01_model_plane.rs:690,695`），其台账 observed 中 "carries only A's endpoint/key" 的 **key 维度在其自有证据中为空集**（wire 上本无 key）。本审查的二进制级 keyed 加强版（§4-C04）补齐了该维度：不同 key 的双 provider 下，请求只带本 provider 的 key、他方 key 从未上任何 wire。建议 T02 保留 keyed 双 provider 测试形态。
- **O2（C06 维度适用性澄清）**：检查点维度列表含 "tools-undeclared chat"，但 T01 配置模型**本无 per-model 工具/图像能力轴**（现役 chat 面同样没有）；T01 适用切片是 operation/协议族级拒绝（已亲验零外发）。per-model tools/image 能力轴须由引入该轴的后续阶段（T03 及以后）承接对应维度，不得静默遗漏——callsite/范围矩阵已如实记录此边界。
- **O3（台账措辞）**：台账 C04 observed 的 "stub A carries only A's key" 对实现者自身证据轻微过头（见 O1）；对审查者加强版证据则成立（A 端点 0 命中、A key 零上线）。已按实际证据语义记录，不影响裁决。

## 7. ALF 环境项独立复核（实现者裁定为环境项 → 本审查确认成立，证据链闭环）

背景：实现者终态全量 991/992，唯一失败 `r00_management_leaves` 的 LAN 用例被裁定为 macOS Application Firewall（ALF）拦持新链接 adhoc 签名二进制的入站 LAN 流。

我的独立复核（全部亲自执行，`alf-reverify/`）：

1. **零 Lingxi 代码探针复现（17:04）**：自编探针 `zz_probe2_review`（pid 25440）→ LOOPBACK OK / LAN CONNECT OK / WRITE OK / **LAN STALL**（内核级握手与写完成，连接 10s 内未送达 `accept()`）；`log show` 同步出现该 pid 的 Enqueue + Prompt 记录。
2. **即时全因果链捕获（17:39，撰写本报告前复核时）**：新编译探针 `zz_alfprobe_r1`（sha256 `194bcb31…`，adhoc 签名，pid 30845）运行；ALF 日志（`alf-reverify/alf-log-zz_alfprobe_r1.txt`）完整记录：`Handle flow (pid 30845)` → `Enqueuing new inbound flow` → `Prompting for a filtering decision` → 4.7 秒后用户应答 `DoAnswer` → `Allowing app zz_alfprobe_r1` → 探针立即收到被拦持的连接（LAN DELIVERY OK）。**拦持—prompt—放行—送达全链在单次运行中闭环**。
3. **环境状态核验**：ALF 仍处于 enabled（State = 1）；`socketfilterfw --listapps` 现列名：我的两个探针、实现者的 `/private/tmp/zz_probe2`、以及多个 `r00_management_leaves-*` 测试二进制哈希（verdict 已授予 → 我 17:27 全量测试 992/992 全绿的原因）。
4. **代码侧排除**：`git diff` 确证 T01 未触 bind/accept/NetworkMode 任何相关路径；探针为零 Lingxi 代码，行为差异纯由 ALF verdict 状态决定。

结论：机制真实、归因为环境项成立，非代码回归；实现者的失败面裁定（恰好此一项）与我的证据一致。**注意**：该拦截会在任何"新哈希 adhoc 二进制接收入站 LAN 流"的场景复发（含后续 stage 的 verify-stage 内同类用例），属验证环境管理事项，不属 T01 代码范畴。

## 8. 验收锚点覆盖（§4-T01）

- **A01（范围/对账）**：由 C01 覆盖 —— 130 叶双向零差异、deferral 无密钥类理由、三矩阵可再生（字节不变）。
- **A02（真实接线）**：由 C02 覆盖 —— 审查者独立二进制驱动 + 运行时 nonce 链，强度高于一报复用。
- **A15（全链负测）**：T01 切片内由 C03/C06 的诚实失败与零外发覆盖；A15 全链 16 负测归 T08（附录 C/D），不在本切片，本报告不就其下结论。

## 9. 最终结论

**PASS**。R05-T01 的 C01–C12 十二项检查点全部通过，其中 C02/C03/C04/C06 四项高风险项由审查者自建独立证据链（非实现者 harness）在真实二进制上验证；范围清单经独立重算对账零差异；接口演进无残留消费者、无第二 Agent loop；ALF 环境项证据链闭环。三个观察项（O1 证据形态、O2 维度适用性澄清、O3 台账措辞）不阻塞通过，建议后续 stage 承接。
