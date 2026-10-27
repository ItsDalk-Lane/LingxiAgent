# R04-T05 报告｜命令、PTY 与真实进程树取消

执行代理：EXECUTOR-R04-T05-E01（一次性执行代理）。
日期：2026-09-30。分支 `codex/rust-tauri-migration`，基线 `80b4edbf5`（R04-T04 已独立
PASS 并推送确认）。工作区起点仅含本派单文件（未跟踪，非用户已提交修改，未触碰）。
状态：**READY_FOR_REVIEW**（普通逐项 + 对抗性两层自查完成；不自行判独立 PASS）。

---

## 1. 目标与结论

`exec_command` / `write_stdin` 在 Rust 中具备真实交互与真实资源生命周期（任务书
R04-T05）：不依赖 Pi 工具工厂、不依赖 node-pty。交付三层：

1. **真实 ProcessSupervisor**（`rust/crates/lingxi-service/src/procsupervisor.rs`）：
   POSIX 进程组归属（每次 spawn `setsid` 自成组 + `getpgid` 事后核证）、CSPRNG
   句柄身份（`proc:<32hex>`，非裸 PID）、有界清理责任链（标记 Terminating →
   `killpg(SIGKILL)` → 有界等待退出观察（默认 5s）→ reaper 侧回收（有界 stdio
   宽限 250ms、管道/master 关闭、spill 关闭）→ 终态记录 + 审计收据；超限诚实
   `CleanupTimedOut`，绝不伪造成功）。tokio `kill_on_drop` 显式关闭且从不是清理
   保证；调用 future 被丢弃时由 Drop guard 发起 `terminate_detached`（killpg 同步
   发出，有界尾段跑在 supervisor 自己的任务上——清理责任不随 future 丢失）。
2. **原生 exec_command / write_stdin 执行器**（`src/exectools.rs`）：结构化 argv
   与 shell cmd 双形态（shell 走 `/bin/bash -c`，安全靠授权/沙盒，无关键词黑名
   单）；环境**显式白名单**（`SAFE_ENV_PASSTHROUGH` 9 项 + 调用方显式 env，
   上限 32 项；服务端环境永不整体继承——关闭现役 `env ?? process.env` 缺口）；
   输出滚动窗口 + head/tail 截断（现役词表：`[Showing first X and last Y of Z
   lines. Full output: <path>]`）+ 有界 spill 落盘引用（ResourceRef, file:// URI,
   尺寸）；超时看门狗（默认 120s、钳 600s）走 supervisor 终止链；tty=true 经真
   PTY（posix_openpt/grantpt/unlockpt/ptsname + setsid + TIOCSCTTY + TIOCSWINSZ，
   全 libc，零新增依赖）返回 `Running{handle}`——「已启动」永不冒称完成；
   write_stdin 校验调用者（trusted ctx 的 principal+session 必须等于终端登记
   owner）+句柄授权（格式门/未铸造/一次性进程/已退出各有确定词表）。
3. **网关集成**：exec_command 以 Execute 类注册、经 `bind_executor_with_resources`
   绑定 cwd 资源派生（prepare 时 ResourceAccess 授权、execute 时重推导复验——
   T04 冻结链）；write_stdin 经 `bind_executor`（进程句柄授权在执行器内）；两者
   都只经 T02 网关 prepare→execute_prepared 到达（B1 静态边界未触碰）。批准面：
   Execute 类在 T03 ApprovalService 下 operate=放行 / ask=等批准 / read_only=
   `ACTION_BLOCKED_BY_READ_ONLY` 零派发（真实链测试钉住，批准记录携带 cwd 资源
   范围）。

结论：A09/A10 两个基础验收在**真实 macOS**（本机 darwin 27.0.0 arm64）以 PID 级
探针观察通过；8 个追加对抗项全部实现并测试；fmt/clippy/双门禁/T01-T04 套件/
R03 十套件+A15+A16 回归全绿（§5 退出码在案）；workspace 全量 823 通过、唯一失败
为已登记的 `r00_management_leaves` 防火墙环境项（本轮二连复现，隔离复跑核证为同
一自证文案，未销项）。

## 2. 实现摘要（关键文件:行）

### 2.1 ProcessSupervisor（核心交付）

- `rust/crates/lingxi-service/src/procsupervisor.rs`（新文件，1967 行含 6 单测）：
  - **归属身份**：`ProcessHandleId`（L97，CSPRNG 铸造；`parse` L133 只接受精确铸造
    形态——伪造句柄在格式门即拒）。`ProcessOwner`（L186：principal_kind/subject/
    session/run/tool_call，全部来自 trusted RunContext）。终止只接受已登记且**非
    终态**记录的句柄：记录非终态 ⇔ 子进程未 reap ⇔ PID 不可复用，`killpg` 的
    pgid 又在 spawn 后经 `getpgid` 核证等于子 PID（L919 verify_group，失败即杀
    并 reap 后响亮拒绝）——「不按裸 PID/名称回收未知进程」是结构性保证（无收养
    API，外来进程不可能成为受管记录）。
  - **有界清理责任链**（`terminate` L1306 → `mark_terminating` L1479 → reaper
    `finalize_exit` L1518）：标记与 `killpg` 在记录锁内一步完成（两个终止者不会
    分歧）；`await_termination` 有界等待终态（watch 广播 + deadline），超限置
    `CleanupTimedOut`（诚实记录 kill 已发、退出未观察）并回收己方句柄。reaper
    （supervisor 自有 tokio 任务，L957-995）负责：wait 退出 → **有界 stdio 宽限**
    （250ms，孙进程可能持有管道写端；超限 abort 泵任务=关闭读端，即管道回收）→
    `finalize_exit`（关 PTY master、关 spill、置 reclaimed、广播终态）→ settle
    入有界 ring（1024）。
  - **Future 丢弃不丢归属**：`terminate_detached`（L1346）——killpg 同步发出，
    有界尾段 `tokio::spawn` 到 supervisor 任务；无 runtime 时诚实审计
    （reaper 仍会记录退出）。执行器侧 `ProcessOwnershipGuard`（exectools.rs L500）
    在调用 future 被 run 取消树丢弃时触发；一切正常完成路径显式 disarm。
  - **持续终端寿命登记**：`ProcessKind::PersistentTerminal`（L211）+ owner 记录；
    tty 终端跨调用存活（run 取消不杀——工具调用早已返回 running）；死于显式
    close 或 `shutdown_all`（L1440，冻结的应用退出策略：有界终止一切受管进程，
    逐进程收据）。`Drop` 兜底同步 killpg + 删除 supervisor 专属 spill 目录
    （唯一命名目录，绝不通配清理共享 /tmp）。
  - **PTY**：`open_pty_pair`（L1675，posix_openpt/grantpt/unlockpt + 全局锁内
    ptsname + slave open，均 O_CLOEXEC；master 置 **O_NONBLOCK**——AsyncFd 必需，
    对抗自查修复的挂死根因）；pre_exec `setsid`+`TIOCSCTTY`（node-pty 同款接线，
    \x03 INTR 与 SIGWINCH 因此可达前台组）；`pty_read_loop`（L1771，AsyncFd 就绪
    + 原始 read(2) 进 transcript ring）；`pty_write_all`/`pty_resize`
    （TIOCSWINSZ）。
  - **输出有界**：一次性收集器（滚动窗口 100KiB + 计数 + spill 上限默认 64MiB，
    封顶即停并如实标注）；PTY transcript ring（256KiB，丢未投递数据计数入
    `dropped_undelivered_bytes`）+ spill。UTF-8 边界：`deliver_since_cursor`
    （L626）按字节偏移推进游标——尾部不完整序列整块留在 ring 下次重拼（绝不复制
    /腰斩多字节字符），终态后 force 投递（悬挂部分 lossy 诚实置换）。
  - **平台诚实**：非 Unix 构建spawn 失败关闭（`UnsupportedPlatform`）——不发明
    Windows Job Object 行为，真机 Windows 验证维持阶段递延口径。

### 2.2 exec_command / write_stdin 执行器（核心交付）

- `rust/crates/lingxi-service/src/exectools.rs`（新文件，1084 行）：
  - **参数**（`parse_exec_params`）：argv/cmd 二选一（都给/都缺=响亮
    `EXEC_COMMAND_INVALID_PARAMS`）；`workdir`（默认工具 cwd）；timeout_seconds
    （≤0→默认 120，>600 钳 600 并在结果标注 `[timeout clamped to 600s]`）；
    max_output_bytes（≥1000）；env（≤32 项、键 ≤64B、值 ≤8KiB、禁 '=' 与 NUL，
    值必须字符串）；tty/cols/rows（1..=1024）。schema 层 additionalProperties:
    false + executor 语义校验双层。
  - **环境白名单**（L103 + `build_child_env`）：PATH/HOME/LANG/LC_ALL/LC_CTYPE/
    TERM/TZ/TMPDIR/SHELL 显式透传 + 调用方显式 env 覆盖/追加；其余一概不继承。
  - **一次性执行**（`run_exec_command` L587）：授权 cwd（执行器侧重验）→ spawn →
    Drop guard → `select!{biased; wait_terminal; sleep_until(timeout)}`；超时分支
    走 `terminate(Timeout)` 完整有界链后渲染 `Command timed out after N seconds
    (default timeout). For long-running work pass timeout=<seconds> (max 600), or
    run with tty=true and continue via write_stdin.`（现役文案）；退出码
    `128+signal`（shell 约定）；输出经 `truncate_head_tail`（L386，现役保头保尾
    算法移植，字符边界安全切片）+ spill ResourceRef（`lingxi-exec-output-*.log`）。
  - **tty 形态**：返回 `Running{handle}` + 启动说明（绝不出现 exited/exit code——
    测试钉住「已启动不冒称完成」）。
  - **write_stdin**（L755）：process_id 格式门 → 记录存在 → 必须是
    PersistentTerminal（`WRITE_STDIN_NOT_INTERACTIVE`）→ owner=principal+session
    匹配（`WRITE_STDIN_NOT_OWNED`，跨主体拒）→ 写 master（非运行态=
    「not running」诚实失败）→ 投递自上次游标后的输出 + 状态行 + transcript 路径；
    空 chars=轮询（poll）；终态后仍可读最终输出。
  - **注册**（`register_process_tools` L933）：exec_command（Execute 类 +
    cwd 资源派生 L1024）与 write_stdin（Execute 类，无文件资源）注册进真实
    registry+gateway；组合根默认不调用（生产默认入口零变化，同 T04 口径）。
  - **错误词表**：EXEC_COMMAND_INVALID_PARAMS / EXEC_SPAWN_FAILED（含现役
    cwd-missing 提示）/ WRITE_STDIN_PROCESS_ID_REQUIRED / WRITE_STDIN_UNKNOWN_PROCESS /
    WRITE_STDIN_NOT_OWNED / WRITE_STDIN_NOT_INTERACTIVE。

### 2.3 依赖与 TS/Node 面

- **零新增 crate**：PTY/进程组全用 libc 0.2.189（既有直接依赖）+ std +
  tokio。tokio 仅新增 **feature flag "process"**（版本 1.53.1 不变；其依赖
  bytes/libc/mio 均已在锁内——serde_json R04-T01 先例）；dev-dependencies 增
  libc（同锁定版本，测试 PID 探针）。`rust/Cargo.lock` **零变化**（git diff 为
  空，已核）。现役 TS 栈未触碰（仅语义参照）；npm 侧按「如触碰」条件不适用。

## 3. 调用链与修改范围

真实调用链：provider `ToolRequests` → 驱动 digest 门（T01）→ **网关 prepare**
（目标/schema/规范化 → **cwd 资源规范化**（新：authorize→ResourceScope Read，
越权=零派发 `gateway_resource_scope_denied`）→ T03 策略裁决（operate 放行/ask
等批准/read_only 拒）→ prepared 记录绑定 resources）→ journal intent → RunGrant
授权判定（未动）→ 批准面（ask 腿等待真实 ApprovalService，批准人看到 cwd 资源
范围）→ journal authorized/started → **网关 execute_prepared**（身份/单次/注册表
复验 → 资源重推导复验 → 真实执行器：spawn（setsid+getpgid 核证）→ guard +
wait/timeout → 输出收集/截断/spill）→ fence→收据。**run 取消树**（CALL 级 biased
drop）丢弃执行器 future → guard `terminate_detached` → killpg → supervisor 有界
尾段（等待+回收+收据）→ journal 保持 started-无收据（R03 Unknown 窗口）。

修改文件清单：
- 新增：`rust/crates/lingxi-service/src/procsupervisor.rs`（+6 单测）、
  `src/exectools.rs`、`tests/r04_t05_process_tools.rs`（14 集成验收/对抗测试）、
  `artifacts/rust-tauri/R04/T05-E01/**`（证据+复跑脚本）、本报告。
- 修改（产品）：`lingxi-service/src/lib.rs`（pub mod + 再导出，12 行）、
  `lingxi-service/Cargo.toml`（tokio +process feature 注释；dev-deps libc）。
- 修改（文档）：`docs/rust-tauri/R04/R04_TEST_MAP.json`（T05 条目）。
- 未触碰：kernel crate、协议 wire 面（check-contracts 626 entries 零漂移）、
  toolgateway.rs/approval_service.rs/runs.rs（T02-T04 冻结面零改动——资源派生经
  T04 既有 `bind_executor_with_resources` 注入）、生产默认入口、xtask stage maps、
  check 脚本、`ORCHESTRATOR_PROGRESS.json`。

## 4. 验收场景与逐项结果（真实链：bootstrap_with_deps 组合根→真实驱动→真实
journal→真实注册表→真实网关→真实 ApprovalService→真实进程/PTY；替身仅外部模型
脚本/扮演批准人的测试）

| ID | 要求 | 实现/测试 | 结果 |
|---|---|---|---|
| R04-A09 | 命令创建子/孙进程+无关哨兵；取消后受管树退出、哨兵存活、管道回收、收据准确（真实 OS 观察） | `r04_a09_managed_tree_is_killed_and_the_unrelated_sentinel_survives`（全链）：bash 子进程+两个 sleep 孙进程发布 PID 文件（barrier 1）→supervisor 活记录（barrier 2）→trusted run id 捕获（barrier 3）→真实用户取消入口 `cancel_run` → **三个 PID 全部 `kill(pid,0)` 失败**（被 reaper 回收，非僵尸假象）→哨兵 `kill(pid,0)` 仍存活→记录=Terminated(CallerDropped) 且审计含 killpg_sent/exit_observed/termination_settled(reclaimed=true)→journal phase=Started 且无收据（R03 Unknown 窗口） | PASS |
| R04-A10 | PTY 交互不退化：输入/多次读取/resize/中断/退出 | `r04_a10_pty_input_reads_resize_interrupt_and_exit`：bash -i 起 PTY（80x24）→`echo MARKER_ONE` 往返→`stty size`="24 80"→TIOCSWINSZ resize 100x40 后="40 100"→\x03 中断（前台 sleep 死：`NEVER_REACHED` 永不出现；shell 存活：`AFTER_INTR` 出现）→`exit 7`→phase=Exited(7)、final poll 报 exited (exit code 7)、事后写入诚实失败 not running；启动结果为 Running 句柄（非完成声明） | PASS |
| 对抗·父退出孙持管道 | 有界 stdio 宽限不拖住调用 | `adversarial_parent_exit_with_grandchild_holding_the_pipe_is_bounded`：父 <1.5s 返回（孙 sleep 2 仍持管道）+ PARENT_DONE + exit 0 + 孙短暂存活（现役语义：正常退出不杀组）；测试按精确 PID 清理孙进程 | PASS |
| 对抗·输出风暴 | 有界+截断+落盘引用 | `adversarial_output_storm_is_bounded_truncated_and_spilled`（spill 上限调 64KiB）：~500KB 输出→truncated=true+`[Showing first …]` 提示+尾部可见（滚动窗口语义=现役 2x 窗口）+spill ResourceRef 存在且字节数=64KiB（恰好封顶）+exit 0 | PASS |
| 对抗·UTF-8 分片 | 多字节字符跨块不腰斩 | 集成：`printf 'a中文'; sleep 1; printf '字符ok'` → 结果含 `a中文字符ok`；单测：`transcript_delivers_split_multibyte_characters_intact`（日=跨两次 append，先投 "a" 后投 "日"，无复制无腰斩）+ `decode_prefix_utf8_boundary_variants` + force 冲刷悬挂部分的 lossy 诚实置换 | PASS |
| 对抗·无响应进程 | 超时链有界 | `adversarial_unresponsive_process_times_out_within_the_bound`：sleep 300 + timeout 1 → 1s≤耗时<6s、超时文案（含 tty 续接教学）、Exited(137)、live_handles 空 | PASS |
| 对抗·双流阻塞 | stdout+stderr 同时超管道缓冲 | `adversarial_both_streams_blocking_still_completes`：两流各 ~180KB（>64KB 内核缓冲）→并发泵送完成、exit 0、两端可见（串行读会死锁） | PASS |
| 对抗·跨主体 write_stdin | 调用者+会话授权 | `adversarial_cross_session_and_forged_write_stdin_are_refused`：会话 B 写 A 的终端=NOT_OWNED；A 仍可轮询；`proc:deadbeef` 与未铸造 hex=UNKNOWN；一次性进程句柄（经活记录探针取得）=NOT_INTERACTIVE 后经有界链清理 | PASS |
| 对抗·取消与正常退出竞争 | 终态一致无悬挂 | `adversarial_cancel_racing_natural_exit_keeps_invariants`：ready barrier 后取消→run 终态、记录终态、PID 消失；journal 要么 Started-无收据（取消赢）要么 Succeeded/Failed-有收据（完成赢），绝不伪造 | PASS |
| 网关/策略/资源 | T02+T03+T04 面作用于真实进程工具 | `process_tools_go_through_the_gateway_policy_and_cwd_authorization`：read_only=prepare 拒 ACTION_BLOCKED_BY_READ_ONLY+零派发；workdir 越权（restricted）与**符号链接指向越权区**（按真实目标判）=gateway_resource_scope_denied 零派发+哨兵字节不变；工作区内 workdir 放行且以 canonical cwd 运行 | PASS |
| 批准面 | ask 档真实往返 | `ask_session_exec_command_round_trips_the_approval_face`：ask 会话 park→PendingView.resources 含 canonical 工作目录→真实 answer(Approve)→journal Succeeded+dispatched=true+live_handles 空 | PASS |
| 环境白名单 | 秘密不继承 | `environment_is_whitelist_plus_explicit_overrides_never_full_inherit`：测试进程植入 LINGXI_SECRET_TOKEN→子进程 env 含 LINGXI_TEST_MARKER=42+PATH、**不含**秘密名与值 | PASS |
| 持续终端寿命 | 显式登记+有界退出 | `persistent_terminal_lifetime_is_explicit_and_shutdown_is_bounded`：tty 终端跨调用存活（kind=persistent_terminal）+运行中可轮询→shutdown_all 有界终止（1 收据：Terminated/fact 137/reclaimed=true）→PID 消失 | PASS |
| 终止幂等/派发失败 | 已退出不误杀；失败响亮 | `terminate_after_natural_exit_is_already_terminal_and_spawn_failures_are_loud`：已退出记录 terminate=AlreadyTerminal(Exited 0)+审计零 killpg（防 PID 复用误杀）；不存在二进制=EXEC_SPAWN_FAILED；不存在 cwd=现役提示；缺 cmd/argv=INVALID_PARAMS | PASS |

## 5. 验证（真实命令与退出码）

环境：macOS darwin 27.0.0 arm64；rustup 1.98.1（`/Users/study_superior/.cargo/bin/
cargo`）。仓库根执行；原始输出归档 `artifacts/rust-tauri/R04/T05-E01/`（gates/ +
exit-codes.txt），复跑脚本 `run_t05_validation.sh`（绝对路径口径）。以下为最终
候选一轮：

| # | 命令 | 退出码 | 备注 |
|---|---|---|---|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast` | 101（仅 1 环境失败） | **823 通过 / 0 断言失败 / 77 二进制 ok**；唯一失败 r00 见 §5.1（fail-fast 形态在同一二进制停住，同项） |
| 4 | `cargo run … -p xtask -- check-contracts` | 0 | 626 entries 零漂移（未触碰协议面） |
| 5 | `cargo run … -p xtask -- check-boundaries` | 0 | O1–O8+D1–D5+B1；`--self-test`（N1–N17）另跑 exit 0（新文件不触受保护符号） |
| 6 | `cargo test … --test r04_t05_process_tools` | 0 | 14/14（A09/A10+十二对抗/集成） |
| 7 | `cargo test … --lib procsupervisor` | 0 | 6/6 单测 |
| 8 | `cargo test … --test r04_t01_tool_catalog` | 0 | 6/6 |
| 9 | `cargo test … --test r04_t02_tool_gateway` | 0 | 13/13 |
| 10 | `cargo test … --test r04_t03_approval_service` | 0 | 11/11 |
| 11 | `cargo test … --test r04_t04_file_tools` | 0 | 9/9 |
| 12 | `cargo test … -p lingxi-kernel --lib` | 0 | 77（kernel 零改动） |
| 13 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh <dir>` | 0 | 十套件钉数精确全绿 |
| 14 | `bash scripts/rust-tauri/r03_t08_matrix.sh <dir>` | 0 | A15 28 叶+11 组合全绿 |
| 15 | `bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh <dir>` | 0 | A16 四相全绿 |

npm/TS 侧未运行：本 Task 未触碰任何 TS/Node 源（§2.3），按派单「如触碰」条件不适用。
T05 套件并行全绿无 flake（测试根/spill 按 pid+纳秒+序号唯一）。

### 5.1 r00 环境项（如实登记，不销项）

`r00_management_leaves::r00_management_positive_and_negative_branches_on_real_service`
本轮在 workspace 全量与隔离复跑中**二连失败**，panic 文案自证为 macOS 应用防火墙/
代理 TUN 拦截**新建未签名测试二进制**的入站连接（"request to 192.168.3.5:… stalled
… macOS application firewall / proxy TUN may be blocking…"）——与 T01/T02 登记的
同项同形（T03/T04 轮曾通过；本轮 kernel/service 代码变化→测试二进制哈希变化→
防火墙视为新未签名 app）。隔离复跑 `r00_management_leaves_isolated_rerun.log`
（exit 101，同一文案）。该测试与进程工具无交集；823 个绿测试与其互不掩盖。
需要环境层处置（对本机测试二进制放行），非代码可修；T08 最终 Gate 重跑 R03 时
将再现。

## 6. R03 行为不变量与回归证明（R04-SUP-05）

- **授权判定点/journal 写序零改动**：T02-T04 冻结面（toolgateway.rs/
  approval_service.rs/runs.rs）diff 为零；本 Task 的新资源派生经 T04 既有
  `bind_executor_with_resources` 注入。workspace 823 通过（除环境项）+ 十套件
  钉数 + A15 + A16 全绿。
- 取消语义与 R03 契约一致：run 取消 → 执行器 future 在 await 点被丢弃 → 有界清
  理链完成（PID 级验证）→ journal started-无收据 = R03 Unknown 分类——「已派发
  无可信回执」不伪造确认失败、不盲重试（A09 钉住）。
- T01-T04 成果复验：6/6、13/13、11/11、9/9；kernel lib 77。

## 7. 两层自查记录（普通→对抗）

普通逐项：§4 表逐 ID 给出前提/动作/预期/实际/退出码；每个拒绝场景都有合法对照
（工作区内命令执行、owner 轮询、合法 argv/cmd、批准后执行、正常退出与退出码——
拒绝全部不是安全绿灯）。

对抗性自查（推翻自己的尝试、发现与修复）：
1. **PTY master 阻塞挂死（发现并修复）**：初版 master 未置 O_NONBLOCK——AsyncFd
   就绪循环里 read(2) 直接阻塞 worker 线程，A10 首跑挂死 >5 分钟。修复：
   `open_pty_pair` 内 fcntl(F_SETFL, O_NONBLOCK)（portable-pty 规则）；随后
   A10 全绿。
2. **transcript 尾部悬挂字节复制缺陷（发现并修复）**：初版用独立
   `pending_tail` 缓冲回滚游标——但悬挂字节仍留在 ring 的原 chunk 里，下次投递
   会把 pending_tail 与原 chunk **双重拼接**（重复字节）。修复：删除独立缓冲，
   游标按**字节偏移**推进（部分消费的 chunk 整块保持未投递、下次整体重拼），
   终态后 force 投递（悬挂序列 lossy 置换，诚实不静默）。单测
   `transcript_delivers_split_multibyte_characters_intact` 钉住无复制无腰斩。
3. **中断判定的假阳性风险**：初版断言 ^C 出现即证 SIGINT——bash-3.2 在 SIGINT
   杀死前台 sleep 后**继续执行命令表**（`;` 链），`NEVER_REACHED` 照样打印，
   假阳性。修复：探针命令改 `&&` 链（sleep 被 SIGINT 杀→非零→短路），断言
   `NEVER_REACHED` 永不出现 + `AFTER_INTR` 出现（shell 存活）——正向+反向双证。
4. **read_only 拒绝形态**：初版按「Failed 工具结果」断言——实际是 prepare 阶段
   的 `GatewayRefusal::PolicyDenied`（零派发，更早更严）。按真实形态修正测试
   （Err(refusal) 断言 code），实现无需改。
5. **「已杀」与「已 reap」的区分**：`kill(pid,0)` 对**僵尸**也返回 0——若 reaper
   不 reap，A09 会假绿。审视确认 reaper 持有 Child 并 `wait().await`（真 reap），
   测试断言的是 ESRCH 而非僵尸存活；同时哨兵（非我们子进程）在测试进程下不产
   僵尸（存活期间），断言成立。
6. **PID 复用面**：terminate 的 killpg 只在记录非终态时发出（子进程未 reap ⇒
   PID 不可复用）；已终态记录 terminate=AlreadyTerminal 且审计无 killpg——
   `terminate_after_natural_exit…` 测试钉住。
7. **风暴内存/文件上界**：窗口 100KiB + ring 256KiB + spill 64MiB（测试调小验
   证封顶行为）：三重上界，spill 封顶即停并如实标注——非无界。
8. **并行隔离**：测试根/spill 目录 pid+纳秒+序号唯一；env 植入的秘密对其他测试
   不可见（白名单不透传）。

## 8. 未验证项与风险（如实登记）

1. **r00 环境项**（§5.1）：二连复现，需环境层处置；不销项。
2. **跨平台**：仅 macOS arm64 实测。Windows：未实现（spawn 失败关闭，
   `UnsupportedPlatform`）——POSIX 进程组机制的 Windows 对应（Job Object）按
   阶段平台矩阵递延登记，不虚构。Linux：同族 POSIX 调用，未在本机实测（登记）。
   未做 windows 目标交叉编译检查（本机未装目标）。
3. **映射决策——shell 选择**：现役按平台解析 shell（powershell/cmd/bash）并注入
   cd；Rust 栈 shell 形态固定 `/bin/bash -c` + 原生 cwd 传参（等价且更简）；
   Windows shell 家族选择归 Windows 形态（递延）。
4. **映射决策——wait_mode=auto/后台转交**：现役 exec_command 的 auto 模式（PTY
   起跑+前台窗口+转后台+延迟结果回送，依赖 DeferredResultStore/TaskRegistry——
   R06/R07 会话状态与任务注册面）未迁移；Rust 侧长任务出路=timeout 续跑或
   tty=true+write_stdin（超时提示明示）。登记为受控缺口，非静默降级。
5. **映射决策——run-code/沙盒/escalation**：现役 sandbox_permissions/
   require_escalated、bwrap/seatbelt 沙盒前缀与 run-code worker 归 T06
   （沙盒）；本 Task 的命令以 T04 资源判定+T03 策略面为约束，无关键词黑名单。
6. **映射决策——终端 transcript 持久化**：现役 terminal-session-manager 落
   jsonl transcript+元数据+事件流（UI 面）；Rust 侧为内存 ring+supervisor 专属
   spill 文件（supervisor 生命周期=文件生命周期，Drop 删除专属目录）；产品级
   会话终端登记归 R06+。
7. **映射决策——close 动作**：现役 terminal 工具的 close/start 列表动作未整体
   迁移（terminal 工具本身是另一注册面）；显式关闭走 supervisor
   terminate(Close)/shutdown_all（测试覆盖）。
8. **PTY ECHO/回显噪声**：write_stdin 投递含终端回显与提示符（现役
   node-pty 行为一致）；模型侧解析归提示词/R05+。
9. **spill 引用生命周期**：结果中 ResourceRef 指向 supervisor 专属目录——
   supervisor 存活期间有效；长寿命产物登记（会话文件化）归 R06+（已注册）。
10. **执行器超时/并发上限目录元数据**：manifest timeout_ms=None（按调用
    timeout_seconds）；max_concurrency 未强制（T08 面登记，同 T04 §8.10）。
11. **测试替身边界**：RunIdCaptureProvider（外部模型响应+从 trusted RunContext
    捕获 run id 供取消锚定）/测试扮演批准人/测试创建的哨兵与孙进程（精确 PID
    清理）——进程监督、PTY、killpg、资源判定、网关、批准、journal 全部真实实现，
    无替身替代被测面。

## 9. 回退

回退范围＝本 Task 获准修改（§3 清单）：删除 `procsupervisor.rs`/`exectools.rs`/
`r04_t05_process_tools.rs`，还原 `lib.rs` 与 `Cargo.toml`（tokio feature 与
dev-deps libc）、TEST_MAP T05 条目即回到基线 80b4edbf5 形态。新模块是注入面
（组合根默认不调用）；T02-T04 冻结面零改动，删除不影响任何现有生产路径；
Cargo.lock 本就零变化。

## 10. 证据索引

- `artifacts/rust-tauri/R04/T05-E01/run_t05_validation.sh`——复跑脚本（绝对路径
  口径；exit-codes.txt 汇总）。
- `gates/`：rust_fmt_check / rust_clippy / workspace-test（no-fail-fast 全量，
  823/1 环境项）/ check_contracts / check_boundaries(+selftest) /
  r04_t05_acceptance_tests（14）/ r04_t05_unit_tests（6）/ r04_t01-t04 回归 /
  kernel_lib_tests / r03_g07_repair_suites / r03_t08_matrix_a15 /
  r03_t08_a16_seed 各 .log（退出码注记在卷）。
- `r00_management_leaves_isolated_rerun.log`——环境失败隔离复跑核证（exit 101，
  同一防火墙自证文案）。
- 新测试文件：`rust/crates/lingxi-service/tests/r04_t05_process_tools.rs`；
  单测内嵌 `rust/crates/lingxi-service/src/procsupervisor.rs`（6 条）。
