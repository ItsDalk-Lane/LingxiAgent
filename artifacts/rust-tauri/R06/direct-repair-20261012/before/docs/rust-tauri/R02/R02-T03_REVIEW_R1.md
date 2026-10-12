# R02-T03 独立对抗性验收报告 R1

- 验收代理：REVIEWER-R02-T03-R1（第 1 轮；全新独立代理，未参与 R02-T03 任何执行；只读审查 + 独立复跑 + 自造对抗测试；唯一写入为本报告；复跑证据全部重定向 `/tmp/r02t03-review/`，未覆盖执行者存档 `artifacts/rust-tauri/R02/T03/`；未修改任何产品源码/测试/配置/脚本；未 commit/push）
- 日期：2026-09-26｜分支 `codex/rust-tauri-migration`｜TASK_BASE_SHA = `0bdacbd86f87a4b477757f7f799ee8f203a5a4a5`（实测 = 当前 HEAD）｜候选 = 该 SHA + 当前未提交工作树
- 环境：macOS 27.0 arm64（Darwin 27.0.0）；rustup 锁定 **1.98.1**（经 `~/.cargo/bin` rustup 代理调用；Homebrew rust 未使用）；本验收专属全新 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t03-review`（不与执行者 `/tmp/rust-target-r02-t03` 共享）；cargo 一律 `--locked` + `CARGO_NET_OFFLINE=true`；全部网络敏感命令 `env -u` 剥离六个代理变量
- 验收对象：R02-T03「实现 HTTP/WS 认证与资源范围」（执行者报告 `docs/rust-tauri/R02/R02-T03_REPORT.md` 全部声明）；acceptance **R02-A05 / R02-A06（均 REQUIRED）**；交付物三件：HTTP/WS 接入层、认证授权服务、端点权限表
- **最终判定：VERDICT: PASS**（依据见 §13；两条 REQUIRED acceptance 由本代理亲自重跑 + 57 个自造对抗用例复核通过；findings F01–F05 均 MINOR，无 BLOCKING）

---

## 1. 候选清单与工作区核实【实际运行】

`git status --short` 实测恰为执行者报告 §10 声称的集合，无多无少：已跟踪修改 7 个（rust/Cargo.lock、lingxi-service Cargo.toml/config.rs/lib.rs/main.rs/tests/instance_lifecycle.rs/tests/service_health.rs）；未跟踪新增 9 组（auth/limits/sessions/transport/ws 五个源文件、tests/auth_matrix.rs、两个脚本、报告与 artifacts/rust-tauri/R02/T03/）。验收全部复跑完成后再次核对：工作树与本验收开工时逐项一致——**本验收零残留**（review 服务进程已停、/tmp 合成 home 已删、`ps` 复查零 review 进程）。

关键"零改动"证明（`git diff 0bdacbd86 -- <path>` 实测）：

| 路径 | diff | 结论 |
|---|---|---|
| `package.json`、`package-lock.json`、`desktop/`、`server/`、`core/`、`lib/`、`shared/`、`cli`、`hub`、`plugins`、`skills2set`、`tests/`、`contracts/`、`.sync-audit/`、`PROGRESS.md`、`ORCHESTRATOR_PROGRESS.json`、任务书 | **0 行** | Node/Electron 生产入口与现役认证契约源码（core/server-auth.ts、device-registry.ts、ws-auth-ticket.ts、security-principal.ts、route-security.ts、transport-context.ts、cors-policy.ts）零触碰——执行者"镜像其语义"是在只读参照下完成的，属实 |
| `rust/crates/lingxi-kernel`、`lingxi-protocol`、`lingxi-spike`、`lingxi-browser-spike`、`rust/Cargo.toml`、`rust-toolchain.toml` | **0 行** | 协议冻结 crate 零改动；schema 生成 `--check` 零漂移（§7）与之一致 |
| `rust/crates/` 目录列表 | 5 个 crate | lingxi-adapters 未提前建（本任务不需要；R01_HANDOFF directory_conventions 遵守） |

**Cargo.lock 逐行核对**：diff 恰为 lingxi-service 依赖列表 **+5 行引用**（base64/hyper/hyper-util/sha1/sha2），`[[package]]` 计数前后一致（**零新包**）；五个版本（base64 0.23.1 / hyper 1.11.1 / hyper-util 0.1.21 / sha1 0.11.0 / sha2 0.11.0）全部在 base SHA 的锁内既有（sha1/base64 原为 lingxi-protocol 依赖、hyper/hyper-util 为 axum 传递依赖、sha2 已在锁内）；`grep -c tungstenite rust/Cargo.lock` = 0——"不启用 axum ws 特性以免引入未锁 tungstenite"的选型声明属实。

## 2. 任务完整性（Steps 1–4 逐条）【源码确证 + 实际运行】

| 步骤 | 要求 | 独立核实 | 结论 |
|---|---|---|---|
| 1 | 默认 loopback，LAN 仅显式配置；不因 127.0.0.1 免认证 | `NetworkMode::Loopback` 默认；`--network-mode` 严格词汇（LAN/大写均拒，单测+我方二进制复测 exit 2 且文案指名 opt-in）；默认模式 + 非 loopback `--bind` = `NetworkModeBindMismatch` exit 2（auth_matrix.rs 集成 + 我方矩阵 a06-non-loopback-bind 复现）；loopback 上一切业务路由（health 除外）照常过认证——health 是唯一 public 面，其余全要求凭证 | 达成 |
| 2 | 接入现役 token/设备密钥/账号契约；身份只在可信边界创建；业务请求不能自行覆盖 principal | 语义镜像经我方逐文件对照现役源码核证：bearer 优先/query 仅 local/loopback token 仅 local 连接（`loopback_token_requires_local_transport`）/设备凭证前缀+盐化哈希+过期/撤销/连接策略/成功才写 lastUsedAt——与 `core/server-auth.ts`+`core/device-registry.ts` 一致；分歧（scrypt→迭代盐化 SHA-256 4096 轮、unix-ms、web-session 不接入、::1 Origin 增补、Host 畸形更严）**全部有记录非静默**（auth.rs 模块文档 + 报告 §4.2/§4.3/§9.3）。身份只在 `AuthService::authenticate` 创建；`Principal` 仅经中间件 Extension 注入；请求体身份字段 `deny_unknown_fields` 硬拒（本代理 WS 帧级复测：`principalId` 走私 → `invalid_message` 帧 + close） | 达成 |
| 3 | HTTP/WS 共享授权服务；Origin/Host/票据生命周期；请求体/速率/连接数限制 | 单一 `classify_route`+`authorize` 表两路共用（auth_guard 对 `/ws` 与其余路径同一张表）；transport_guard（Origin 白名单→Host/网络模式→限速）先于认证、覆盖含 health 的全部路径；票据 TTL 30s/上限 512/单次消费/绑定 (principal, connectionKind, path)；限制三项（429/413/503）**本代理全部在真实二进制上亲自触发**（§5.2/§6） | 达成 |
| 4 | 每请求绑定 agent/session/resource scope；health 最少信息 | session 归 `owner_user_id`，本地 owner 全可见/设备仅同 userId（HTTP 与 WS session_read 同一 `SessionStore::can_access`）；execute 把 principal_id/credential_kind 写进 run 记录（内存最小面，无提前 SQLite——全 grep 仅 sessions.rs 文档注释一处提及 T04）；health 恰 6 字段（本代理 JSON 键集断言：status/serverKind/serverVersion/wireProtocolMin/wireProtocolMax/dataEpoch，无路径/实例/配置/凭证） | 达成 |

交付物三件真实：HTTP/WS 接入层（lib.rs 中间件链 + ws.rs 710 行）、认证授权服务（auth.rs 1357 行）、端点权限表（`classify_route` 单一权威 + auth.rs 模块文档表 + `route_policy_table` 单测逐条钉死）。无占位。

## 3. 真实接线追踪（入口→守卫→认证→处理器→WS 升级）【源码 + 真实进程】

```text
main.rs: parse_cli（+--network-mode 严格解析）→ from_sources（非loopback bind+默认模式=exit 2）
  → prepare_layout → acquire（OS 文件锁，在 bind 前——T02 契约保持）
  → ServiceState::bootstrap（锁后 bind 前；失败 exit 2：本代理以损坏 devices.json 实测
    exit 2 + "auth bootstrap failed: … not valid JSON"，不存在无认证 serve 路径——
    run() 收 ServiceState、路由只由 build_router(state) 构建）
  → run: bind → publish instance.json → READY 行（字段未变，T01 harness 兼容）
  → transport_guard（最外层：Origin 白名单（含 health）→ infer_connection_kind（Host×网络
    模式×物理 remote）→ 每 peer 固定窗口限速 → 注入 ConnectionKind/SocketAddr）
  → auth_guard（classify=public 直通；/ws: 票据单次消费或 bearer/query 认证；其余:
    authenticate(bearer|query[仅local]) → authorize 同一张表 → 注入 Principal）
  → 处理器（Extension<Principal>——身份只能来自这里）/ GET /ws: WsUpgrade 提取器 →
    连接上限（101 前）→ 101（Sec-WebSocket-Accept=sha1+base64）→ OnUpgrade+TokioIo →
    run_ws_session（ClientHello→negotiate_protocol→ServerHello；session_read 同一所有权规则）
```

axum layer 次序核证：`.layer(auth_guard)` 先加、`.layer(transport_guard)` 后加 ⇒ transport 在外层先执行，符合"传输先于认证"设计。本代理在真实二进制上沿该链逐环打点（伪造身份/跨主体/Origin/Host/票据/限制共 57 例，§5–§6），行为与源码一致。**无 mock 掉核心**：集成测试（auth_matrix.rs 13 个）走真实 axum+loopback TCP+真实 WS 升级；证据脚本走真实二进制进程。

## 4. 现役契约镜像与分歧记录核对【逐文件源码对照】

| 面 | 现役（我方独立读取） | Rust 实现 | 判定 |
|---|---|---|---|
| 凭证优先级 | `parseCredential`：bearer → query(仅 local) | 同（`authenticate`） | 一致 |
| loopback token | `randomBytes(16).hex`、写 0600 文件、仅 local、owner 全放行 | hex_random(16)、原子写+chmod 0600（实测 mode 600）、instanceId 绑定、每启动轮换（**本代理重启实测旧 token 401/新 token 200**） | 一致 |
| 设备凭证 | `hana_dev_`+b64url(32B)、prefix 18、scryptSync、状态/作用域/过期、成功写 lastUsedAt | 同形；scrypt→迭代盐化 SHA-256（4096 轮） | 分歧**已记录**（auth.rs 文档+报告） |
| principal | normalizePrincipal 词表+派生 id；local_owner 三元组放行 | 同（is_local_owner 三元组） | 一致 |
| WS 票据 | `hana_ws_`+b64url(32B)、TTL 30s、上限 512、消费即删、绑 (principal,kind,path) | 同（`WsTicketService`；prune 顺序亦镜像 JS Map） | 一致 |
| Origin | 正则 localhost/127.0.0.1（无 ::1）+file://+null | 增补 ::1 | 分歧**已记录** |
| Host | loopback 模式须 loopback host；`\d{1,3}` | 严格 u8 点分四段（`127.0.0.256` 拒） | 分歧**已记录**（更严） |
| web-session | `hana_session` cookie 轴 | 不接入 | 分歧**已记录**（§4.3，非静默） |

另核实：撤销语义（in-process）单测真实（`device_credential_lifecycle_create_auth_expire_revoke`）；但运行中服务对跨进程撤销不可见——见 **F02**（报告未将其记录为分歧，且 A05 表格"集成层"覆盖声明不准确）。

## 5. R02-A05｜伪造身份无效（REQUIRED）——独立复跑 + 对抗变体【实际运行】

### 5.1 执行者脚本复跑

`bash scripts/rust-tauri/r02_t03_auth_matrix.sh /tmp/r02t03-review/matrix` → **exit 0**，**60 PASS / 0 FAIL**（A05 段 33 PASS：无凭证四端点 401、伪造 principal 头不认证、/me 回显服务器计算身份且零 forged 字段、身份形 body 400 invalid_message、伪 sessionId 404、过期凭证 401、跨主体 403/WS 4403、认证失败注册表逐字节不变、授权失败仅审计字段（我核对 state-*.txt 哈希一致 + audit-strip 语义 diff PASS）、runCounts 全程 0→0、**正控制 owner execute 200 + runCount 恰 +1**——防"全盘拒绝冒充安全"）。执行者存档（16:00 时间戳，未被本验收覆盖）与报告逐项吻合。

### 5.2 本代理自造 HTTP 对抗变体（45 例，真实二进制）

全绿 42 例 + 3 例分析如下（我方期望修正，非服务缺陷 2 例、真实不一致 1 例→F04）：

- **Bearer 格式变体**：`bEaReR`（混合大小写 scheme）/tab 分隔/双空格/尾随空格均 200（与现役 `/^Bearer\s+/i` 语义一致）；`Bearer<token>` 无空格/`Basic <有效token>`/裸 `Bearer` 均 401 `missing_credential`（镜像现役 parseCredential 返回 null → missing）。
- **token 混淆**：后缀追加/前缀截断/大写化/换行尾（hyper 层 400 拒绝畸形头——请求走私防护正确，我方期望 401 属误设）全部拒绝；`hana_ws_` 票据当 HTTP bearer 或 query token 用 → 401 `invalid_credential`（凭证族不串用）。
- **未授权签发面**：设备凭证持有者调 `POST /lingxi/v1/devices/credentials` → 403 `local_owner_required`（设备不能铸凭证）。
- **fail-closed**：`/lingxi/v1/secret` 匿名 401 / owner 404（不泄露存在性）/ 设备凭证 403；`/api/sessions`（旧面）owner 404。
- **health 最小面**：JSON 键集恰 6 字段。
- Host/Origin 变体见 §6。

## 6. R02-A06｜恶意网页无法借 loopback 越权（REQUIRED）——独立复跑 + 对抗变体【实际运行】

### 6.1 执行者脚本 + WS 探针复跑

同命令（A06 段 27 PASS）：evil Origin 打 health（公开面）与带有效 token 的 /me 均 403 `bad_origin`（传输先于认证）；Host 篡改（evil.example/rebinder + 有效 token 变体）403 `loopback_host_mismatch`；过期票据（真实 30s TTL 等待）401 `invalid_ws_ticket`；票据重放 401；合法 Origin（http://localhost:port）+ 票据 101 → ServerHello(selectedProtocol=1) → session_read；无 Origin CLI 形态（bearer、?token=）HTTP 200 / WS 101；`ws_probe.py` 对每个 101 **独立重算 RFC 6455 Accept**（sha1(key+GUID)+base64）并断言相等——帧编码真实性由官方 §1.3 向量单测 + 探针双证。

### 6.2 本代理自造对抗变体

**Host 矩阵（12 例）**：`LOCALHOST:port` 200（大小写不敏感）；`localhost.`（尾点）403；`127.0.0.01` **200**（前导零八位组在 u8 域内——与现役 `\d{1,3}` 同为放行，且 Host 判定不单独授权：物理 remote 仍须 loopback，无越权路径，见 N1）；`[::1]:port` 200；`127.000.000.001` 200（同前导零类）；`0177.0.0.1`（八进制形）/`2130706433`（十进制形）/`localhost:p:p`/`[::ffff:127.0.0.1]`/`127.0.0.1.evil.com`/`evil.example`（含携带有效 token 变体）全部 403 `loopback_host_mismatch`——DNS rebinding 家族与畸形编码 fail-closed。

**Origin 矩阵（11 例）**：`HTTP://localhost:port`（大写 scheme）403（与现役正则同大小写敏感，fail-closed）；`https://localhost`（无端口）200；带路径 200（正确剥离）；`localhost.:port`（尾点）/`[::ffff:127.0.0.1]`/`http://user@localhost`/`http://evil.com@localhost`（userinfo 双向）/`file://localhost`/`Null`（大写）/空串全部 403 `bad_origin`——无 userinfo 混淆或大小写绕过。

**WS 矩阵（12 例，真实升级）**：
- **同票据并发双升级**：两线程同时用同一 ticket 升级 → 结果恰 `['101','401']`——单次消费在 Mutex 内原子，恰一 101。
- 票据跨路径（`/lingxi/v1/other?wsTicket=`）→ 401（路径绑定）；票据当 HTTP query token → 401。
- 设备凭证 WS 读 owner 会话 → `cross_principal_access` ProtocolError 帧 + **close 4403**（帧序：错误帧→close，与文档一致）。
- WS 帧内走私 `principalId` → `invalid_message`（deny_unknown_fields）+ close——wire 词汇同样不接受客户端身份。
- ping→pong 回显；声明 8MiB 帧 → 连接被丢弃（不缓冲，配 `oversized_frame_is_rejected` 单测）。
- 升级形状负向（缺 Sec-WebSocket-Version / Upgrade: h2c / 缺 Key）→ 403 + 机读 reason（拒绝成立；状态族不一致见 F03）。
- **Accept key**：每个 101 独立重算校验通过。

**限制真实生效（本代理亲触）**：限速轰到 **429**（矩阵复跑内）；>1MiB 体 **413**；持有 16 条已升级 WS 连接后第 17/18 次 **503**（`Counter({101:16, 503:2})`——上限恰 16、RAII 释放后可复用，单测+我方二进制双证）。

**独立判定：R02-A06 PASS**（正常桌面/CLI 形态全链不受损：§6.1 的 101+ServerHello+session_read 与 CLI bearer/query 200）。

## 7. 回归矩阵（全部本代理亲自重跑，真实退出码）

| 命令（rustup 1.98.1 + /tmp/rust-target-r02-t03-review + --locked + 代理剥离） | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | 无 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 0 告警 |
| `cargo test --workspace --locked` | 0 | **135 passed / 0 failed**（service lib 70 + auth_matrix 13 + instance_lifecycle 3 + service_health 4 + kernel 7 + protocol 19 + spike 7+1 + browser-spike 11），与报告数字一致 |
| `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py --self-test` | 0 | RESULT: OK（负向电池全拒） |
| 同上（当前树实跑） | 0 | RESULT: OK（含 D5-reverse：5 个 workspace 成员全部已登记） |
| `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | 56 生成文件 + 624 API 项零漂移（**协议 crate/生成物零改动**） |
| `bash scripts/rust-tauri/r02_t03_auth_matrix.sh /tmp/…` | 0 | §5.1/§6.1，60 PASS |
| `bash scripts/rust-tauri/r02_t01_service_smoke.sh /tmp/…` | 0 | **T01 回归**：READY 行/health/SIGTERM exit 0 全绿（脚本不含未知路由断言——404→401 收窄对其无影响，实测确认） |
| `bash scripts/rust-tauri/r02_t01_boundary_negative.sh /tmp/…` | 0 | T01 边界回归（三注入三拦截三复绿+零残留） |
| `bash scripts/rust-tauri/r02_t02_dual_instance.sh /tmp/…` | 0 | T02 A03 回归（锁次序保持：锁→auth bootstrap→bind） |
| `bash scripts/rust-tauri/r02_t02_path_priority.sh /tmp/…` | 0 | T02 A04 回归 |
| `npx vitest run tests/post-verification-audit-seal.test.ts` | **1** | 1 failed / 2 passed——预存在，归属核验见 §8 |

测试有效性：单测 38+（纯逻辑/真实文件系统）、集成 13+3+4（真实 TCP/WS）、脚本（真实二进制）；生产代码 unwrap/expect 仅 `switching_protocols_response` 一处 panic 注释论证的静态响应构建失败分支（fail-loud 方向，可接受）；无永真断言、无 skipped 当通过。

## 8. 审计封印红归属核验【独立验证——比 stash 更强的证明】

- 实测（当前工作树）：exit 1，`Tests 1 failed | 2 passed (3)`，失败项为 allowlist 断言。
- **数据源核验**：`tests/post-verification-audit-seal.test.ts` L80 唯一的 diff 来源是 `execFileSync("git", ["diff", "--name-only", sha..HEAD])`——**纯提交图函数，不读工作树**（全文件无 status/worktree 读取）。因此该测试在 HEAD 提交态与工作树态结果恒等；执行者的"stash 后基线同红"声明由源码级证明取代成立（无需真实 stash 冒险）。
- **失败清单归属**：`git diff --name-only ab4f2281..HEAD` = 39 文件，逐项核对全部为 R02-T01/T02 已提交交付与总控账本（T01/T02 源码脚本证据、ORCHESTRATOR_PROGRESS.json、DEPENDENCY_RULES.json、.gitignore、.sync-audit 三件等）——**零个 T03 文件**（T03 改动全部未提交，对该测试不可见；HEAD = TASK_BASE_SHA）。
- 结论：该红（i）在 TASK_BASE_SHA 提交态即存在；（ii）与本任务未提交改动无因果；（iii）与 R01 期间登记的"预期封印前红"同族（同一 allowlist 断言，坐标 ab4f2281 落后于已授权 R02 提交）；（iv）本任务未使其变绿/变红（tests/ 与 .sync-audit/ 0 diff）。按背景条目核验通过，**不算本 Task 新失败**；坐标推进属获准提交后的封印流程，本验收不代位、不提议虚报坐标或扩白名单。

## 9. 安全与数据面

- **token 文件**：0600 实测（binary 实例 stat = 600）；camelCase 5 字段（schemaVersion/kind/token/instanceId/createdAtUnixMs）无多余泄露；每启动轮换实测（旧 401/新 200）。
- **时序侧信道（数学审查）**：`constant_time_eq` 先长度门（等长路径）后定长累积 XOR，无数据依赖分支/早退——两处真实调用均为定长摘要（loopback token 双侧 SHA-256 各 32B；设备密 hex 64 字符），长度本身不泄密；hash-then-compare 的伪造等价于找 SHA-256 碰撞（可忽略）。设备凭证前缀快路径的候选收窄耗时差与现役 `secretPrefix` 设计同构（prefix 是公开元数据），非新增面。`hash_secret` 4096 轮顺序 SHA-256 仅对前缀命中的候选执行，且有 240/10s 限速兜底。
- **票据并发**：单次消费持 Mutex——并发双升级恰一 101（实测）。
- **无真实数据触碰**：全部证据脚本/集成测试只用 `/tmp` 合成 home 与合成凭证；我方复查验收后无生产目录触碰。
- **孤儿进程**：`ps` 独立确认两个先前会话遗留 lingxi-service（14:59 启动、/tmp/rust-target-r02-t02 二进制、/tmp 合成 home）——与本任务二进制（/tmp/rust-target-r02-t03）、端口（本验收随机端口）、数据根均无关；按"保留无关状态"未处置，如实登记属实。

## 10. 发现问题

**F01（MINOR）｜WS 帧编解码接受未掩码客户端帧：违反 RFC 6455 §5.1，且与自身注释矛盾**
- 位置：`rust/crates/lingxi-service/src/ws.rs` `read_ws_frame`（L363-370：注释称 "Clients MUST mask; an unmasked client frame is a protocol violation"，但 `mask` 为可选——未掩码帧被原样解码处理）。
- 证据：本代理真实二进制升级后发送**未掩码** session_read 文本帧 → 服务端正常回复 `sessionReadResult`（未关闭连接）。R01 原型（lingxi-proto-server.rs L164-190）同样可选掩码，故"镜像已审原型"字面为真，但两者都未满足 RFC 的 MUST-close。
- 为什么不是 BLOCKING：掩码防护的对象是中间代理缓存投毒；本服务 loopback 绑定、无中间人场景在 R02 威胁模型内；能剥掩码的在途攻击者同样能直接读写明文。
- 后果/根因：协议符合性缺口 + 注释-行为失配（与 T02-REVIEW F02 同类）；真实浏览器客户端总是掩码，互操作性不受影响。
- 修复要求：对客户端帧 `masked=false` 直接协议错误关闭（或修正注释并登记 R08 前收口）；同族检查 RSV 位与 continuation 帧当前也为拒绝/不支持（当前实现拒绝未知 opcode 含 0x0/0x2——子集语义成立）。需重跑：ws 单测 + auth_matrix WS 组 + ws_probe。

**F02（MINOR）｜运行中服务的设备注册表是启动时快照：跨进程撤销/过期变更对活服务不可见，报告未记录该分歧且 A05"集成层"撤销覆盖声明不准确**
- 位置：`rust/crates/lingxi-service/src/auth.rs`（`bootstrap` 一次性加载 `AuthInner`；`authenticate` 只读内存副本，无 reload 路径）。
- 证据：`tests/auth_matrix.rs` L636-667 的内联注释自证——通过第二进程 bootstrap 撤销后"the running server will NOT see it"，该集成测试随即放弃 wire 级撤销断言改测篡改秘密；二进制矩阵脚本亦无撤销用例。现役 `core/device-registry.ts` 每次 `authenticateDeviceCredential` 重新读盘（L56-73），撤销即时生效——这是真实语义分歧。
- 报告不一致：报告 §4.2 称"撤销经 `revoke_device_credential`（…R02 记录为 T08 前 CLI/库层操作）"，但库层/CLI 对**运行中**服务所在 home 的撤销实际无效（需重启）；A05 表格"撤销 token …（单测 + 集成层）"中"集成层"不成立（仅单测覆盖，且我方复核单测真实通过）。
- 为什么不是 BLOCKING：R02 无 HTTP 撤销面；凭证全为隔离根内合成物；运行时目录 0700 单 owner；进程内撤销语义真实（单测）。
- 后果：若照报告把"CLI/库层撤销"当可运行 Story，会出现"已撤销凭证仍有效直至重启"的安全错觉。
- 修复要求：报告补记该分歧；实现侧在接入真实凭证消费者（R08/R09）前改为按认证读盘或提供撤销失效通道。需重跑：auth 单测 + A05 撤销 wire 级新增断言。

**F03（MINOR）｜`WsUpgradeRejection.status`（400/405）为死字段：所有升级形状拒绝被折叠为 403 `invalid_transport`**
- 位置：`rust/crates/lingxi-service/src/lib.rs` `ws_handler`（`Err(rejection) => EndpointError::invalid_transport(rejection.reason)`——丢弃 `rejection.status`）。
- 证据：本代理实测缺 Sec-WebSocket-Version / Upgrade: h2c / 缺 Key 均 403（机读 reason 正确），而非提取器设计的 400/405。
- 为什么不是 BLOCKING：拒绝仍然发生且 fail-closed、reason 可诊断；仅状态码族与 RFC 6455 握手失败惯例（400）不一致 + 死字段代码味。
- 修复要求：透传 `rejection.status` 或删字段统一 403 文档化。需重跑：ws/auth_matrix 升级负向组。

**F04（MINOR）｜`classify_route` 剥全部尾斜杠而 axum 路由精确匹配：公开路径的尾斜杠形态产生未认证 404，破坏"未认证未知路径一律 401"的边界一致性**
- 位置：`rust/crates/lingxi-service/src/auth.rs` `classify_route`（`trim_end_matches('/')`）× `build_router` 精确路由。
- 证据：本代理实测未认证 `GET /lingxi/v1/health/` → **404**（public 策略放行后路由 404），而 `/lingxi/v1/nope` 未认证 → 401。业务路径不受影响（`/lingxi/v1/me/` 等 classify 非 public → 401）。
- 为什么不是 BLOCKING：404 出现在不存在的路径上，不泄露任何存在路由的信息；纯一致性问题。
- 修复要求：路由层与 classify 层用同一规范化（或 classify 不剥斜杠让未知带斜杠路径落入 local_only）。需重跑：service_health 未知路由组。

**F05（MINOR）｜报告/测试命名的小幅精度问题（合并报告一致性）**
- A05 表格撤销行"集成层"声明（属 F02 证据面）；`service_health.rs` 更名后的 `unknown_route_is_not_found_for_owner_but_closed_for_strangers` 只断言 stranger 半边（owner→404 由本代理实测成立但树内无该断言）；`instance.rs` 中 `principalAllowsConnection` 的 tunnel→custom_remote 轴未列入报告分歧清单（代码注释已声明 out-of-scope，R02 端点集确无该面）。
- 修复要求：措辞对齐；不阻塞。

**NON-ISSUE**：
- **N1**：`Host: 127.0.0.01`/`127.000.000.001`（前导零八位组）按 loopback 放行——与现役 `\d{1,3}` 正则行为一致，且 Host 判定不构成单独授权（loopback 模式下物理 remote 亦须 loopback，否则 `loopback_remote_mismatch`；我方 `0177`/十进制/多点等族全部实测拒绝）。报告"畸形更严"的表述精确范围是越界八位组（`127.0.0.256` 拒，单测+源码核证）；前导零类非更严非更松。无需动作。
- **N2**：本服务不回设 CORS 响应头（现役 `applyCorsResponseHeaders` 有 ACAO 面）。R02 无浏览器消费者；WS/桌面/CLI 形态已验；旧 CORS 响应面属 API_COMPAT_MATRIX 迁移项（R08/R09 接旧客户端时必须补）。建议登记交接，不算本任务缺口。
- **N3**：头值内嵌换行 → hyper 400（请求走私防线正确）。
- **N4**：OPTIONS 无路由 → 未认证 401（fail-closed）；浏览器预检会失败——与 N2 同属"R02 无浏览器消费者"边界。
- **N5**：`run_ws_session` 首帧非文本（如先 ping）→ close 4409，符合"首帧必须 ClientHello"。

## 11. Scope 与报告一致性

- 无无关重构、无提前 T04（grep rusqlite/sqlite 仅 sessions.rs 文档注释一处）、无提前 T05 事件流、无 lingxi-adapters/xtask 提前落盘；无隐藏删除（7 个修改文件均有任务映射；两个测试文件的改动是 run(state) 签名适配 + 未知路由契约按 T03 拥有的变更更新并注明——断言是收紧不是放松）。
- 报告声称 vs 实测：135/0、fmt/clippy 0、self-test OK、schema 零漂移、矩阵 60 PASS、60=33+27 分段、stderr 标记 20 行、Cargo.lock +5 引用零新包、5 依赖版本、孤儿进程、退出码 2（auth bootstrap 失败，我方实测）——逐项相符。**唯一不实项**：撤销的"集成层"覆盖声明（F02/F05）。两次契约收窄（未知路由 401、run 签名）均如实声明且回归绿。

## 12. 未验范围（如实声明）

1. 非 macOS 平台未编译验证（0600 chmod 的非 unix 退化分支、Windows 锁）——与执行者声明一致，属 R09/R10。
2. LAN 模式真实跨机流量未实测（无第二台机器）；LAN 分类/loopback-token-over-LAN 拒绝/设备凭证 LAN 语义为单测+集成注入层验证——执行者 §9.2 已登记。
3. release 构建、长时资源增长（R10/T08）；全量 npm test 未跑（Node 侧 0 diff + 0 引用面 + 封印单测单独复跑并归属；A16 属 T08）。
4. scrypt 原语一致性与 web-session cookie 轴的回补属后续阶段（分歧已记录）。
5. 本报告不改任何被验收文件；findings 均为验收意见，不构成代改。

## 13. 对 PASS 标准的逐条对照

1. **R02-A05/A06 真实 PASS**：本代理亲自重跑矩阵 exit 0 / 60 PASS，另加 57 个自造对抗用例（HTTP 45 + WS 12）——所有拒绝面保持、正控制成立、429/413/503 亲触、票据并发恰一 101——满足。
2. **生产路径真实接通**：§3 全链源码追证 + 真实二进制行为一致；无 mock 核心链、无占位、无未接线入口——满足。
3. **无 BLOCKING**：F01–F05 均 MINOR——满足。
4. **无测试篡改**：存量测试仅签名/字面量适配与契约收紧（方向为更严），检查器/门禁/DEPENDENCY_RULES 零改动——满足。
5. **无未解释安全缺口**：认证/授权/Origin/Host/票据/限制逐面复核；发现的宽松点（未掩码帧、撤销快照、状态码折叠、尾斜杠）全部开列 findings 并附修复归属——满足。
6. **回归绿**：§7 十二项；封印红按 §8 以强于 stash 的源码级证明归属为预存在——满足。
7. **证据与候选一致**：工作树=声明集合、存档↔报告↔复跑三方相符（含唯一不实项已开列 F02/F05 并定级）——满足。

## 14. 判定

**VERDICT: PASS**

R02-T03 的三件交付（HTTP/WS 接入层、认证授权服务、端点权限表）真实、接线完整、语义镜像现役契约且分歧有记录；两条 REQUIRED acceptance（R02-A05、R02-A06）经本代理独立复跑及 57 个自造对抗变体确认 PASS。五条 MINOR findings（未掩码客户端帧、运行中撤销快照语义及其报告措辞、升级拒绝状态码折叠、尾斜杠边界一致性、报告精度）不构成阻塞，均有明确修复要求与归属（F01/F02 需在 R08/R09 真实消费者接入前收口）。本判定不代位 R02 阶段验收，也不授予 commit/push/发布/封印坐标推进权限。

（本报告由 REVIEWER-R02-T03-R1 于 2026-09-26 生成；报告文件自身 SHA-256 见验收答复，不写入本文件。）
