# R05-T06 独立审查报告（REV-T06 R01）

- 审查对象：R05-T06「多模态、辅助模型与 Worker 回调（operations 方言 + egress 守卫 + OperationService + worker async 桥）」（总控 §2.3/§2.4、§4-T06、附录 A 之 A11/A12、附录 B T06 C01–C12）
- 审查轮次：R01（首轮独立验收）
- 基线：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656`；R05-T01—T06 全部以未提交工作树交付（no_commit_push_authorization）
- 审查日期：2026-10-03；审查者：未参与 R05 任何实现的独立会话
- 证据目录：`artifacts/rust-tauri/R05/REVIEW-T06/`（本审查者亲跑产生，与实现者证据 `artifacts/rust-tauri/R05/T06/` 物理分离）
- 只读边界遵守声明：本审查未修改任何产品代码/测试/文档/台账；新增文件仅在本目录（六份复跑日志、egress 对抗探针及其输出、manifest 复算日志、本报告）。探针为独立 path-dep crate（`probe-egress/`，target dir 在 `/tmp`，不污染 `rust/target`），只调用生产公共入口 `EgressGuard`。

## 总体判定：CONDITIONAL_GO（离线范围）

13 个 case（C01–C12 + C11B）的执行证据真实：本审查者按台账原命令复跑六个测试组全部 exit 0（27+7+9+5+6+3 = 57 tests），worker async 桥的四条高风险链（C05 单线程不饥饿、C06 配额=1 嵌套不死锁、C10 挂起回调有界清理、C07 重放不二次外发）与 OperationService 的诚实媒体状态机（C12）均亲自复现。**但**本审查者自写对抗探针证明 C11B 出口守卫的"字面 IP 一律拒绝"策略存在实现面缺口（发现 F-01，必修）：非十进制 IPv4 拼写（hex/八进制/短式/整数/前导零/尾点）与 IPv4-mapped IPv6（`::ffff:a.b.c.d`）能以"https 公网主机名"分支通过守卫，而 reqwest 实际使用的 WHATWG URL 解析器（url 2.5.8，与 rust/Cargo.lock 同版）会把它们规范化为回环/私网地址——13/17 探针行为 BYPASS。这不属于已登记的 DNS-rebinding 离线不可判定缺口（无需任何 DNS）。其余两项（F-02 manifest 覆盖面、F-03 say argv 防御纵深）为不阻塞建议。LIVE 供应商验证维持任务书允许的 `BLOCKED_NOT_AUTHORIZED`，判定限定为离线范围。

## 复核范围声明（三类证据分明）

- **亲自运行**：六组定向测试复跑（台账原命令）；egress 对抗探针（对生产 `EgressGuard::check_url` 逐 URL 判定 + url 2.5.8 规范化对照，17 行电池、13 行 BYPASS，输出存 `probe-egress/probe-output.txt`）；实现者 working-tree-manifest 63 文件 sha256 全量复算（63 matched / 0 mismatched / 0 missing）+ manifest 覆盖面核对。
- **源码推断（亲读，未单独动态复现）**：`workerrpc.rs` 执行器回调分支全文（C09 身份字段拒绝、C07 预算键、`timeout_at` 约束）、`workermodel.rs` 配额获取与 deadline 夹紧、`BoundedWorkerModel` 计数/回执缓存生命周期、`operations.rs`（OperationService 全文）、`operations/mod.rs` 方言执行纪律、`auxiliary.rs`（槽位路由 + 诚实失败）、`egress.rs` 解析与下载全文、`quotas.rs` acquire 分层、`runs.rs:1299` 主模型 permit 在工具执行前 drop（C06 结构前提）、`lib.rs:1153-1231` bootstrap 接线、`management.rs` reload egress 换源、`r04_t07_fixture.rs` 新增 4 个 worker 模式、`R05_INTERFACE_EVOLUTION.md` §29.9–29.13/§30。
- **未验证**：真实供应商 LIVE 流量（未授权，登记 BLOCKED_NOT_AUTHORIZED）；全 workspace 套件与 fmt/clippy/check-contracts/check-boundaries 五门禁未由本审查者重跑——编排者在本树亲跑全绿（fmt/clippy/contracts/boundaries/workspace-test 均 exit=0，evidence-window 2026-10-03T09:49:16Z），本审查者据其报告登记、不冒充自跑；Windows/Linux 平台行为；本机未复现 R05-ENV-ALF-UNSIGNED-TEST-BINARY（我的六组定向复跑均为 loopback-only，不含 r00 LAN 段）。

## 亲跑证据（命令均真实执行，退出码亲见）

| 证据 | 命令（与台账一致） | 结果 |
| --- | --- | --- |
| `logs/reviewer-t06-adapters-operations.log` | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t06_operations` | 27 passed / 0 failed，exit 0（C01 27 项 + C02 4 项方言腿） |
| `logs/reviewer-t06-service-operations.log` | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t06_operations` | 7 passed / 0 failed，exit 0（C02/C03/C12 服务级） |
| `logs/reviewer-t06-service-worker-model.log` | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t06_worker_model` | 9 passed / 0 failed，exit 0（C04–C11 真实子进程链） |
| `logs/reviewer-t06-egress-lib.log` | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --lib egress` | 5 passed / 0 failed，exit 0（C11B/T05-C11B 单元腿） |
| `logs/reviewer-t06-workerrpc-units.log` | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib workerrpc` | 6 passed / 0 failed，exit 0（含 per-invocation 预算隔离/回收、0-token 与超长 prompt 前置拒绝） |
| `logs/reviewer-t06-tool-render.log` | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --lib tool_render` | 3 passed / 0 failed，exit 0（N-02：file:// 不进模型渲染） |
| `probe-egress/probe-output.txt` | `CARGO_TARGET_DIR=/tmp/r05t06-review-probe cargo run --quiet --offline`（probe-egress 独立 crate，path-dep 真实 lingxi-adapters + url 2.5.8） | 17 行电池：基准 3 行 REFUSE、zone-id 1 行 REFUSE、**13 行 BYPASS**（守卫 PASS 而 url 解析为 GUARDED 地址） |
| `logs/reviewer-manifest-check.log` | 实现者 working-tree-manifest 63 文件 sha256 全量复算 | 63 matched / 0 mismatched / 0 missing；另核实 egress.rs / tool_render.rs / operations/mod.rs **不在** manifest 内（F-02） |

## 审查者探针（自写、隔离于 REVIEW-T06/，不触碰产品代码）

**probe-egress（C11B 对抗探针，发现 F-01 的证据）**：独立 crate 通过 path 依赖编译**当前工作树的真实** `lingxi-adapters`，对生产 `EgressGuard::from_provider_endpoints(["https://api.example.test/v1"])` 逐 URL 调 `check_url`，再用 reqwest 所用的同一 WHATWG 解析器（url 2.5.8——与 `rust/Cargo.lock` 中版本逐字一致，已在探针 Cargo.lock 与工作树 lock 双向核对）解析同一 URL 的 host。结果（完整表见 probe-output.txt）：

```text
https://127.0.0.1:18080/asset.png   guard=REFUSE  url=Ipv4 127.0.0.1 (GUARDED)   ok     ← 守卫认识的形态
https://0x7f.0.0.1/asset.png        guard=PASS    url=Ipv4 127.0.0.1 (GUARDED)   BYPASS
https://0x7f000001/asset.png        guard=PASS    url=Ipv4 127.0.0.1 (GUARDED)   BYPASS
https://2130706433/asset.png        guard=PASS    url=Ipv4 127.0.0.1 (GUARDED)   BYPASS
https://0177.0.0.1/asset.png        guard=PASS    url=Ipv4 127.0.0.1 (GUARDED)   BYPASS
https://127.1/asset.png             guard=PASS    url=Ipv4 127.0.0.1 (GUARDED)   BYPASS
https://127.000.000.001/asset.png   guard=PASS    url=Ipv4 127.0.0.1 (GUARDED)   BYPASS
https://127.0.0.1./asset.png        guard=PASS    url=Ipv4 127.0.0.1 (GUARDED)   BYPASS
https://3232235885/asset.png        guard=PASS    url=Ipv4 192.168.1.109 (GUARDED) BYPASS
https://[::ffff:127.0.0.1]/…        guard=PASS    url=Ipv6 ::ffff:127.0.0.1 (映射→127.0.0.1 GUARDED) BYPASS
https://[::ffff:169.254.169.254]/latest/meta-data  guard=PASS  BYPASS
https://[::ffff:10.1.2.3]/…         guard=PASS    BYPASS
https://[::ffff:192.168.3.5]/…      guard=PASS    BYPASS
bypass rows: 13
```

根因（源码亲读）：`egress.rs:124-140` 的 `parse_ipv4` 只认严格十进制四段（前导零拒绝——比 WHATWG **严**，但反向缺口是"不认识"的形态全部落入主机名分支）；`egress.rs:217-242` 的 `ip_is_guarded` 16 字节臂没有 IPv4-mapped（`::ffff:0:0/96`）回落到 v4 判定；`egress.rs:311-318` 让"https 公网主机名"放行。reqwest 侧由 url crate 解析后拨号的是规范化地址（这一步为源码推断：reqwest 0.13.5 以 `url::Url` 为 host/port 来源，WHATWG 规范化发生在 parse 时；本审查者未搭建回环 TLS 服务做端到端拨号演示——守卫对非同源 URL 只放行 https，演示需受信任 TLS 端点）。

影响面如实界定：该下载**不携带任何凭证**、不跟随重定向、64 MiB 读中上限、Content-Type 必须 media/*——所以这是"无凭证 GET 打到回环/私网 https 端点 + media 字节可能被登记为产物"的 SSRF 面，不是凭证外泄面；plain-http 目标（如 169.254.169.254 元数据）被"plain http 仅同源"规则另行挡住（探针未单列，源码 egress.rs:313 亲读 + 单测 `https_public_passes_and_plain_http_is_same_origin_only` 亲跑）。触发前提是攻击者能左右某个 provider 返回的 `MediaProductRef::Url`——这恰是 C11B 要防的模型。

## 13 个 case 逐条核对

| C-ID | 结论 | 支撑（本审查者核实方式） |
| --- | --- | --- |
| C01 操作/能力矩阵 | PASS | 27 项 adapters 测试亲跑绿：embedding 五族 wire+解析（数量/顺序/维度/有限值核验断言亲读）、speech 三族、transcribe 四族、image 七族（含 codex SSE 聚合、dashscope submit/pending/query、gemini 远端 URL 拒绝）、video submit、rerank、tiers golden 尺寸与逐字拒绝、multipart/头组逐字节断言。矩阵"operation→入口→wire→结果"在测试中逐族成立 |
| C02 跨供应商媒体 | PASS（A11） | `c02_media_providers_in_one_task_never_cross` 亲跑绿：三把独立合成密钥各走各的 Authorization、text_a 材料在媒体线零出现、text 端点零命中、产物 bytes-out=bytes-in 落盘登记 |
| C03 大资源与产物真实性 | PASS（egress 腿受 F-01 约束） | 授权读/MIME 表/20/25 MiB 上限 refused-never-truncated、同源下载零 Authorization、幽灵 b64 产物拒登记、越权读 Forbidden——均亲跑绿；跨源回环拒绝腿用的是 dotted-decimal + plain-http 形态（被 plain-http 规则挡住，真实有效），但该守卫对非十进制拼写的行为见 F-01 |
| C04 worker 真实调用宿主模型 | PASS（A12） | `c04_…` 亲跑绿：r04_t07_fixture 真实子进程 → workerrpc → BoundedWorkerModel → GatewayWorkerModel → AuxiliaryExecutor → openai-completions wire；stub 命中恰 1、模型名=max_tokens=stream 断言、`aux-summarize-{invocation}-{cb_id}` 父子关联由 RecordingTrace + worker 回传的 request_id 双向钉死 |
| C05 同步端口升级不阻塞 | PASS | `current_thread` 单线程 + 400ms 延迟回调 + 心跳任务 ≥5 跳，亲跑绿；`WorkerModelPort::complete` 为 boxed-future（workerrpc.rs:225 亲读），执行器无 block_on |
| C06 嵌套配额不死锁 | PASS | 正向腿：全局并发=1 下主 run → worker 工具 → 回调完整链 run=completed、stub 恰 1 次（亲跑绿）；结构前提 `runs.rs:1299` `drop(model_permit)` 先于工具循环（亲读）；负向腿：外部持有唯一 permit → 400ms 有界 `model_budget_exceeded`、零外发（亲跑绿） |
| C07 预算按真实调用身份 | PASS | 预算键=宿主铸造 invocation id（`begin_invocation` 于 execute() 铸 id 后、RAII guard 全路径回收，workerrpc.rs:969-973 亲读）；单测 per-invocation 隔离/回收/回收后 id 复用零残留亲跑绿；重放 cb_id 由回执缓存应答、provider 恰 1 次（服务级测试亲跑绿） |
| C08 预算不能由载荷扩大 | PASS | 0 token、>4096 token、>64KiB prompt 均在 inner 端口前拒绝且零外发（单测 + 服务级 999999-token 腿亲跑绿）；deadline 由宿主铸造、线格式不接受该字段（`WorkerCallbackLine` 无此字段，workerrpc.rs:464-474 亲读） |
| C09 载荷不是权限 | PASS | 身份主张字段（provider/model/endpoint/auth/api_key/apiKey/run_id/parent_run_id/credential(s)）→ `worker_identity_not_negotiable`；白名单外 purpose → `model_purpose_not_granted`；两腿零外发亲跑绿。旁证：未列字段被 serde 丢弃、不具权限效果（亲读）；GatewayWorkerModel 对不可映射 purpose 二次拒绝（workermodel.rs:130-134，纵深防御） |
| C10 回调等待仍可取消 | PASS | HangingModel 永不返回 → deadline 到 → 协议 cancel + 进程组 kill + Unknown（"the model callback did not settle within …ms … side effects unconfirmed"）、<20s 有界（亲跑绿） |
| C11 worker 不继承秘密 | PASS | 真实链上 API key 只出现在对 stub 的 Authorization（正面证明宿主认证了），worker 可见字节零出现（亲跑绿）；env 面源码亲读：`SAFE_WORKER_ENV_PASSTHROUGH` 白名单 + `env_clear()`（workerrpc.rs:673-680） |
| C11B egress 守卫 | **CONDITIONAL（F-01 必修）** | 5 项单测亲跑绿且断言真实（dotted-decimal/同源例外/userinfo/scheme/http-同源/严格 IPv4/默认端口隐去）；但策略登记文本（§29.11/§30.1"字面 IP 的私网/回环/link-local/unspecified 一律拒绝"）与实现对"字面 IP"的认定面不一致——13 行对抗 BYPASS（见探针）。已交付测试全部真实，缺口在未测拼写 |
| C12 辅助/媒体生命周期诚实 | PASS（A12） | video: submit=job 回执（remote_cancel:"unsupported"）≠完成、pending=Generating、本地取消后查询 CancelledLocally、线上恰 2 请求、伪造 id 拒绝；dashscope Pending→Completed 仅在下载+登记后、下载零凭证、产物根恰 1 文件；in-flight drop 零登记零续跑；空文本 trim 语义拒绝零请求——4+3 项亲跑绿 |

台账抽查：13/13 条 `testNames` 逐条 grep 核实真实存在且被本审查者复跑；A-ID 映射核对（A11→C02；A12→C04–C12）精确无摊派；`executedCount`/`exitCode=0` 与复跑一致；mock 边界声明（loopback raw-TCP stub、NOT_REAL_API）与测试源码一致。未发现伪造证据或 PASS 空心化。

## 反向审查声明（先疑后验）

- 对 C11B 我没有接受实现者"5/5 绿 + DNS rebinding 已声明缺口"的自评：DNS rebinding 需要 DNS 解析，而我的 BYPASS 行是**纯字面量拼写**、离线可判定，不属于该已声明缺口——这是本审查与实现者登记的关键分歧点（F-01）。
- 对 C06 正向腿，先核对了结构前提是否真实（permit 是否真的在工具前释放），再接受测试绿。
- 对 C07，先找"按 run/worker 字符串键"的旧形态是否残留（R04 代码 `format!("{}/{}", ctx.run_id, worker)` 已在 diff 中删除并被 invocation id 取代），再接受隔离断言。
- 对 C09，先验证 FORBIDDEN 键表大小写变体（"Provider"）不会形成漏授权——大小写变体被 serde 丢弃、无权限效果，只有清单内精确键触发响亮拒绝（防御纵深，非授权面）。
- 对 C12 的"job accepted ≠ 完成"，反向找了把 jobId 当产物登记的路径（`register_product` 空字节即拒、`settle_poll` 仅在下载+登记后返回 Completed），未发现压平。

## 发现清单

### F-01（必修，high）：egress 守卫对非十进制 IPv4 拼写与 IPv4-mapped IPv6 放行，违反 §29.11/§30.1 登记策略

- 位置：`rust/crates/lingxi-adapters/src/models/egress.rs:124-140`（parse_ipv4 仅严格十进制四段）、`egress.rs:230-239`（ip_is_guarded 16 字节臂无 ::ffff:0:0/96 回落）、`egress.rs:311-318`（https 公网主机名放行分支）。
- 问题：URL 的 host 含十六进制/八进制/短式/单整数/前导零十进制/尾点 IPv4 拼写或 bracketed IPv4-mapped IPv6 时，守卫视其为普通主机名并按"https 公网主机名"放行；reqwest 使用的 url 2.5.8（与工作树 lock 同版）将其规范化为回环/私网地址后拨号。探针 17 行中 13 行 BYPASS（含 `https://0x7f.0.0.1/`、`https://2130706433/`、`https://127.1/`、`https://[::ffff:169.254.169.254]/latest/meta-data`）。
- 实际后果：能左右 provider 产物 URL 的攻击者可让服务对回环/私网 https 端点发起无凭证 GET（不跟随重定向、64MiB 上限、Content-Type 须 media/*）；这不属于已登记的 DNS-rebinding 离线不可判定缺口。C11B/C03 的已交付测试腿本身真实（dotted-decimal 与 plain-http 形态确实被拒），缺口是策略认定面窄于 URL 规范。
- 最小证据：`artifacts/rust-tauri/R05/REVIEW-T06/probe-egress/probe-output.txt`（对生产守卫亲跑）。
- 修复建议（同根因一次修全）：`check_url`/`parse_ipv4` 按 WHATWG host 解析语义识别全部字面 IP 形态（hex/octal/short/integer/前导零/尾点——即改用 url crate 解析 host 后按 `Host::Ipv4/Ipv6` 判定，或对守卫自有解析器补齐这些形态并显式拒绝"看着像数字主机"的形态）；`ip_is_guarded` 补 `::ffff:0:0/96` 映射回落。回归矩阵：上表 13 行 BYPASS 全部转 REFUSE + 既有 5 单测与 C03 服务级腿保持绿 + 同源例外（含默认端口隐去语义）不回归。
- 关联：C11B、T05-C11B（同守卫同策略）、R05-A11 的"B 承担请求且 A 凭证不泄露"腿（凭证面不受影响，取数面受影响）。

### F-02（建议，low）：working-tree-manifest 未覆盖 untracked 的 adapters `models/` 交付文件

- 位置：`artifacts/rust-tauri/R05/T06/working-tree-manifest.txt`（63 条）。
- 问题：台账 `workingTreeDigest` 指向该 manifest 作为 T06 终树冻结证据，但它不含 `rust/crates/lingxi-adapters/src/models/egress.rs`、`tool_render.rs`、`operations/*`（T06 交付清单列名文件）；63 条内全部匹配（0 mismatch）只是被列文件的匹配。终树可复验性打折。
- 修复建议：最终候选时把 untracked 新增文件一并入 manifest（或以 `git add -N` + 全量 shasum 重新生成），并在台账注明生成方式。

### F-03（建议，low）：system_speech 的 voice 字段未做 argv 防御

- 位置：`rust/crates/lingxi-service/src/operations.rs:821-831`。
- 问题：`SpeechRequest.voice` 原样进入 `/usr/bin/say` argv；以 `-` 开头的 voice 会被 say 当旗标解析。当前无不可信输入路径到达该入口（OperationService 尚无 HTTP/工具面，voice 为宿主侧构造），注释也自称 host-validated——属防御纵深登记，非现行漏洞。
- 修复建议：在该入口对 voice 做非 `-` 前缀校验或显式登记"voice 为宿主受控字段"；若后续 R07 把操作面暴露给模型/用户输入，此校验必须先行。

## 登记

- **registeredBlockers**：LIVE 供应商验证 `BLOCKED_NOT_AUTHORIZED`（RR-BLK-CREDENTIALS，最迟 R10）——任务书允许的延期，不计 FAIL；平台延期（Windows/Linux）维持任务书 §9 状态。
- 环境项 R05-ENV-ALF-UNSIGNED-TEST-BINARY：本次六组定向复跑未触发（loopback-only）；按台账登记处理，未据此改动任何门禁或测试。
- 五门禁（fmt/clippy/workspace-test/check-contracts/check-boundaries）：编排者在本树亲跑全 exit 0（`T06/evidence-window.txt` + 五份 gates 日志 102 suites ok / 0 failed）；本审查者未重跑、如实记为编排者证据。
- 修复闭环预期：F-01 修复后需对最终候选重新复验本报告的探针电池（BYPASS→REFUSE）与 C11B/C03 相关套件；届时本审查报告的状态由 CONDITIONAL_GO 转判。
