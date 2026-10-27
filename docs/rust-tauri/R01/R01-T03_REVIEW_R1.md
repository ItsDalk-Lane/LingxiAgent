# R01-T03 REVIEW R1 — 锁定依赖与构建原型（独立对抗性验收）

验收代理：ZCode:R01-T03-review-r1（未参与执行；只读审查 + /tmp 隔离复跑；未修改任何已提交文件与执行者交付物）
日期：2026-09-25｜分支 codex/rust-tauri-migration｜基线 HEAD `9705fe0fbaa4c8898dbede0a62a5c97465ca64e3`（实测 `git rev-parse HEAD` 一致）
被审交付：执行者 ZCode:R01-T03-exec-r1 的工作区差异 + `docs/rust-tauri/R01/R01-T03_REPORT.md` + `artifacts/rust-tauri/R01/T03/`

**最终判定：PASS**（一处预存发现 F1 归属 T02 交付物，不阻塞本任务，须由总控派单收口）

---

## 1. 候选清单与复算哈希

`git status --porcelain` 与 `git diff --stat HEAD` 实测与工作区声明一致：候选 = rust-toolchain.toml（新）、rust/（Cargo.toml、Cargo.lock、crates/lingxi-spike/ 新、crates/lingxi-protocol/ 格式化改写）、docs/rust-tauri/R01/{DEPENDENCY_DECISIONS.md（新）、PLATFORM_BUILD_MATRIX.json（新）、R01-T03_REPORT.md（新）、DEPENDENCY_RULES.json（改）}、artifacts/rust-tauri/R01/T03/（新）。`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 为既存总控账本修改（diff 实测仅账本字段：T02 push_result + T03 task_base_sha），非候选，本验收未触碰。

哈希复算（验收代理独立执行，非转述）：

| 项 | 方法 | 结果 |
|---|---|---|
| rust/Cargo.lock | `shasum -a 256` | `480cb513de317235cf11de31af6db26a9b1e937effbcd521a077c7eddd1f3560`，与执行者声明逐字符一致 |
| 全部 48 项交付物+证据 | 仓库根 `shasum -a 256 -c artifacts/rust-tauri/R01/T03/SHASUMS.txt` | 48 OK / 0 FAILED（仅 4 行注释头被 shasum 警告知跳过） |
| 锁表 18 个关键版本 | 解析 Cargo.lock `[[package]]` 比对执行者锁表 | 全部一致（tokio 1.53.1 / axum 0.8.9 / reqwest 0.13.5 / rustls 0.23.45 / aws-lc-rs 1.18.1 / rustls-platform-verifier 0.7.1 / rusqlite 0.40.2 / libsqlite3-sys 0.38.2 / tracing 0.1.44 / tracing-subscriber 0.3.23 / jsonschema 0.57.0 / rmcp 3.4.1 / serde 1.0.229 / serde_json 1.0.151 / schemars 1.2.2 / sha1 0.11.0 / sha2 0.11.0 / base64 0.23.1）；锁文件 279 个包条目（272 个唯一名，含平台条件条目） |

## 2. 逐场景独立复跑

证据分层标注：**[实跑]** = 本验收代理亲自执行；**[源码]** = 源码审查确证；**[环境]** = 环境限制说明。

### R01-A05 锁文件可复现 — PASS [实跑]

独立环境：`CARGO_HOME=/tmp/r01t03-review-a05/cargo-home`（全新）、`CARGO_TARGET_DIR=/tmp/r01t03-review-a05/target`（全新）、联网命令剥离六个代理变量、经 rustup 代理的 cargo 1.98.1。

| 步骤 | 命令（要点） | 退出码 | 结果 |
|---|---|---|---|
| 构建前锁哈希 | `shasum -a 256 rust/Cargo.lock` | — | 480cb513…f3560 |
| fetch | `cargo fetch --locked --manifest-path rust/Cargo.toml` | 0 | 全部 279 包下载自 crates.io |
| fetch 后锁哈希 | 同上 | — | 480cb513…f3560（不变） |
| 离线构建 | `cargo build --workspace --locked --offline` | 0 | 16.47s 全量编译（含 aws-lc-sys C 构建），三 crate 产物齐 |
| 构建后锁哈希 | 同上 | — | 480cb513…f3560（不变） |

构建日志 `/tmp/r01t03-review-a05.log`。构建中观察到的实际编译版本与执行者锁表逐项一致（见 §1 表）。构建目标 aarch64-apple-darwin（本机）。

### R01-A06 依赖缺失不掩盖 — PASS [实跑，独立设计]

本验收未重复执行者的 rmcp 删除，改用另一依赖 **libsqlite3-sys 0.38.2**（bundled SQLite 引擎本体，属"必需运行库"语义）：

- 隔离副本 `/tmp/r01t03-review-a06/work`（rust/ + rust-toolchain.toml），全新 `CARGO_HOME=/tmp/r01t03-review-a06/cargo-home`、`CARGO_TARGET_DIR=/tmp/r01t03-review-a06/target`——开发机全局 `~/.cargo` registry 物理上不在搜索路径，不存在掩盖通道。
- fetch 补齐后删除 `registry/src/*/libsqlite3-sys-0.38.2` 与 `registry/cache/*/libsqlite3-sys-0.38.2.crate`。
- 缺失侧：`cargo run --locked --offline -p lingxi-spike --bin spike_check_deps` → **exit 101**，明确报 `failed to download 'libsqlite3-sys v0.38.2'` + `attempting to make an HTTP request, but --offline was specified`。缺失被点名、无静默降级。
- 对照侧：重新 fetch 后同一命令 → **exit 0**，`SPIKE_CHECK_DEPS_OK`，JSON 报告 sqlite 3.53.2 bundled (static)、tls rustls+platform-verifier、动态链接库仅 Security/SystemConfiguration/CoreFoundation/libiconv/libSystem（均 dyld 缓存），无系统 libsqlite3——独立证实 bundled 声明。

补充 [源码]：spike_check_deps 的 exit 3 MISSING_DEPENDENCY 分支（逐条校验 otool -L 枚举的动态库存在性）为真实代码；该分支在 macOS 上无法用系统库实跑触发（dyld 会在进程启动前失败），属源码确证+替代路径实跑，spike 阶段可接受，四平台门禁时归 R09/R10 实测。

### 工具链锁定真实性 — PASS [实跑]

- rustup 1.29.1 (d95a37b6a 2026-08-13) 真实存在于 `~/.cargo/bin/rustup`；`~/.rustup/toolchains/` 含 `1.98.1-aarch64-apple-darwin` 与 `stable-aarch64-apple-darwin`。
- 仓库根：`rustup show active-toolchain -v` → `1.98.1-aarch64-apple-darwin`，**active because: overridden by rust-toolchain.toml**；`rustc --version` = 1.98.1 (48a229cea 2026-09-01)。/tmp（无 toml）下解析为 default stable（同版本但解析路径不同，证明 toml 真正生效而非碰巧）。
- 对照：Homebrew cargo/rustc 1.93.0（`/opt/homebrew/bin`）不读 rust-toolchain.toml，本机默认 PATH 中 Homebrew 优先——即"锁定仅对经 rustup 代理的调用生效"声明属实；该注意事项已写入 rust-toolchain.toml 注释与 DEPENDENCY_DECISIONS.md D-01。
- 核心/桌面同一固定工具链的约定：02 §2 布局要求写明在 rust-toolchain.toml 头注释（"核心与桌面必须使用同一固定 stable 工具链"；desktop/src-tauri 未来在仓库根之下自动继承）+ D-01 选择理由。约定已写明。

### T01/T02 门禁回归 [实跑]

| 门禁 | 命令 | 退出码 | 结论 |
|---|---|---|---|
| T01 正向 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | PASS（D5/D5-reverse 含 lingxi-spike 注册后仍通过） |
| T01 负向 N1–N15 | `... --self-test` | 0 | 15 条弹药全部按预期被拒（逐条 PASS-NEG 输出核对） |
| T01 生成器 | `r01_t01_build_ownership.py --check` | 0 | OWNERSHIP_TARGET_UP_TO_DATE features=736 stores=69 |
| cargo test | `cargo test --workspace --locked --offline`（锁定工具链、独立 CARGO_HOME/target） | 0 | kernel 7 + protocol 19 + spike 7 全过，0 ignored |
| fmt | `cargo fmt --all -- --check`（1.98.1） | 0 | 干净 |
| clippy | `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 | 干净 |
| T02 check-generated | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | **1** | [1/2] 56 生成文件零 diff；[2/2] 仅 headSha 一行漂移——见 F1 |
| T02 roundtrip | `r01-t02-roundtrip.sh` | 0 | 12 golden Rust→TS→Rust 字节一致 + tsc --noEmit + 负向 |
| T02 handshake | `r01-t02-handshake.sh` | 0 | HTTP/WS 协商、版本不兼容诊断（400/4409 带 details）真实发生 |

### 原型真实运行（本验收从自己的 A05 干净构建产物直接执行）[实跑]

| 原型 | 退出码 | 实测输出要点 |
|---|---|---|
| spike_health | 0 | SPIKE_HEALTH_OK hello=200 health=200（axum 服务 + reqwest 回环自检） |
| spike_sqlite_queue | 0 | SPIKE_SQLITE_QUEUE_OK sqlite=3.53.2 journal_mode=wal jobs=201 capacity=8 |
| spike_mcp_handshake | 0 | SPIKE_MCP_OK negotiated_protocol=2025-11-25 server=lingxi-spike-mcp list_tools 成功 |
| spike_schema_validate | 0 | 正向 valid；负向 protocolMin 篡改被拒（"one" is not of type "integer"） |
| spike_tls_probe | 0 | SPIKE_TLS_OK HTTPS 200 verification=enabled backend=rustls-platform-verifier |
| spike_check_deps | 0 | SPIKE_CHECK_DEPS_OK（见 A06 对照侧） |

### 安全与许可 [实跑]

- `cargo audit --file rust/Cargo.lock`（cargo-audit 0.22.2，RustSec advisory-db 1269 条，联网实取）：**exit 0**，279 crate 扫描无漏洞无警告。
- 许可独立复扫（`cargo metadata --locked --offline` 全树 license 字段）：全部宽松许可；唯一含 GPL 字样条目为 `MIT OR Apache-2.0 OR LGPL-2.1-or-later`（r-efi ×2，可选宽松支，无 copyleft 义务）；CDLA-Permissive-2.0 ×1（webpki-root-certs，数据许可）；无 GPL/AGPL/SSPL 强制义务项；UNLICENSED ×3 为 workspace 自有 crate（publish=false）。与执行者盘点一致。

### 生产零改动 [实跑]

`git diff HEAD --stat -- desktop server core lib shared cli hub plugins skills2set package.json package-lock.json` 为空；对应路径 `git status --porcelain` 为空。Node/npm 锁文件未动。

## 3. 源码审查结论

**lingxi-spike 非占位 [源码]**：queue.rs（400 行）为真实有界单写者队列——专用 std::thread 独占 rusqlite Connection、tokio mpsc 有界通道、try_submit 满时显式 `QueueError::Full`（屏障驻留测试确定性触发）、PRAGMA journal_mode=WAL 且测试断言实测值、init 失败在 open 时显式上报；mcp_handshake 用官方 rmcp `ServiceExt` 双端真实 initialize 协商并校验 server 身份；tls_probe 默认 ClientBuilder（校验开启）；check_deps 真枚举 otool -L。全 workspace 搜索 `dangerously|insecure|no_verify|danger_accept` 零命中——TLS 校验未关闭。锁树中 tauri/electron/tao/wry/webkit2gtk/gtk/webview2-com/openssl-sys/native-tls 全部 absent（grep 确证）。

**lingxi-protocol 改写行为不变 [源码+实跑]**：逐行审查全部 7 文件 diff——10 处纯换行重排；4 处等价改写逐一核对：`map_or(false, f)`→`is_some_and(f)` ×3（语义等价）、`parsed <= (1u64<<53)-1`→`parsed < (1u64<<53)`（u64 下 2^53 精确可表示，两式等价）、proto-server 中双分支同返 400 的死等价 if 合并（行为等价且消除误导）。行为不变另有实跑佐证：protocol 19 测试、roundtrip 字节一致、check-generated [1/2] 56 文件零 diff、handshake 转录一致。

**DEPENDENCY_RULES.json 改动诚实 [源码+实跑]**：diff 仅两处纯增量——新增 lingxi-spike 模块条目（kind=prototype，声明不拥有业务事实）+ DEP-07 applies_to 追加 lingxi-spike（收紧而非放宽）；无既有规则删改。N13 负向弹药证明未注册成员必被 D5 拒绝，故注册是合规必需动作。

**四平台矩阵 [源码]**：macOS arm64 VERIFIED（逐项有本机实测证据引用）；macOS x64 / Windows x64 / Linux x64 均 UNVERIFIED + owner（R09/R10 平台验收负责人，总控指派）+ verify_by_stage（R09 最迟 R10）。TLS 根证书（Security framework/Schannel/openssl-probe 探测+webpki 回退）、代理（system-proxy 各平台机制）、交叉编译（x86_64-apple-darwin 可交叉、windows-msvc 需 Windows 构建机、musl/glibc 下限未定）、动态链接（vcruntime/glibc/musl 决策点）分析均实质具体；PR-WIN-01（aws-lc-sys/NASM）、PR-LNX-01（精简容器根证书回退未实测）、PR-LNX-02（glibc 下限）已登记。符合 01 §6"未关闭交接风险+明确后续关卡"规则。

## 4. 发现问题

### F1（Medium-Low，预存，归属 T02 交付物；不阻塞本任务）

**现象**：`r01-t02-check-generated.sh` exit 1；完整再生 diff 仅 `API_COMPAT_MATRIX.json` 第 6 行 `generatedFrom.headSha`（盘 a98d2487… vs 当前 HEAD 9705fe0f…），624 条目与 sourceDigests 逐项一致。

**最小重现（本验收实测）**：`git worktree add /tmp/r01t03-review-baseline 9705fe0f`（纯净基线、零 T03 改动）→ `node scripts/rust-tauri/r01-t02-extract-api-surface.mjs --check` → exit 1，同一行漂移。证明该失败在基线 HEAD 即存在，非本任务引入。

**根因**：`scripts/rust-tauri/r01-t02-extract-api-surface.mjs:367-369,388` 把 `git rev-parse HEAD`（易变值）嵌入**已提交的**生成物。该戳只在"生成后、提交前"的窗口内与盘一致——T02 的 PASS 记录正是在该窗口取得；任何后续提交（包括记录该 PASS 的账本提交 9705fe0f 本身）都使 --check 变红。**定性：(a) T02 生成器设计缺陷**——"重新生成无 diff"契约在任何静止已提交 HEAD 上结构性不可满足，不是可接受的基线戳设计（若是 (b)，门禁应在静止态为绿）。

**严重度**：Medium-Low。实质内容（API 面 624 条目 + 源文件摘要）完整且已验证无漂移，兼容性保护功能未失效；但门禁静止态恒红会造成告警疲劳、诱使后续执行者"顺手重生成"掩盖真实漂移，属于必须收口的门禁健康缺陷。

**归属与修复范围**（同根因完整覆盖）：归 T02 交付物（extract-api-surface.mjs + 生成物 + 门禁脚本），建议由总控派 T02 修复任务或并入 R01 阶段收口（T08）。修复三选一（需保持防漂移能力不下降）：① --check 比较时规范化/豁免 `generatedFrom.headSha` 字段（最小改动）；② 戳改为提取源内容摘要的派生值而非 HEAD；③ 明确规定"提交后重戳"步骤并自动化。配套更新 T02 报告/验收记录中对 exit=0 的窗口限定说明。**本任务执行者处置正确**：未代为改写、未静默通过、如实披露并给出根因——符合"审阅/事实收集不自动转修复"与禁止掩盖的红线。

### F2（Info，门禁稳健性备注）

check-generated.sh 调裸 `cargo`；在本机默认 PATH（Homebrew 1.93.0 优先）下 [1/2] 会跑在非锁定工具链上。当前生成器直写文本不经 rustfmt，故今日无害；若未来生成链引入 rustfmt 敏感步骤，工具链漂移会破坏该门禁。建议门禁脚本显式解析锁定工具链（如 `rustup run`）。归 T02/T08 门禁加固，与 F1 可同单处理。

### F3（Info，环境限制如实声明）

spike_check_deps 的"动态库缺失→exit 3"分支在 macOS 无法实跑触发（dyld 先于 main 失败）；已由源码确证分支真实存在、由 A06 的 cargo 级缺失诊断实跑覆盖"明确报缺失"语义。四平台门禁（R09/R10）应对 Linux ldd 路径做实跑触发。

## 5. 执行者声明核对总表

| 声明 | 核对结果 |
|---|---|
| rust-toolchain.toml 1.98.1 被 cargo 实际遵守 | 属实（override 解析实测） |
| 锁表 18 版本 + 279 包 | 属实（Cargo.lock 逐项复算） |
| A05 干净 CARGO_HOME fetch/build --locked --offline，锁哈希不变 | 独立复跑属实（哈希 480cb513… 三时点一致） |
| A06 缺 rmcp exit 101 + 对照 OK | 语义独立复跑属实（本验收改删 libsqlite3-sys，exit 101/0） |
| 六原型实跑标记 | 全部由本验收从自建产物独立复跑复现 |
| T01/T02 回归全绿，唯一例外 check-generated | 属实；例外定性为 T02 预存缺陷（F1），非本任务引入、未被掩盖 |
| cargo audit 0 漏洞 0 警告、许可无 GPL/AGPL/SSPL | 独立复扫属实 |
| 矩阵仅 macOS arm64 VERIFIED、余 UNVERIFIED+关卡 | 属实且符合任务书交接规则 |
| 生产零改动 | 实测属实 |
| 环境变更（rustup/cargo-audit 安装） | 属实且在任务授权范围内 |

## 6. 判定

**PASS**。R01-A05、R01-A06 两个 REQUIRED 场景经独立设计复跑通过；锁定真实可验证；原型非占位；T01/T02 门禁除 F1 外全绿且 F1 已定性为预存 T02 缺陷、归属清晰；无 TLS 校验关闭、无生产改动、证据链完整可复算（SHASUMS 48/48）。

后续强制项（不属本任务阻塞）：F1/F2 派单 T02/T08 收口；F3 与 UNVERIFIED 平台项归 R09/R10 关卡。
