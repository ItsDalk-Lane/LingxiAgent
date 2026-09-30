# R04-T04 报告｜原生读写改文件与资源权限

执行代理：EXECUTOR-R04-T04-E01（一次性执行代理，ZCode Agent 工具派发）。
日期：2026-09-30。分支 `codex/rust-tauri-migration`，基线 `c72e0fa02`（R04-T03 已独立
PASS 并推送确认）。工作区起点仅含本派单文件（未跟踪，非用户已提交修改，未触碰）。
状态：**READY_FOR_REVIEW**（普通逐项 + 对抗性两层自查完成；不自行判独立 PASS）。

---

## 1. 目标与结论

文件工具的正确性包含并发修改和路径边界（任务书 R04-T04）：read 的分页/编码/二进制/
截断语义、write/edit 的匹配/冲突语义（不默认静默覆盖）在 Rust 中原生完成；ResourceRef
（`ResourceScope`）与路径授权（工作区＋主体）连到同一资源判定，真实目标验证（符号链接
跟踪到真实路径再判权，不信字符串前缀）；写入走同目录临时文件＋原子替换，检查到使用的
间隙用目录相对打开（`openat`+`O_NOFOLLOW` 组件走查）＋换前身份复验约束 TOCTOU；写
入失败不登记成功产物；保留 checkpoint/rewind 需要的显式修改记录接口；read/write/edit
作为真实注册工具经 T02 网关 prepare→策略/批准（T03 面）→执行，资源范围进入批准记录，
并补上 T03 审查 OBS-1 的真实别名解析腿。

输入缺口如实登记：派单要求必读的总控提示词
`/Users/study_superior/Downloads/Lingxi_R04_自动执行总控提示词_2026-09-30.md`
**在本机不存在**（Downloads 无此文件）。与 T03 审查（R04-T03_REVIEW_R1.md 输入说明）
相同的处置：以派单全文（内嵌任务要点与验收原文）、任务书 R04-T04 节 + 共同规范
01/02/05、R04_SCOPE_MATRIX、T01-T03 报告与 T03 审查 OBS-1/OBS-4 转述的 §4.1/§4.2
调用链（目标解析→可用性→参数校验→资源规范化→权限→批准→临近执行重检→执行）为准，
未构成阻塞。

结论：全部当前到期义务已实现并真实执行（§5 验证，全部退出码在案）；A07/A08 两个基础
验收场景与追加对抗项在真实文件系统隔离测试根上通过；R03 行为不变量经 workspace 全量
+ 十套件钉数 + A15/A16 复验无回归。

## 2. 实现摘要（关键文件:行）

### 2.1 ResourceAccess / ResourceScope（核心交付一）

- `rust/crates/lingxi-service/src/resourceaccess.rs`（新文件，760 行含 7 单测）：
  - `ResourceOp`（L63）/`ResourceScope`（L81：canonical 真实路径 + 操作）——后续所有
    阶段绑定的同一资源判定值：网关 prepared 记录、批准记录、执行前重检全部消费它。
  - `ResourceAccess`（L178）：workspace roots（实例级读写）＋ per-(principal, session)
    授权根（`grant_root` L214，读根/写根分级；`revoke_grants`）——一个会话的授权永不
    扩大另一个会话/主体的访问（对应现役 authorizedFolders/getExternalReadPaths 的
    session 作用域语义）。
  - `authorize`（L279）：词法绝对化（相对 cwd）→ `resolve_real`（L411：符号链接全程
    跟踪；目标不存在时走到最近存在祖先 canonical 化后拼回缺失尾段——现役
    `_resolveReal` 语义，mkdir -p 形态可判权）→ 对 ROOT 也 canonical 后**按组件**
    匹配（`is_inside` L442 用 `strip_prefix`，`/tmp/ws` 永不授权 `/tmp/ws-secret`——
    非字符串前缀）。拒绝分三因（L98）：`outside_write_scope`（读根内写）/`blocked`/
    `unresolvable`，拒绝文案携带**真实目标路径**并给出出路——与 path-guard 的
    cause/文案结构对应。
  - Windows 形态（L455 `is_reparse_point`，cfg(windows)，含 cfg 门控单测）：junction/
    reparse 检测面登记；canonicalize 同样解析 junction；真机 Windows 验证保持阶段递延
    （SCOPE_MATRIX governance_deferrals 口径）。
  - 本机（macOS）实测形态：符号链接（单测 `symlink_is_judged_by_its_real_target`：越
    权链路拒于真实目标、链内授权链解析到真实路径）、相对路径/`..`、大小写不敏感文件
    系统解析到真实磁盘拼写（`case_insensitive_filesystems_resolve_to_the_real_casing`）。

### 2.2 原生 read/write/edit 执行器（核心交付二/三/四）

- `rust/crates/lingxi-service/src/filetools.rs`（新文件，2411 行）：
  - **read**（`run_read` L1506 + `render_text_read` L2075 + `truncate_head` L1159）：
    现役 `read.js` 语义——1-indexed offset/limit；offset 越界显式错误（含 split 的幻影
    行计数，与现役逐字一致）；2000 行/50KiB 双限截断（整行、绝不半行），续读提示
    `[Showing lines X-Y of Z. Use offset=N to continue.]` / `[N more lines in file. …]`，
    `truncated` 标志进结构化结果；首行超限的专门提示（现役 bash 回退文案按 Rust 栈现
    状改写为 offset 出路，登记为映射决策）；UTF-8 BOM 剥离；非 UTF-8/二进制不回乱码
    ——返回尺寸+sha256 的诚实元数据结果（GBK/xlsx/docx 解码登记未迁移，见 §8）；
    紧邻同身份重复读取返回现役 `[Duplicate read: …]` 存根（30s 窗口+stat 未变）。
  - **write**（`run_write` L1630）：父目录创建、UTF-8 写入、`Successfully wrote to
    {path}`；对已读文件的外部改动返回 `FILE_STALE_SINCE_READ` 冲突（现役
    wrapMutationToolWithFreshness 文案）——零副作用；成功产出 existence-verified
    `ResourceRef`（file:// URI + sha256 + 尺寸，Artifact kind——session-file 产品登记归
    R06，映射决策见 §8）。
  - **edit**（`run_edit` L1704 + `apply_edits` L941）：`edits[]` 对原文匹配（非增量）；
    BOM 剥离匹配/回写保留；CRLF 检测→LF 匹配→回写还原；精确匹配优先，失败走简化模糊
    层（行尾空白+弯引号/长破折号/Unicode 空格折叠；**NFKC 未迁移**——登记缺口，仅
    NFKC 差异回落精确匹配错误）＋未改行保真叠加（`apply_replacements_preserving_
    unchanged_lines`，逐行 span 组算法的移植）；错误词表逐字保留：not-found / `Found
    N occurrences` / 空 oldText / overlap / no-change / `Could not edit file: … ENOENT`。
  - **原子写与并发**（`finish_mutation` L1838）：同目录 `openat(O_CREAT|O_EXCL)` 临时
    文件（随机名，熵失败降级为进程内单调计数名——绝不可猜测名）→ 写入+fsync → 原权限
    位保留（fchmod）→ **换前身份复验**（同一 dirfd `O_NOFOLLOW` 重开，(dev,ino,mtime,
    size) 必须等于读到的身份——外部换入新 inode 或同 inode 修改均判
    `FILE_CHANGED_DURING_MUTATION` 冲突，永不丢更新）→ 单次 `renameat` 原子替换 →
    目录 fsync；`TempGuard`（L1248）RAII——早退/冲突/错误/**panic** 路径全部清理临时
    文件；per-canonical-path 互斥（现役 withFileMutationQueue 语义，有界注册表）。
  - **TOCTOU 可证明机制**（`open_with_churn` L1395 + fsio L416-607）：Unix 下从 `/`
    开始逐组件 `openat(O_DIRECTORY|O_NOFOLLOW)` 走查 canonical 父链（canonical 路径
    本不含符号链接，任何 ELOOP 都是授权后的替换）→ 最终组件 `openat(O_RDONLY|
    O_NOFOLLOW)` → 链接搅动时**重新解析真实目标并重新授权**（≤8 轮，超出报
    `RESOURCE_SYMLINK_CHURN`）。不是「canonicalize 一次就声称无竞态」。
  - **失败诚实性**：写失败/权限/中断不登记成功产物——变更日志零记录、零 ResourceRef
    （测试断言 records 不变）；after-fingerprint stat 失败返回 Internal「视为 UNKNOWN」
    而非成功。`FileModificationRecord`/`FileChangeLog`（L370/L382）是 checkpoint/rewind
    的显式接口（Created/Modified、before/after 版本 digest+size+mtime；完整产品归
    R06/R07）。
  - **Windows 形态**（fsio cfg(windows) L609-706）：std 回退——reparse/符号链接最终
    组件在变更路径上检测并拒绝；O_NOFOLLOW 走查不可用（开局链接替换竞态未闭合）——
    登记为 Windows 形态，真机验证递延。
  - `MutationTestHook`（L1225）：模块文档化的测试专用接缝（变更窗口内触发；生产构造
    不设置），使 FILE_CHANGED_DURING_MUTATION 可确定性验证——scope matrix
    test_hooks 许可口径。

### 2.3 网关集成与资源范围（核心交付五）

- `rust/crates/lingxi-service/src/toolgateway.rs`：
  - `ResourceExtractionInput`（L93，主体事实来自可信入口）+ `ResourceExtractor`
    （L107，由 effective 参数推导+授权真实资源范围的派生函数）；`bind_executor_
    with_resources`（L755）绑定执行器与其资源派生（旧 `bind_executor` 保持，无派生）。
  - **prepare 阶段资源规范化**（L839）：按冻结链（目标解析→可用性→参数校验→**资源
    规范化**→权限→批准）在策略裁决之前运行派生；未授权范围=响亮的
    `GatewayRefusal::ResourceScopeDenied`（L408，code `gateway_resource_scope_denied`）
    ——**批准人永远不会被请求批准资源边界本就会拒绝的路径**，且零派发。
  - **execute 阶段临近重检**（L1193 4b）：同一派生器对当前文件系统重推导，必须逐项
    复现 prepared 记录绑定的 (canonical path, op)，否则
    `ResourceScopeChanged`（L411，`gateway_resource_scope_changed`）零派发——批准针对
    的是旧的真目标，不是现在在那里的东西。
  - `PreparedInvocation.resources`（L620 起）与 `PreparedRecord.resources`（L655）。
- `rust/crates/lingxi-service/src/approval.rs` L53 / `approval_service.rs` L241 /
  `runs.rs` L1493：`ApprovalRequest/ApprovalRecord/PendingView` 携带真实资源范围，
  驱动把 prepared.resources 传入批准面；铸造审计日志输出全部资源范围（path:op）。
  A05 语义对文件路径成立：批准记录绑定 canonical 写范围；不同文件=不同 digest/不同记
  录（T03 证明 + 本 Task pending.resources 断言）。
- `rust/crates/lingxi-service/src/lib.rs`：`pub mod resourceaccess/filetools` + 再导出。
- **注册面**：`register_core_file_tools`（filetools.rs L2237）——read（Read 类）/write/
  edit（Execute 类）真实 manifest + 真实执行器 + 资源派生绑定；生产默认 bootstrap 不
  调用（生产默认入口零变化）。
- **T03 OBS-1 关闭腿**：`r04_t04_file_tools.rs`
  `approval_record_binds_real_file_scope_and_alias_resolves_to_the_executor`——经真实
  网关 `prepare(ToolTargetRef::ByName{alias})` 的**真别名解析**到真实 read 执行器
  （非字面 target id 双腿）；OBS-2 的澄清：批准/预授权的路径级绑定即记录的
  resources+digest 组合（capability_base 由 target 唯一决定，不构成请求面可用的判别，
  维持 T03 登记口径）。

### 2.4 依赖与 TS/Node 面

- 无新增 crate 依赖（Cargo.lock/Cargo.toml 零变化；libc/sha2/serde_json 均为既有锁定
  版本边）。现役 TS 栈未触碰（仅语义参照）；npm 侧检查按「如触碰」条件不适用。

## 3. 调用链与修改范围

真实调用链：provider `ToolRequests` → 驱动 digest 门 → **网关 prepare**（目标/代次/
schema 校验+规范化 → **资源规范化（新：authorize→ResourceScope，拒绝=零派发）** →
策略裁决（T03 面）→ prepared 记录绑定 resources）→ journal intent → RunGrant 授权
判定（未动）→ 批准需求（Allowed 直进 / NeedsApproval 等待面——**批准请求携带
resources**）→ journal authorized/started → **网关 execute_prepared**（身份/单次/注册
表复验 → **资源范围重推导复验（新）** → 真实文件执行器：授权→dirfd 走查→新鲜度/身份
冲突检查→原子替换→修改记录/ResourceRef）→ fence→收据。授权判定点/journal 写序零改动
（SUP-02 保持）；B1 静态面未触碰（新文件不引用受保护符号）。

修改文件清单：
- 新增：`rust/crates/lingxi-service/src/resourceaccess.rs`（+7 单测）、
  `rust/crates/lingxi-service/src/filetools.rs`、
  `rust/crates/lingxi-service/tests/r04_t04_file_tools.rs`（9 集成验收/对抗测试）、
  `artifacts/rust-tauri/R04/T04-E01/**`（证据+复跑脚本）、本报告。
- 修改（产品）：`lingxi-service/src/{toolgateway.rs,approval.rs,approval_service.rs,
  runs.rs,lib.rs}`。
- 修改（测试适配，断言不降级）：`tests/r04_t03_approval_service.rs`（ApprovalRequest
  字面量补 `resources: Vec::new()`——等价更新，3 处；全部断言原样，11/11 复绿）。
- 修改（文档）：`docs/rust-tauri/R04/R04_TEST_MAP.json`（T04 条目）。
- 未触碰：生产默认入口（ServiceDeps 默认 tool_gateway=None 的 R03 形状）、kernel
  crate（词表/判定零变化）、协议 wire 面（check-contracts 626 entries 零漂移）、xtask
  stage maps、check 脚本、`ORCHESTRATOR_PROGRESS.json`。

## 4. 验收场景与逐项结果（真实链：bootstrap_with_deps 组合根→真实驱动→真实 journal→
真实注册表→真实网关→真实 ApprovalService→真实文件执行器；真实文件系统隔离测试根
（workspace/外部读根/受限哨兵）；替身仅外部答复者/外部模型脚本/扮演用户的外部写者）

| ID | 要求 | 实现/测试 | 结果 |
|---|---|---|---|
| R04-A07 | 工具按版本1准备编辑，用户改成版本2：返回冲突、版本2不丢、授权重读后可重试 | `r04_a07_concurrent_edit_never_overwrites_the_user_version`（全链）：read v1（登记指纹）→**用户在轮间写 v2**（外部改写者）→edit-per-v1：journal Failed 收据 detail 含 FILE_STALE_SINCE_READ、**dispatched=true**（工具真实执行并拒绝——与 R03 收据语义一致，非静默跳过）；文件仍为 v2 字节 →重读（新指纹，非重复存根——stat 已变）→edit oldText=BETA-v2 成功；最终文件 `alpha\nBETA-final\ngamma\n`（v2 内容进入最终态，未丢）；无临时残留 | PASS |
| R04-A08 | 符号链接不能逃逸授权目录：按真实目标判权，越权零副作用 | `r04_a08_symlinks_are_judged_by_their_real_targets`（直连网关两阶段）：工作区链接→受限哨兵：read/write/edit 三腿全部 `gateway_resource_scope_denied`（prepare 拒绝，零派发），哨兵字节不变、受限区文件数不变、无临时残留；→外部读根链接（已授 read）：读成功（真实目标在授权根内）、写拒 `authorized for reads only`（outside_write_scope）、原文不变；→工作区内链接：读写作用于**真实目标**且链接本身保持为链接。全链腿 `r04_a08_resource_refusal_on_the_full_chain_is_never_dispatched`：never-dispatched Failed 收据+detail 含 gateway_resource_scope_denied+哨兵不变 | PASS |
| 对抗·相似前缀/上级切换 | `/tmp/ws` vs `/tmp/ws-secret`；目录级符号链接切换 | `adversarial_prefix_parent_switch_and_unicode_names`：`../ws-secret/` 写拒（资源拒绝）+兄弟目录内容不变+未在其外创建父目录；`switchdir`（→restricted 的目录链接）下读拒于真实目标；合法对照：空格/中文/emoji 文件名 write-read-edit 全程成功（`文件 名 称 📄 with spaces.txt`） | PASS |
| 对抗·CRLF/长尾/重复匹配/非法输入 | 现役语义逐项 | `adversarial_read_pagination_truncation_and_edit_vocabulary`：offset+limit＋remaining 提示（含幻影行计数，与现役 split 语义一致）；offset 越界错误；2500 行截断（`1-2000 of 2501` 续读提示+truncated 标志；offset=2001 续读不截断）；60KB 单行专门提示；紧邻重复读存根；BOM 剥离；二进制 5 字节诚实元数据；edit 词表全项（Found 3 occurrences/not-found/empty/overlap/no-change/ENOENT，文件逐项不动）；CRLF 编辑保端；弯引号模糊匹配（`it's "quoted" - dash` → 命中 `it’s “quoted” — dash`）；BOM 保留；双编辑单调用；schema 未知键拒绝+合法对照 | PASS |
| 对抗·临时文件清理 | 原子替换无残留 | `assert_no_temp_residue` 递归扫描全测试根：成功/冲突/swap 失败后均零 `.lingxi-write-*`（全部 9 测试内嵌断言）；panic 路径由 `TempGuard` Drop 语义结构性覆盖（错误/冲突路径已测，panic 路径未单测——如实区分） | PASS |
| 对抗·TOCTOU | 检查到使用的间隙 | `adversarial_mid_mutation_swap_is_a_conflict_not_a_lost_update`（确定性，经文档化测试接缝）：读后换前窗口内外部原子替换文件（新 inode）→edit/write 均判 `FILE_CHANGED_DURING_MUTATION`、外部内容原样保留、变更日志零记录、临时清理 | PASS |
| 批准记录资源范围（A05 对文件路径） | 批准绑定具体动作的资源集合 | ask 会话写调用 park→PendingView.resources=[canonical 路径 write]（且路径在 workspace 内；args_summary 的零值泄漏由 T01 `summarize_arguments` 边界保证，本轮未重复断言）→批准→文件按该范围写入→journal Succeeded | PASS |
| T03 OBS-1 | 真别名解析腿 | 同测试后半：经 `prepare(ByName{alias})` 到真实 read 执行器（真文件内容返回）；canonical 名与别名 prepare 得同一 target id；伪句柄拒绝 | PASS（另：OBS-3 的单条件收窄建议不属本 Task 测试文件，维持登记） |
| 只读会话（T03 面作用于真实文件工具） | read_only 拒写/放读 | `read_only_session_denies_file_writes_at_the_policy_face`：写=`ACTION_BLOCKED_BY_READ_ONLY`+dispatched=false+无文件；同 run 读=Succeeded | PASS |
| 修改记录接口 | checkpoint/rewind 预留 | FileChangeLog 捕获：Created（before=None）/Modified（before digest==上次 after digest）；失败写零记录；>8MiB before-digest 截断为显式 None（`adversarial_write_records_conflicts_and_atomicity`） | PASS |

## 5. 验证（真实命令与退出码）

环境：macOS darwin 27.0.0 arm64；rustup 1.98.1（`/Users/study_superior/.cargo/bin/
cargo`）。仓库根执行；复跑脚本 `artifacts/rust-tauri/R04/T04-E01/run_t04_validation.sh`
（绝对路径口径）；以下为**最终候选（代码冻结后）**一轮（复跑脚本 exit-codes.txt +
gates/ 日志归档；R03 回归证据在 regression/）：

| # | 命令 | 退出码 | 备注 |
|---|---|---|---|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 0 | **804 passed / 0 failed / 0 ignored**（77 个 ok 二进制复算；T03 轮 788 + T04 集成 9 + resourceaccess 单测 7）；r00_management_leaves 本轮通过（60s+ 警告行在卷——T01/T02 登记的防火墙间歇环境项，保留登记不销项） |
| 4 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | 0 | 626 entries 零漂移（未触碰协议面） |
| 5 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | 0 | O1–O8+D1–D5+B1；`--self-test`（N1–N17）另跑 exit 0（新文件不触受保护符号） |
| 6 | `cargo test --manifest-path rust/Cargo.toml -p lingxi-service --locked --test r04_t04_file_tools -- --nocapture` | 0 | 9/9（A07/A08×2+六对抗） |
| 7 | `cargo test … --test r04_t03_approval_service` | 0 | 11/11（ApprovalRequest 等价更新后复绿） |
| 8 | `cargo test … --test r04_t02_tool_gateway` | 0 | 13/13 |
| 9 | `cargo test … --test r04_t01_tool_catalog` | 0 | 6/6 |
| 10 | `cargo test … -p lingxi-kernel --locked --lib` | 0 | 77（kernel 零改动） |
| 11 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh <fresh dir>` | 0 | 十套件钉数精确全绿 |
| 12 | `bash scripts/rust-tauri/r03_t08_matrix.sh <abs dir>` | 0 | A15 28 叶+11 组合全绿 |
| 13 | `bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh <abs dir>` | 0 | A16 四相全绿 |

npm/TS 侧未运行：本 Task 未触碰任何 TS/Node 源（§2.4），按派单「如触碰」条件不适用。
T04 套件连续三轮全绿（并行无 flake；harness 测试根/家目录按 pid+纳秒+序号唯一）。

## 6. R03 行为不变量与回归证明（R04-SUP-05）

- 授权判定点/journal 写序零改动：网关 prepare 的新资源规范化位于 intent 之前（拒绝=
  never-dispatched 收据）；执行前资源重检位于 dispatch 之前（Err=零派发）；驱动
  runs.rs 仅新增 resources 传参。workspace 全量 804/0/0＋十套件钉数＋A15/A16 全绿。
- T03 成果复验：11/11（含预授权/别名/O05/SUP-01 矩阵）；T02 13/13；T01 6/6。
- kernel crate 零改动（diff 无 kernel 文件；kernel lib 77 全绿）。

## 7. 两层自查记录（普通→对抗）

普通逐项：§4 表逐 ID 给出前提/动作/预期/实际/退出码；每个拒绝场景都有合法对照（工作
区内读写、读根读、链接内目标操作、合法 offset/unique edit/unicode 文件名——拒绝全
部不是安全绿灯）。

对抗性自查（推翻自己的尝试与结果）：
1. **session_of 漏插入（发现并修复）**：初版 `session_of` 只做淘汰簿记未把新 session
   插入 map——首个 mutation 即 panic。修复后全套件复绿。
2. **读根授权漏返回（发现并修复）**：初版授权循环对「读根+读操作」落穿到拒绝——
   A08 的 doc-link 合法读腿暴露。修复：read_authorized+Read 显式放行。
3. **幻影行计数**：截断提示 `of 2501`/`7 more lines` 与直觉差 1——核对现役
   `split("\n")` 语义确认幻影元素被同样计数，属逐字迁移而非缺陷；测试按现役断言。
4. **「跨进程符号链接竞态真的闭了吗？」**：canonicalize 后任何组件被替换——dirfd
   逐组件 O_NOFOLLOW 走查闭合最终与中间组件的替换（ELOOP→重新解析+**重新授权**）；
   剩余窗口=身份复验到 renameat 之间，以 (dev,ino,mtime,size) 复验收窄（同 inode 修改
   也会变 mtime/size）。诚实边界：stat 粒度内的极窄窗口与 renameat-fstatat 原子性受
   OS 语义限制，未宣称绝对无竞态——机制与边界都写进模块文档。
5. **「批准面会不会看到越权路径？」**：资源规范化在策略/批准**之前**（冻结链顺序），
   越权路径在 prepare 即拒——不存在「用户批准后才被 PathGuard 拒」的浪费弹窗（与现役
   wrapper→ops 顺序不同方向更严，登记为顺序决策）。
6. **并发自写**：同进程多写者经 per-canonical-path 互斥串行（现役语义）；跨进程写者
   由身份复验判冲突（TOCTOU 测试证明）。
7. **「测试是否只能靠 MutationTestHook 过关？」**：FILE_CHANGED_DURING_MUTATION 的
   确定性验证需要窗口内注入——接缝只注入外部副作用（模拟外部写者），被测的检查/冲
   突/清理逻辑全部是生产代码；A07 的用户改版本走真实轮间外部写，不依赖接缝。
8. **「文件真的没被静默降级？」**：GBK/二进制不猜编码、大文件 before-digest 显式
   None、after-stat 失败返回 UNKNOWN 文案——三处都显式登记而非吞掉。

## 8. 未验证项与风险（如实登记）

1. **总控提示词文件缺失**（§1）：§4.1/§7.T04 原文未能直接读取；以派单+任务书+前序
   报告转述为准。若总控原文对 T04 有超出派单转述的要求，需在审查轮对照。
2. **跨平台**：仅 macOS arm64 实测。Windows：junction/UNC 的 cfg 形态与 reparse 检测
   已登记编译面（本机不编译执行 windows 目标的测试）；std 回退的开局链接替换竞态未
   闭合——按 SCOPE_MATRIX Windows 递延口径登记，不伪造验证。Linux：O_NOFOLLOW/
   renameat 语义同族，未在本机实测（登记）。
3. **映射决策——编码/office/图像**：read 对非 UTF-8 返回诚实元数据（不转 GBK）；
   xlsx/docx 解析与图像附件是 worker/多模态面（R05+/R07）；登记未迁移，不静默降级。
   首行超限提示的 bash 回退改写为 offset 出路（Rust 栈尚无 bash 工具）。
4. **映射决策——资源 ACL 与会话模式的分层**：会话 read_only/ask/operate 判定在 T03
   策略面（未动）；路径 ACL（workspace/grant）在资源边界与执行器——对应现役
   session-permission-wrapper（外）与 PathGuard（ops 内）分层。PathGuard 的完整
   Lingxi-home/agent-dir/blocked 词表与 full-access 模式归 T06 沙盒（派单明示）。
5. **映射决策——ResourceKind.Artifact**：write/edit 产出的 ResourceRef 用 Artifact
   kind+file:// URI；session-file 产品登记（现役 recordFileOperation/serializeSessionFile）
   归 R06。write before-digest 捕获上限 8MiB（超限显式 None）。
6. **映射决策——NFKC 未迁移**：edit 模糊层无 NFKC（锁定树无 unicode-normalization
   crate）；仅 NFKC 类差异（如全角）回落精确匹配错误。弯引号/破折号/行尾空白照旧。
7. **中断语义**：执行器为单段阻塞工作（spawn_blocking），取消树在 await 点切断——
   in-flight 变更完成后落盘但无收据→journal started-无收据=R03 UNKNOWN 分类（契约一
   致）；进程级硬杀在临时文件创建与 rename 之间的残留由 RAII 清理（panic 路径），
   SIGKILL 级不可清理（OS 事实，如实说明）。
8. **磁盘满专项**：写失败路径（写/flush/replace 错误）全部原文件不动+零记录+临时清
   理（错误分支在案）；未单独模拟满盘设备（无真实满盘环境；同一代码路径，登记）。
9. **r00 环境项**：本轮通过，保留 T01/T02 登记（防火墙间歇拦截未签名测试二进制）。
10. **执行器超时/并发上限**：manifest timeout_ms/max_concurrency 目前仅是目录元数据，
    网关未强制（T05/T08 面登记）。
11. **测试替身边界**：StepsProvider（外部模型响应+扮演用户的外部写者）、
    ChangeLogCapture（记录接口消费）、MutationTestHook（文档化测试接缝，仅注入外部副
    作用）——资源判定、网关、批准、journal、新鲜度/冲突、原子替换全部真实实现，无替
    身替代被测面。

## 9. 回退

回退范围=本 Task 获准修改（§3 清单）：删除 `resourceaccess.rs`/`filetools.rs`/
`r04_t04_file_tools.rs`，还原 `toolgateway.rs`/`approval.rs`/`approval_service.rs`/
`runs.rs`/`lib.rs` 与 T03 测试的等价更新、TEST_MAP T04 条目即回到基线 c72e0fa02 形态。
网关/批准的扩展全部是加字段/加方法/加拒绝分支（旧 `bind_executor` 行为不变；无资源派
生的目标 resources 为空、一切 T01-T03 路径不触新逻辑）；删除不影响任何现有生产路径。

## 10. 证据索引

- `artifacts/rust-tauri/R04/T04-E01/run_t04_validation.sh`——复跑脚本（绝对路径口径；
  首版两处脚本缺陷——repo-root 层级与 per-suite manifest 路径——已修复后最终轮全
  绿，缺陷与修复如实登记）。
- `gates/`：rust_fmt_check / rust_clippy / workspace_test / check_contracts /
  check_boundaries / r04_t04|t03|t02|t01_acceptance_tests / kernel_lib_tests /
  r03_g07_repair_suites / r03_t08_matrix_a15 / r03_t08_a16_seed 各 .log +
  exit-codes.txt。
- `workspace-test-cargo-test-workspace-locked.log`——冻结前一轮全工作区原始输出
  （804/0/0，r00 通过行在卷）。
- `regression/`——R03 十套件/A15/A16 复跑产物（最终轮）。
- 新测试文件：`rust/crates/lingxi-service/tests/r04_t04_file_tools.rs`；
  resourceaccess 单测内嵌 `rust/crates/lingxi-service/src/resourceaccess.rs`。
