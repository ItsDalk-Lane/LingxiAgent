# R02-T08｜独立服务交付与门禁 — 独立对抗性验收报告（R1）

- 审阅者：REVIEWER-R02-T08-R1（独立验收代理；未参与实现；不信任执行者声明，全部关键证据亲自重跑/重读/重算）
- 日期：2026-09-27
- 审阅对象：TASK_BASE_SHA `5741989165fe7e04c9a58a9d35c7747d3599d274`（= 分支
  codex/rust-tauri-migration HEAD = T08 开工基线）+ 未提交工作树
- 平台：macOS 27.0.0 arm64（Darwin 27.0.0）；rustup 1.29.1 + 锁定 rustc/cargo
  1.98.1（`rust-toolchain.toml` 在仓库根，亲核；任务简报所写
  `rust/crates/rust-toolchain.toml` 路径不存在，实际文件为仓库根
  `rust-toolchain.toml`，channel=1.98.1）；cargo 全程 `--locked` +
  `CARGO_NET_OFFLINE=true` + 专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t08-review`；
  网络命令剥代理
- 判定依据：任务书 `R02_…md` §4 R02-T08、task-catalog `R02-T08`、acceptance-catalog
  `R02-A15`/`R02-A16`（均 REQUIRED）、01/02/05/06/91 共同必读（05 §3 xtask 接口
  契约逐句核对）、R01 RISK_REGISTER（RR-T08-F1 / RR-T02-FINFO1 原文）、
  R02 T01–T07 报告与全部既有 REVIEW（移交修正清单逐项对质）
- 结论先行：**VERDICT: PASS**（0 BLOCKING / 3 MINOR / 4 NON-ISSUE；A15/A16 亲跑
  真实通过；xtask 全对抗矩阵真实；RR-T08-F1 错绑绿在本代理自建克隆上完整复现并
  验证硬化有效；反掩码独立成立）

---

## 1. 验证命令清单及退出码（全部亲跑）

| # | 命令（环境：剥代理 + `CARGO_NET_OFFLINE=true` + `--locked` + 专属 target） | 退出码 |
|---|---|---|
| V1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check`（rustup run 1.98.1） | 0 |
| V2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 |
| V3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast` | 0（亲数：**287 passed / 0 failed / 39 个 "test result: ok" 行**，与声称一致） |
| V4 | `cargo test -p xtask`（负向单测 16 例） | 0（16 passed） |
| V5 | `cargo run -p xtask -- check-contracts` | 0（56 文件 / 624 条目零漂移，路径正确指向本仓） |
| V6 | `cargo run -p xtask -- check-boundaries` | 0（RESULT: OK；D5 反向闭包含 xtask=exists） |
| V7 | `xtask verify-stage R02 --evidence /tmp/r02t08-review/verify-stage`（本代理自建全新证据目录） | 0（**overall PASS，16/16**；逐命令真实退出码 0、时长真实：a16=136.4s、a05_a06=51.0s、a14=33.1s；testedSha=574198916…、worktreeDirty=true、platform=macos/aarch64、toolchain=1.98.1） |
| V8 | `bash scripts/rust-tauri/r02_t08_full_chain_smoke.sh /tmp/r02t08-review/a15-standalone`（`CARGO_TARGET_DIR` 指向**本代理自建** target） | 0（26 条 PASS 行，全链真实） |
| V9 | A16（在 V7 内以本代理证据目录真实执行，含全量 npm test） | 0（npm test 原始 exit 1，分类后收敛；详见 §4） |
| V10 | `bash scripts/rust-tauri/r01-t02-check-generated.sh`（本仓，硬化后） | 0（派生 target `/tmp/lingxi-r01t02-target-2264d3b2b2c370e8`，后缀=本仓路径 SHA-256 前 16 位，亲算一致；日志 "under /Users/.../LingxiAgent/contracts/generated"） |
| V11 | `bash scripts/rust-tauri/r01-t02-roundtrip.sh /tmp/…/roundtrip-out` | 0（12 golden 样本字节相等） |
| V12 | `bash scripts/rust-tauri/r01-t02-handshake.sh /tmp/…/handshake-out` | 0（5 场景，version_incompatible 负向正确） |
| V13 | xtask CLI 负向：`verify-stage R09` / `verify-stage R03` / 未知子命令 / 缺子命令 / 缺 `--evidence` / 缺 stage / `--evidence` 两次 / 多余位置参数 | 全部 exit 2 |
| V14 | xtask `--help` / `-h` | exit 0（全文） |
| V15 | F1 克隆复现 6 件套（本代理自建 /tmp 双生树，见 §5） | 全部符合预期 |
| V16 | `lingxi-service --log-max-bytes 63` / `64` / `--log-max-files 1` / 8 旗标 ×（0/垃圾/旗形/未知） | 63→exit 2；64→正常启动+READY+SIGTERM exit 0；1→exit 2；8 旗标全部 exit 2 |
| V17 | 封印族基线：pristine 克隆（`git clone` + checkout 574198916 + symlink node_modules）跑 3 个封印族测试文件 | exit 1（**3 文件 / 6 用例失败，与工作树完全一致**，见 §4 反掩码） |
| V18 | `python3 docs/rust-tauri/R02/r02_t08_worktree_digest.py` 重算 | digest=`94f95e7a…`、9707 条目——与 R02_REPORT §3 / R02_HANDOFF 钉住值**逐字一致**（重算时点：本报告写入前） |
| V19 | `git diff 574198916 --diff-filter=D --name-only`；`-- .sync-audit/ PROGRESS.md docs/rust-tauri/ORCHESTRATOR_PROGRESS.json .gitignore`；`git status --porcelain artifacts/rust-tauri/R02/`（滤 T08） | 全部为空（零删除；治理面零改动；T01–T07 已提交证据零覆写） |
| V20 | xtask 依赖树 `cargo tree -p xtask --locked` | 仅 serde_json 1.0.151（锁内既有）+ 其既有传递依赖；Cargo.lock diff = +7 行成员块，零新增包/版本（亲核） |

测试有效性核验：全量 diff 17 个修改文件 + 13 个未跟踪路径逐文件亲读；**零既有
测试删除/弱化**（删除行均为：旧 CARGO_TARGET_DIR 固定值、旧文案、旧误诊叙述、
uchg 错误定性、长空格残留——无一为断言或保护性逻辑）；新增测试为纯增量
（config 1 + devgate 1 + xtask 16）。

---

## 2. Acceptance 独立判定

### R02-A15｜真实二进制完成全链 — **PASS**（独立复跑 ×2）

- V7 内（a15_full_chain）+ V8 独立直跑（本代理自建 target 的二进制），两轮均 exit 0。
- 链完整性逐项亲为真实：启动→READY 行解析（`LINGXI_SERVICE_READY addr=…`）→
  health 200 最小面 → 无凭证 401 → 读 0600 token 文件 → /me 200（服务端计算
  principal）→ execute 提交（真实 runId）→ 真 RFC6455 客户端 subscribe（subscribed
  携带 snapshotSeq 边界）→ 订阅中 execute 的活事件越界送达（seq > snapshotSeq）→
  HTTP 读回一致 → 伪造 future cursor 显式 `invalid_message/future_cursor` 拒绝 →
  SIGTERM → exit 0 → **pgrep（按本 home 限定）空 + 端口拒连** → 同 home 重启 →
  **旧 token 401 / 新 token 200** → session runCount=2 → events head 不回退（4→4）→
  二次关闭 exit 0 + instance.json 已移除。
- 遗留核验：V8 后全局 `pgrep -f lingxi-service`（排除一个**先前会话遗留**的
  PID 5667，T03 时期 target，非本任务产物，见 N4）与 `lsof -iTCP -sTCP:LISTEN`
  均无本任务残留。
- 判定：**真实 PASS**。探针非 mock：真 HTTP/WS/socket、真进程对、真 SQLite 持久化。

### R02-A16｜不影响旧入口 — **PASS**（独立复跑 + 独立反掩码）

- V7 内 a16_legacy_regression 以本代理证据目录真实执行 136.4s：
  - E1：`git diff 574198916..工作树 -- core/ server/ desktop/ shared/ tests/
    package.json package-lock.json` = 空（本代理另以 V19 独立复核相同面 = 空）；
    `package.json main = desktop/bootstrap.cjs`；launch.js/main.cjs/bootstrap.cjs/
    boot.cjs 对 `lingxi-service|rust-target|/rust/` 0 命中。
  - E2/E3：typecheck / typecheck:core-contracts exit 0。
  - E4：check:dependency-boundaries / check:tool-invocation-boundaries exit 0。
  - E5：全量 npm test exit 1；失败 = 恰好 3 个封印族文件 / 6 用例；1463 文件通过。
- **反掩码（本代理独立做，不依赖脚本内的检查）**：
  1. 从本代理的 e5 日志提取全部失败清单条目（632 条去重），逐条
     `git cat-file -e HEAD:<path>` 核验：**632/632 全部为 HEAD 中已提交文件**
     （R01/R02 T01–T07 已提交交付物 + 历史 .gitignore），**0 个 T08/新栈未提交
     文件**；
  2. **pristine 基线对照（V17）**：干净克隆 574198916（零 T08 文件）上跑同一
     3 个测试文件 → 同样 3 文件 / 6 用例失败，失败测试名**逐一相同**
     （post-verification-audit-seal×1、round2×3、round3×2）——红为预存在，
     T08 未新增任何失败；
  3. 机制核对：post-verification-audit-seal 仅比较两个**已提交** ref
     （`git diff VERIFIED_SOURCE_SHA..HEAD`，坐标 `ab4f2281`），不读工作树——
     T08 未提交改动在原理上不可能影响它；round2/round3 虽经
     `git ls-files --cached --others` 看到未跟踪文件（其 firstDiff 诊断行确实
     列出了 T08 证据文件名），但失败点在 seal guard（已提交 diff 含非审计文件），
     有无 T08 文件同样红（V17 实证）。
  4. 与 T07 时点基线对比：T07-R1 V10 记录同时点 post-verification-audit-seal
     1 failed / 2 passed、失败清单零 T07 文件——同一预存在红族，方向一致。
- 判定：**真实 PASS**；执行者未把任何真实回归红伪装成封印红（本代理逐文件归属
  抽查不止 3 个——实际为全部 632 条逐一核验）。

---

## 3. xtask 对抗验证（最重项）

### 3.1 判定逻辑源码核验（反「仅统计手填 PASS」）

亲读 `verify.rs`/`stage_map.rs`/`main.rs` 全部：scenario PASS ⟺ 其引用命令
**真实 exit 0 ∧ 未超时 ∧ 声明证据文件存在**（`CommandOutcome::passed`）；
整体 PASS ⟺ 全部 scenario PASS；结果 JSON 的 `overall` 字段由该判定**产出**而非
输入。全部代码路径**不存在**读取任何 JSON/文本中 PASS 字符串的入口。命令经
`std::process::Command` 真实 spawn（stdin 关闭、stdout/stderr 逐命令落盘、
100ms 轮询 + deadline kill）。

**实证（自建 scratch 副本，见 3.2）**：HANDPASS 场景命令写出内容为
"verdict: PASS / overall: PASS" 的证据文件但 exit 1 → verify-stage 判 **FAIL**
（exit 1）——证明判定与文本内容无关。

### 3.2 对抗矩阵（本代理在 /tmp 自建 scratch xtask 副本上二进制级执行；仓库零改动）

方法：复制 `rust/crates/xtask` 至 `/tmp/xtask-neg-ws/rust/crates/xtask`（最小
workspace 包装 + git init），仅在**副本**中追加注册对抗阶段图（EMPTY/NEGCMD/
MISSEV/SLOW/HANDPASS/PASSFWD/STALE），锁定工具链 1.98.1 离线构建。

| 对抗场景 | 预期（05 §3 契约） | 实测 | 判定 |
|---|---|---|---|
| 未知阶段 `verify-stage R09` | 非零（列出已注册阶段） | exit 2 + "unknown stage … registered stages: R02" | ✓ |
| `verify-stage R03`（未注册阶段） | exit 2（契约行为；无提前 R03） | exit 2 | ✓ |
| 未知子命令 / 缺子命令 | 非零 | exit 2 / exit 2 | ✓ |
| `--help` / `-h` | exit 0 | exit 0（全文） | ✓ |
| 缺 `--evidence` / 缺 stage / 多余参数 / `--evidence` 两次 | 非零 | exit 2 ×4 | ✓ |
| **空场景集合**（scenarios:[]） | 非零（空集合绝不绿） | exit 2："scenarios must not be EMPTY" | ✓ |
| 命令失败真实退出码 | exit 1 + 结果记实码 | exit 1；结果 JSON exitCode=**7**（注入值） | ✓ |
| **exit 0 但缺证据** | exit 1 | exit 1；missingEvidence 如实列出 | ✓ |
| **超时**（限 2s，命令 sleep 30） | kill + exit 1 | 墙钟 ~2s 被 kill；timedOut=true、exitCode=null、exit 1 | ✓ |
| **PASS 字符串陷阱**（证据文件内容写 "overall: PASS" 但 exit 1） | FAIL | overall FAIL / exit 1 | ✓ |
| 诚实通过（exit 0 + 证据真实生成） | PASS | exit 0 | ✓ |
| 阶段图解析负向（执行者单测，V4 亲跑）：空 commands/场景无命令/引用未知命令/缺 evidencePaths/重复 id/坏 schemaVersion/占位值/坏 JSON | 全部拒绝 | 16/16 passed | ✓ |

另复核执行者归档的实战负向：`gates/verify-stage-run1-missing-evidence-FAIL.json`
为真实 FAIL 留档（三条命令 exit 0 但 declared 证据路径错误 → A11/A12/A16 FAIL，
overall FAIL）——缺证据执法在真实管线上发生过并被归档，非纸面声称。

### 3.3 阶段图 ↔ acceptance-catalog 完备对齐

`stage_maps/R02.json`：16 场景 id = R02-A01..A16 恰好（单测
`r02_scenario_ids_match_the_taskbook` 钉死顺序与集合），全部 REQUIRED，每条
commandRefs 非空且指向已注册命令；与 acceptance-catalog 的 R02-A01..A16（全
REQUIRED）**无缺漏、无多余**。13 个命令全部指向真实存在的 T01–T07/T08 二进制级
脚本（逐一核对文件存在）。LEDGER ↔ 阶段图逐场景命令串一致性：程序比对
**零 mismatch**。

### 3.4 结果 JSON 字段

含 resultVersion/stage/testedSha（真实 `git rev-parse HEAD`）/worktreeDirty
（真实 `git status --porcelain`）/platform（std::env::consts 真实值）/
toolchainChannel（读 rust-toolchain.toml）/逐命令 exitCode+durationMs+
missingEvidence/逐场景 status/overall——满足 05 §7 证据字段要求的本阶段部分。

---

## 4. RR-T08-F1 硬化核验（本代理自建克隆完整复现）

布置（全部 /tmp，仓库零改动）：`/tmp/r02t08-base-check` = pristine 克隆
@574198916；`Y-clean-base` / `X-drift-base` = 其 APFS 副本；X 的
`contracts/generated/MANIFEST.json` 被本代理**故意注入漂移**（追加一行）。
`/tmp/lingxi-r01t02-target` 清空后作为旧式固定共享 target。

| # | 场景 | 实测 | 判定 |
|---|---|---|---|
| F1-a | **修复前**：Y（干净、基线脚本）跑 check-generated，默认共享 target | exit 0，日志 "under …/Y-clean-base/contracts/generated"（把 Y 二进制灌入共享 target） | ✓ 布置 |
| F1-b | **修复前**：X（**已漂移**、基线脚本）跑同一共享 target | **exit 0 绿灯**，日志打印 "no diff under /private/tmp/**Y-clean-base**/contracts/generated"——cargo 跨检出复用 Y 二进制，校验的是 Y 的树；X 的真实漂移被掩蔽 | ✓ **错绑绿完整复现** |
| F1-c | **修复后**：X 升级为新脚本+新源码（rsync 工作树 rust/ 与 scripts/），默认派生 target | exit 1，检出真实漂移：`drift: MANIFEST.json (disk 6109B sha256=7fb4ab2b…, regenerated 6088B sha256=c9e4d8d1…)`（字节数差恰为本代理注入的 21B） | ✓ 硬化生效 |
| F1-d | **修复后**：X + 显式 poisoned target（`/tmp/f1/shared-poison` 内为另一检出 Y2 编译的新 devgate 二进制） | exit 2："repo-root mismatch: compiled in /private/tmp/f1/Y2-newsrc, CWD belongs to /private/tmp/f1/X-drift-base. Refusing…" | ✓ 双保险第 2 层生效 |
| F1-e | 跨检出直接运行：本仓编译的 lingxi-protocol-gen / lingxi-protocol-verify / xtask 分别从 X 的 CWD 执行 | 全部 exit 2 + mismatch 诊断 | ✓ |
| F1-f | 无检出标记的裸 /tmp CWD 运行 gen | exit 2（拒绝猜测根） | ✓ |
| F1-g | 本仓内三脚本复跑（V10/V11/V12） | 全绿；默认 target 派生值与本仓路径哈希亲算一致；显式 CARGO_TARGET_DIR 仍被尊重（此时 devgate 运行期绑定兜底） | ✓ |

与执行者归档证据（`f1-hardening/` 六件套日志）方向、措辞、路径形态一致；
devgate.rs 为纯 std::path 运行期绑定，**fail-closed、无回退**；canonicalize 双侧
比较，/tmp↔/private/tmp 符号链接别名被正确收敛（本代理全部路径实验均经
/private/tmp 真实形态）。

---

## 5. 移交修正逐项对质

| 移交项 | 要求 | 本代理核验 | 判定 |
|---|---|---|---|
| T07-R1 F01（Mailbox 定性勘误） | T07 报告叙述改为"前向稳健加固"、删"缺陷级修复/已复现"措辞；代码不动 | diff 亲读：§2.6/§3/§9.6/§11.3 四处全部改写，残留 "丢失唤醒" 字样均处于勘误叙述语境；`git diff` 对 events.rs **零改动**（代码未动，符合"代码无需变更"） | 落实且如实 |
| T07-R1 F02（脱敏分歧清单 + R05 登记） | redaction.rs 模块文档 + 报告 §4.2/§9.2 补精确边界/未镜像清单/实测一致项；列入 R05 交接风险 | diff 亲读全部落地；R02_HANDOFF unresolved_items 登记 `RR-R05-REDACTION-BOUNDARY`（OPEN，截止 R05）+ 原编号 MERGED 可溯 | 落实且如实 |
| T07-R1 F03（数字对齐提交证据） | 报告数字 = 提交的最终证据 | 本代理直接读**已提交**证据：`storm-results.json` executes_concurrent=1000、durable_key_events=3384；`rss-samples.csv` 147 样本、first=11136、peak=17040——与修正后报告（3384/3384、147、11136/17040、EXECUTES=1000）**逐字一致**；脚本 note 去硬编码、summary 逐轮标注（diff 亲读） | 落实且如实 |
| T07-R1 F04（旗标一致性） | 二选一统一；USAGE/文本一致 | 选 (i) 解析期执法：本代理二进制级实测 63→exit 2、64→正常启动、files 1→exit 2，**8 旗标**（max-ws-connections/max-subscribers/event-subscriber-queue/event-reorder-bound/db-queue-bound/http-rate-max/log-max-bytes/log-max-files）×（0/垃圾/旗形/未知旗）全部 exit 2；USAGE 文本与行为一致；新单测 `log_flag_range_violations_are_parse_time_errors` 真实存在且通过 | 落实且如实 |
| T02-R1 F02（0700 文档失配） | 修订 doc 或改行为 | paths.rs 模块头 + 函数 doc 改为 "NORMALIZED to exactly 0700（更宽收紧、更严加回 owner-write）"，与实测行为一致；纯文档 | 落实且如实 |
| T04-R1 F01（uchg 头注释） | 改写为实测机制 | storage_transactions.rs 头注释改为 RLIMIT_FSIZE/EFBIG 并明示 uchg 经实测否决（指向 §3.7）；纯文档 | 落实且如实 |
| T04-R1 F02（docstring/字符串） | 补 exit 5、清长空格 | main.rs 模块 docstring 已含 exit 5（与 SERVICE_START_AND_SHUTDOWN §4 退出码表一致）；长空格残留 grep 复核零命中 | 落实且如实 |
| T06-R2 F05（证据原始性惯例） | 惯例采纳登记 | R02_HANDOFF handoff_conventions 第 1 条登记；本任务证据均为原始日志（本代理抽查 gates/ 与 verify-stage/ 下日志均为原始命令输出） | 落实且如实 |
| RR-T02-FINFO1 | PROTOCOL_SPEC §10 补说明，不改需求语义 | diff 亲读：§10 末段新增 contentSha 复算限制说明（哈希输入=提取器运行期内部全量清单，复算必须重跑提取器），纯追加、无语义改动；`check-contracts`/extract `--check` 亲跑 exit 0 | 落实且如实 |
| RR-T08-F1 | 按登记方向硬化 | 见 §4：两个建议方向（运行期根绑定 + 派生 target）均落地并经本代理克隆复现核验 | 落实且如实 |

---

## 6. 架构边界与范围核验

- **xtask planned→exists（D5）**：DEPENDENCY_RULES.json diff = 3 行字段级改动
  （status/responsibility/+established_by），**无整文件重排**（T02-R1 F03 教训被
  吸取）；`check-boundaries` 亲跑 exit 0，D5 反向闭包 "every cargo workspace
  member is a registered module" PASS（含 xtask）。
- **devgate 不污染 wire 面**：`pub mod devgate` 为纯 std::path 工具模块；
  `check-contracts` 亲跑 56 文件 / 624 条目**零漂移**——协议生成物与 API 面
  快照对新模块无感知，wire 类型面零变化。
- **xtask 不引入业务逻辑**：三子命令均为编排（spawn 既有脚本/检查器并传播真实
  退出码）；零新增第三方依赖（V20）。
- **R00 封存原件零改动**：R02_IMPLEMENTATION_MAP.json 钉住的 STORES/
  ENTRYPOINTS/ACCEPTANCE_MAP 三 SHA-256 与本代理现算**逐一相符**；覆盖层设计
  （冻结原件+新栈覆盖层）满足"更新为实际实现路径"语义且保封印链。
- **无提前 R03**：`verify-stage R03` exit 2（契约行为）；stage_maps/ 仅 R02.json；
  main.rs 中 R03 仅为注释指引。
- **治理面零触碰**：.sync-audit/、PROGRESS.md、ORCHESTRATOR_PROGRESS.json、
  .gitignore 零 diff；任务书目录零改动；无 commit/push。
- **无隐藏删除**：`--diff-filter=D` 为空；17 修改 + 13 未跟踪全部有任务映射。
- **证据指针抽查**（3 条，Ledger 声明）：A03 a03-summary.txt（存在，内容含
  exit-3 双实例拒绝记录）、A13 redaction-scan/summary.txt（存在，0 命中声明与
  本代理对脚本逻辑的阅读一致）、A16 e1-diff-node-surface.txt（存在，0 字节 =
  空 diff 声明相符）。另抽：执行者 cargo-test-workspace.log 聚合 = 287/0（与
  本代理 V3 一致）；gates/xtask-unknown-stage-exit2.txt 内容与本代理 V13 实测
  逐字一致。

---

## 7. Findings

### F01｜MINOR — T08 交付文档内的数字/指针漂移簇（同根因：多轮运行后未统一收口）

- **位置**：`docs/rust-tauri/R02/R02-T08_REPORT.md` 头部与 §8.5；
  `R02-T08_REPORT.md` §9；`R02_REPORT.md` §3；`R02-T08_REPORT.md` §3.2。
- **证据**：
  (a) T08_REPORT 两处引用工作树 digest `f6058f26…`，而 R02_REPORT §3 与
      R02_HANDOFF 钉住 `94f95e7a…`——本代理用交付的计算器**重算 =
      `94f95e7a…`（9707 条目，与最终文档逐字一致）**，T08_REPORT 内的是早期
      运行的残留值；
  (b) T08_REPORT §9 "git status --short 全文见 R02_REPORT.md §10（同题小节）"——
      R02_REPORT **不存在该小节**（其 §10 为「已知缺陷」）；两份报告实际均未
      包含 porcelain 全文（本代理已独立以 git status/numstat 重建全集核对无异）；
  (c) R02_REPORT §3 称 "11 个未跟踪新文件/目录"，实际 git status 为 **13** 条
      `??` 路径；
  (d) T08_REPORT §3.2 称 "16 场景绑到 14 个真实证据脚本（T01–T07 的 12 个
      既有 + 2 个新增）"——阶段图实际注册 **13** 个命令（11 个既有 +
      2 个新增）；`r02_t02_f01_env_token_negative.sh` 未注册进图（16 场景覆盖
      完备性不受影响——该脚本对应 T02-R1 F01 收口项，非 16 个基础场景之一）。
- **问题**：交付文档四处数字/指针与最终证据不符。
- **为什么违反任务**：05 §7 要求证据写法精确可核；91 模板要求报告与证据一致。
- **后果**：低——全部实质证据（digest、diff 全集、阶段图、退出码）经本代理
  独立重算/重跑核对无误，无任何 PASS 被伪造；但审阅者需自行分辨哪处数字为
  权威，与 T07-R1 F03 同类的"报告-证据漂移"在本任务交付物上重现。
- **根因**：报告跨多轮运行撰写，早期值未随最终重跑统一更新；未跟踪计数与
  命令计数为手工誊写。
- **同类路径**：T07-R1 F03（已收口）；本簇四项同根因合并为一条。
- **修复要求**：T08_REPORT 的 digest 值统一为最终重算值（或明示 "f6058f26 为
  早期轮次值，最终以 HANDOFF 为准"）；修正或删除 §9 的悬空指针并把 porcelain
  全文落到实际文件；R02_REPORT §3 计数改 13；§3.2 命令计数改 13/11。纯文档，
  无行为重跑需要。
- **必须重跑**：无需行为重跑；文档修正后建议重跑
  `python3 docs/rust-tauri/R02/r02_t08_worktree_digest.py` 复核引用一致。

### F02｜MINOR — xtask 阶段图 `resultVersion` 必填但不校验语义、无负向测试

- **位置**：`rust/crates/xtask/src/stage_map.rs`（L80-85 仅校验非空）；
  `verify.rs` L206（输出版本恒为 crate 常量 `RESULT_VERSION`，从不读图内值）。
- **证据**：图内 `resultVersion` 为 schema 必填字段，但任何非空字符串均可通过
  解析（本代理读码确认无相等性校验）；8 个解析负向单测 + 6 个 runner 单测中
  **无一针对 resultVersion**（无缺失/空值/错误值用例）。05 §3 字面要求
  "其输入schema、场景→测试映射、空集合检查、超时和**结果版本**必须有负向测试"。
- **后果**：低——图内字段当前是装饰性的：输出结果 JSON 的版本由二进制常量
  产生，不存在"图内伪造版本污染结果"的路径，任何 verdict 判定不受影响；但
  契约字面（结果版本须有负向测试）未满足，且必填却不校验的字段是机制异味
  （R03 建图者可能误以为该值生效）。
- **根因**：schema 声明与执行语义未对齐；负向测试矩阵漏掉该字段。
- **同类路径**：schemaVersion 有 ==1 校验且有负向测试——resultVersion 应同标准。
- **修复要求**：二选一——(i) 解析期强制 `resultVersion == RESULT_VERSION` 并加
  负向单测（错误值/缺失/空）；(ii) 从 schema 删除该字段并在 USAGE/模块文档
  说明输出版本的权威来源。
- **必须重跑**：`cargo test -p xtask` + 一次 `verify-stage R02`（本代理建议随
  R03 建图时一并收口，R02_HANDOFF 已登记的 RR-R03-XTASK-MAP 可携带本项）。

### F03｜MINOR — verify-stage 证据新鲜度洞：存在性检查 + 证据目录不清空

- **位置**：`rust/crates/xtask/src/verify.rs` L99-112（evidence 仅 `path.exists()`
  判定）；L132-133（`create_dir_all(evidence_root)`，不清空）。
- **证据（本代理 scratch 实证）**：STALE 场景第一轮真实生成证据 exit 0 PASS；
  随后把同一阶段图的命令改为**什么都不写**仍 exit 0，**复用同一证据目录**重跑
  → overall **PASS**（陈旧证据被当作本次产出）。
- **问题**：命令"exit 0 但停止产出证据"在证据目录复用时会被掩蔽为 PASS。
- **为什么违反任务**：05 §3 的精神是"缺证据必须失败"——陈旧证据在语义上就是
  本次缺失；任务书同节要求门禁不可被包装成总是成功。
- **后果**：低（现行惯例下不可达）：执行者与各前序任务每轮均使用全新证据目录，
  本任务 run1 实战也证明"目录为当时新填"时缺证据执法真实生效；触发该洞需要
  调用方主动复用非空证据目录且命令退化为不写证据仍 exit 0 的组合。属门禁
  健壮性缺口（与 RR-T08-F1 同族、烈度更低），不是本次任何 PASS 的疑点。
- **根因**：只检查存在性、未绑定"本次运行产生"；入口不拒绝非空证据根。
- **同类路径**：无（verify-stage 为该机制唯一入口）。
- **修复要求**：二选一——(i) 入口拒绝已存在且非空的证据根（或在结果 JSON
  记录每证据文件的 mtime ≥ 命令启动时刻并断言）；(ii) 在 USAGE 与
  SERVICE_START_AND_SHUTDOWN/交接约定中显式"证据目录必须每轮新建"，并在
  xtask 检测到非空目录时打印警告。建议随 R03 阶段图机制一并收口。
- **必须重跑**：`cargo test -p xtask`（新增负向）+ 一次 verify-stage R02。

### 记录性 NON-ISSUE（不列编号缺陷）

- **N1**：F04 修正后范围违例的报错文案为 "must be a positive decimal integer
  (bytes per log file before rotation (>= 64))"——对"63"这类值，"positive
  decimal integer" 措辞不精确（63 是正整数，只是低于下限）；行为（exit 2 +
  含真实下限）完全正确。文案级。
- **N2**：`stage_map.rs` L233-234 重复 `#[cfg(test)]` 属性两行。纯外观，clippy/
  fmt 均绿。
- **N3**：verify-stage 进度行打印的是**未替换**的 `{EVIDENCE}` 占位 argv
  （`spec.argv`），实际执行用替换后的 argv；日志观感与落盘目录自洽
  （`> <command_dir>` 为替换后真实路径）。纯外观。
- **N4（环境观察）**：本机存在一个**先前会话遗留**的 lingxi-service 进程
  （PID 5667，`/tmp/rust-target-r02-t03/…`，启动于 05:30，早于本代理全部动作），
  指向某 T03 时期合成 home。非本任务产物；A15 的遗留检查按合成 home 精确限定
  且端口为临时分配，不受影响。本代理未处置（非本任务资源）。提示总控：历史
  评审会话曾遗留真实进程，属"超时/中断时进程组未清理"已知边界的实证。

### 已知风险如实登记（执行者 §8 与本代理核验一致）

1. 跨平台（Windows/Linux）未验证：devgate 的 canonicalize 比较、脚本 shasum
   派生、RLIMIT_FSIZE 等均为 macOS 实测；归 R09/R10 平台关卡。
2. A16 为源码级 + 测试回归级：未启动真实 Electron GUI（本机隔离约束一致）。
3. xtask 超时 kill 为 `child.kill()`（SIGKILL 单子进程），无进程组；被杀 bash
   脚本的 EXIT trap 负责清理其子进程（本仓脚本均有 trap）；报告 §8.4 已披露，
   本代理 SLOW 超时实验实测 ~2s 按时 kill 且无遗留。
4. 工作树 digest 为报告时点值；本代理重算一致（在本报告写入前）；最终候选须
   在授权提交后按 05 协议绑定最终 SHA 重跑关键门禁（xtask 结果 JSON 的
   testedSha/worktreeDirty 字段已为此就位）。

---

## 8. PASS 标准逐条核对

1. **R02-A15/A16 真实 PASS（亲自重跑）**——✅（§2：A15 双轮、A16 全量含真实
   npm test）。
2. **xtask 三命令 + 负向全真实**——✅（V5/V6/V7 + §3.2 十二项对抗矩阵全中，
   含空集合/缺证据/命令失败/超时/PASS 字符串陷阱）。
3. **F1 硬化经克隆复现核验**——✅（§4：错绑绿复现 → 修复后检出真实漂移 →
   显式污染 target/跨检出/裸路径三向 exit 2）。
4. **FINFO1/移交修正全部落地且如实**——✅（§5 十项逐项判定）。
5. **无 BLOCKING**——✅（3 MINOR：文档漂移簇 / resultVersion 负向缺口 /
   证据新鲜度洞；均无 PASS 伪造路径）。
6. **无测试篡改 / 反掩码成立**——✅（零删除零弱化；632 条失败清单全归属
   HEAD 已提交文件；pristine 基线 6 用例失败与工作树完全一致）。
7. **回归绿（封印预存在红如实记录）**——✅（fmt/clippy/287 全绿；封印族
   3 文件 6 用例红 = 预存在坐标落后，已独立归因，执行者未触碰坐标/白名单）。
8. **证据与候选一致**——✅（digest 重算逐字一致；ledger↔阶段图零 mismatch；
   执行者归档日志与本代理实测一致；F01 所列文档漂移不影响实质一致性）。

## VERDICT: PASS

R02-T08 的四件交付（R02_REPORT / R02_HANDOFF / 独立二进制冒烟结果 / xtask
门禁及负向）真实且符合 91 模板结构；xtask 满足 05 §3 全部核心契约（真实命令
真实退出码、空集合/未知阶段/缺证据非零、无 PASS 字符串路径）；RR-T08-F1 硬化
经独立克隆复现确认有效；A15/A16 两条 REQUIRED 场景经本代理独立重跑确认；
全部移交修正落地。三条 MINOR（F01 文档漂移簇、F02 resultVersion 负向缺口、
F03 证据新鲜度洞）不阻塞本任务放行，建议在 R02 阶段收口/R03 建图时一并修复
（修复后按各条"必须重跑"执行）。

本判定不授予 commit/push/发布/封印坐标推进权限；tested SHA 为
`5741989165fe7e04c9a58a9d35c7747d3599d274` + 未提交工作树（本代理重算 digest
`94f95e7a53dac99f5cfacea2e6eca3043da6ff20afb261cf9c68b7388ce32332`，9707 条目，
重算时点在本报告写入前——本报告自身成为新的未跟踪文件，最终候选以授权提交后
重算为准）。

---

## 附：审阅方法与限制

- 全部结论基于本机亲跑命令与亲读源码；对抗性 scratch 副本
  （`/tmp/xtask-neg-ws`、`/tmp/f1/{Y,X,Y2…}`、`/tmp/r02t08-base-check`）均在
  /tmp 构建，**未触碰仓库产品源码/测试/配置**；唯一仓库写入 = 本报告文件。
- 本代理的验证产物：`/tmp/r02t08-review/`（verify-stage 全量证据、a15-standalone、
  seal-family-BASE.log、三脚本日志、digest 明细等）。
- 限制：跨平台与 Electron GUI 未验证（同执行者披露）；xtask 内部逻辑变更后的
  全量回归以 R03 建图为自然关口；本报告不改任何被验收文件，findings 均为
  验收意见，不构成代改。
