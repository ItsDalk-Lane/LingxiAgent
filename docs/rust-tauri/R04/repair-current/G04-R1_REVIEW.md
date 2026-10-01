# G04-R1 独立复审报告 — R04-RR1-F04（PTY 混合分片）+ R04-RR1-F05（输出完整性）

- Reviewer：REVIEWER-R04-RR1-G04-R01（一次性，未参与 G04 执行/修复，无共享上下文）
- 审查基线：BASE_SHA=614fab1af（codex/rust-tauri-migration，含 G01/G02/G03 修复）
- 候选：当前工作区未提交改动（procsupervisor.rs、exectools.rs 修改 + 3 个新集成测试文件）
- 复测证据目录：`artifacts/rust-tauri/R04/RR1-G04-R1/`（本报告全部命令日志在此）
- 工具链：`~/.cargo/bin/cargo` 1.98.1 / rustc 1.98.1，`--manifest-path rust/Cargo.toml --locked`
- 平台：macOS darwin 27 arm64（本机；不代替其他平台）
- 主树只读遵守情况：源码零改动（`git status` 与复审开始时一致）；复测在 /tmp 独立 worktree（已清理，自有进程已核清）

## 0. 结论先行

**两条缺陷的虚构分支均已核销，全部 9 个 C-ID 有效通过，内存/FD/spill 预算仍界，同族回归全绿。**

VERDICT: **PASS**（一行结论见文末）。

## 1. 审查输入（均已完整读取）

- `/tmp/r04_repair_package/Lingxi_R04_Review/Lingxi_R04_对抗性复审与定点修复总控_2026-10-01.md` §R04-RR1-F04、§R04-RR1-F05、§本地算法复算、§5（F04/F05 行）、§6 模板、§7 安全边界
- `R04_RR1_修复验收清单.json` issues[3]（F04 C01–C04）、issues[4]（F05 C01–C05）
- `algorithm_probes.py`（反例形状参考：混合 chunk `[a E6 97]`、150 024B/102 400 窗/200 000 预算）
- `docs/rust-tauri/R04/dispatches/R04_ORCHESTRATOR_PROMPT_2026-09-30.md` §七 R04-T05 第 1 条（输入输出分片/截断语义）、R04-T08（产物登记核验）
- 候选 diff（1 096 insertions / 112 deletions，仅两个源文件）与现行源码；执行者报告 `G04-E01_REPORT.md` 仅作线索（其 red 日志与我的独立复现相互印证，但未转抄）。

## 2. 旧红新绿（独立自写反例，两 F 各一）

我自写反例（非执行者测试）：F04 为单测 `reviewer_indep_mixed_chunk_is_consumed_byte_exactly`（同 chunk `[0x61,0xE6,0x97]` poll→a；空闲重 poll；append `[0xA5]` 再 poll；断言拼接恰为 `a日`、空闲 poll 空、held-back 不计 dropped）；F05 为集成测试 `r04_review_g04r1_indep_f05.rs`（真实网关 prepare→execute + 真实 bash + 真实 spill：150 024B 单行 / 200 000 预算断言「真头必须可见、truncated=false 则必须字节精确全流」；300 024B 腿断言「淘汰必须标 truncated 且真头在」）。两份测试在基线与候选均编译运行。

| 反例 | 基线 614fab1af（/tmp worktree） | 候选 |
|---|---|---|
| F04 混合 chunk | **exit 101**：`joined="aaa日"`（a 交付三次，重复消费实锤）+ held-back 2 字节被计入 dropped | **exit 0** |
| F05 窗口前缀丢失 | **exit 101**：`truncated=false` 且文本以 `"xxxx…"` 开头（HEAD_MARKER 已丢却自称完整）；300KB 腿淘汰未标 | **exit 0** |

日志：`artifacts/rust-tauri/R04/RR1-G04-R1/logs/red_green.log`（BASELINE_F04_EXIT=101 / BASELINE_F05_EXIT=101 / CANDIDATE_F04_EXIT=0 / CANDIDATE_F05_EXIT=0）。候选复测树与主工作区逐字节 shasum 校验一致后进行。

## 3. F04 代码级核对（procsupervisor.rs）

1. **chunk 内 consumed 偏移模型**：`TranscriptChunk{seq,bytes,consumed}`；投递只 join `bytes[consumed..]`，按解码消费字节数精确推进各 chunk 偏移，整块耗尽即出队；部分消费 chunk 作为队首挂起（cursor 指向其 seq）。join 永不含已消费前缀 → 同一字节至多消费一次。**force 排空路径** consumed=joined.len()（全部消费、全部出队），后续 poll joined 为空返回空——无重复。代码路径核读无漏洞；`ring_bytes` 账目（append 加、消费减、淘汰减 `len-consumed`）自洽。
2. **dropped_undelivered_bytes 只计真实环形淘汰**：淘汰只累计 `evicted.bytes.len()-evicted.consumed`（已消费前缀不再计入）；确定性测试 `transcript_ring_overflow_while_holding_back_counts_only_real_evictions`（cap=6，投 a+E697 后再压 5 字节 → dropped 恰为 2=E6 97）。
3. **三态分立 + consumed**：`pending_incomplete_bytes`（等待补齐，非丢失、非 truncated 的独立事实行）、`replaced_bytes`（force 排空悬挂尾部 1×U+FFFD，保持旧契约 truncated=false——旧测 `transcript_force_delivery_flushes_a_dangling_partial` 原样通过）、`dropped_undelivered_bytes`（真实丢失）、`consumed_bytes`。exectools `run_write_stdin` 三条事实行各只在各自事实成立时出现；「dropped by the bounded ring」只在真实丢失时出现。
4. **decode_prefix_utf8 迭代式与 from_utf8_lossy maximal-subpart 对齐**：内部非法子序列逐段消费并各给 1×U+FFFD（error_len 即标准库最大子部分长度）；真正的尾部不完整序列即使前面有非法字节也挂起（执行者新发现的同族 F04-3：补齐字节到来时整字恰交付一次）。边界手工核读：非法后跟不完整尾 `[FF,E6,97]`→FFFD+挂起 2B；连续非法 `[FF,FF,E6,97]` force→3×FFFD（与 lossy 一致）；4 字节 emoji 拆 1+1+1+1 逐 poll 恰一次。属性测试（P2，新种子）断言拼接==`from_utf8_lossy(input)` 全过。

## 4. F04 用例独立运行

| C-ID | 我的独立运行 | 结果 |
|---|---|---|
| C01 | 我的反例（上表）+ 执行者 `transcript_mixed_chunk_prefix_is_consumed_byte_exactly_r04_rr1_f04_c01` + **旧分 chunk 控制组** `transcript_delivers_split_multibyte_characters_intact`（基线/候选逐字节 diff 确认未改动、未削弱） | 全绿 |
| C02 | 我自造两个真实链变体（`r04_review_g04r1_indep_c02c05.rs`）：(a) 运行期挂起段——每个观察 poll 断言文本⇄结构化一致（held-back 行 ⟺ truncated=true 且绝无 drop 文案；无解释的 truncated=true 即 panic）；补齐后 `日DONE` 恰一次、exit 4 上报；(b) 悬挂尾部永不补齐→force 排空：mid-character 文案、无 drop 文案、truncated=false（旧契约） | 全绿 |
| C03 | 属性测试换 3 组全新 seed（1–24→101–124、25–48→201–224、49–72→301–324，共 72 条新随机序列）：P1 合法输入拼接==原文；P2 非法==一次性 lossy；P3 账目恒等 consumed+dropped+pending==total（含最小失败样例收缩器） | 全绿 |
| C04 | 真实 PTY 握手链：posix_openpt+真实 bash+网关全链。代码核读确认无 sleep 碰绿——阶段 2 由程序自身 `PHASE1_READY` 就绪回执驱动（bounded poll-until-marker），ack（`read`）释放补尾；文件内 sleep 仅为 poll 间隔（40/20ms，均有 deadline+内容谓词） | 绿（0.10s） |

## 5. F05 代码级核对（exectools.rs + procsupervisor.rs）

1. **truncated 判定纳入淘汰**：`truncated = head_tail.truncated || evicted_bytes > 0`（exectools.rs:980）——预算大小不改变丢失事实；淘汰通知独立陈述（精确字节数；头窗截在字符中间的 trim 字节计入淘汰数，保守诚实方向）。
2. **内存上界 2×100KiB/进程**：`head_cap = window_cap = limits.output_window_bytes`（默认 `DEFAULT_OUTPUT_WINDOW_BYTES=100*1024`）；head 封满即冻（`if head.len() < head_cap` 才 extend）。不无限保存全文。我用真实 settled record 断言精确恒等式：head.len==min(cap,total)、window.len==min(cap,total)、lost_middle==max(0,total−2cap)。
3. **单超长行回退字节边界安全**：行收集后任一侧实际内容字节数为 0（含末尾空行伪影）也走 `floor_char_boundary`/`ceil_char_boundary` 字节切分回退——头尾内容必在、无 U+FFFD。可达路径上无重叠重复（触发截断必 total>预算≥head_budget+tail_budget，head+tail≤total）。
4. **full_output_claim 四态**：Full 仅当 `!capped && !failed && bytes_written >= total_bytes`；capped/failed/None 各自显式 unavailable+原因。`spill_resource_ref`：failed→**None（零 ResourceRef，无幽灵完整文件）**；capped→展示名 partial。transcript 侧同词汇（`transcript_spill_claim`）。
5. **失败 spill 与封顶分立**：`SpillWriter` 错误臂置 `failed`（非 capped 兜底），writer 死亡不重试；EACCES 目录 open 失败→spill None→「no spill file was kept」。
6. **资源存在性检查保留**：网关 `audit_success_artifacts`→`audit_delivered_file`（存在+常规文件）未被本 diff 触及（toolgateway.rs/artifactverify.rs 不在改动集）；capped spill 的 ResourceRef 仍过该审计。
7. **assemble_retained_output 全保留分支**：total≤head+tail 时 head + tail 的「最后 total−head 字节」后缀拼接，逐字节恰一次（边界含等号）；我以 300KB 腿+账目恒等式实测。

## 6. F05 用例独立运行

| C-ID | 我的独立运行 | 结果 |
|---|---|---|
| C01 | 三腿：(a) 150 020B/200 000 预算——HEAD+TAIL 在，truncated=false 时字节精确全流（我的反例同形状独立复测）；(b) **控制组** 80KiB<窗、>预算：truncated=true（返回级）且**无** eviction 声明、spill 完整可称 full——两事实分清；(c) 300 020B：真头+真尾+`95220 bytes … evicted` 精确计数+spill 300 020B | 全绿 |
| C02 | 执行者 3 条单测 + 集成腿（中填充多字节单行、换行仅末尾；无 U+FFFD；`omitted` 中段标记）+ 我的 F05 反例 300KB 腿（真头断言） | 全绿 |
| C03 | capped spill 恰 64KiB；display 无 "exec_command full output"、含 partial；正文 `Full output: unavailable … capped`；**BEYOND_CAP 只在内存结果、不在封顶文件**（文件级窗口扫描断言） | 绿 |
| C04 | EFBIG（RLIMIT_FSIZE+SIGXFSZ——注入的是**真实 OS write 路径**的失败，非 mock 被测对象）：0 ResourceRef、遗留文件≤失败点、含 HEAD 不含 AFTER_FAILURE、`spill write failed after`+`in-memory result is unaffected` 诊断；EACCES 目录（0o500）：`no spill file was kept`、0 引用、0 遗留文件 | 全绿 |
| C05 | 执行者压力腿（PTY ~170KB 多 poll+双流并发+中途 terminate：spill 关闭稳定、≤cap、live 清空、FD 回基线）+ **我自核腿**：3 个 120KB+stderr 进程后 `/dev/fd` 回基线（≤+2）、每 record 窗口精确恒等式（见 §5.2）、live_handles 空 | 全绿 |

## 7. 同族回归与门禁（主树真实退出码）

| 命令 | 退出码 | 备注 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | |
| `cargo test --workspace --locked` | 0 | **88 套件 951 passed / 0 failed**（含 3 个新套件） |
| `cargo run -p xtask -- check-contracts` / `check-boundaries` | 0 / 0 | |
| G01 `r04_t05_registry_capacity` | 0 | 9/9 |
| G02 `r04_rr1_f02_reaper_cleanup` | 0 | 11/11 |
| G03 `r04_rr1_f03_stop_honesty` | 0 | 7/7 |
| T05 `r04_t05_process_tools` | 0 | 14/14 |
| T06 `r04_t06_sandbox` | 0 | 10/10 |
| T08 `r04_t08_tool_matrix` | 0 | 10/10（300s，含 R03 回归链） |
| 新三套件（f04_pty_consumption / f05_output_integrity / f05_spill_failure） | 0 | 1+7+2 |

**旧测试无删除/削弱**：两源文件函数名集合 diff 为纯新增（0 删除）；`transcript_delivers_split_multibyte_characters_intact`、`transcript_force_delivery_flushes_a_dangling_partial`、`transcript_ring_drop_of_undelivered_is_counted_honestly`、`decode_prefix_utf8_boundary_variants` 四条旧测逐字节比对与基线一致且在候选通过。G01–G03 的语义（LiveSlot 预留、reaper 观测门、StopUnconfirmed 诚实链）与 F04/F05 改动正交，套件全绿。

## 8. 诚实边界核对

- 执行者未验证项（非 macOS 平台、阶段级 verify-stage R04 留给 G05/独立阶段审查）在其报告中**如实申报**，且与我的复核一致：我同样仅在本机 macOS 复测；verify-stage 不在本工作单复测最低集合内。
- **无 mock 顶替**：三个集成文件均走真实 `ToolInvocationGateway`→真实 `ProcessTools`→真实 `ProcessSupervisor`（真实 posix_openpt、真实 /bin/bash、真实 spill 文件）。RLIMIT_FSIZE/SIGXFSZ 是对真实写入路径的故障注入（OS 向真实 SpillWriter 返回 EFBIG），`/dev/fd` 计数是真实观察——不构成对被验收对象的 mock。
- 执行者 red 证据（`artifacts/rust-tauri/R04/RR1-G04-E01/repro/`，F04 `joined` 重复、F05 headless）与我的独立复现相互印证；本结论不依赖其日志。
- 属性测试为固定 seed 确定性（我另换 72 条新序列复算），非穷举——已知边界，如实。

## 9. 问题清单

**阻塞/FAIL 级：无。**

非阻塞观察（不构成缺陷，无需本单修复）：
1. [OBS-1, Info] `SpillWriter::info()` 对从未写入的 spill 仍返回 Some（bytes_written=0）；两处调用点均以 `bytes_written > 0` 过滤、claim 退化为 unavailable，无失诚实路径。定位 procsupervisor.rs `SpillWriter::info`。
2. [OBS-2, Info] 属性测试 `minimize` 收缩器的 halve 分支用固定重生成 seed 且未用入参——测试内部质量问题，不影响不变性断言效力。
3. [OBS-3, Info] 透明披露：我初审自写 C05 腿时有两处**我自己测试代码**的笔误（字符串字面量未插值；账目恒等式写成 == 而窗口重叠时应为 ≤），修正后通过——该过程与候选代码无关，候选未因此改动。

## 10. 裁决依据对照

- 9 个 C-ID 全部有效通过（§4、§6，含我自写反例/变体/新 seed/自核账目）。
- 两条缺陷的虚构分支核销：F04 重复消费与「暂存=已丢」误报（旧红 joined="aaa日" 与 dropped=2 实锤→新绿）；F05 尾窗自称完整与封顶冒充 Full（旧红 truncated=false 无头→新绿：全流字节精确或显式淘汰标记；四态 claim；失败 spill 零引用）。
- 内存/FD/spill 预算仍界（§5.2、§6 C05：2×100KiB 窗上界、spill ≤cap、FD 回基线）。
- 无本组阻塞；同族（PTY transcript spill 词汇、CollectorCore 消费者、G01–G03/T05/T06/T08/R03 链）无未修复回归。

VERDICT: PASS
