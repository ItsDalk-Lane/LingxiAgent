# R02-T01｜实现 workspace 与组合根 — 执行报告

- 执行者：ZCode:R02-T01（EXECUTOR-R02-T01，一次性执行代理；不负责独立验收，不提交/推送）
- 状态：**READY_FOR_REVIEW**（PASS/FAIL 判定归总控另派的独立验收）
- 日期：2026-09-26
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R02_Rust独立服务、存储与事件基础.md` §4 R02-T01
  （场景 R02-A01「无桌面启动」、R02-A02「违反依赖规则失败」，均 REQUIRED）

## 1. 范围

任务书四步：按 R01 冻结边界建立 rust workspace 组合根（service 注入 ports、domain 不直接
访问 HTTP/UI/环境变量/数据库）；只建当前实际使用的抽象；加入 fmt/clippy/单测/依赖边界/
schema 生成检查；提供独立 service 二进制与受控 test harness，现役 Node/Electron 默认启动
不变。交付：Rust workspace（含可独立启动服务）+ 架构边界检查。

不在本任务内（属 R02 后续 T）：配置优先级与单写者锁（T02）、HTTP/WS 认证（T03）、存储
ports 与事务（T04）、事件顺序与续读（T05）、备份恢复（T06）、日志脱敏与资源上限（T07）、
xtask 门禁三件套（T08）。本任务不预建空 manager（不建 lingxi-adapters/xtask 空壳——
按总控指令「本 Task 只建本 Task 实际用的」）。

## 2. 源码基线与环境

- 开工实测：分支 `codex/rust-tauri-migration`，HEAD =
  `201584f2917a7fd96d6ea603bdeddbd420082cfe`（= TASK_BASE_SHA，=远端 HEAD，R01 已正式
  封印推送），`git status --short` 为空（干净）。
- tested SHA：`201584f2917a7fd96d6ea603bdeddbd420082cfe` + 本任务未提交改动（§7 逐文件
  列出）。
- 平台：macOS 27.0 arm64（Darwin 27.0.0）。
- 工具链：rustup 锁定 1.98.1（`rust-toolchain.toml`，组件 rustfmt+clippy）；所有 cargo
  命令经 `~/.cargo/bin` rustup 代理调用（PATH 中置于 Homebrew cargo 1.93.0 之前——后者
  不读 toolchain 文件，禁止使用）。
- 构建隔离（RR-T08-F1 落实）：全部 cargo 命令 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t01`
  （本任务专属，不复用 R01 目录或默认 target）+ `CARGO_NET_OFFLINE=true` + `--locked`。
- 网络：本机失效代理（127.0.0.1:7890）已从全部命令环境剥离（`env -u …_proxy`）；
  构建全程离线（依赖已在 R01 锁定并缓存）。git 网络操作：无（本任务无 commit/push）。
- 测试隔离：服务与全部测试只用 `/tmp` 下合成 home（mktemp/进程号唯一目录），未触碰
  真实用户目录（§R02 阶段边界）。

## 3. 观察事实（先读再动手，未混写设计）

1. 开工时 rust workspace 成员 4 个：lingxi-protocol、lingxi-kernel、lingxi-spike、
   lingxi-browser-spike；`DEPENDENCY_RULES.json` 已登记 lingxi-service 为
   `status=planned, establish_stage=R02`（模块注册表第 4 条）。
2. R01 边界检查器 `docs/rust-tauri/R01/r01_t01_check_ownership.py` 的 D5 是双向的：
   planned 模块一旦落盘即违规；workspace 成员未登记也违规。其违规消息本身写明处置是
   「deliberate registry update（re-registered as exists with rules effective）」——
   即建 lingxi-service 必须同步把注册表状态翻为 exists，DEP-07 桌面禁令随即对其生效。
3. DEP-01..07 覆盖桌面栈禁令（kernel/protocol/全 workspace）与 kernel→adapters 反向禁令，
   但**没有**规则禁止 kernel→service 倒挂、kernel 直接依赖 HTTP 客户端/服务器或 SQLite
   ——恰是本任务步骤 1 契约句「domain 不直接访问 HTTP、UI、环境变量或数据库」的机器执法缺口。
4. lingxi-protocol 已导出版本单一事实源：`WIRE_PROTOCOL_MIN/MAX_SUPPORTED`（=1）、
   `WIRE_PROTOCOL_NAME`、`ContractVersions::R00_BASELINE`（data_epoch=1）。
5. 锁表（DEPENDENCY_DECISIONS.md）：axum 0.8.9 / tokio 1.53.1 / serde 1.0.229 /
   serde_json 1.0.151 / tracing 0.1.44 / tracing-subscriber 0.3.23 均已锁定。
6. 根 `.gitignore` 无 `rust/target` 条目（R01 HANDOFF allowed_next_scope 建议项）。
7. 检查器对 kernel 的源码 token 扫描按 token 列表顺序报告首个命中（`AppHandle` 先于
   `tauri::`）——负向脚本断言按实际报告 token 写。
8. `cargo metadata`（无 `--locked`）会就地重写 Cargo.lock；注入 path 依赖后必须还原锁文件
   （A02 脚本已备份/还原并 shasum 校验）。
9. 没有任何 vitest/Node 测试直接调用 r01_t01_check_ownership.py 或 rust workspace
   （grep tests/ scripts/ package.json 零命中）——本任务对 Node 侧回归面为零改动
   （见 §9 未验证说明）。

## 4. 设计决定

1. **只建 lingxi-service 一个新 crate**。本任务实际需要的抽象 = 显式配置 + 组合根路由 +
   健康检查 + 优雅关闭。不建 lingxi-adapters/xtask（无实际消费者，空壳违反步骤 2）；
   不建 lingxi-cli（计划 R07）；不动 tauri-host（计划 R09，独立 manifest）。
2. **lingxi-service 不依赖 lingxi-kernel（暂时）**。kernel 的 ports（RunStore/ModelPort/
   ToolPort/CredentialPort）尚无任何实现可注入（存储 T04、事件 T05、认证 T03）；现在挂上
   kernel 依赖是"假组合"。依赖边由第一个真正注入 port 实现的任务加上。service 当前依赖：
   axum/tokio/serde/serde_json/tracing(+subscriber)/lingxi-protocol——全部复用已锁版本，
   **零新增第三方版本**。
3. **home 必须显式且绝对**：`--home` 缺省即拒启（exit 2），无任何静默默认（红线：禁止
   静默降级/不落真实用户目录）。完整优先级解析（CLI/env/config）归 T02，本任务只钉住
   "显式根"不变量。
4. **就绪契约**：binary 在 stdout 打恰好一行 `LINGXI_SERVICE_READY addr=… home=…`，
   其余日志走 stderr（tracing）。harness 据此确定性等待，不靠 sleep 猜测。
5. **健康检查最小面**：`GET /lingxi/v1/health` 返回
   `{status, serverKind, serverVersion, wireProtocolMin, wireProtocolMax, dataEpoch}`，
   版本字段全部取自 lingxi-protocol 常量（单一事实源，测试锁死）。不回显路径/实例信息
   （T03 收紧端点权限表时沿此面收口）。健康类型放 service 层，不进 lingxi.wire 冻结词表
   ——protocol crate 零改动，schema 生成零漂移。
6. **优雅关闭**：SIGINT/SIGTERM → axum graceful shutdown → exit 0；关闭失败/启动失败
   非零退出并打印结构化错误，不吞错。生产代码（lib+bin）无 unwrap/expect（错误全部
   Result + Display；仅测试代码使用 unwrap/expect/panic 惯例）。
7. **DEP-08（新增规则，扩展而非另起炉灶）**：补上观察事实 3 的执法缺口——kernel 禁依赖
   lingxi-service/axum/reqwest/hyper/rusqlite（传递依赖 + manifest 声明扫描，与 D1 同一
   执行器）。这是把本任务自己的契约句交给既有门禁机器执法的最小增量；registry 同步
   deliberate update：lingxi-service status planned→exists（D5 消息规定的处置方式）。
   std::env 环境变量访问不是依赖边，由 T02 在 service 侧收口（规则 added_by 字段注明）。
8. **A01/A02 验收脚本化**：`scripts/rust-tauri/r02_t01_service_smoke.sh`（真实进程五步链）
   与 `scripts/rust-tauri/r02_t01_boundary_negative.sh`（注入→拦截→还原→复绿→零残留，
   DIRTY-trap 保证脚本自身失败也回滚）——整流程可重跑，证据可复现。
9. `.gitignore` 增补 `rust/target/`（R01 HANDOFF 建议项；门禁惯例仍要求仓库外专属
   target dir）。

## 5. 验收场景：命令 / 预期 / 实际 / 退出码

证据目录 `artifacts/rust-tauri/R02/T01/`（注意：仓库 `.gitignore` 忽略 `*.log`，与 R01
证据惯例一致——日志在本机可查且可由脚本重跑再生；`.txt`/`.json` 摘要可入库）。

### R02-A01｜无桌面启动（REQUIRED）

命令：`env -u …_proxy PATH=~/.cargo/bin:$PATH bash scripts/rust-tauri/r02_t01_service_smoke.sh`
（内部：rustup run 1.98.1 cargo build --locked -p lingxi-service → cargo metadata 桌面扫描
→ 真实进程启动（合成 /tmp home，loopback 临时端口）→ curl 健康检查 → SIGTERM 干净关闭）。

| 步骤 | 预期 | 实际 | 退出码 | 证据 |
|---|---|---|---|---|
| 构建 | 编译通过 | `Finished dev profile`（0.78s 冷链首建；终态复跑 0.05s） | 0 | a01-build.log |
| 依赖树无桌面包 | 解析图 0 命中 | 5 成员/283 包，对 tauri/electron/tao/wry/webkit2gtk/winit/webview 子串扫描 **0 hits**（与 D1 同语义） | 0 | a01-deptree-evidence.{log,txt}、a01-cargo-metadata.json（1.4MB 原始解析图） |
| 启动真实进程 | READY 行+home 创建 | `LINGXI_SERVICE_READY addr=127.0.0.1:55862 home=/tmp/lingxi-r02-t01-smoke-home.dM8M5w`，home 目录存在 | 0 | a01-service-stdout.log、a01-service-stderr.log |
| 健康检查 | 200+最小 JSON | HTTP 200；`{"status":"ok","serverKind":"lingxi-service","serverVersion":"0.0.0","wireProtocolMin":1,"wireProtocolMax":1,"dataEpoch":1}`（逐字段+无多余字段断言） | 0 | a01-health-body.json、a01-health-check.log |
| 干净关闭 | exit 0/无子进程/端口释放 | SIGTERM→exit **0**；`pgrep -P` 无子进程；端口连接拒绝 | 0 | a01-shutdown.{log,txt} |

**判定（执行者结论，供验收）：达成**——服务可用性由真实二进制进程证明，桌面自由由
cargo metadata 解析图（机器证据）证明；冒烟进程树中无 Tauri/Electron/WebView。

### R02-A02｜违反依赖规则失败（REQUIRED，负向）

命令：`env -u …_proxy PATH=~/.cargo/bin:$PATH bash scripts/rust-tauri/r02_t01_boundary_negative.sh`
（门禁 = R01 现行检查器 `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py`，对更新后
的 DEPENDENCY_RULES.json 执法；注入物仅落 domain crate lingxi-kernel）。

| 阶段 | 注入 | 预期 | 实际（本地化消息） | 退出码 | 证据 |
|---|---|---|---|---|---|
| 基线 | 无 | 绿 | `RESULT: OK` | 0 | a02-green-baseline.log |
| N-A 源码级宿主类型 | kernel lib.rs 追加 `pub type HostAppHandle = tauri::AppHandle;` | 拒绝+定位 | `FAIL [D3] DEP-02: forbidden token 'AppHandle' in rust/crates/lingxi-kernel/src/lib.rs` | **1** | a02-na-source-token.log |
| N-A 还原 | 移除注入 | 复绿 | `RESULT: OK` | 0 | a02-green-na-restored.log |
| N-B 依赖边→桌面栈 | kernel Cargo.toml 追加 `tauri = { path = /tmp/fake-tauri }`（真实 path 依赖，进解析图） | 拒绝+定位边 | `FAIL [D1] DEP-02: module lingxi-kernel transitively depends on forbidden ['tauri'] (pattern 'tauri')` | **1** | a02-nb-dep-edge.log |
| N-B 还原 | 还原 Cargo.toml+Cargo.lock | 复绿 | `RESULT: OK` | 0 | a02-green-nb-restored.log |
| N-C 组合根倒挂（本任务新增 DEP-08） | kernel Cargo.toml 追加 `lingxi-service = { path = ../lingxi-service }` | 拒绝+定位边 | `FAIL [D1] DEP-08: module lingxi-kernel transitively depends on forbidden ['lingxi-service']` | **1** | a02-nc-service-inversion.log |
| N-C 还原 | 还原 | 复绿 | `RESULT: OK` | 0 | a02-green-nc-restored.log |
| 残留 | — | 字节一致 | shasum -a 256 -c 三文件全 OK（kernel lib.rs / kernel Cargo.toml / rust/Cargo.lock） | 0 | a02-residue-check.log |

**判定（执行者结论，供验收）：达成**——门禁对三类违规全部非零退出并点名违规 crate 与
依赖边；整个流程脚本化可重跑；负向电池 N1–N15（检查器 --self-test）另证 15/15 全拒绝
（check-ownership-positive-and-selftest.log）。

过程事故（如实记录）：A02 脚本首跑因断言先于还原的顺序缺陷，在 N-A 断言失败退出时把
注入残留在了 kernel lib.rs；以 `git checkout -- rust/crates/lingxi-kernel/src/lib.rs`
还原（该文件当时唯一差异即本脚本自己的注入，未触碰任何用户改动）；随后为脚本加
DIRTY 标记 + EXIT trap 回滚，修复后全流程通过，终态残留检查字节级干净。

### 辅助检查（任务书步骤 3 要求的五类检查）

| 检查 | 命令（均 rustup 1.98.1 + 专属 target dir + --locked） | 结果 | 退出码 | 证据 |
|---|---|---|---|---|
| fmt | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 无 diff | 0 | cargo-fmt-check.log |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 无告警 | 0 | cargo-clippy.log |
| 单测 | `cargo test --workspace --locked` | **57 passed / 0 failed**（lingxi-service lib 8 + 集成 4 新增；kernel 7、protocol 19 等存量回归全绿） | 0 | cargo-test-workspace.log |
| 依赖边界 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py --self-test` | O1–O8/D1–D5 全 PASS；DEP-01..08 逐模块 PASS（lingxi-service 首次出现在 DEP-07 执法名单）；N1–N15 全 REJECTED | 0 | check-ownership-positive-and-selftest.log |
| schema 生成 | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 56 生成文件零漂移；API_COMPAT_MATRIX 624 项零漂移 | 0 | r01-t02-check-generated.log |

## 6. 关键生产调用链（binary → 健康检查实现）

```text
rust/crates/lingxi-service/src/main.rs        # 入口：tracing(stderr)→CLI 解析→--home 校验
  └ ServiceConfig::from_cli_args()            # lib.rs：--bind(默认 127.0.0.1:0)/--home(必填,绝对)；非法即 exit 2
  └ ServiceConfig::prepare_data_home()        # lib.rs：缺失则创建；存在非目录/创建失败即 exit 2（不静默换根）
  └ lingxi_service::run(config, shutdown_signal(), on_ready)
      ├ tokio::net::TcpListener::bind         # loopback 临时端口
      ├ on_ready(addr)                        # stdout 恰一行 LINGXI_SERVICE_READY（harness 契约）
      ├ build_router(ServiceState::new(config))  # 组合根：状态注入点（port 实现由 T03/T04/T05 注入此结构）
      │   └ GET /lingxi/v1/health → health_payload()
      │       ├ WIRE_PROTOCOL_MIN/MAX_SUPPORTED   # lingxi-protocol/src/handshake.rs（单一事实源）
      │       └ ContractVersions::R00_BASELINE.data_epoch  # lingxi-protocol/src/lib.rs
      └ axum::serve(...).with_graceful_shutdown(shutdown_signal)  # SIGINT/SIGTERM→drain→exit 0
```

## 7. 改动清单

**新增（生产/测试）**
- `rust/crates/lingxi-service/Cargo.toml`（依赖全部复用锁表版本；注释说明为何暂不依赖
  lingxi-kernel/reqwest/rusqlite）
- `rust/crates/lingxi-service/src/lib.rs`（组合根：ServiceConfig/ConfigError/
  HealthResponse/ServiceState/build_router/run + 8 单测）
- `rust/crates/lingxi-service/src/main.rs`（binary：CLI/就绪行/优雅关闭；--help/--version）
- `rust/crates/lingxi-service/tests/service_health.rs`（受控 harness：真实 loopback 服务 +
  手写最小 HTTP/1.1 客户端（零新增 HTTP 客户端依赖）+ oneshot 就绪/停机通道，4 集成测试）
- `scripts/rust-tauri/r02_t01_service_smoke.sh`（A01 可重跑验收）
- `scripts/rust-tauri/r02_t01_boundary_negative.sh`（A02 可重跑负向验收）
- `docs/rust-tauri/R02/`（本报告；目录随本任务建立）
- `artifacts/rust-tauri/R02/T01/`（§5 证据；.txt/.json 可入库，.log 本机留存）

**修改**
- `rust/Cargo.toml`（members += lingxi-service；头注释更新执法说明）
- `rust/Cargo.lock`（**+34 行 0 删**：新增 lingxi-service 成员条目；新增 2 个传递依赖条目
  signal-hook-registry 1.4.8、errno 0.3.14——由 tokio 1.53.1 既有 `signal` feature 激活
  （优雅关闭必需），非新增第三方直接依赖；**既有条目版本零变化**。首次注册成员按惯例
  需解锁重写一次 lock，此后全部命令 --locked）
- `docs/rust-tauri/R01/DEPENDENCY_RULES.json`（lingxi-service status planned→exists + 
  established_by 记录 deliberate registry update；新增 DEP-08 kernel-no-infrastructure
  [lingxi-service/axum/reqwest/hyper/rusqlite]，effective_from_stage R02-T01）
- `.gitignore`（构建产物区增补 `rust/target/`）

**未触碰**：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/`、`contracts/generated/`、
`.sync-audit/`、`PROGRESS.md`、`ORCHESTRATOR_PROGRESS.json`、`package.json`、
`desktop/`、`server/`、`core/`、`lib/`、`shared/` 及其余 R01 交付（OWNERSHIP_TARGET.json
未改——生成器只读 R00 清单，O7 漂移检查绿）。

## 8. 测试列表（含层级归属，01 §5 分层）

| 测试 | 层级 | 断言要点 |
|---|---|---|
| lib: config_requires_explicit_home / rejects_relative_home / rejects_unknown_and_missing_values / parses_bind_and_home / defaults_to_loopback_ephemeral / rejects_garbage_bind | 纯逻辑 | 显式 home 契约、绝对路径、非法参数全部显式拒绝 |
| lib: health_response_sources_versions_from_protocol | 纯逻辑 | 版本单一事实源锚定（=1/=1/=1） |
| lib: health_response_serializes_minimal_camel_case | 纯逻辑 | 健康响应恰为 6 字段最小面 |
| tests: health_check_over_real_loopback | 契约/服务集成 | 真实 axum 服务+真实 TCP：200+精确 JSON；停机后端口拒绝连接 |
| tests: unknown_route_is_not_found | 契约/服务集成 | 路由面收敛（404） |
| tests: prepare_data_home_creates_and_is_idempotent / rejects_file_as_home | 纯逻辑（真实文件系统，合成 /tmp） | 数据根创建幂等、非目录显式失败 |
| scripts/rust_t01_service_smoke.sh | 真实二进制进程 | A01 全链（构建→启动→健康→SIGTERM exit 0→无子进程→端口释放） |
| scripts/r02_t01_boundary_negative.sh | 负向门禁 | A02 三注入三拦截三复绿+零残留 |
| 检查器 --self-test N1–N15 | 负向门禁（存量） | 15 类绕过全部按预期拒绝 |

## 9. 未验证 / 风险 / 交接

**未验证（如实）**
1. release（--release）构建未跑：A01 冒烟用 debug 二进制（功能可用性证据；性能协议的
   release 测量归 R10/T08 交付门禁）。
2. 非 macOS 平台未测（Windows/Linux：main.rs 的 SIGTERM 分支 cfg(unix) 已留非 unix 回退
   但未编译验证）。
3. Node/现役入口回归未跑：本任务零 Node 侧改动且无测试引用 rust workspace（观察事实 9），
   按 05 §2「未修改且无影响输入不无理由重复」不重跑全量 npm test；R02-A16 旧入口回归
   属 T08。
4. WS/认证/存储/事件能力不存在（属 T03–T06），健康检查即本任务全部对外面。
5. 长时运行/资源增长未测（R10）。

**已知风险**
1. DEP-08 是本任务对冻结契约的**增量**修改（registry 状态翻转 + 新规则）：沿 D5 消息
   预设的 deliberate update 路径，非推翻既有规则；若验收认为越权，回退仅涉及
   DEPENDENCY_RULES.json 两处编辑（crate 与门禁已按新规则全绿）。
2. 健康端点当前无认证（loopback + 最小信息面）。T03 契约「不能因为是 127.0.0.1 就无认证」
   尚未适用——health 是否豁免/如何授权由 T03 端点权限表定夺，本任务不在协议冻结词表中
   加字段，避免抢占 T03 决策。
3. `LINGXI_SERVICE_READY` stdout 行含 home 路径：合成 home 无敏感信息；真实部署下的路径
   泄露面由 T07 日志脱敏统一治理。
4. Cargo.lock 新增 2 个传递条目（signal-hook-registry/errno）来自 tokio signal feature：
   若后续阶段不用 signal 可移除，当前为优雅关闭（A01 通过条件）所必需。

**交接（给 R02-T02 起的实现任务）**
- `ServiceConfig{bind_addr, data_home}` 是配置的唯一载体：T02 在此扩展 --home/env/config
  优先级解析与实例记录/单写者锁；`prepare_data_home` 是目录规范化/权限检查的挂点。
- `ServiceState` 是组合根注入点：T03 认证服务、T04 StoragePort、T05 EventPort 的实现
  注入该结构（届时把 lingxi-kernel 依赖边加上，DEP-03/DEP-08 已保证方向合法）。
- 就绪契约（stdout 单行）/优雅关闭/退出码语义请后续任务保持（harness 与 A15 全链依赖）。
- 边界门禁新增 DEP-08；A02 负向脚本可作为 T08 verify-stage 的登记命令候选。
- RR-T08-F1 已按 HANDOFF 要求落实（专属 target dir）；RR-T07-PROD-DEFECT-1 与
  RR-T02-FINFO1 未在本任务处理（归属后续 T），本任务设计未与其冲突。

## 10. 最终工作树状态

见本报告同日 git status（§7 改动清单即全部差异；无其他文件被动改动）。
