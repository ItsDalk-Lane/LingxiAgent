# R04-T06 报告｜跨平台沙盒与逃逸防护

执行代理：EXECUTOR-R04-T06-E01（一次性执行代理）。
日期：2026-09-30。分支 `codex/rust-tauri-migration`，基线 `84e6499fe`（R04-T05 已独立
PASS 并推送确认）。工作区起点仅含本派单文件（未跟踪，非用户已提交修改，未触碰）。
状态：**READY_FOR_REVIEW**（普通逐项 + 对抗性两层自查完成；不自行判独立 PASS）。

---

## 1. 目标与结论

从现役沙盒提取实际读写/网络/环境/子进程边界，在 Rust 中建立 `SandboxPort`（沙盒策略
接口）、平台策略矩阵与 helper 版本/可信验证（任务书 R04-T06），复用系统隔离机制而不
重造：**macOS 现役机制经真实调用链确认是 Seatbelt**（`lib/sandbox/seatbelt.ts` →
`sandbox-exec -f <SBPL profile> /bin/bash <script>`，helper 为系统 `/usr/bin/sandbox-exec`，
可用性检查 `which sandbox-exec`）；**Linux 现役是 bubblewrap**（`bwrap.ts` allowlist
挂载）；**Windows 现役是 restricted-token helper**（`win32-sandbox-helper.ts` 的
`lingxi-win-sandbox.exe` + `win32-exec.ts` 调用面）——以源码为准，未猜测。

结论：全部当前到期义务已实现并在**本机 macOS（darwin 27.0.0 arm64）真实测得**——
A11（缺沙盒不裸跑）三腿、A12（隔离保证实际成立）四腿与全部追加对抗项通过
（`tests/r04_t06_sandbox.rs` 10/10 + `sandbox.rs` 内嵌单测 7/7）；实现前先以 6 组真实
探针钉死 sandbox-exec 语义（realpath 必需/链接改向按真实目标拒/网络 allow-deny/
原地 exec 保 pid/组杀可达孙进程/helper 无 --version），见
`artifacts/rust-tauri/R04/T06-E01/probes/PRE_IMPLEMENTATION_PROBES.md`。
fmt/clippy/双门禁/T01-T05 套件/R03 十套件+A15+A16 回归全绿（§5 退出码在案）。

## 2. 实现摘要（关键文件:行）

### 2.1 SandboxPort + 平台策略矩阵 + helper 验证（核心交付）

- `rust/crates/lingxi-service/src/sandbox.rs`（新文件，1355 行含 7 单测）：
  - **策略（现役逐字移植）**：`BLOCKED_FILES/BLOCKED_DIRS/READ_ONLY_*/READ_WRITE_*`
    常量（L95-128，对应 `policy.ts`）+ `SandboxPolicy::derive`（L209，对应
    `deriveSandboxPolicy`：可写=agent 读写目录+home 读写目录+workspace roots+runtime
    writable；只读、拒读（auth.json 等）、写保护（.git/session-files）集；network
    立场）。无效配置（空 home/空 roots）= 响亮 `sandbox_config_invalid`，绝不猜。
  - **SandboxPort**（L520 trait）：`backend()/capabilities()/wrap()/helper_trust()`；
    `SandboxCapabilities`（L494）为冻结矩阵的代码权威（read/write/network/env/
    subprocess/verification 六维，镜像进 PLATFORM_CAPABILITIES v2）。
  - **Seatbelt 后端**（L540 `SeatbeltSandbox`）：`build_profile`（L579 逐字移植
    `generateProfile`——deny 行严格在 allow 行之后（SBPL last-match-wins）、
    `/dev/ttys` 正则与 pseudo-tty、网络 `(allow network-outbound)`/`(deny network*)`；
    路径经 `realpath_or_lexical`（L197——macOS `/tmp`→`/private/tmp` 实测必需）嵌入；
    `profile_literal`（L657）**拒绝**含 `"`/`\`/NUL 的路径=策略注入防护（现役缺口
    已闭合））。包裹形态 `[sandbox-exec, "-p", profile, "--", argv…]`（L735；现役
    `-f` 临时文件改为 `-p` 内联——同一 helper 机制，消临时 profile 文件的 TOCTOU
    与清理面，登记为映射决策）。
  - **helper 版本/可信验证（已选方案）**（L396 `verify_helper_metadata`）：
    **pinned 绝对路径**（`/usr/bin/sandbox-exec`/`/usr/bin/bwrap`——非 PATH 查找，
    关闭现役 `which` 的 PATH 欺骗面）+ 常规文件 + 期望 basename（防指向 `/usr/bin/env`
    类可执行）+ **owner uid 0** + 非组/全局可写 + 可执行位；seatbelt 无 `--version`
    （实测 `illegal option`）——**双腿强制探针**（L674：允许腿 `/usr/bin/true` 在
    containment core profile 下 exit 0 + 拒绝腿同 profile 下写 `/dev/null` 必须
    失败）即为版本/行为验证；bwrap 另加 `--version` 解析（L942
    `bubblewrap x.y.z`）。构造期与**每次 wrap 双层验证**（L711 每次 wrap 重新核对
    元数据——中途换 helper 被拒，A11 腿 2 实测）。
  - **Bwrap 后端**（L795，源码级适配）：`build_args`（L832 逐字移植
    `buildBwrapArgs`：ro-bind / / 基座+dev/proc/tmpfs+unshare-pid+new-session+
    die-with-parent+contained 时 unshare-net+私有运行时 env+cwd bind/chdir+可写根
    遮蔽+protected/readable ro-bind（仅存在路径，同现役 existingPaths）+deny-read
    tmpfs 或 /dev/null 遮蔽+home 缓存 tmpfs 掩蔽）。真机 Linux 未验证，如实登记。
  - **Unsupported 后端**（L1039）：`wrap` 一律 `sandbox_policy_unsupported` 拒绝——
    Windows（restricted-token helper 未移植）与未知平台的诚实形态：绝不裸跑。
  - `detect_sandbox_backend`（L1107）+ `platform_sandbox`（L1122，cfg 分派）。
  - **失败关闭**：`SandboxRefusal`（L289）五稳定码
    `sandbox_{config_invalid,helper_missing,helper_untrusted,policy_unsupported,
    profile_path_unsafe}`——不存在任何返回未包裹命令的路径。

### 2.2 exec 链集成（核心交付：隔离执行路径接入）

- `rust/crates/lingxi-service/src/exectools.rs`：
  - `ProcessTools` 增 `sandbox: Option<Arc<dyn SandboxPort>>`（L543）+ `with_sandbox`
    （L561）；`register_process_tools` 增 `sandbox` 参数（L1024——组合面注入，生产
    默认 bootstrap 不调用，同 T04/T05 口径）。
  - **一次性执行隔离路径**（L631）：authorize_cwd（T04）之后、spawn 之前，argv 经
    `port.wrap(Contained)` 包裹——**约束交集**：T04 cwd 资源授权（prepare+执行器
    重验）∩ T05 环境白名单（唯一 env 通道，沙盒不新增透传）∩ T06 OS 沙盒；三层
    各自拒绝均零派发零副作用，无独立兜底放行。拒绝=
    `EXEC_SANDBOX_REFUSED: <refusal>`（L660，Forbidden），稳定码可诊断。
  - **诚实标注**（L813）：`sandbox: seatbelt (contained; … network denied)` 仅在
    argv 真实经过沙盒包裹时出现；未绑定沙盒（T05 形态）或 tty 终端永不出现——
    不谎报「已在沙盒中」。被沙盒拒绝的操作输出附现役 `sandbox.writeRestricted`
    等义提示（L818）。
  - **tty 终端**按现役语义（`sandboxed = !tty && …`）不套 OS 沙盒（授权+环境白名单
    仍生效）——登记进冻结矩阵并被对抗测试钉住。
- `rust/crates/lingxi-service/src/lib.rs`：`pub mod sandbox` + 再导出（L48/L135-140）。
- `rust/crates/lingxi-service/tests/r04_t05_process_tools.rs`：`register_process_tools`
  调用点等价补 `None` 参数（3 行注释+1 参数；全部断言原样，14/14 复绿）。

### 2.3 平台能力矩阵冻结（核心交付）

`docs/rust-tauri/R04/R04_PLATFORM_CAPABILITIES.json` 升 v2（T06 冻结）：
macos-arm64 全维真机实测指针；macos-x64 同源码未真机；windows-x64 后端=拒绝
（restricted-token 未移植，递延 R03-WINDOWS-R09-R10）；linux-x64 bwrap 源码级
（NOT machine-verified 如实标注）。

### 2.4 依赖与 TS/Node 面

**零新增 crate**（全 std+libc 既有边）；`rust/Cargo.lock`/`Cargo.toml` 零变化
（git diff 为空，已核）。现役 TS 栈未触碰（仅语义参照）；npm 侧按「如触碰」条件不适用。

## 3. 调用链与修改范围

真实调用链：provider `ToolRequests` → 驱动 digest 门（T01）→ **网关 prepare**
（目标/schema/规范化 → cwd 资源规范化（T04，越权=零派发）→ T03 策略裁决 → prepared
绑定 resources）→ journal intent → RunGrant 授权（未动）→ 批准面（T03）→ journal
authorized/started → **网关 execute_prepared**（身份/单次/注册表复验 → 资源重推导
复验 → 真实执行器：**authorize_cwd → sandbox.wrap(Contained)（新）→ 失败=
Failed 收据零 spawn 零副作用；成功 → supervisor.spawn（setsid+getpgid 核证，
sandbox-exec 原地 exec 保组）→ wait/timeout → 输出/截断/spill + 沙盒行标注**）→
fence→收据。取消树经 T05 guard → killpg → **穿 wrapper 达孙进程**（A12 树测试实测）。

修改文件清单：
- 新增：`rust/crates/lingxi-service/src/sandbox.rs`（+7 单测）、
  `tests/r04_t06_sandbox.rs`（10 验收/对抗测试）、
  `artifacts/rust-tauri/R04/T06-E01/**`（证据+复跑脚本+探针记录）、本报告。
- 修改（产品）：`exectools.rs`（包裹+标注+失败关闭+参数）、`lib.rs`（模块+再导出）。
- 修改（测试适配，断言不降级）：`tests/r04_t05_process_tools.rs`（1 处调用点补
  `None` 参数）。
- 修改（文档）：`R04_PLATFORM_CAPABILITIES.json`（v2 冻结）、`R04_TEST_MAP.json`
  （T06 条目）。
- 未触碰：kernel crate、协议 wire 面（check-contracts 626 entries 零漂移）、
  toolgateway/approval_service/runs（T02-T04 冻结面零改动）、生产默认入口、
  xtask stage maps、check 脚本、`ORCHESTRATOR_PROGRESS.json`、现役 TS 沙盒。

## 4. 验收场景与逐项结果（真实链：bootstrap_with_deps 组合根→真实驱动 journal→
真实注册表→真实网关→真实 ApprovalService→真实 supervisor→**真实 seatbelt 沙盒**；
测试根在 cargo CARGO_TARGET_TMPDIR 下（**有意在 $TMPDIR 外**——现役契约允许写
$TMPDIR//private/tmp，把哨兵放那里会令允许/拒绝对照退化）；替身仅外部模型脚本）

| ID | 要求 | 实现/测试 | 结果 |
|---|---|---|---|
| R04-A11 | 移除/换错版本 helper→必须隔离的命令被拒+哨兵不变+不回落裸跑 | `r04_a11_missing_helper_at_composition_is_a_loud_refusal`：helper 路径不存在→组合期 `sandbox_helper_missing`，无绕过构造路径。`r04_a11_replaced_helper_is_refused_and_the_impostor_never_runs`（三腿）：①用户所有冒名 helper（`exec "$@"`，会照跑任何命令）组合期 `sandbox_helper_untrusted`（owner 非 root）；②合法 symlink 别名（→/usr/bin/sandbox-exec）组合成功且沙盒内运行（sanity 腿含 sandbox: seatbelt 标注）→**中途换成冒名 helper**→经真实网关链 `EXEC_SANDBOX_REFUSED`+`sandbox_helper_untrusted`、哨兵字节不变、live_handles 空（冒名者从未执行——若回落裸跑哨兵必被改写）；③彻底删除=同形态 `sandbox_helper_missing`。`r04_a11_unsupported_backend_refuses_through_the_real_chain`：Windows 形态后端经真实链拒绝+哨兵不变+零存活 | PASS |
| R04-A12 | 冻结策略下受限路径/网络/环境真实受限+允许对照成功；未具备的保证明确未支持 | `r04_a12_filesystem_write_isolation_holds_with_real_sentinels`：工作区写**允许对照**成功；受限外哨兵写拒（Operation not permitted+安全提示+哨兵字节不变）；**符号链接改向**（ws 内链→受限文件）按真实目标拒；**绝对路径** .git 写保护拒（deny 覆盖 allow）；auth.json **读拒**（AUTH-SECRET 不可读）；**TMPDIR 临时资源允许**；沙盒内子进程允许。`r04_a12_network_isolation_against_the_registered_loopback_service`：本测试自建登记回环服务（127.0.0.1 随机口，非阻塞 accept+截止期）——contained（真实 exec 链）连接拒；同策略 network-capable（port+真实 supervisor spawn）**拿到 pong 对照**；网络拒绝策略请求 network-capable=`sandbox_policy_unsupported`。`r04_a12_environment_whitelist_holds_through_the_sandbox_wrapper`：植入服务端秘密不达沙盒子进程；白名单 PATH+显式 env 到达。`r04_a12_sandboxed_process_tree_still_honors_the_cancellation_chain`：沙盒内孙进程 2s 超时链 killpg 真实回收（sandbox-exec 原地 exec 保组）。未支持项明确：linux 源码级未真机/windows 后端拒绝（矩阵登记） | PASS |
| 对抗·策略注入 | 层1 SBPL 字面量逃逸+层2 模型参数走私 | `adversarial_policy_injection_is_refused_at_both_layers`：含引号工作区根→`sandbox_profile_path_unsafe` 拒绝执行（命令不跑）；模型参数伪造 `sandbox_profile` 键→prepare 期 `ArgumentsInvalid` 零派发（T01/T02 冻结 schema additionalProperties:false 被 T06 威胁模型复验） | PASS |
| 对抗·继承秘密环境 | 白名单交集 | 植入 LINGXI_T06_SECRET→沙盒子进程 env 无秘密名/值；显式 env+PATH 到达（同 T05 面经 wrapper 复验） | PASS |
| 对抗·tty 不谎报 | 诚实状态 | `adversarial_tty_terminals_are_honestly_not_claimed_as_contained`：tty 启动结果含 Interactive process started、**永不含 sandbox: 标注**（现役 sandboxed=!tty 语义） | PASS |
| helper 信任/能力矩阵 | 审计+包裹形态 | `seatbelt_helper_trust_and_capabilities_are_recorded`：trust={owner_uid 0, resolved=/usr/bin/sandbox-exec}（双腿探针经构造隐式通过）；wrapped argv[0]=helper、argv[1]=-p、profile 含 (deny network*)；capabilities 六维 | PASS |

## 5. 验证（真实命令与退出码）

环境：macOS darwin 27.0.0 arm64；rustup 1.98.1（`/Users/study_superior/.cargo/bin/
cargo`）。仓库根执行；复跑脚本 `artifacts/rust-tauri/R04/T06-E01/run_t06_validation.sh`
（绝对路径口径；gates/ 各 .log + exit-codes.txt 归档）。最终冻结候选一轮：

| # | 命令 | 退出码 | 备注 |
|---|---|---|---|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast` | 101（仅 1 环境失败） | **840 通过 / 0 断言失败**（T05 轮 823+T06 验收 10+单测 7）；唯一失败 r00 见 §5.1 |
| 4 | `cargo run … -p xtask -- check-contracts` | 0 | 626 entries 零漂移（未触碰协议面） |
| 5 | `cargo run … -p xtask -- check-boundaries` | 0 | O1–O8+D1–D5+B1；`--self-test`（N1–N17）另跑 exit 0 |
| 6 | `cargo test … --test r04_t06_sandbox` | 0 | 10/10（A11×3+A12×4+对抗×3） |
| 7 | `cargo test … --lib sandbox::` | 0 | 7/7 单测 |
| 8 | `cargo test … --test r04_t05_process_tools` | 0 | 14/14（参数等价更新后复绿） |
| 9 | `cargo test … --test r04_t04_file_tools` | 0 | 9/9 |
| 10 | `cargo test … --test r04_t03_approval_service` | 0 | 11/11 |
| 11 | `cargo test … --test r04_t02_tool_gateway` | 0 | 13/13 |
| 12 | `cargo test … --test r04_t01_tool_catalog` | 0 | 6/6 |
| 13 | `cargo test … -p lingxi-kernel --lib` | 0 | 77（kernel 零改动） |
| 14 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh <fresh dir>` | 0 | 十套件钉数精确全绿 |
| 15 | `bash scripts/rust-tauri/r03_t08_matrix.sh <abs dir>` | 0 | A15 全绿 |
| 16 | `bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh <abs dir>` | 0 | A16 四相全绿 |

npm/TS 侧未运行：本 Task 未触碰任何 TS/Node 源（§2.4），按派单「如触碰」条件不适用。

### 5.1 r00 环境项（如实登记，不销项）

`r00_management_leaves::r00_management_positive_and_negative_branches_on_real_service`
workspace 全量与隔离复跑各失败一次（51s 后 panic），文案自证为 macOS 应用防火墙/
代理 TUN 拦截新建未签名测试二进制的入站连接（"request to 192.168.3.5:… stalled …
macOS application firewall / proxy TUN may be blocking…"）——T01/T02 起登记的同一
间歇环境项（T03/T04 轮曾通过；本轮 kernel/service 代码变化→测试二进制哈希变化→
防火墙视为新未签名 app）。与沙盒面无交集；840 个绿测试与其互不掩盖。需环境层处置。

### 5.2 T05 套件首次运行 1 例未复现瞬态

冻结前对 T05 套件的**第一次**执行报 13 通过/1 失败（tail 截断未捕获失败用例名）；
随后连续 5 次全绿 + 最终冻结候选轮（§5 #8）14/14 全绿。本 Task 对该文件的唯一改动
是调用点补一个 `None` 参数（行为中性，§2.2）；该套件含多个时间上界敏感用例
（如 unresponsive 1s≤耗时<6s）。如实登记：一次未复现瞬态、六次后续全绿、改动面
行为中性；不销项也不掩盖。

## 6. R03 行为不变量与回归证明（R04-SUP-05）

- 授权判定点/journal 写序零改动（T02-T04 冻结面 diff 为零）；沙盒包裹位于执行器内
  spawn 之前（拒绝=已派发但零 spawn 零副作用的 Failed 收据——与 T04 资源拒绝同为
  执行器层失败关闭，网关面未动）。workspace 840 通过（除环境项）+ 十套件钉数 +
  A15 + A16 全绿。
- 取消语义经 wrapper 保持：sandbox-exec 原地 exec（探针 P4）→ supervisor
  setsid/getpgid/killpg 链穿过 wrapper 达孙进程（A12 树测试 PID 级实测）。
- T01-T05 成果复验：6/6、13/13、11/11、9/9、14/14；kernel lib 77。

## 7. 两层自查记录（普通→对抗）

普通逐项：§4 表逐 ID 给出前提/动作/预期/实际/退出码；每个拒绝场景都有允许对照
（工作区写、network-capable pong、白名单 env、别名 sanity 腿、沙盒子进程——拒绝
全部不是安全绿灯）。

对抗性自查（推翻自己的尝试、发现与修复）：
1. **裸 `(deny default)` 探针 SIGABRT（发现并修复）**：初版双腿探针用
   `(deny default)+(allow process-exec*)` 作允许腿——在该 OS 上连 exec 都会中止
   （SIGABRT），8 测试连挂。修复：探针改用 containment core（process/mach/ipc/
   sysctl/reads 允许、其余拒绝——现役 profile 的子集），两腿语义不变（允许腿 true
   exit0；拒绝腿写 /dev/null 必败），实测通过。
2. **「受限哨兵放 $TMPDIR 下」假对照（发现并修复）**：初版测试根在 $TMPDIR——
   现役契约允许写 $TMPDIR//private/tmp，受限写探针**实际成功了**（这不是沙盒缺陷
   而是冻结策略的临时资源契约）。修复：测试根迁至 cargo CARGO_TARGET_TMPDIR
   （$TMPDIR 之外），允许/拒绝唯一差异=是否在可写集；TMPDIR 单独作为「临时资源
   允许」正向腿。
3. **回环服务线程楔死（发现并修复）**：初版 server 线程阻塞 accept 固定 8 次——
   contained 腿连接被拒后 accept 永不返回，测试挂死（sample 定位）。修复：非阻塞
   accept+10ms 轮询+60s 截止+stop 标志。
4. **bwrap 保真缺口（发现并修复）**：对照 `bwrap.ts` 逐行审——初版漏 `--chdir cwd`
   与 home 缓存（~/.cache/~/.npm）tmpfs 掩蔽、漏 existingPaths 存在性过滤。补齐
   （`SandboxCommandRequest.cwd` 透传）并钉进单测。
5. **「中途换 helper 真能被抓住？」**：构造期验证+探针不够——helper 在构造后被换
   仍需拒绝。实现每次 wrap 元数据重验；测试以合法 symlink 别名构造后换冒名脚本，
   经真实链拒绝且哨兵不变（若回落裸跑，`exec "$@"` 冒名者必写哨兵——负向证明无
   裸跑）。
6. **「冒名 helper 若是 root 属主怎么办？」**：元数据检查可被 root 属主冒名绕过——
   双腿强制探针兜底（不强制 profile 的二进制无法让拒绝腿失败）；组合面=元数据+
   basename+探针三层。测试用户态只能造非 root 冒名（uid 检查腿），探针腿由真
   helper 通过性+逻辑覆盖（诚实边界：root 属主破坏性冒名的完整对抗需主机层配合，
   非测试可模拟）。
7. **沙盒行标注可被命令输出伪造？**：命令自身可打印 "sandbox: …" 文本——标注由
   执行器在命令输出之后追加，仅陈述执行器事实；未包裹路径（无沙盒/tty）执行器不
   追加任何标注（对抗测试钉住「不出现」）。命令输出的自述不构成执行器承诺（与
   任何输出伪造同类，登记残余风险）。
8. **「wrapped argv 会逃逸吗？」**：argv 经 `-p <profile> -- <argv>` 内联传递
   （execve argv 元素无 shell 解释）；argv[0] 以 `-` 开头被 `--` 屏蔽；嵌套
   sandbox-exec 只能再受限不能放宽（seatbelt profile 单向收紧）；含 NUL 的 argv
   在 spawn 期 io 错误（失败关闭）。

## 8. 未验证项与风险（如实登记）

1. **r00 环境项**（§5.1）：防火墙拦截未签名测试二进制；需环境层处置；不销项。
2. **Linux**：bwrap 后端为源码级适配（构造逻辑+参数形有单测），**无 Linux 主机真
   机验证**——PLATFORM_CAPABILITIES v2 明示 NOT machine-verified；后续具备环境按
   同套探针（哨兵/回环/环境）补验。
3. **Windows**：restricted-token helper（lingxi-win-sandbox.exe）未移植——后端=
   UnsupportedSandbox 一律拒绝（fail-closed，绝不裸跑）；真机验证递延
   R03-WINDOWS-R09-R10（不丢失）。未做 windows 目标交叉编译检查（本机未装目标，
   沿用 T05 登记）。
4. **映射决策——require_escalated/sandbox_permissions 未迁移**：现役 require_
   escalated（经批准面的联网执行通道）需要 T02/T03 冻结面的按调用权限分类耦合；
   R04 的 exec_command 模型面=contained-only（现役默认路径逐字对应：defaultSandboxExec
   网络全禁）；**网络能力本身已在端口层真实可用并验证**（NetworkCapable+冻结策略
   Allowed+回环对照）。模型面升级通道留待后续显式决策，非静默降级。
5. **映射决策——seatbelt -f→-p**：同一 sandbox-exec helper 与 profile 语义；-p
   内联消除临时 profile 文件的 TOCTOU（先前运行沙盒进程可改写 $TMPDIR 下 profile）
   与清理面。探针与测试均按 -p 实测。
6. **macos-x64**：同 cfg 源码；真机验证按平台登记口径归 R09/R10。
7. **tty 终端不套 OS 沙盒**：现役语义（`sandboxed=!tty`）如实保留并登记矩阵；
   其安全边界=授权+环境白名单+用户可见交互（review 分类的批准面在 T03）。
8. **B1/边界**：新文件不引用 dispatch_executor/execute_prepared（check-boundaries
   self-test N1-N17 exit 0）；sandbox.rs 位于 lingxi-service（组合根 crate，允许
   std::env 读取——TMPDIR/HOME 只在该 crate，DEP-08 合规，check-boundaries 0）。
9. **测试替身边界**：IdleProvider（外部模型响应）、测试自建回环服务与哨兵/冒名
   helper（精确清理自己的目录/PID/句柄）——沙盒推导、网关、策略、监督、helper
   验证、profile 编译全部真实实现，无替身替代被测面。

## 9. 回退

回退范围=本 Task 获准修改（§3 清单）：删除 `sandbox.rs`/`r04_t06_sandbox.rs`，还原
`exectools.rs`/`lib.rs` 与 T05 测试调用点、PLATFORM_CAPABILITIES/TEST_MAP 条目即回到
基线 84e6499fe 形态。沙盒面为注入面（`register_process_tools(sandbox=None)` 时全部
T01-T05 路径零变化；生产默认 bootstrap 不调用）；Cargo.lock 本就零变化。

## 10. 证据索引

- `artifacts/rust-tauri/R04/T06-E01/run_t06_validation.sh`——复跑脚本（绝对路径
  口径；exit-codes.txt 汇总 16 项）。
- `gates/`：rust_fmt_check / rust_clippy / workspace_test（no-fail-fast 全量，
  840/1 环境项）/ check_contracts / check_boundaries / r04_t06_acceptance（10）/
  r04_t06_unit（7）/ r04_t01-t05 回归 / kernel_lib_tests / r03_g07_repair_suites /
  r03_t08_matrix_a15 / r03_t08_a16_seed 各 .log。
- `probes/PRE_IMPLEMENTATION_PROBES.md`——实现前 6 组真实探针（realpath 必需/
  链接改向/网络 allow-deny/原地 exec/组杀穿 wrapper/helper 事实）。
- `probes/workspace-test-cargo-test-workspace-locked.log`——冻结前一轮全工作区
  原始输出。
- `regression/`——R03 十套件/A15/A16 复跑产物（最终轮）。
- 新测试文件：`rust/crates/lingxi-service/tests/r04_t06_sandbox.rs`；单测内嵌
  `rust/crates/lingxi-service/src/sandbox.rs`（7 条）。
