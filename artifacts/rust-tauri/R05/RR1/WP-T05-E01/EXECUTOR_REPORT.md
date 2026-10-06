# R05 RR1 WP-T05 执行者报告（R1，2026-10-05）

工作包：R05-T05「统一网络策略、解析后边界与全过程预算」（F14/F15/F16）。
工具链：`/Users/study_superior/.cargo/bin/cargo +1.98.1`（rust-toolchain.toml 锁定；未使用 PATH/Homebrew cargo）。
分支 `codex/rust-tauri-migration`，未提交任何 commit（收口统一汇总）。

## 1. 旧行为反例（先红，已固化）

`old-red-probe/`（独立 probe crate，path 依赖当前树；Cargo.lock 内 tokio=1.53.1、serde_json=1.0.151 与工作区锁定版本一致）。
四条正确行为断言在**未修复候选树**上全部失败（`old-red-on-unfixed-tree.log`，cargo exit 101，4 failed / 0 passed）：

| 反例 | F-ID | 未修复树上的失败事实 |
|---|---|---|
| `network_policy_section_parses_and_validates` | F14 | `deny_unknown_fields` 拒绝 `network` 段：`unknown field 'network', expected 'providers' or 'models'`（策略载体不存在） |
| `system_proxy_environment_routes_the_shared_client_through_the_proxy` | F14 | 进程级导出 HTTP(S)_PROXY 后，共享 client 仍直连（`.no_proxy()` 固定），计数代理 hits=0（真实网络边界：代理零连接） |
| `domain_resolving_to_loopback_must_not_be_connected` | F15 | 审计 CN-F06 原样迁移：allowlist 仅 `authorized.example.invalid`，下载 `https://localhost:<port>/`，OS 解析后未授权 127.0.0.1 哨兵读到 TLS 握手字节（连接=1） |
| `error_body_must_obey_total_deadline` | F16 | 审计 CN-F07 原样迁移：401 头+1 字节 body 后停住，deadline=50ms，250ms 外层观测窗口超时（`response.text()` 无 deadline） |

运行命令（代理端口在进程启动前导出，避免进程内 env 竞态）：
```
HTTP_PROXY=http://127.0.0.1:$PORT HTTPS_PROXY=… http_proxy=… https_proxy=… PROXY_ENV_TEST_ADDR=$PORT \
  /Users/study_superior/.cargo/bin/cargo +1.98.1 test --offline --locked -- --test-threads=1 --nocapture
```

## 2. 修复后转绿（同一批断言）

`green-probe-adapted/`（同一 crate，仅按接口演进适配调用形状：client 构造行改为
`build_client_under_policy(..., system_plane().policy())`；断言文本逐字未动）：
`green-on-fixed-tree.log` = 4 passed / 0 failed，cargo exit 0。

## 3. 修复内容

### F14 统一网络策略
- 新模块 `lingxi-adapters/src/models/network.rs`：
  - `NetworkConfigSection`/`NetworkProxyConfig`（serde camelCase、deny_unknown_fields）＝现役
    `shared/network-proxy.ts` 契约镜像：mode `system|manual|direct`（缺省 system）、
    httpProxy/httpsProxy（http/https/socks/socks5、有 host、无 userinfo/path/query/fragment，
    manual 须至少一个 URL）、noProxy（`localhost, 127.0.0.1, ::1` 缺省）、trustedCaPem
    （PEM bundle，解析且须含 ≥1 证书——`from_pem_bundle` 会静默跳过垃圾文本，空列表是响亮配置错误）。
  - 运行时 `NetworkPolicy{ProxyPolicy, trusted_ca_pem}`：`System{env 快照}`（load 时快照
    HTTP(S)_PROXY/ALL_PROXY/NO_PROXY 大小写两种拼写、非法 URL 跳过不信任——现役
    `proxyConfigFromEnvironment` 语义）/`Manual`/`Direct`；
    `effective_proxy_for_url` 完整移植现役 `isNoProxyMatch`+`resolveProxyForUrl`
    （强制 loopback bypass：localhost/::1/127.x 永不走代理；`*`/`.suffix`/`*.suffix`/精确/
    `[v6]`/逐条 `:port`；http→http||https、https→https||http）。语法有逐条单测钉住。
  - `NetworkPlane`（AtomicU64 代次 + RwLock 策略）与 `NetworkClientHandle`（懒重建：代次变了
    下一次请求重建 client；重建失败响亮，绝不静默沿用旧策略）。
- `dispatch.rs`：`build_client_under_policy(timeouts, policy)`（no-redirect + connect timeout +
  per-URL `Proxy::custom` 拦截器（装显式代理同时禁用 reqwest 环境自动探测）+
  `add_root_certificate(from_pem_bundle)` 叠加于 rustls-platform-verifier 之上——
  链/主机名/有效期校验永不放宽，无任何 accept-invalid 路径）；`build_client_with_timeouts`
  保持 = Direct（遗留构造器语义）。`build_client_pinned_under_policy` 加 `resolve_to_addrs` 钉住。
- 消费方接线（单一 plane，一代策略）：五 chat family（`new_with_timeouts_and_network`）、
  `GatewayedProvider::new_with_network`、`AuxiliaryExecutor::new_with_network`（辅助/worker
  callback 同喉）、`OperationDispatcher::with_timeouts_and_network`、
  `EgressGuard::from_provider_endpoints_and_network`（资源下载）、
  `OAuthHttp::new_with_network`（token/refresh；`ProductionRefreshDriver::with_network`）。
- 服务面：`ServiceDeps.network_plane`（新字段）；main.rs 从已校验 plane 的 network 段建 plane
  （system 模式在此快照环境）并传入凭证 bootstrap；lib.rs 组合根把同一 plane 传给
  provider/aux/operations；management.rs `reload_models` 在 gateway 交换后
  `plane.apply(NetworkPolicy::from_config(...))` 原子发布新代次（in-flight 请求用旧 client 完成，
  下一请求观察到新策略）。
- 依赖：reqwest 0.13.5 追加 `socks` 特性（空 cfg 门，锁内版本不变；现役允许 socks 代理）。

### F15 解析后边界
- `egress.rs`：`EgressResolver` 可注入（默认 tokio lookup_host＝getaddruid 同路径；测试注入
  受控候选集，守卫范围判断始终在本模块）。`download()`：`check_parts`（同步规则）→
  非 same-origin 的 https **主机名**走 `resolve_candidates`：解析失败/空候选集拒；任一候选
  落入守卫范围（含混合公私记录、v4-mapped 折叠）拒；全公开候选集返回后**钉住拨打**
  （`pinned_client` 用 `resolve_to_addrs`，SNI/Host/证书校验仍按域名——传输层无法再解析到
  未判定地址，检查与拨号之间的 DNS 变更结构性失效）。同源例外与字面 IP 保持原语义（例外
  仍 origin 精确：同主机名异端口照样拒）。代理策略下：本地候选校验照做（纵深防御），
  拨打走普通策略 client（代理为解析权威，CONNECT 隧道）。`check_url_resolved` 供服务层预检。
- 原「DNS rebinding 离线不可判」声明缺口撤销（egress.rs 模块头文档更新）。

### F16 绝对 deadline 全阶段
- `dispatch.rs::send_with_timeouts`：错误 body 改 `read_error_body_bounded`（剩余绝对预算 +
  64KiB 硬上限；超时/截断/读失败事实保留在分类错误消息里，不再 `unwrap_or_default()` 吞掉）。
- `operations/mod.rs::classify_error_response`：新增 deadline 参数（原传 `None` 且吞错），
  走同一有界读取；video.rs 两个调用点传入各自 deadline。
- `service/operations.rs::admit(lane, deadline)`：排队等待包裹剩余预算，预算耗尽 →
  `BudgetExceeded`「admission wait outlived the call budget」（审计 F-WU06 排队腿：25ms 预算
  不再等 30s 配额窗）。8 个 admit 调用点全部传各自 deadline。
- `runs.rs::acquire_or_break(…, deadline)`：select 增加 budget 臂——模型 permit 排队等待在
  deadline 处退出，调用方以新鲜时钟读区分 `budget_exceeded`（ProviderFailed 语义，与 pre-send
  检查同形）与 `quota_exhausted`；工具 lane 显式传 None（R03-T03 取消语义不变）。
- `provider.rs`：401 协调刷新等待包裹剩余预算（parked refresh 不再越过总预算；
  BudgetExceeded 非重试）。
- `oauth.rs`：token/设备授权 body 读取改 256KiB 有界（时间上原有 flow-clock 窗口保持；
  超时/截断/传输失败事实随错误文本保留）。

## 4. 永久测试（仓库内）

`lingxi-service/tests/r05_t05_network.rs`（21 测试，全部真实生产面 + 受控替身）：
- rr1_f14（9）：配置段解析/校验（含 userinfo 拒）、manual 代理真实改道（计数代理绝对形式
  收到 chat 请求、合成源 host 直连 0）、强制环回 bypass（空 noProxy 下 127.0.0.1 端点仍直连：
  代理 0/源 1）、direct 显式模式、system 模式环境快照（注入 env map，无进程 env 竞态）、
  私有 CA 授权成功+未授权默认拒绝、错误主机名/过期证书/错误链三负例（rcgen 生成 CA/证书，
  IP SAN、过去有效期窗口）、OAuth refresh+operation 共享同一 manual 代理（一次 plane、逐面计数）、
  策略代次发布后下一请求改道（route 换非环回 host + plane.apply，代理 1/源不变）。
- rr1_f15（6）：localhost 零连接（默认 OS 解析器，审计反例原文迁移）、私有候选零连接
  （受控解析器+哨兵）、混合公私候选拒+全公开过判定、**钉住拨打不再重解析**（解析器首答
  192.0.2.1、后答 127.0.0.1 哨兵——哨兵 0 连接且解析器恰好被调 1 次，重绑定窗口结构性关闭）、
  同源例外真实字节下载（正向）+异端口拒、代理下载走 CONNECT 且守卫名仍拒（代理边界+认证头
  不带已由下载面自身保证）。
- rr1_f16（6）：审计错误 body deadline 反例原文迁移（返回+保留 deadline 事实）、超大错误体
  64KiB 截断事实保留、mid-body 读失败保留（不再吞成空）、operation 排队预算（permit 被停住
  的 sibling 占用，第二调用 250ms 预算内 BudgetExceeded、HTTP=1 仅停住的那次、不等 30s 窗）、
  chat run 驱动排队预算（真实 RunSupervisor/storage/events/quota，注入 ParkOnceProvider：
  run B 400ms 预算在队列中耗尽 → failed.provider_error（budget）而非 quota_exhausted，<2s 落定）、
  parked 刷新等待预算（401→report_unauthorized 挂起→300ms BudgetExceeded，消息点名
  credential refresh 阶段）。
- `lingxi-adapters/src/models/network.rs` 单测：NO_PROXY 语法（强制环回、`*`、前后缀点、逐条
  端口、scheme 选择、env 优先级与非法 URL 跳过、空快照=直连）、配置校验各负例+合法形状
  （含 socks5）、plane 代次懒重建（built_generation 可观测）。

## 5. 自检命令与结果

（本报告随自检完成更新，全部经 rustup 代理 1.98.1 执行；命令原文与退出码见 §7 表。）

## 6. 共享文件改动登记（矩阵同步）

| 文件 | 共享方 | 改动 | 回归范围 |
|---|---|---|---|
| `lingxi-adapters/src/models/dispatch.rs` | T02(F05 脱敏)/T03/T04 | client 构建策略化 + 错误 body 有界读取 | adapters+service 全量 |
| `lingxi-adapters/src/models/config.rs` | T01/T02 | +network 段（deny_unknown_fields 新键）+校验+错误变体 | 同上 |
| `lingxi-adapters/src/models/provider.rs` | T01(F01/F02)/T03 | +new_with_network；401 刷新等待预算包裹（F02 的代次 fence 语义未动） | 同上 |
| `lingxi-adapters/src/models/oauth.rs` | T02(F04/F05) | OAuthHttp 策略化 + body 有界 | 同上 |
| `lingxi-adapters/src/models/operations/mod.rs`、`video.rs` | T06/T07 | dispatcher 策略化；classify_error_response 增 deadline | 同上 |
| `lingxi-adapters/src/models/egress.rs` | T06(C11B) | 解析候选校验+钉住+策略 client（check_url 同步规则与 C11B 既有断言未动，全部保留绿） | 同上 |
| `lingxi-service/src/operations.rs` | T06(F17-F19)/T07(F21) | admit 带 deadline（F16 指名入口）；8 调用点传参 | service 全量 |
| `lingxi-service/src/runs.rs` | T04(F11-F13) | acquire_or_break deadline 臂（T04 的 origin 记录/整批准入语义未动） | service 全量 |
| `lingxi-service/src/credentials/mod.rs` | T02(F03/F04) | bootstrap_with_network/with_network（handle/栅栏/种子语义未动） | service 全量 |
| `lingxi-service/src/lib.rs`、`management.rs`、`main.rs` | T01/T08 | ServiceDeps.network_plane + 组合根接线 + reload 发布策略 | service 全量 |
| `rust/Cargo.lock` | 全体 | reqwest `socks` 特性（版本不变）+ probe 无关 | — |

## 7. 命令与退出码（全部经 /Users/study_superior/.cargo/bin/cargo +1.98.1 实跑）

| 命令（参数列表形式） | 退出码 | 结果 |
|---|---|---|
| `cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t05_network`（旧红时为 probe 形式，见下） | 0 | 21/21 绿（rr1_f14 9、rr1_f15 6、rr1_f16 6） |
| probe 旧红：`HTTP(S)_PROXY=…$PORT PROXY_ENV_TEST_ADDR=$PORT cargo +1.98.1 test --manifest-path <old-red-probe>/Cargo.toml --offline --locked -- --test-threads=1 --nocapture` | 101 | 4 failed / 0 passed（`old-red-on-unfixed-tree.log`） |
| probe 转绿：同命令（green-probe-adapted，仅接口演进适配） | 0 | 4 passed / 0 failed（`green-on-fixed-tree.log`） |
| `cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters` | 0 | 24 个测试二进制全 ok（300 passed / 0 failed；含 network.rs 单测与既有 egress C11B 套件全绿） |
| `cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service` | 0 | 65 个测试二进制 ok（865 passed / 0 failed；`green-service-full.log`） |
| `cargo +1.98.1 fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | 干净 |
| `cargo +1.98.1 clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | 0 error |

环境项如实登记：`-p lingxi-service` 全量第一次后台运行时 `r00_management_leaves` 的
LAN 腿失败一次（53s 停滞，测试自身 panic 文本即 macOS 应用防火墙对非环回自地址
入站的归因——R05-ENV-ALF-UNSIGNED-TEST-BINARY 既有环境项，T03/T04 轮次同样现象已
在矩阵登记）；单独复跑与最终全量复跑均通过（上述 865/0 记录为最终全量）。未为使
其变绿修改任何门禁或测试。

## 8. 未覆盖/边界（如实）

1. system 模式＝导出的代理环境变量快照（现役 Node `proxyConfigFromEnvironment` 语义），
   不读 macOS 系统代理设置（现役亦不读）。
2. socks/socks5 代理 URL 经配置校验接受且 reqwest `socks` 特性已启用；本机无 socks
   替身，未实测 socks 服务端拨号（语法与 client 构建路径有测）。
3. F16 的子进程等待/交付段、共享刷新按等待者隔离取消归 F20/F21（WP-T06/T07）联合
   验收；OAuth 登录流（管理面）使用 flow-clock 每请求窗口+新增大小上限，不在 run
   预算内（非 run 生命周期）。
4. LIVE/真实供应商代理环境未测（BLOCKED_NOT_AUTHORIZED，与审计结论一致；本包全部
   义务为离线可判定项）。

## 9. 状态

F14/F15/F16 → SELF_CHECKED（矩阵已同步，rounds=1）；待全新独立审查者实测验收。
未 commit（收口统一汇总）。
