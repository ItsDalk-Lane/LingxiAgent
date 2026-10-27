# R02-T02｜配置、路径与安全启动 — 执行报告

- 执行者：ZCode:R02-T02（EXECUTOR-R02-T02，一次性执行代理；不负责独立验收，不提交/推送）
- 状态：**READY_FOR_REVIEW**（PASS/FAIL 判定归总控另派的独立验收）
- 日期：2026-09-26
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R02_Rust独立服务、存储与事件基础.md` §4 R02-T02
  （场景 R02-A03「双实例拒写」、R02-A04「路径优先级不意外覆盖」，均 REQUIRED）
- 附加收口：R02-T01 验收 `R02-T01_REVIEW_R1.md` findings **F01**（DEP-08 环境变量执法留白）与
  **F03**（CLI 语义边界简化），均属本任务范围，已收口（§5.3/§5.4）。

## 1. 范围

任务书四步：①显式解析 --home/配置/环境优先级并在安全日志显示有效路径，测试模式强制隔离
home；②规范化目录、权限检查、临时目录与 platform path，拒绝越权/不合法根，不把用户字符串
直接拼进敏感路径；③生成 instanceId/startNonce/版本握手，状态文件原子写入，清理只删除自己
拥有的实例记录；④本地单写者锁与同目录双启动拒绝，陈旧检测不得仅靠 PID 存在判断。
交付：配置解析器；实例/单写者控制；启动诊断。

不在本任务内（属后续 T）：HTTP/WS 认证与端点权限表（T03）、SQLite 存储与事务（T04——
「库校验」在 T02 以现有数据面（实例记录/锁/运行时目录）的逐文件哈希承载，见 §5.1）、
事件（T05）、备份/关闭协调器（T06）、日志脱敏与资源上限（T07——本任务安全日志仅含路径与
实例 ID，与 T01 同口径）、xtask 门禁（T08）。现役 Node/Electron 生产入口与默认启动零改动。

## 2. 源码基线与环境

- 开工实测：分支 `codex/rust-tauri-migration`，HEAD = `42c49faaaf35d98d41dff95bb16d0aecf463418e`
  （= TASK_BASE_SHA，=远端 HEAD，R02-T01 已完成推送），`git status --short` 为空（干净）。
- tested SHA：`42c49faaaf35d98d41dff95bb16d0aecf463418e` + 本任务未提交改动（§7 逐文件列出）。
- 平台：macOS 27.0 arm64（Darwin 27.0.0）。
- 工具链：rustup 锁定 1.98.1（`rust-toolchain.toml`；`rustc 1.98.1 (48a229cea 2026-09-01)`
  实测）；全部 cargo 经 `~/.cargo/bin` rustup 代理调用。
- 构建隔离（RR-T08-F1）：全部 cargo 命令 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t02`
  （本任务专属）+ `CARGO_NET_OFFLINE=true` + `--locked`；**Cargo.lock 与 Cargo.toml 零改动，
  零新增第三方依赖**（单写者锁用 Rust 1.89 起标准库稳定化的 OS 文件锁
  `File::try_lock/unlock`，实例 ID 熵源用 `/dev/urandom` 读 + 降级混拌，均无新 crate）。
- 网络：失效代理（127.0.0.1:7890）已从全部命令剥离（`env -u …_proxy`）；构建全程离线。
- 测试隔离：全部服务/测试只用合成 home（mktemp / 进程号+纳秒唯一目录），未触碰真实用户
  目录；env 注入全部发生在子进程或函数参数层，不污染外层 shell。

## 3. 观察事实（先读再动手，未混写设计）

1. T01 交付的 `lingxi-service` 现状：`--home` 必填且绝对、`--bind` 默认 127.0.0.1:0、stdout
   恰一行 `LINGXI_SERVICE_READY addr=… home=…`、SIGINT/SIGTERM 优雅关闭 exit 0、配置错误
   exit 2；`ServiceConfig` 为唯一配置载体，`prepare_data_home` 是目录处理挂点（T01 报告 §9
   交接原话）。
2. 现役 Node 同语义（只读参照）：`server/index.ts:252` 用
   `resolveLingxiHome(process.env.LINGXI_HOME)`（`shared/hana-runtime-paths.cjs`：env 缺省落
   `~/.lingxi`、`~` 展开、`path.resolve`）。同宅互斥闸（server/index.ts:262-289）在端口监听
   与任何 store 打开之前跑，用 `server-info.json` + token 认证探测判断记录是否存活，注释明写
   「不信任裸 PID，因为 PID 会被系统复用」；探测不通视为残留锁自清。
3. T01 REVIEW_R1 F01：DEP-08 只禁依赖边（lingxi-service/axum/reqwest/hyper/rusqlite），
   `std::env` 访问不是依赖边，D1/D3 均不可见——「domain 不碰环境变量」当时只剩约定；
   kernel 源码扫描 `env` 零命中（基线无违规）。F03：`--bind --home /x` 会把 `--home` 当
   bind 值消费后报 BadBind（响亮但误导），重复 `--home` 后者静默覆盖前者。
4. 检查器 `r01_t01_check_ownership.py` 完全数据驱动：D3 对任何规则的
   `forbidden_source_tokens` 生效（src/**/*.rs、注释剥离）；R02-T01 验收已确认「DEP-08 纯
   数据驱动生效」的先例（review §4）。
5. `std::fs::File::try_lock/unlock` 在 1.98.1 可用且实测同进程两个独立 open 互斥（flock
   语义按 open file description 计），为「锁随进程死亡自动释放」提供内核权威。
6. macOS `/tmp` → `/private/tmp` 符号链接：不经 canonicalize 的路径字符串会把同一目录当成
   两个「home」。
7. `cargo metadata`（无 --locked）会重写 Cargo.lock（T01 观察事实 8）；本任务全程 --locked，
   未触发。
8. tracing-subscriber 默认对重定向到文件的 stderr 也输出 ANSI 转义，破坏证据脚本 grep——
   需显式 `with_ansi(stderr.is_terminal())`。

## 4. 设计决定

1. **数据根优先级协议（文档化 + 测试钉死 + 安全日志可见）**：
   `--test-mode` > `--home <DIR>`（CLI）> `LINGXI_HOME`（env，与现役 Node 同名）>
   `--config <FILE>` 的 `home` 字段。无任何来源 = MissingHome exit 2（继承 T01「无静默默认」
   不变量；与 Node 的 `~/.lingxi` 缺省与 `~` 展开是有意分歧——组合根拒绝猜真实用户目录，
   `~…` 按相对路径拒绝，由调用方 shell 展开）。config 文件只在显式 `--config` 时读取，
   **没有**约定位置自动发现（静默读真实用户目录正是本任务要禁的行为）；文件为严格 JSON
   `{"home": "<绝对路径>"}`，未知键/缺 home/非字符串/不可读/非法 JSON 全部响亮报错。
   **显式提供的来源即使输掉优先级也照样校验**（CLI home + 坏 config 文件 = 配置错误，
   不是静默跳过）。安全日志（stderr tracing，非 TTY 无 ANSI）首行
   `data root resolved effective_home=… source=… ignored_sources=…`；READY 行增加
   `source=` 字段（addr/home 字序不变，T01 harness 的 `addr=[^ ]*` grep 兼容，属加法扩展）。
2. **测试模式强制隔离**：`--test-mode` 使 home = `<系统临时目录>/lingxi-service-test-<pid>-<纳秒>`
   （每次启动唯一），CLI/env/config 的 home 全部忽略并逐项记入 ignored_sources 日志。
   A04 用文件探针证明三个「生产形态」候选根目录全程不被创建。
3. **规范化布局与权限**：一切敏感路径 = **canonicalize 后的 home** + 固定名
   （`lingxi-service/instance.lock|instance.json|instance.stale.json|tmp/`），用户字符串
   只作为 home 本身进入、且先经符号链接消解（观察事实 6 的别名双启动由 canonicalization
   收敛，A03-D 实测拒绝）；拒绝相对路径、`/`（文件系统根）、非目录存在物。unix 上运行时目录
   与 tmp 目录创建/收紧为 0700（umask 之后仍强制收紧并核验）；Windows 分支为目录检查
   （未编译验证，平台边界如实登记）。实例 ID/startNonce/原子写临时文件名全部由生成值构成，
   不拼接用户输入。
4. **原子写**：tmp 文件（同目录、生成名）→ write → fsync → rename → fsync 父目录
   （Windows 无目录 fsync，登记）。状态文件（instance.json、instance.stale.json）全部经此
   路径写入。
5. **单写者锁选型**：`{home}/lingxi-service/instance.lock` 上的 OS 建议锁
   （`File::try_lock`；unix=flock、Windows=LockFileEx，均标准库、1.89 稳定）。否决候选：
   O_EXCL 锁文件（SIGKILL 后永久残留，需人工清理——正是任务书禁止依赖的形态）、
   third-party `fs4/fs2`（违反零新增偏好）、socket 占用推断（与数据根无关）。
   锁属 **open file description**：进程死亡（含 SIGKILL）即释放，结构上不存在陈旧锁文件。
   锁文件本身永不删除（删除有竞态；A03-E 断言它保留）。
6. **陈旧检测 = 锁，绝不信 PID**：锁可得 + 盘上有记录 ⇒ 记录陈旧（其主已死）→
   原子归档为 `instance.stale.json`（保留最近一份，内容逐字保留）→ 打
   `LINGXI_SERVICE_STALE_RECORD_TAKEN_OVER home=… previousInstanceId=… previousPid=…
   previousStartNonce=… handshakeMismatches=…` 机读标记 → 新实例成为属主。
   锁不可得 ⇒ 活着的手里：exit 3 + `LINGXI_SERVICE_SINGLE_WRITER_BLOCKED home=…
   recordedPid=… recordedInstanceId=… recordedStartNonce=… recordedAddr=… probe=live|unreachable
   authority=os-file-lock` 机读标记 + 人类诊断。**PID 仅作诊断输出，从不参与判定**——
   PID 复用既不能造成假接管也不能造成假拒绝（A03-F：伪造记录声称存活的无关系 pid=1，
   接管照常；反例：记录声称活 pid + 锁被持有 ⇒ 仍拒绝，单测
   `held_lock_with_live_foreign_pid_still_rejects`）。这一语义与现役 Node 闸的哲学一致
   （观察事实 2），但用了更强的原语：Node 靠 token 探测判死活，这里由内核代答，HTTP 健康
   探测只作为拒绝消息的诊断增强（500ms 预算，std 实现，永非权威）。
7. **实例身份与版本握手**：instanceId=16 字节、startNonce=8 字节十六进制（unix 读
   `/dev/urandom`；读失败降级 pid+纳秒 xorshift 混拌并在安全日志标注 entropy=pid-time-fallback）。
   记录快照单一版本权威（serverKind/serverVersion/wireProtocolMin/Max/dataEpoch 取自
   lingxi-protocol 常量）；接管陈旧记录时逐项比对，不一致记入 handshakeMismatches
   （如「data epoch 2 vs current 1」——信息性提示，**不阻断**：数据版本政策归 ADR-004 的
   epoch 闸，本模块不越权）。
8. **清理只删自己的记录**：正常停机时重读 instance.json，instanceId 相同才删；
   他人记录原样保留并告警（单测 `release_never_deletes_a_foreign_record` 防御性验证）。
   锁由 unlock()+drop 释放（进程退出亦释放）。
9. **启动链次序**（main.rs）：CLI 严格解析 →（组合根唯一 env 读取点）优先级解析 → 安全
   日志 → canonicalize/规范化目录/权限 → **取锁（在端口绑定之前）**→ 陈旧检测/身份生成 →
   bind → publish 记录（原子）→ READY 行 → serve → 停机清理。退出码契约：0 干净停机；
   1 serve 失败；2 配置/启动错误；3 单写者锁冲突（含锁 IO 失败）；4 停机清理失败。
10. **F03 收口（严格 CLI 语义）**：重复 flag = DuplicateArgument；`--` 开头的值 =
    FlagShapedValue（点名 flag 与值，不再误报 BadBind）；`--home=/x`/未知 token/位置参数 =
    UnknownArgument（钉住 T01 的响亮失败）；缺值 = MissingArgumentValue。全部有单测 +
    真实二进制负向探针（A04 case 6）。
11. **F01 收口（机器执法）**：DEP-08 增加
    `forbidden_source_tokens: ["std::env::", "env::var"]`（D3 源码扫描、注释剥离、纯数据
    驱动——观察事实 4），`added_by` 留痕指向 REVIEW_R1 F01。负向脚本注入
    `std::env::var` 到 kernel → 检查器 `FAIL [D3] DEP-08` 点名文件 → 备份字节还原（shasum
    核对）→ 复绿。lib 侧配套：环境值与 temp base 一律作为参数注入（`from_sources`），
    库本身不碰进程环境，优先级因此可无污染单测。
12. **与 ADR-004 的关系**：本任务的锁是新内核在自己根内的锁，ADR-004 D2 明言「新内核可以
    在自己的 epoch-2 根内使用任何新锁；任何安全性论证不得依赖旧程序认识这些锁」——本实现
    恰好不要求任何其他进程认识该锁（OS 语义），也不与旧 Node 的 server-info.json 互操作
    （R02 阶段边界：不切换现有用户目录；两个服务各锁各的合成根，互不触碰）。

## 5. 验收场景：命令 / 预期 / 实际 / 退出码

证据目录 `artifacts/rust-tauri/R02/T02/`（`.log` 被根 .gitignore 忽略、本机留存可重跑再生；
`.txt`/`.json` 摘要可入库——与 T01 惯例一致）。全部命令经
`env -u …_proxy PATH=~/.cargo/bin:$PATH` 且脚本内部强制 rustup 1.98.1 + 专属 target dir
`/tmp/rust-target-r02-t02` + `--locked` 离线。

### 5.1 R02-A03｜双实例拒写（REQUIRED）

命令：`bash scripts/rust-tauri/r02_t02_dual_instance.sh`（真实二进制进程 ×6：实例 1/2/2b/3/4
+ 构建）。结果 **exit 0**。

| 阶段 | 预期 | 实际 | 退出码 | 证据 |
|---|---|---|---|---|
| A 首实例持有目录 | READY+health 200+记录发布 | pid=15553 addr=127.0.0.1:62442；instanceId=021b69ac…；health 200 | 0 | a03-inst1-{stdout,stderr}.log、a03-inst1-health.json |
| B 第二实例同 home | 非零+明确诊断+无 READY | **exit 3**；`LINGXI_SERVICE_SINGLE_WRITER_BLOCKED home=/private/tmp/… recordedPid=15553 recordedInstanceId=021b69ac… recordedStartNonce=80c4e11d… recordedAddr=127.0.0.1:62442 probe=live authority=os-file-lock`；stdout 无 READY | 3 | a03-inst2-{stdout,stderr}.log、a03-inst2-marker.txt |
| C 首实例不受损+数据一致 | health 200 + 树哈希不变 | 拒绝前后 health 均 200；逐文件 SHA-256 树摘要 `8a28c733…e69bc77` 前后**逐字节一致**（「库校验」以 T02 现有数据面承载：instance.json/lock/tmp 全量文件） | 0 | a03-summary.txt、a03-inst2b-* |
| D 别名双启动（/private 前缀） | 同一 canonical 根 ⇒ 拒绝 | exit 3 + 同一 marker | 3 | a03-inst2b-stderr.log |
| E SIGKILL 崩溃→重启接管 | 陈旧识别+归档+干净停机 | 实例 3 启动成功并 health 200，stderr 含 `LINGXI_SERVICE_STALE_RECORD_TAKEN_OVER … previousInstanceId=021b69ac…`；`instance.stale.json` 内容 == 死者记录；SIGTERM → exit 0，**自己的** instance.json 已删、instance.lock 保留 | 0 | a03-inst3-{stdout,stderr}.log |
| F PID 复用对抗 | 记录声称活 pid（pid=1，launchd）+ 无锁 ⇒ 接管照常 | 实例 4 正常启动 + STALE marker；SIGTERM exit 0 | 0 | a03-forged-record.json、a03-inst4-stderr.log |

**判定（执行者结论，供验收）：达成**——两个真实进程互斥由 OS 文件锁证明；陈旧判定从不
读 PID 存在性；首实例数据在拒绝前后哈希一致。

### 5.2 R02-A04｜路径优先级不意外覆盖（REQUIRED）

命令：`bash scripts/rust-tauri/r02_t02_path_priority.sh`（真实二进制 ×5 正例 + ×7 负例）。
结果 **exit 0**。文件探针 = 服务运行期间 `<根>/lingxi-service/instance.json` 存在性 +
候选根（含 /private 别名）不存在性；解析断言 = READY 行 `source=`/`home=` + 安全日志
`data root resolved` 行。

| 案例 | 输入 | 实际选择（探针） | READY source | 证据 |
|---|---|---|---|---|
| 1 仅 CLI | `--home R_cli`（env 剥离、无 config） | 仅 R_cli 物化；R_env/R_cfg 不存在 | cli | a04-cli-* |
| 2 仅 env | `LINGXI_HOME=R_env` | 仅 R_env 物化 | env | a04-env-* |
| 3 仅配置 | `--config C.json` | 仅 R_cfg 物化 | config-file | a04-config-file-* |
| 4 冲突 CLI>env>配置 | 三者同给（互异根） | 仅 R_cli 物化；ignored_sources 日志含 `env(LINGXI_HOME)=R_env` 与 `config-file=C.json`（config 仍被校验——严格 JSON 通过） | cli | a04-conflict-* |
| 5 测试模式 | `--home R_cli2 + env R_env2 + --config C2.json + --test-mode` | **R_cli2/R_env2/R_cfg2 三者全程不存在**；实际根为 `$TMPDIR/lingxi-service-test-<pid>-<纳秒>` 且物化；ignored_sources 列全三项 | test-mode | a04-test-mode-* |
| 6 负向（7 种） | 无来源 / 相对 home / 重复 --home / `--home=/x` / `--bind --home /x` / config 缺失 / config 非法(/dev/null) | 全部 **exit 2**、无 READY、诊断明确（"missing data root"/"more than once"/"flag-shaped"…） | — | a04-neg-* |

**判定（执行者结论，供验收）：达成**——五类组合的实际根严格按声明协议选择，选择由文件
探针（非日志自述）证明；测试模式未触碰任何生产形态候选根。

### 5.3 F01 收口｜DEP-08 环境变量机器执法（R02-T01 REVIEW_R1）

改动：`docs/rust-tauri/R01/DEPENDENCY_RULES.json` DEP-08
`forbidden_source_tokens = ["std::env::", "env::var"]` + enforcement/added_by 留痕（检查器零
改动，纯数据生效）。

命令：`bash scripts/rust-tauri/r02_t02_f01_env_token_negative.sh` → **exit 0**：

| 阶段 | 实际 | 退出码 | 证据 |
|---|---|---|---|
| 基线绿 | `RESULT: OK` | 0 | f01-0-baseline-green.log |
| 注入 `std::env::var` 到 kernel | `FAIL [D3] DEP-08: forbidden token 'std::env::' in rust/crates/lingxi-kernel/src/lib.rs` | **1** | f01-1-injected-rejected.log |
| 还原 | 备份回写 + shasum 对比 pre-run 值一致 | 0 | （脚本内断言） |
| 复绿 | `RESULT: OK` | 0 | f01-2-restored-green.log |

过程事故（如实记录）：负向验证首跑用临时内联命令，restore 顺序缺陷（先置 DIRTY=0 再
restore，restore 成 no-op）把注入残留在了 kernel lib.rs，且首版脚本漏写 `cp` 备份导致一次
空文件覆盖；两处均以 `git checkout -- rust/crates/lingxi-kernel/src/lib.rs` 还原（该文件在
本任务中无任何其他改动，还原后与 HEAD 逐字节一致，shasum `cd5e89d0…` 双向核对）。修正后
脚本以「EXIT trap 先于清标记 + 备份字节还原 + pre-run shasum 校验」三重防护全流程通过，
终态 kernel 零 diff。与 T01 A02 首跑事故同类，教训一致：注入类脚本必须把回滚放在任何
标记清理之前。

### 5.4 F03 收口｜完整 CLI 语义（R02-T01 REVIEW_R1）

严格解析落在 `config.rs::parse_cli`（§4.10）；单测 7 个
（duplicate×2 / flag-shaped / equals-form+unknown / missing-value / 全 flag 解析 / 空参数）+
真实二进制 5 种负向探针（A04 case 6，全部 exit 2 无 READY）。原「响亮失败 exit 2」契约
保留并被钉死，静默 last-wins 与误报 BadBind 消除。

### 5.5 辅助检查（任务书 §4 固定方法第 4 步）

| 检查 | 命令（均 rustup 1.98.1 + /tmp/rust-target-r02-t02 + --locked） | 结果 | 退出码 | 证据 |
|---|---|---|---|---|
| fmt | `cargo fmt --all -- --check` | 无 diff | 0 | cargo-fmt-check.log |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 无告警 | 0 | cargo-clippy.log |
| 单测/集成 | `cargo test --workspace --locked` | **91 passed / 0 failed**（service lib 39 + 集成 instance_lifecycle 3 新增 + service_health 4 存量回归；kernel 7、protocol 19+1、spike 7、browser-spike 11 存量全绿） | 0 | cargo-test-workspace.log |
| 边界检查器 | `python3 -B …/r01_t01_check_ownership.py --self-test` | RESULT: OK（含 DEP-08 新 token 规则下逐模块 PASS + N1–N15 全拒） | 0 | check-ownership-selftest.log |
| schema 生成 | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 56 文件 + 624 API 项零漂移 | 0 | r01-t02-check-generated.log |
| T01 回归 | `r02_t01_service_smoke.sh` + `r02_t01_boundary_negative.sh` | A01/A02 全 PASS、注入还原零残留（READY 行加法扩展兼容其 `addr=[^ ]*` 解析） | 0 | /tmp 复跑后清理（可随时重跑再生） |
| 审计封印 | `npx vitest run tests/post-verification-audit-seal.test.ts` | **1/3 失败（预存在，非本任务造成）**——详见 §9 风险 5 | 1 | audit-seal-test.log |

## 6. 关键生产调用链（main → 配置解析 → 锁获取 → READY → shutdown 清理）

```text
rust/crates/lingxi-service/src/main.rs            # 入口：tracing(stderr, 非TTY无ANSI)
  ├ parse_cli(argv)                               # config.rs：严格 CLI（重复/flag形态值/未知token→exit 2）
  ├ std::env::var(LINGXI_HOME)                    # 组合根唯一环境读取点（DEP-08 禁 kernel 触碰 env）
  ├ ServiceConfig::from_sources(cli, env, temp)   # config.rs：优先级 test-mode > cli > env > config-file
  │   └ read_config_home(path)                    #   严格 JSON（eager：输掉优先级也校验）
  ├ tracing::info!("data root resolved" …)        # 安全日志：有效根+来源+被忽略来源
  ├ prepare_layout(&config.data_home)             # paths.rs：canonicalize+规范化目录+0700+tmp/
  │                                               #   （拒绝相对根//非目录；别名收敛）
  ├ acquire(&layout)                              # instance.rs：File::try_lock（OS权威）
  │   ├ Err HeldByPeer → SINGLE_WRITER_BLOCKED 标记+诊断 → exit 3
  │   └ Ok(guard, stale) → STALE_RECORD_TAKEN_OVER 标记+归档+版本握手比对
  ├ lingxi_service::run(config, shutdown_signal(), on_ready)
  │   ├ tokio TcpListener::bind                   # 锁在端口绑定之前
  │   └ on_ready(addr): guard.publish(addr)       # instance.json 原子写（tmp+fsync+rename+dirsync）
  │       └ println!("LINGXI_SERVICE_READY addr=… home=… source=…")
  ├ axum serve → GET /lingxi/v1/health            # T01 契约面不变
  └ 停机：guard.release()                         # 只删自己的记录（重读比对 instanceId）→ unlock
      → exit 0（清理失败 exit 4；serve 失败 exit 1）
```

## 7. 改动清单

**新增（生产/测试）**
- `rust/crates/lingxi-service/src/config.rs`（优先级解析器 + 严格 CLI + 严格 config 文件 +
  测试模式；22 单测）
- `rust/crates/lingxi-service/src/paths.rs`（canonicalization/规范化布局/0700 权限/原子写；
  6 单测）
- `rust/crates/lingxi-service/src/instance.rs`（实例身份/单写者锁/陈旧归档/清理/诊断探针；
  10 单测）
- `rust/crates/lingxi-service/tests/instance_lifecycle.rs`（集成层：真实 run 环路 × 锁 ×
  记录 × 健康，3 测试）
- `scripts/rust-tauri/r02_t02_dual_instance.sh`（A03 可重跑验收，真实双进程）
- `scripts/rust-tauri/r02_t02_path_priority.sh`（A04 可重跑验收，文件探针）
- `scripts/rust-tauri/r02_t02_f01_env_token_negative.sh`（F01 负向，注入→拒绝→字节还原→复绿）
- `artifacts/rust-tauri/R02/T02/`（§5 证据；.txt/.json 可入库，.log 本机留存）
- `docs/rust-tauri/R02/R02-T02_REPORT.md`（本报告）

**修改**
- `rust/crates/lingxi-service/src/lib.rs`（模块注册 config/instance/paths + 重导出；
  ServiceConfig 增 `home_source` 与 `from_sources`（env/temp 参数注入）；
  `from_cli_args` 保留为 T01 兼容面；run() 保持传输纯）
- `rust/crates/lingxi-service/src/main.rs`（新启动链：优先级/安全日志/布局/锁/记录发布/
  READY source= 字段/退出码 3 与 4/--config/--test-mode/USAGE 文档化）
- `rust/crates/lingxi-service/tests/service_health.rs`（ServiceConfig 字面量补
  home_source；断言未变）
- `docs/rust-tauri/R01/DEPENDENCY_RULES.json`（DEP-08 +forbidden_source_tokens，F01）

**零改动（含验证）**：`rust/Cargo.toml`、`rust/Cargo.lock`（零新依赖）、
`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/`、`contracts/generated/`、`.sync-audit/`、
`PROGRESS.md`、`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`、`package.json`、`desktop/`、
`server/`、`core/`、`lib/`、`shared/`、`tests/`、其余 R01 交付（`git status --short` 全集
见 §10）。

## 8. 测试列表（层级归属，01 §5 分层）

| 测试 | 层级 | 断言要点 |
|---|---|---|
| config: cli_rejects_duplicate_{home,bind_and_config_and_test_mode} / cli_rejects_flag_shaped_values… / cli_rejects_equals_form_and_unknown_tokens / cli_rejects_missing_values / cli_parses_all_flags / cli_empty_is_no_flags | 纯逻辑 | F03 严格 CLI 契约（重复/flag形态值/未知token/缺值/全flag/空参） |
| config: precedence_cli_wins_over_env_and_config / precedence_cli_wins_but_broken_config_is_still_loud / precedence_env_wins_over_config / precedence_only_cli_only_env_only_config_and_none / test_mode_overrides_all_sources_and_is_unique_per_call | 纯逻辑 | A04 解析断言半面：五类组合 + eager config 校验 + 测试模式唯一性 |
| config: config_file_{missing_is_loud,rejects_unknown_keys…,reads_absolute_home} / test_mode_home_name_is_unique_and_prefixed | 纯逻辑 | 严格 config 文件、测试目录名唯一 |
| paths: layout_creates_normalized_dirs_with_private_mode（含符号链接别名 canonicalize 等断言）/ layout_rejects_relative_and_root_and_file_as_home / layout_tightens_wide_runtime_dir / atomic_write_replaces_content_and_leaves_no_tmp / atomic_write_many_rounds_do_not_collide | 纯逻辑（真实文件系统，合成 /tmp） | 规范化布局/0700/别名收敛/非法根拒绝/原子写无残留 |
| instance: identity_is_unique_and_well_formed / second_acquire_in_process_is_rejected_with_diagnostics / stale_record_is_archived_and_taken_over_regardless_of_pid / stale_record_with_nonexistent_pid_is_still_taken_over / held_lock_with_live_foreign_pid_still_rejects / release_never_deletes_a_foreign_record / record_handshake_mismatch_is_reported / record_json_round_trips_camel_case / unreadable_leftover_record_is_replaced_not_fatal / probe_peer_reports_unreachable_for_dead_port | 纯逻辑（真实锁/文件系统） | 锁权威/PID 无关陈旧判定/仅删己有记录/版本握手/损坏记录不致命 |
| tests/instance_lifecycle: locked_instance_survives_rejected_second_claim / crashed_owner_leaves_stale_record_and_restart_takes_over / config_source_field_reaches_the_service_config | 契约/服务集成 | 真实 axum+TCP+锁+记录全链；诊断探针对活对端报 live |
| scripts/rust_t02_dual_instance.sh | 真实二进制进程 ×4 | A03 全链（同上 §5.1） |
| scripts/rust_t02_path_priority.sh | 真实二进制进程 ×12 | A04 全链（文件探针，§5.2） |
| scripts/rust_t02_f01_env_token_negative.sh | 负向门禁 | DEP-08 env token 注入拒绝+还原（§5.3） |

## 9. 未验证 / 风险 / 交接

**未验证（如实）**
1. 非 macOS 平台未编译验证：Windows 的 `File::try_lock`（LockFileEx）分支、权限检查的
   `cfg(not(unix))` 目录检查分支、原子写无目录 fsync 分支——源码已留分支并注释，属
   R09/R10 平台矩阵。
2. release 构建未跑（性能协议归 R10/T08）；长时运行/资源增长未测（R10）。
3. 「库校验」以实例记录/锁/运行时目录的逐文件哈希承载——T02 阶段尚无 SQLite 运行库
   （T04 交付）；A03 场景在 T04 后应随真实库回归扩证。
4. 诊断探针的 token 认证不存在（T03 交付前 health 无认证，T01 已登记风险）；当前探针仅
   以 serverKind 识别对端，且永非权威。
5. Node/现役入口回归未跑全量 npm test：本任务 Node 侧零改动且无测试引用 rust workspace
   （T01 观察事实 9 仍成立）；按 05 §2「未修改且无影响输入不无理由重复」不重跑，A16 属 T08。

**已知风险**
1. **审计封印测试 1/3 红（预存在，非本任务改动造成）**：
   `tests/post-verification-audit-seal.test.ts` 对 `git diff --name-only
   ab4f2281..HEAD` 执法，HEAD（42c49faa）已含总控提交的 R02-T01 交付（rust/Cargo.toml、
   Cargo.lock、lingxi-service、docs/rust-tauri/R02/* 等）与账本，而封印坐标（.sync-audit/
   verified-source-sha.txt）仍停在 ab4f2281——该测试只读已提交状态，本任务的未提交改动
   对其不可见，开工前即为红。按 AGENTS.md：如实报告、不虚报坐标、不扩白名单、不退役门禁；
   坐标推进属获准提交后的封印流程（PROGRESS.md），本执行代理无提交权限。
2. READY 行加法扩展（`source=` 字段）：T01 harness 的 `addr=[^ ]*` 解析实测兼容（T01 冒烟
   脚本复跑通过）；若后续验收认定该行应冻结为两字段，回退仅涉及 main.rs 一处 println。
3. 单写者锁是**协作式**的（对本服务自族有效）：旧 Node server / 其他进程不认识该锁文件
   （ADR-004 D2 第 3 条原话）；本任务的安全论证不依赖任何旧程序认识新锁，但跨程序族的
   物理隔离仍由 ADR-004 分离根/epoch 闸负责，不在本锁职责内。
4. 测试模式目录名熵 = pid+纳秒（无随机源）：对抗性预测强度有限，但 test-mode 目录本身不含
   秘密、且仅在显式 --test-mode 时使用；如后续需要更强不可预测性再引入 OS 随机（instanceId
   已用 /dev/urandom）。
5. `instance.stale.json` 只保留最近一份陈旧归档（有界，防无限增长）：更早的历史在安全日志
   的 takeover 标记行中留有 instanceId/时间戳级痕迹。
6. 审计封印红（见上 1）意味着「已验证坐标」叙述与当前分支事实存在未收口的账面差——这是
   R02-T01 提交后的既有状态，不影响本任务各门禁的真实退出码。

**交接（给 R02-T03 及后续）**
- `ServiceConfig{bind_addr, data_home, home_source}` + `prepare_layout()` 是路径/布局的
  唯一入口；T03 起的任何持久化都应落在 layout 指定的规范化目录内（tmp/ 供原子写）。
- `InstanceGuard` 由 main 持有；T06 的关闭协调器接管停机次序时应调用其 `release()` 语义
  （只删己有记录）。锁在端口绑定前获取的次序请保持。
- 机读 stderr 标记三件套（SINGLE_WRITER_BLOCKED / STALE_RECORD_TAKEN_OVER / READY source=）
  可供桌面壳/CLI 复用为诊断契约（对应 Node 的 LINGXI_* 标记风格）。
- A03/A04/F01 三脚本可作 T08 verify-stage 的登记命令候选。

## 10. 最终工作树状态

`git status --short`（全集，tested SHA = 42c49faaa + 以下未提交改动）：

```text
 M docs/rust-tauri/R01/DEPENDENCY_RULES.json
 M rust/crates/lingxi-service/src/lib.rs
 M rust/crates/lingxi-service/src/main.rs
 M rust/crates/lingxi-service/tests/service_health.rs
?? artifacts/rust-tauri/R02/T02/
?? rust/crates/lingxi-service/src/config.rs
?? rust/crates/lingxi-service/src/instance.rs
?? rust/crates/lingxi-service/src/paths.rs
?? rust/crates/lingxi-service/tests/instance_lifecycle.rs
?? scripts/rust-tauri/r02_t02_dual_instance.sh
?? scripts/rust-tauri/r02_t02_f01_env_token_negative.sh
?? scripts/rust-tauri/r02_t02_path_priority.sh
?? docs/rust-tauri/R02/R02-T02_REPORT.md
```

（kernel/protocol/spike/browser-spike/Cargo.lock/Cargo.toml/任务书/contracts/generated/
.sync-audit/PROGRESS.md/ORCHESTRATOR_PROGRESS.json 均零改动。）

## 11. 推荐独立验收重点

1. 复跑三脚本（真实进程证据链）：A03（重点看 marker 行的 recorded* 字段与两次树哈希一致）、
   A04（重点看 case 5 三个候选根的不存在探针与 test-mode 实际根物化）、F01（注入拒绝与
   字节还原）。
2. PID 无关性的对抗复核：篡改 `instance.json` 的 pid 字段（分别填活 pid/死 pid）后在锁自由
   与锁被持两种状态下启动，验证判定只随锁变。
3. DEP-08 数据改动授权性：仅 token 追加 + added_by 留痕（对比 `git diff
   docs/rust-tauri/R01/DEPENDENCY_RULES.json`），检查器源码零改动；--self-test 全绿。
4. READY 行兼容性：T01 冒烟脚本复跑（本报告已跑，验收可复核）。
5. 退出码契约表（0/1/2/3/4）与 USAGE 文档一致性。
6. 审计封印红（§9.1）按预存在项核对：开工基线即可复现（stash 本任务改动后仍红）。
