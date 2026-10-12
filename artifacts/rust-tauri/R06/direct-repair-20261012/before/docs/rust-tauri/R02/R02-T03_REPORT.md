# R02-T03｜实现 HTTP/WS 认证与资源范围 — 执行报告

- 执行者：ZCode:R02-T03（EXECUTOR-R02-T03，一次性执行代理；不负责独立验收，不提交/推送）
- 状态：**READY_FOR_REVIEW**（PASS/FAIL 判定归总控另派的独立验收）
- 日期：2026-09-26
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R02_Rust独立服务、存储与事件基础.md` §4 R02-T03
  （场景 R02-A05「伪造身份无效」、R02-A06「恶意网页无法借 loopback 越权」，均 REQUIRED）

## 1. 范围

任务书四步：①默认 loopback、LAN 仅显式配置启用、不因 127.0.0.1 免认证；②接入现役本地
token/设备密钥/账号契约、身份只在可信边界创建、业务请求不能自行覆盖 principal；③HTTP 与
WS 共享授权服务，校验 Origin/Host 与票据生命周期，设置请求体大小、速率和连接数量限制；
④为每次请求绑定 agent/session/resource scope，公开 health 返回最少信息。
交付：HTTP/WS 接入层；认证授权服务；端点权限表。

设计边界（任务书）：不连真实供应商、不执行真实高权限工具、不切现有用户目录、不引入新
UI。业务端点为代表性的「读会话」（GET /lingxi/v1/sessions/{id}，含 WS session_read）与
「执行」（POST /lingxi/v1/sessions/{id}/execute）最小实现，但真实过授权链、真实拒绝/
放行、真实可观测副作用（run 记录）；存储/事件内部留给 T04/T05 替换，端点形状（ID/
所有权/run 记录）为其预留。token 全部为隔离数据根内合成 token，创建/读取/过期/撤销
语义真实；不写真实用户目录、不外发。

不在本任务内（后续 T）：SQLite 存储与事务（T04）、事件流与断线续读（T05）、关闭协调器
（T06）、日志脱敏深化（T07）、xtask 门禁汇总（T08）、web-session cookie 轴（现役
`core/web-session-store.ts`；R02 无 web 登录流，bearer/query/设备凭证已覆盖本阶段面，
见 §4.3）。现役 Node/Electron 生产入口零改动。

## 2. 源码基线与环境

- 开工实测：分支 `codex/rust-tauri-migration`，HEAD = `0bdacbd86f87a4b477757f7f799ee8f203a5a4a5`
  （= TASK_BASE_SHA = 远端 HEAD，R02-T01/T02 已完成推送），`git status --short` 为空（干净）。
- tested SHA：`0bdacbd86f87a4b477757f7f799ee8f203a5a4a5` + 本任务未提交改动（§10 全集）。
- 平台：macOS 27.0 arm64（Darwin 27.0.0）。
- 工具链：rustup 锁定 1.98.1（`rust-toolchain.toml`；`rustc 1.98.1 (48a229cea 2026-09-01)`
  实测，经 `~/.cargo/bin` 代理；本机 Homebrew rust 1.93.0 不读取 toolchain 文件，未用于
  任何构建）。全部 cargo `CARGO_NET_OFFLINE=true` + `--locked` + 专属
  `CARGO_TARGET_DIR=/tmp/rust-target-r02-t03`（RR-T08-F1）。
- 网络：失效代理（127.0.0.1:7890）已从全部命令剥离。
- 测试隔离：全部服务/测试只用合成 home（mktemp / pid+tag 唯一目录）与合成 token；
  未触碰真实用户目录与真实凭证。
- 环境观察（非本任务造成）：本机存在两个先前会话遗留的 lingxi-service 孤儿进程
  （/tmp/rust-target-r02-t02 二进制、/tmp 合成 home，2026-09-26 14:59 启动），只占用各自
  /tmp 合成根，与本任务所有端口/数据根无冲突；按「保留无关状态」原则未杀，如实登记。

## 3. 观察事实（先读再动手，未混写设计）

1. 现役认证契约源码（本任务接入其语义）：
   - `core/server-auth.ts`：`authenticateRequestDetailed`——bearer 优先，query token 仅
     local 连接；loopback token（`SERVER_TOKEN`）匹配即 owner，但非 local 连接拒绝
     （`loopback_token_requires_local_transport`）；否则设备凭证（过期/撤销/连接策略
     `principalAllowsConnection`）。denial reason 词汇：`missing_credential`/
     `invalid_credential`/`connection_not_allowed`。
   - `server/index.ts:351`：`SERVER_TOKEN = LINGXI_TOKEN || randomBytes(16).hex`，写入
     `server-info.json` 0600（「本机最高权限凭据」注释）；desktop 从该文件读 token 作
     `Authorization: Bearer`（desktop/main.cjs:1212/5395）。
   - `core/device-registry.ts`：`hana_dev_` + base64url(32B) 秘密；存
     `secretPrefix`（18 字符快路径）+ scryptSync salted hash + 状态/作用域/过期；
     认证成功才写 `lastUsedAt`；`devices.json`/`device-credentials.json` 0600。
   - `core/ws-auth-ticket.ts`：`hana_ws_` + base64url(32B)；TTL 30s；表上限 512；
     消费即删（单次）；绑定 (principal, connectionKind, path)。
   - `core/security-principal.ts`：principal 规范化（kind/credentialKind/
     connectionKind/trustState/scopes + 派生 principalId）；`isLocalOwnerPrincipal`
     = local_user+local+loopback_token（全路由放行）。
   - `server/http/route-security.ts` + `shared/access-scope-profiles.ts`：策略种类
     public/authenticated/local_only/scope/studio_owner；scope 匹配=精确/命名空间/
     `命名空间.*`；未知 `/api/*` 兜底收紧、非 `/api` 默认 LOCAL_ONLY（fail-closed）。
   - `server/http/transport-context.ts`：loopback 模式 Host 必须是 loopback
     （DNS rebinding 防护）、remote 必须是 loopback；lan 模式 loopback Host+remote 仍判
     local。`server/http/cors-policy.ts`：Origin 白名单=loopback http(s) host:port、
     `file://`、`null`。
2. R02-T01/T02 交付的 lingxi-service：`run()` 保持传输纯；`ServiceState::new(config)` 只
   有 config；health 公开；单写者锁/READY 行/退出码 0-4 契约（T02 报告 §6 调用链）。
3. axum 0.8.9 的 `ws` 特性**未启用**且 `tungstenite`/`tokio-tungstenite` 不在
   rust/Cargo.lock（`grep -c tungstenite` = 0）——启用该特性会引入未锁第三方包，违反
   「新依赖仅限已锁版本」；hyper 1.11.1 / sha1 0.11.0 / base64 0.23.1 / sha2 0.11.0 /
   hyper-util 0.1.21 均已在锁内（axum/lingxi-protocol 的既有依赖）。axum 0.8 通过
   `parts.extensions.remove::<hyper::upgrade::OnUpgrade>()` 暴露连接接管（axum 自家
   ws.rs 同一机制）；`OnUpgrade` 自身即 `Future<Output=Result<Upgraded>>`；`Upgraded`
   经 `hyper_util::rt::tokio::TokioIo` 适配 tokio AsyncRead/Write。
4. T01 集成测试 `unknown_route_is_not_found` 断言裸 404——加入 fail-closed 认证后，
   未认证未知路径必须先拒（401），属 T03 拥有的契约变更（见 §4.9/§7）。
5. 审计封印测试族在开工基线（stash 本任务全部改动后实测）即为 1/3 红——预存在，
   与任务书「预期封印前红」一致，不归因本任务（§9 风险 1）。

## 4. 设计决定

1. **网络模式与「不因 loopback 免认证」**：`ServiceConfig.network_mode`
   （`Loopback` 默认 / `Lan` 仅 `--network-mode lan` 显式启用，严格 CLI 词汇）。默认
   模式下非 loopback `--bind` 是响亮配置错误（exit 2，`NetworkModeBindMismatch`，
   指名显式 opt-in），不存在静默 LAN 暴露。loopback 上的一切业务路由照常要求凭证
   （health 之外的每条路由都过认证；health 仅最少信息）。
2. **认证模型（语义镜像现役，隔离根内合成演练）**：
   - **loopback token**：每次启动轮换的 128-bit hex（镜像 `randomBytes(16).hex`），
     原子写入 `{home}/lingxi-service/local-token.json` 0600（umask 后显式 chmod），
     绑定 instanceId——语义对应现役 server-info.json 的 token 面（本机最高权限、
     owner-only 可读）；仅 local 连接可用，LAN 连接呈现即拒
     （`loopback_token_requires_local_transport`）。
   - **设备凭证**：`hana_dev_` + base64url(32B)；`devices.json`/
     `device-credentials.json`（0600、原子写）存 `secretPrefix`（18 字符）+ salted
     hash + 状态 + scopes + 可选 `expiresAtUnixMs`；创建（local-only 管理路由）/
     认证（前缀快路径 + 全量哈希校验）/过期/撤销语义真实。**记录分歧**（非静默降级）：
     现役 scryptSync → 本实现迭代盐化 SHA-256（4096 轮，scrypt 不在锁内）；时间字段用
     unix-ms 整数而非 ISO 字符串。撤销经 `AuthService::revoke_device_credential`
     （管理面；HTTP 无撤销动词，R02 记录为 T08 前 CLI/库层操作）。
   - **principal 模型**：镜像 `normalizePrincipal` 核心（kind/credentialKind/
     connectionKind/trustState/scopes/派生 principalId，camelCase serde）。身份只在
     `AuthService::authenticate` 的令牌验证边界创建；HTTP/WS 请求体中的身份字段一律
     不可信（`deny_unknown_fields` 硬拒 + 头部忽略，测试钉死）。
   - **常量时间比较**：hash-then-compare——两侧 SHA-256 摘要后定长逐字节累积异或
     （`constant_time_eq`），长度不再作为侧信道；设备凭证哈希同经此比较。
3. **接入面的取舍（记录，不是静默跳过）**：现役 web-session cookie 轴
   （`hana_session`）不接入 R02——本阶段无 web 登录流；bearer/query/设备凭证/ws 票据
   覆盖全部所需场景。studio_owner 策略面（`studio.owner` scope）在 R02 端点集中无对应
   端点，未建占位路由；本地 owner scope 集合保留该词表。
4. **HTTP/WS 共享授权服务**：一条中间件链（`transport_guard` → `auth_guard` → 路由）
   同时服务 HTTP 与 WS 升级：传输守卫先做 Origin 白名单（对所有路径含 health）、
   Host/网络模式一致性（DNS rebinding 防护）、每 peer 固定窗口限速（429）；认证中间件
   做凭证解析（bearer / local-only query token / WS 票据单次消费）与路由授权
   （同一 `classify_route`+`authorize` 表）。端点错误复用 lingxi-protocol 冻结
   `ProtocolError`（code/message/retryable/details.reason/requiredScope），未新增 wire
   类型、未改协议 crate。
5. **端点权限表**（`auth::classify_route` 单一权威；见 auth.rs 模块文档表）：

   | 方法+路径 | 策略 |
   |---|---|
   | GET /lingxi/v1/health | public（最少信息，无路径/实例/配置回显） |
   | GET /lingxi/v1/me | authenticated（服务器侧计算的身份回显） |
   | POST /lingxi/v1/ws-ticket | scope `chat` |
   | GET /lingxi/v1/sessions | scope `chat` |
   | GET /lingxi/v1/sessions/{id} | scope `chat` + 处理器内所有权检查 |
   | POST /lingxi/v1/sessions/{id}/execute | scope `chat` + 处理器内所有权检查 |
   | POST /lingxi/v1/devices/credentials | local_only（镜像现役 LOCAL_ONLY /api/devices/） |
   | GET /lingxi/v1/ws | scope `chat`（WS 升级） |
   | 其余一切（含未知） | local_only（fail-closed 默认） |

   本地 owner（loopback token + local 连接）全表放行（镜像 `isLocalOwnerPrincipal`）；
   作用域匹配=精确/命名空间/`命名空间.*`。
6. **资源范围绑定**：session 归 `owner_user_id` 所有；本地 owner 全可见，设备
   principal 仅同 userId 会话（HTTP 与 WS session_read 同一规则）。execute 把
   `principal_id`/`credential_kind` 写进 run 记录（R02 内存态；T04 持久化、T05 流式）。
   「伪 principal」零效果：/me 证明身份只来自令牌。
7. **Origin/Host 策略（显式文档化 + 测试矩阵）**：Origin **缺失**=允许（CLI/curl 形态；
   浏览器无法剥离自身 Origin，故该分支对浏览器流量不可达——文档化并入选 matrix）；
   Origin 存在则必须在白名单：`http(s)://localhost[:port]`、`http(s)://127.0.0.1[:port]`、
   `http(s)://[::1][:port]`（较现役正则增补 ::1，已记录）、`file://`、`file:///`、`null`
   （file:// 页的浏览器产物；http(s) 页面无法产生）。其余（evil.example、
   sub.localhost、127.0.0.1.evil.example、ws:// 等）一律 403 `bad_origin`，先于认证。
   Host：loopback 模式必须是 loopback host（`localhost`/`127.x`/`::1`，含端口剥离与
   IPv6 括号形式；`127.0.0.256` 等畸形按拒绝处理——较现役 `\d{1,3}` 正则更严，
   fail-closed），否则 403 `loopback_host_mismatch`（携带有效 token 亦拒——传输
   先于认证）；lan 模式 loopback Host+remote 仍判 local。
8. **票据生命周期**：`POST /lingxi/v1/ws-ticket`（scope chat）签发 `hana_ws_`+
   base64url(32B)，TTL 30s、表上限 512、绑定 (principal, connectionKind,
   /lingxi/v1/ws)；消费即删（单次）——过期/重放/换路径/换连接类型全部
   `invalid_ws_ticket` 401（升级前拒绝，无 101）。WS 升级后首帧必须是
   `lingxi.wire` `ClientHello`→`negotiate_protocol`→`ServerHello`（复用 R01 冻结协商，
   不兼容时 ProtocolError 帧 + close 4409）；随后 `session_read` 请求走与 HTTP 相同的
   所有权规则（跨主体 → ProtocolError 帧 + close 4403；未知名 → close 4404）。
   close 码族（4409/4401/4403/4404）沿用 R01-T02 原型词汇，全表面最终映射仍归
   RR-T02-F5（R08）。
9. **限制**（全部真实生效）：请求体 1 MiB（axum DefaultBodyLimit，413；JSON 拒绝映射
   区分 413/400）；每 peer 固定窗口 240 req/10s（429，可注入窗口供测试）；并发 WS
   升级上限 16（RAII 计数，101 前拒绝 503）；WS 单帧上限 1 MiB；execute 记录输入
   截断 2000 字符。
10. **未知路由 fail-closed**：T01 的裸 404 契约收窄——未认证未知路径 401（不泄露路由
    存在性），已认证未知路径仍 404；集成测试已按新契约更新并注明归属。
11. **WS 实现选型**：不启用 axum `ws` 特性（会引入未锁 tungstenite），改用已锁
    hyper `OnUpgrade` 接管连接 + 自实现 RFC 6455 子集（accept key 用已锁 sha1+base64，
    与 lingxi-protocol 原型同角色；帧编解码镜像该原型的已审实现，客户端帧解掩码、
    服务端帧不掩码、大小上限）。

## 5. 验收场景：命令 / 预期 / 实际 / 退出码

证据目录 `artifacts/rust-tauri/R02/T03/`（`.log` 按根 .gitignore 本机留存可重跑再生，
`.txt`/`.json` 可入库；stderr 标记已另存 `server-stderr-markers.txt`）。全部命令经
`env -u …_proxy PATH=~/.cargo/bin:$PATH` 且脚本内部强制 rustup 1.98.1 + 专属 target dir
`/tmp/rust-target-r02-t03` + `--locked` 离线。

### 5.1 R02-A05｜伪造身份无效（REQUIRED）

命令：`bash scripts/rust-tauri/r02_t03_auth_matrix.sh`（真实二进制：构建 + 服务进程
+ curl/python 全链）。结果 **exit 0**，A05 段 30 PASS + 正控制 3 PASS。

| 覆盖（任务书点名） | 操作 | 实际 | 证据 |
|---|---|---|---|
| 无凭证 | GET/POST 读会话、执行、ws-ticket、/me、WS 升级 | 全部 **401** `missing_credential`；WS 升级 401（无 101） | summary.txt、server-stderr-markers.txt（20 行 LINGXI_AUTH_REJECTED/TRANSPORT_REJECTED） |
| 伪 principal | X-Lingxi-Principal/X-Lingxi-User 头 + 携带伪 principalId/userId 的 body | 头不认证（401）；有效 token 下 /me 回显**服务器计算**的 local_user 身份、零 forged 字段；身份形 body=400 `invalid_message`（deny_unknown_fields）不执行 | summary.txt a05-me-*、a05-identity-shaped-body-* |
| 伪 sessionId | 已认证请求未知 sessionId（读+执行） | **404** `not_found`（401/等价的拒绝族），无副作用 | a05-forged-session-id* |
| 过期 token | expiresAtUnixMs=1 设备凭证（owner 管理路由签发） | **401** `invalid_credential`（读+执行） | a05-expired-credential* |
| 撤销 token | `revoke_device_credential` 后再认证 | **401** `invalid_credential`（单测 `device_credential_lifecycle_create_auth_expire_revoke` + 集成层） | cargo-test-workspace.log |
| 他人 token 冒用 sessionId（跨主体） | user_remote_b 设备凭证读写 user_local 会话 | 认证成功（/me=device）但读/执行 **403** `cross_principal_access`；WS session_read → close **4403**；列表为空 | a05-cross-principal-*、auth_matrix.rs a05_cross_principal_* |
| 无读写副作用 | runCount 快照 + 注册表文件哈希 | 全部负向请求后 runCounts 不变（0 0）；认证失败类注册表**逐字节一致**（state-before/after-authn.txt）；授权失败类仅审计字段（lastUsedAt/lastSeenAt/updatedAt）可变（devices/creds-before/after-authz.json 语义 diff，其余字段零变化）——认证成功写 lastUsedAt 是现役语义 | state-*.txt、summary.txt a05-authn/authz-* |
| 正控制 | owner 执行 | 200 + runId，runCount 恰 +1（证明链路活着而非全盘拒绝） | a05-positive-control-* |

### 5.2 R02-A06｜恶意网页无法借 loopback 越权（REQUIRED）

同命令（A06 段）；WS 子矩阵 `scripts/rust-tauri/r02_t03_ws_probe.py`（真实升级握手、
RFC 6455 帧计算校验 Sec-WebSocket-Accept）。合计 exit 0。

| 覆盖 | 操作 | 实际 | 证据 |
|---|---|---|---|
| 恶意 Origin 的 HTTP | `Origin: http://evil.example` 打 health（公开面！）与 /me（带有效 token） | **403** `bad_origin`（传输先于认证） | a06-evil-origin-* |
| 恶意 Origin 的 WS | WS 升级携 evil Origin + 有效 bearer | **403**（无 101） | ws-matrix.json ws-evil-origin |
| Host 头篡改 | `Host: evil.example`/`rebinder.example`（DNS rebinding 签名），含携带有效 token 变体 | **403** `loopback_host_mismatch`；`Host: localhost`/`127.0.0.1:port`/`[::1]` 放行 | a06-foreign-host-*、ws-foreign-host、a06-localhost-host-ok |
| 过期 WS 票据 | 签发后等待真实 30s TTL 过期再升级 | **401** `invalid_ws_ticket` | ws-matrix.json ws-ticket-expired |
| 票据重放 | 同票据第二次升级 | **401** `invalid_ws_ticket` | ws-ticket-replay |
| 正常桌面形态 | 合法 Origin（http://localhost:port）+ 票据 | **101** → ClientHello → ServerHello(selectedProtocol=1) → session_read 结果帧 | ws-ticket-legit-origin-upgrade/ws-server-hello/ws-session-read-owner |
| 无 Origin 非浏览器客户端（CLI/curl） | 无 Origin + bearer（HTTP+WS）、?token= query | HTTP 200；WS **101** + ServerHello；query token（local）200 | a06-cli-*、ws-no-origin-cli-bearer/ws-cli-server-hello |
| Origin 白名单边界 | localhost:port/127.0.0.1:port/file:///null 放行；sub.localhost、127.0.0.1.evil.example 拒 | 200×4 / 403×2 | a06-allowed-origin-*、a06-rebinding-origin-* |
| 限制真实生效 | 限速轰到 429；>1MiB 体 413；WS 并发上限（集成层 503） | 429 观测到、413 观测到、503 集成断言 | a06-rate-limit-live、a06-body-limit-live、auth_matrix.rs limits_* |
| LAN 仅显式 | 默认模式 `--bind 0.0.0.0:8080` | **exit 2**，错误文案指名 `--network-mode lan` | a06-non-loopback-bind-requires-explicit-lan-mode |

### 5.3 辅助检查（任务书 §4 固定方法第 4 步）

| 检查 | 命令（rustup 1.98.1 + /tmp/rust-target-r02-t03 + --locked） | 结果 | 退出码 | 证据 |
|---|---|---|---|---|
| fmt | `cargo fmt --all -- --check` | 无 diff | 0 | cargo-fmt-check.log |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 无告警 | 0 | cargo-clippy.log |
| 单测/集成 | `cargo test --workspace --locked` | **135 passed / 0 failed**（service lib 70 + auth_matrix 13 + instance_lifecycle 3 + service_health 4 + kernel 7 + protocol 19+1 + spike/browser-spike 存量） | 0 | cargo-test-workspace.log |
| 边界检查器 | `python3 -B …/r01_t01_check_ownership.py --self-test` | RESULT: OK | 0 | check-ownership-selftest.log |
| schema 生成 | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 56 文件 + 624 API 项零漂移（**未改协议 crate/生成物**） | 0 | r01-t02-check-generated.log |
| T01 回归 | r02_t01_service_smoke.sh + r02_t01_boundary_negative.sh | A01/A02 PASS（READY 行未动） | 0 | regression-r02-t01-*.log |
| T02 回归 | r02_t02_dual_instance.sh + r02_t02_path_priority.sh | A03/A04 PASS（锁次序保持：锁→auth bootstrap→bind） | 0 | regression-r02-t02-*.log |
| 审计封印 | `npx vitest run tests/post-verification-audit-seal.test.ts` | **1/3 失败（预存在）**——stash 本任务全部改动后在干净基线实测同为 1/3 红；详见 §9 风险 1 | 1 | audit-seal-test.log |

## 6. 关键生产调用链（main → 守卫 → 认证授权 → 端点/WS）

```text
rust/crates/lingxi-service/src/main.rs            # 入口：tracing(stderr, 非TTY无ANSI)
  ├ parse_cli(argv)                               # config.rs：严格 CLI（+--network-mode loopback|lan）
  ├ ServiceConfig::from_sources(...)              # 优先级解析 + 网络模式校验（非loopback bind+默认模式=exit 2）
  ├ prepare_layout(&config.data_home)             # paths.rs：canonicalize/0700/原子写挂点
  ├ acquire(&layout)                              # instance.rs：OS 文件锁（在端口绑定之前）
  ├ ServiceState::bootstrap(config, &layout)      # lib.rs：auth.rs bootstrap（轮换 loopback token
  │                                               #   写 local-token.json 0600、建 devices/creds 注册表）
  │                                               #   + WsTicketService + SessionStore(seeded) + 限流器
  ├ run(state, shutdown, on_ready)                # bind → publish instance.json → READY 行（不变）
  │   └ axum::serve(listener,
  │       build_router(state).into_make_service_with_connect_info::<SocketAddr>())
  │       ├ transport_guard（每请求，先于一切）    # lib.rs：Origin 白名单 → Host/网络模式 → 限速
  │       │   └ 拒绝：403 bad_origin / 403 loopback_host_mismatch / 429（stderr 机读标记）
  │       ├ auth_guard                            # classify_route=public 直通；否则：
  │       │   ├ /ws：ws_credential_from → 票据单次消费（consume）或 bearer/query 认证
  │       │   ├ 其余：authenticate(bearer|query[仅local], conn_kind)
  │       │   │   └ auth.rs：loopback token（常量时间比较，仅 local）→ 设备凭证
  │       │   │       （前缀+盐化哈希+过期/撤销+连接策略；成功才写 lastUsedAt）
  │       │   └ authorize(method,path,principal)  # 端点权限表（local owner 放行/作用域/fail-closed）
  │       │       └ 拒绝：401/403 + ProtocolError(details.reason)（stderr 机读标记）
  │       └ 路由处理器（Extension<Principal>——身份只能来自这里）
  │           ├ GET /lingxi/v1/health             # public，最少信息
  │           ├ GET /lingxi/v1/me                 # 服务器计算身份回显
  │           ├ GET /lingxi/v1/sessions[/{id}]    # scope chat + 所有权（NotFound/Forbidden）
  │           ├ POST /lingxi/v1/sessions/{id}/execute  # 同上；成功追加 run 记录（principal 绑定）
  │           ├ POST /lingxi/v1/ws-ticket         # scope chat；签发 hana_ws_ 票据（30s/单次/绑定）
  │           ├ POST /lingxi/v1/devices/credentials    # local_only；签发合成设备凭证（secret 仅回显一次）
  │           └ GET /lingxi/v1/ws                 # WsUpgrade 提取器 → 连接上限 → 101（Sec-WebSocket-Accept
  │                                               #   = sha1+base64，RFC 6455）→ OnUpgrade 接管
  │               └ run_ws_session                # ws.rs：ClientHello→negotiate_protocol→ServerHello；
  │                                                   session_read 走同一所有权规则（close 4403/4404）
  └ 停机：guard.release()（只删自己的记录）→ exit 0（退出码 0-4 契约不变）
```

## 7. 改动清单

**新增（生产）**
- `rust/crates/lingxi-service/src/auth.rs`（principal 模型、loopback token 存储与轮换、
  设备凭证注册表（创建/认证/过期/撤销）、常量时间比较、端点权限表 classify/authorize、
  scope 匹配；16 单测）
- `rust/crates/lingxi-service/src/transport.rs`（NetworkMode/ConnectionKind、Host loopback
  判定（含畸形拒绝）、infer_connection_kind、Origin 白名单 check_origin；6 单测）
- `rust/crates/lingxi-service/src/limits.rs`（请求体/速率/WS 连接上限常量、固定窗口
  限速器、RAII WS 连接计数；2 单测）
- `rust/crates/lingxi-service/src/sessions.rs`（最小会话/run 存储、所有权规则、
  ExecuteRequest deny_unknown_fields；4 单测）
- `rust/crates/lingxi-service/src/ws.rs`（票据服务（签发/单次消费/TTL/绑定/上限）、
  WsUpgrade 提取器、101 响应、RFC 6455 帧编解码（tokio 异步、大小上限）、请求词汇；
  8 单测）

**新增（测试/脚本/证据）**
- `rust/crates/lingxi-service/tests/auth_matrix.rs`（A05/A06 集成矩阵 13 测试：真实
  axum+TCP+WS 升级、runCount 快照、伪造身份/跨主体/Origin/Host/票据矩阵、限制生效）
- `scripts/rust-tauri/r02_t03_auth_matrix.sh`（真实二进制 A05+A06 验收，60 PASS）
- `scripts/rust-tauri/r02_t03_ws_probe.py`（真实 WS 探针：升级/Accept 校验/票据/握手/读取）
- `artifacts/rust-tauri/R02/T03/`（§5 证据；.txt/.json 可入库，.log 本机留存）
- `docs/rust-tauri/R02/R02-T03_REPORT.md`（本报告）

**修改**
- `rust/crates/lingxi-service/src/lib.rs`（模块注册与重导出；ServiceConfig+network_mode；
  ServiceState 扩展为注入面（auth/tickets/sessions/limits）并要求 bootstrap（**不存在
  无认证构造路径**）；EndpointError（ProtocolError over HTTP 状态）；transport_guard/
  auth_guard 中间件；全部业务处理器；run() 改收 ServiceState + ConnectInfo）
- `rust/crates/lingxi-service/src/config.rs`（`--network-mode` 严格 CLI + 两个配置错误
  变体 + 单测）
- `rust/crates/lingxi-service/src/main.rs`（启动链插入 auth bootstrap（锁后 bind 前，
  失败 exit 2）；USAGE 增补 network-mode）
- `rust/crates/lingxi-service/Cargo.toml`（+hyper 1.11.1 / hyper-util 0.1.21（仅 tokio
  特性）/ sha1 0.11.0 / sha2 0.11.0 / base64 0.23.1——全部为 Cargo.lock 已锁版本，
  零新增第三方版本；Cargo.lock 仅增加本 crate 的依赖引用 4+1 行，零新包）
- `rust/crates/lingxi-service/tests/service_health.rs` / `tests/instance_lifecycle.rs`
  （适配 run(state) 签名与 ServiceConfig 字面量；未知路由契约按 fail-closed 更新并注明）

**零改动（含验证）**：`rust/crates/lingxi-protocol`、`rust/crates/lingxi-kernel`、
`contracts/generated/`（56 文件 --check 零漂移）、`Lingxi_Rust_Tauri_Taskbooks_…/`、
`.sync-audit/`、`PROGRESS.md`、`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`、`package.json`、
`desktop/`、`server/`、`core/`、`lib/`、`shared/`、`tests/`（Node 侧）。

## 8. 测试列表（层级归属，01 §5 分层）

| 测试 | 层级 | 断言要点 |
|---|---|---|
| auth::bearer_parsing / constant_time_eq_basics / scope_namespace_expansion | 纯逻辑 | 凭证解析镜像、比较原语、作用域展开 |
| auth::local_token_roundtrip_and_owner_principal | 纯逻辑（真实文件系统） | owner 主体/错 token/缺凭证/LAN 拒/query 规则 |
| auth::device_credential_lifecycle_create_auth_expire_revoke | 纯逻辑（真实注册表文件） | 创建/认证/**过期**/**撤销**/前缀伪造拒绝 |
| auth::failed_authentication_never_touches_registry_files | 纯逻辑 | 认证失败注册表逐字节不变 |
| auth::token_file_is_owner_only_and_instance_bound | 纯逻辑 | 0600、instanceId 绑定 |
| auth::route_policy_table / authorize_order_and_denials | 纯逻辑 | 端点权限表逐条 + 401/403/优先级 |
| auth::registries_reload_across_bootstrap | 纯逻辑 | 注册表跨启动保留、token 每启动轮换 |
| transport::network_mode_parses_strictly / host_loopback_detection… / loopback_mode_requires… / lan_mode_still_recognizes… / origin_policy_allowlist | 纯逻辑 | Host/Origin/网络模式矩阵（含 rebinding/畸形/子域） |
| limits::rate_limiter_fixed_window… / ws_connection_counter… | 纯逻辑 | 窗口语义/上限/RAII 释放 |
| ws::websocket_accept_known_vector（RFC 6455 §1.3 官方向量）/ ticket_lifecycle… / ticket_table_is_bounded… / frame_codec_roundtrip… / oversized_frame_is_rejected… / ws_request_vocabulary… / query_pair_parsing / ws_credential_extraction_order | 纯逻辑/组件 | accept 计算、票据单次/过期/绑定/上限、帧往返、超限拒、词汇 deny_unknown |
| sessions::owner_sees_everything… / cross_principal… / same_user_device… / execute_request_rejects_identity_shaped… | 纯逻辑 | 所有权/跨主体 Forbidden/无副作用/身份字段硬拒 |
| tests/auth_matrix（13） | 契约/服务集成 | 真实 loopback TCP 全链：A05 全覆盖（含 runCount 快照）+ A06 Origin/Host/票据/CLI 形态 + 限制（413/429/503）+ 配置层 LAN 拒启 |
| tests/service_health / instance_lifecycle（T01/T02 存量，7） | 契约/服务集成 | 回归：health 最小面、fail-closed 未知路由新契约、锁/记录/READY |
| scripts/rust-tauri/r02_t03_auth_matrix.sh（+ws_probe.py） | 真实二进制进程 | §5.1/§5.2 全矩阵（60 PASS） |

## 9. 未验证 / 风险 / 交接

**未验证（如实）**
1. 非 macOS 平台未编译验证（Windows 目录权限/锁分支为 T02 既有边界；auth 模块的
   0600/chmod 仅 unix 分支，非 unix 退化为存在性检查并注释）。
2. LAN 模式真实跨机流量未实测（无第二台机器）：LAN 连接分类、loopback-token-over-LAN
   拒绝、设备凭证 LAN 语义均为单测/集成层（注入 remote/conn_kind）验证；真实 LAN 矩阵
   归 R09/R10 平台验证。
3. release 构建与长时资源增长未测（R10/T08）；限速器为每-IP 固定窗口（无分布式/
   无 GC 策略 beyond 4096 上限剪枝），对抗强度有限但 fail-closed。
4. 撤销设备凭证无 HTTP 路由（管理面 local-only 签发已有；撤销经库层
   `revoke_device_credential`，单测+集成演练覆盖语义）——HTTP 撤销动词留给后续任务按需
   增加，避免过度建设。
5. `sync`/`Connection: close` 下偶发 RST：真实浏览器/HTTP 客户端不受影响；矩阵脚本
   与集成测试对 RST 容错读取（不影响断言）。
6. Node/现役入口回归未跑全量 npm test：Node 侧零改动且无测试引用 rust workspace
   （T01 观察事实 9 仍成立）；按 05 §2 不无理由重复，A16 属 T08。

**已知风险**
1. **审计封印测试 1/3 红（预存在，非本任务造成）**：开工基线（stash 全部改动实测）
   同为 1/3 红；封印坐标（ab4f2281）落后于已授权 R02 提交。按 AGENTS.md：如实报告、
   不虚报坐标、不扩白名单、不退役门禁；坐标推进属获准提交后的封印流程，本执行代理
   无提交权限。
2. **run() 签名变更（config→state）与未知路由 fail-closed**：T01 契约的两处收窄
   （404→401-when-unauthenticated；run 参数形态），均为 T03 拥有的行为变化，测试与
   USAGE/模块文档同步更新；T01/T02 回归脚本复跑全绿证明 READY/锁/退出码契约未受影响。
3. **设备凭证哈希原语分歧**：迭代盐化 SHA-256 vs 现役 scrypt（§4.2）——R02 合成演练
   语义等价（盐化/前缀/常数时间/过期/撤销），生产前若要求原语一致需引入 scrypt 或
   迁移方案（届时按新依赖流程）。
4. **WS close 码族（4401/4403/4404/4409）**为 R02 服务面词汇；与旧 Electron 客户端的
   最终对齐归 RR-T02-F5（R08）。
5. `local-token.json` 每启动轮换（镜像现役 SERVER_TOKEN 语义）：重启后旧 token 失效，
   读取方需重读文件；这是设计而非缺陷，桌面壳接入（R08/R09）沿用「读文件获 token」
   形态。
6. 本机存在先前会话遗留的两个 lingxi-service 孤儿进程（§2 环境观察），未处置（保留
   无关状态原则）；如验收环境复跑脚本不受影响（端口/根均随机）。

**交接（给 R02-T04/T05 及后续）**
- `ServiceState` 是唯一注入面：T04 的 StoragePort 实现、T05 的事件服务在此挂载替换
  `sessions::SessionStore`（端点形状 sessionId/owner_user_id/run 记录为其预留）。
- 端点权限表新增路由时改 `auth::classify_route` 一处（表驱动 + 单测逐条钉死）；
  中间件链次序（transport→auth→路由）不可倒置。
- 机读 stderr 标记 `LINGXI_TRANSPORT_REJECTED`/`LINGXI_AUTH_REJECTED` 可供桌面壳/CLI
  复用为诊断契约（与 T02 三件套同风格）。
- 票据/限流参数集中在 `ws.rs`/`limits.rs` 常量与 `bootstrap_with_limits` 注入口。
- 本任务脚本（auth_matrix + ws_probe）可作 T08 verify-stage 登记命令候选。

## 10. 最终工作树状态

`git status --short`（全集，tested SHA = 0bdacbd86 + 以下未提交改动）：

```text
 M rust/Cargo.lock
 M rust/crates/lingxi-service/Cargo.toml
 M rust/crates/lingxi-service/src/config.rs
 M rust/crates/lingxi-service/src/lib.rs
 M rust/crates/lingxi-service/src/main.rs
 M rust/crates/lingxi-service/tests/instance_lifecycle.rs
 M rust/crates/lingxi-service/tests/service_health.rs
?? artifacts/rust-tauri/R02/T03/
?? rust/crates/lingxi-service/src/auth.rs
?? rust/crates/lingxi-service/src/limits.rs
?? rust/crates/lingxi-service/src/sessions.rs
?? rust/crates/lingxi-service/src/transport.rs
?? rust/crates/lingxi-service/src/ws.rs
?? rust/crates/lingxi-service/tests/auth_matrix.rs
?? scripts/rust-tauri/r02_t03_auth_matrix.sh
?? scripts/rust-tauri/r02_t03_ws_probe.py
?? docs/rust-tauri/R02/R02-T03_REPORT.md
```

（Cargo.lock 差异仅 5 行依赖引用（hyper/hyper-util/sha1/sha2/base64），全部指向既有
锁定版本，零新增第三方包；kernel/protocol/spike/browser-spike/任务书/contracts/
generated/.sync-audit/PROGRESS.md/ORCHESTRATOR_PROGRESS.json/Node 生产入口零改动。）

## 11. 推荐独立验收重点

1. 复跑 `scripts/rust-tauri/r02_t03_auth_matrix.sh`（真实进程证据链）：重点看 A05 段
   state-before/after 的三类不变性断言（认证失败逐字节、授权失败仅审计字段、runCounts
   全程稳定）与正控制（runCount 恰 +1）——防「全盘拒绝冒充安全」。
2. 对抗复核 Origin/Host：改用 `Origin: https://localhost`（应放行）、`Host: LOCALHOST:port`
   （大小写）、`Host: 127.0.0.01`（畸形应拒）、伪造 `null` 之外的空串 Origin（应拒），
   核对与 `transport.rs` 白名单/畸形规则一致。
3. 票据对抗：同票据并发两路升级（应恰好一路 101）；票据跨 principal 复用（消费绑定
   principal，无冒面）；`ws-ticket` 端点未授权签发被拒。
4. 常量时间比较抽查：`constant_time_eq` 的实现审查（累积异或、无早退）与
   hash-then-compare 论证（auth.rs §Constant-time）。
5. fail-closed 审计：新增一条未登记路由（如临时加 `/lingxi/v1/secret`），验证默认
   local_only 且未认证 401（表驱动默认生效）。
6. 依赖授权性：`git diff rust/Cargo.lock` 仅 5 行引用、`git diff rust/crates/
   lingxi-service/Cargo.toml` 逐条对照已锁版本表（hyper 1.11.1/hyper-util 0.1.21/
   sha1 0.11.0/sha2 0.11.0/base64 0.23.1）。
7. 审计封印红按预存在项核对（§2/§9.1：stash 改动后基线即红）。
