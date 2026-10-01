# G04-E01 交付报告 — R04-RR1-F04 PTY 混合分片重复消费 + R04-RR1-F05 输出完整性冒称

- 执行代理：EXECUTOR-R04-RR1-G04-E01（一次性）
- 基线：614fab1af（分支 codex/rust-tauri-migration，含已合入的 G01/G02/G03 修复）
- 工作单：G04 / 缺陷 R04-RR1-F04（C01–C04）+ R04-RR1-F05（C01–C05）
- 状态：READY_FOR_REVIEW（不自行宣布独立 PASS）
- 未 commit / 未 push（按禁止事项）；全部改动在工作区
- 证据目录：`artifacts/rust-tauri/R04/RR1-G04-E01/`（repro/ 红、logs/ 门禁）

## 1. 当前 HEAD 的 F04/F05 事实重新核实结论

复审报告的定位基于原基线 773d5a696；G01–G03 大改过 procsupervisor.rs/exectools.rs。在当前
HEAD 614fab1af 逐条重新核实，**两组事实全部仍以原形态存在，另发现两条同族新事实**：

| # | 位置（当前 HEAD） | 基线行为 | 核实结论 |
|---|---|---|---|
| F04-1 | `procsupervisor.rs` `TranscriptCore::deliver_since_cursor`（原 856-909 行区域） | joined 已解码前缀返回，但 cursor 只跨**完整消费**的 chunk 推进；部分消费的 chunk 整块留在队列 → 前缀重复投递 | 仍存在（G01–G03 未触及该函数） |
| F04-2 | 同上 + `exectools.rs` `run_write_stdin` | 不完整 UTF-8 尾部计入 `dropped_undelivered_bytes`，文案表述为 "were dropped by the bounded ring" | 仍存在 |
| F04-3（新发现，同族） | `procsupervisor.rs` `decode_prefix_utf8` | 内部非法字节（error_len=Some）触发整段 lossy 并**全部消费**——若其后还有一个尾部不完整序列，补齐字节到来时成为孤儿替换符（保真降级，非重复） | 属性测试（C03）在修复过程中实测命中；一并修复 |
| F05-1 | `exectools.rs` `run_exec_command`（原 776-834 行区域） | `truncated = head_tail.truncated`，只看尾窗是否再被裁剪，不比较 `total_bytes` | 仍存在：150 024 字节/102 400 尾窗/max_output_bytes=200 000 → `truncated=false` 且 HEAD 标记缺失（红测试实测复现） |
| F05-2 | `exectools.rs` `truncate_head_tail` | 单超长行两侧行预算都放不下时 head_lines/tail_lines（可能只剩末尾空行伪影）走「整行省略」分支，正文只剩省略标记 | 仍存在 |
| F05-3 | `exectools.rs` `spill_resource_ref` / 截断通知 / `SpillWriter::write_bytes` 错误臂 | `SpillInfo.capped` 不进入 "Full output/full transcript" 声明；写失败臂把 `capped=true` 当兜底，失败文件仍可被引用为 "exec_command full output"（幽灵完整文件） | 仍存在 |

G01–G03 改动的影响核实：LiveSlot 容量预留、reaper 观测/所有权门、StopUnconfirmed 诚实链均与本单
正交，语义原样保留（§6 回归：三套件 + T05/T06/T08 全绿）。

## 2. 红 → 绿

红测试（最小集，按工作单）真实 cargo 红运行，日志在 `repro/`：

- `repro/F04_C01_logic_red.log`（exit 101）：同 chunk `[0x61,0xE6,0x97]` 投递 `a` 后空闲重 poll
  **再次返回 `a`**（left `"a"` / right `""`）；`dropped_undelivered_bytes` left 2 / right 0
  （暂存被计为已丢）。既有分 chunk 控制组 `[a]|[E6,97]|[A5]` 三例同场全绿——精确反例形状。
- `repro/F05_C01_toolchain_red.log`（exit 101）：真实工具链（网关 prepare→execute + 真实 bash +
  真实 spill 文件）150 020 字节单行、max_output_bytes=200 000 → `truncated=false` 且 `HEAD_MARK`
  不在返回文本（滚动尾窗缺头却自称完整）。

修复后两条均绿（日志 `logs/06_per_cid_and_suites.log`）。

## 3. 根因修复（源码变化）

### 3.1 F04 — `rust/crates/lingxi-service/src/procsupervisor.rs`

- **`TranscriptCore` 字节级消费模型**：chunk 由 `(u64, Vec<u8>)` 改为 `TranscriptChunk{seq, bytes,
  consumed}`。投递只 join 各 chunk 的**未消费余量**，按解码消费的字节数推进各 chunk 的
  `consumed` 前缀偏移，整块耗尽即出队——同一字节至多消费一次；部分消费的 chunk 作为队首挂起
  （cursor 指向其 seq），后续 chunk 完全未消费。
- **四种字节处置分立**（`TranscriptDelivery` 新增 `pending_incomplete_bytes`、`replaced_bytes`、
  `consumed_bytes`）：`dropped_undelivered_bytes` 只累计环形淘汰的真实丢失（淘汰只计
  `len - consumed`）；尾部不完整=等待（非丢失，非 truncated 的独立事实行）；force 最终排空=悬挂
  尾部替换为一个 U+FFFD（保持既有 `truncated==false` 契约，旧测保留）；内部非法=确定性替换消费。
- **`decode_prefix_utf8` 重写为迭代式**：内部非法子序列逐个替换（对齐 `from_utf8_lossy` 的
  maximal-subpart 规则），**真正的尾部不完整序列即使前面有非法字节也挂起**（F04-3 同族修复：
  补齐字节到来时仍整字交付恰一次）。
- `ring_bytes` 语义改为「环内未消费字节数」，投递推进与淘汰出队的账目一致（属性测试
  consumed+dropped+pending==total 全种子成立）。

### 3.2 F04 文案 — `rust/crates/lingxi-service/src/exectools.rs` `run_write_stdin`

- 丢弃/暂存/替换三条事实行分别只在各自事实成立时出现；「dropped by the bounded ring」只在真实
  丢失时出现——文本与结构化 `truncated`（=真实丢失或暂存挂起）一致。
- `transcript_spill_claim`：只有未封顶、未失败、有字节的 spill 才称 "full transcript"；封顶/失败
  状态显式（`transcript_path:` 行亦带 partial/capped/失败标注）。

### 3.3 F05 — 事实建模与声明

- **`CollectorCore` 双窗**：`head: Vec<u8>`（真头，与尾窗同界 100 KiB，封满即冻）+ 既有滚动尾窗；
  内存上界 `head_cap + window_cap`（2×100 KiB/进程，非无限全文）。`OutputSnapshot` 增 `head` 与
  `lost_middle_bytes()`（派生，永不漂移）。
- **`assemble_retained_output`**：总流 ≤ 双窗和 → 去重叠拼接**整流精确呈现**；否则真头 + 中段
  淘汰标记（含头窗截断在字符中间的 trim 计入淘汰数）+ 尾窗（既有续字节跳过）。
- **`truncate_head_tail` 字节切分回退扩展**：行收集后任一侧**实际内容字节数为 0**（单超长行、或
  仅剩末尾换行伪影）也走 UTF-8 字符边界安全的 head/tail 回退——头尾内容必在（C02）。
- **`truncated` 判定**：`head_tail.truncated || evicted_bytes > 0`——预算大小不改变丢失事实。
- **spill 诚实声明**：`SpillInfo` 增 `failed`（写失败与封顶分立）；`full_output_claim` 只有
  未封顶、未失败且 `bytes_written >= total_bytes` 才说 "Full output"，否则
  unavailable+原因（capped/failed/no spill file kept）；`spill_resource_ref` 封顶 → partial 展示名，
  **失败 → 不出 ResourceRef**（无幽灵完整文件）。

## 4. 逐 C-ID 自查（普通 + 对抗）

| C-ID | 测试（文件） | 普通 | 对抗变体 |
|---|---|---|---|
| F04-C01 | `transcript_mixed_chunk_prefix_is_consumed_byte_exactly_r04_rr1_f04_c01`（procsupervisor.rs 单测） | a、日 拼接恰 `a日`，空闲重 poll 空 | 既有分 chunk 控制组 `transcript_delivers_split_multibyte_characters_intact` 原样保留并通过 |
| F04-C02 | `transcript_idle_polls_after_a_partial_delivery_hold_back_without_loss` + C04 链上空闲 poll 段 | 中间 poll 空、暂存不计 dropped、末次只回 `日` | C04 实链同时核 `run_write_stdin` 文本与结构化 `truncated`（无 drop 文案除非真丢） |
| F04-C03 | 3 条属性测试（种子 1–24/25–48/49–72 固定 xorshift64*，含最小失败样例贪心收缩器）+ 确定性淘汰形状 ×2 + 跨多 chunk 边界 | 合法输入拼接==原文；非法字节==一次性 lossy；账目恒等 | 环溢出+尾部不完整同时：`dropped==2` 恰为被淘汰未消费字节；P3 账目 consumed+dropped+pending==total |
| F04-C04 | `tests/r04_rr1_f04_pty_consumption.rs`（真实 posix_openpt + bash + 网关全链） | 握手两段写混合字节：`日` 恰一次、`M1a` 恰一次、终局 force 后仍恰一次 | 无 sleep 碰绿：poll 直到程序自有 PHASE1_READY → ack 释放补尾；stty -echo 关闭回显；跨会话 WRITE_STDIN_NOT_OWNED 不退化；exit 7 上报 |
| F05-C01 | `tests/r04_rr1_f05_output_integrity.rs` 三腿 | (a) 150 020B/200 000 预算：HEAD+TAIL 在且 truncated=false 时**字节精确全流**；(c) 300 020B：真头+真尾+`95220 bytes … evicted`+spill 全量 | (b) 控制组 80KiB<窗、>预算：truncated=true 且**无** eviction 声明、spill 完整可称 full——两事实分清 |
| F05-C02 | exectools 单测 3 条 + 集成腿（中填充单行、换行仅末尾） | 头尾标记内容存活、省略中段有标记、预算内 | ASCII 与多字节混合；字节切分落字符边界（无 U+FFFD）；末尾空行伪影不吞尾内容 |
| F05-C03 | `f05_c03_capped_spill_is_partial_and_honest` | 文件恰 64 KiB 有界、display 无 "exec_command full output"、正文声明 capped/unavailable | BEYOND_CAP 仅存在于超 cap 处：在内存结果中、**不在**封顶文件中 |
| F05-C04 | `tests/r04_rr1_f05_spill_failure.rs`（独立测试二进制） | EFBIG 写失败：内存结果完好（HEAD+TAIL）、0 个 ResourceRef、诊断文案、遗留文件 ≤ 失败点 | RLIMIT_FSIZE+SIGXFSZ=受控磁盘满替身（进程隔离）；失败后继续产出（AFTER_FAILURE 只在内存）；EACCES 目录（权限族）open 期失败 → "no spill file was kept" |
| F05-C05 | `f05_c05_high_output_and_many_pty_polls_stay_bounded` + 取消中途腿 | 双窗 ≤ 各自上界、spill ≤ cap、live 清空、FD 回基线（/dev/fd 计数） | 预算 1000/200000 两档；stdout+stderr 并发；`printf;sleep 30` 中途 terminate → spill 关闭（两次读尺寸相等）且不越 cap |

抖动核证：三个新集成文件连跑 3 轮全绿（SOAK_FAILURES=0）。

## 5. 同族路径检查

- `TranscriptDelivery`/`OutputSnapshot`/`SpillInfo` 的其他消费者：
  `r04_rr1_f02_reaper_cleanup.rs`（window/total 冻结断言）、`r04_t05_process_tools.rs`
  （storm/UTF8/PTY 套件）、`r04_t06_sandbox.rs`（snapshot.window）——全部原样通过（新增字段为
  加法，字段名未变）。
- `pty transcript` 的 spill 与 ring 同族路径已按同词汇修（`transcript_spill_claim`）。
- 幽灵文件族：失败 spill 不再产 ResourceRef；封顶 spill 展示名带 partial——`SpillWriter` 错误臂
  与封顶臂事实分立。
- 未触及：R05 范围（未启动）、`docs/rust-tauri/R04/repair-current/` 其他文件、
  `ORCHESTRATOR_PROGRESS.json`、Cargo.lock。

## 6. 门禁与回归（真实退出码，证据 logs/）

| 命令 | 退出码 |
|---|---|
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0（logs/01_fmt_check.log） |
| `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0（logs/02_clippy.log） |
| `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 0（logs/03_workspace_tests.log；88 套件 ok，951 passed / 0 failed；r00 LAN 腿本轮未触发拦截） |
| `cargo run … -p xtask -- check-contracts` | 0（logs/04_check_contracts.log） |
| `cargo run … -p xtask -- check-boundaries` | 0（logs/05_check_boundaries.log） |
| 逐 C-ID 单独运行（F04 三组 + F05 三组） | 全 0（logs/06） |
| G01 `r04_t05_registry_capacity` / G02 `r04_rr1_f02_reaper_cleanup` / G03 `r04_rr1_f03_stop_honesty` 复跑 | 全 0（logs/06） |
| T05 `r04_t05_process_tools` / T06 `r04_t06_sandbox` / T08 `r04_t08_tool_matrix` 回归 | 全 0（logs/06；含 R03 回归在内的全量已由 workspace gate 覆盖） |

## 7. 未验证项 / 边界

- 修复的验证平台为本机 macOS（darwin 27 arm64）；其他平台与正式打包未验证（本地结果不代替）。
- F05-C04 的 RLIMIT_FSIZE/SIGXFSZ 与 /dev/fd FD 计数为 Unix 专用手段（与被测代码同为 Unix 域）。
- `verify-stage R04`（阶段图级重跑，含 r04_tool_matrix 生成器与 R03 整链）未在本单重复执行——
  其组成部分（workspace tests、fmt、clippy、check-contracts、check-boundaries、相关套件）已逐一
  真实执行并记录；阶段级 verify 留给 G05/独立审查按其证据目录规范执行。
- 属性测试种子为固定 24+24+24 组（确定性；可复算），非穷举。
