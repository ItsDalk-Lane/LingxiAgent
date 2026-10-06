# RR2 WP-D（F43）独立验收 REVIEW-r1 — 2026-10-06（独立上下文验收智能体，未参与实施）

工具链：`/Users/study_superior/.cargo/bin/cargo`。验收对象：HEAD ad5ec4e98 工作树（RR2 未提交改动）。
我的运行输出均存 `/tmp/rv-d-*.log`（未写入本目录以外的 artifacts；本文件是唯一新增产物）。

## Verdict

- **(b) r04_a09 pid 屏障：PASS**
- **(c) f24_sigterm 就绪屏障/SIGTERM 契约：PASS**
- **(a) 根因归因：成立**（本人探针复现，非采信诊断文字）
- **workspace 复跑：exit 0（完整绿窗）**——本轮连 r00 环境项也通过（见下），失败集合为空
- mustFix：无阻断项（两条非阻断观察见文末）

## 1. 静态核对（git diff）

- `tests/r00_management_leaves.rs`：**零改动**（`git diff` 输出 0 行）。
- `tests/r04_t05_process_tools.rs`：`read_pid_file`（filter_map 静默丢弃坏 token）→ `complete_pid_file`（非空 + 逐 token 可解析才返回，`None` 表示未就绪）；屏障 `pid_file.exists()` → 轮询 `complete_pid_file(&pid_file).filter(|pids| pids.len() == 3)`（20s 有界）。`assert_eq!(pids.len(), 3, "child + two grandchildren: {pids:?}")` 及后续子孙回收/sentinel 存活断言**逐字保留**（diff -U10 核对上下文未动）。语义严格加强，无削弱。
- `src/bin/r04_t07_fixture.rs`：仅新增 `ask_model_tagged` match 臂（purpose 固定 "summarize"、tag 走 argv/prompt）；既有 `ask_model` 等模式零改动，纯 additive。
- `tests/r05_t08_production_tools.rs`：
  - 就绪屏障 = `stub.requests()` 含 `model=="summarize-model-f24"` 的真实回调请求 **且** `count_processes(&tag) > 0`（90s 诚实上界；超时诊断带 stub hits/进程数/服务 stderr）。非固定 sleep、非仅 pgrep。
  - SIGTERM 断言 `exit.success()` → `code==0 || code==6`；code 6 必须同时钉住 `"shutdown exceeded the unified 8000ms from-signal budget"` 与 `"transport_drain_timed_out=true"`（两串与 `main.rs:796` 的 exit-6 分支逐字对应；shutdown.rs 头注释明确 5/4/6/0 语义）。4/5/信号死亡仍失败——比"任意退出码"严格。
  - 预算未放宽：`--shutdown-timeout-ms 8000`（argv 原样）、`stop()` 30s SIGTERM 界、15s worker 回收界全部原样；worker 回收断言（"the worker child survived the service shutdown"）逐字保留。
- 根因源码闭环：`lingxi-kernel/model_exchange.rs:682` `from_purpose` 仅映射 title/summarize/memory/vision/approval/guard 六名，nonce → None；`lingxi-service/src/workerrpc.rs:174` `PurposeNotGranted → "model_purpose_not_granted"`。

## 2. 亲跑结果（全部本人执行）

| 项 | 命令（均为 `cargo test --manifest-path rust/Cargo.toml --locked …`） | 结果 |
|---|---|---|
| (b) 正 | `-p lingxi-service --test r04_t05_process_tools` | exit 0，**14/14**（/tmp/rv-d-b-suite.log） |
| (b) 负 | 临时改 printf 只发布 2 pid（改前 sha256 备份、跑后还原并复核一致） | exit 101：`timed out waiting for complete tree pid file (3 parseable pids)`，**20.08s 精确超时**（/tmp/rv-d-b-neg.log） |
| (c) 正 | `-p lingxi-service --test r05_t08_production_tools` | exit 0，**6/6**（9.50s），跑后 `pgrep -fl r04_t07_fixture` 为空（/tmp/rv-d-c-suite.log） |
| (c) 负 | 临时还原 `workers_section("ask_model", &tag, &tag)`（nonce purpose；同法备份/还原/复核） | exit 101：`the worker child never reached its parked callback wait …; stub hits: 32, worker processes: 0`，**90.80s 精确超时**；日志含 32×gateway dispatch + `failed.turn_budget_exceeded`，与执行者 pre-fix 证据逐量一致（/tmp/rv-d-c-neg.log） |
| workspace | `--workspace`（后台，约 50 分钟） | **exit 0**：115 个 `test result: ok`，共 **1476 passed / 0 failed**，含 Doc-tests；其中 r04_t05 14/14、r05_t08 6/6、**r00_management_leaves 1/1（170.25s 通过）**（/tmp/rv-d-workspace.log） |

## 3. (a) 根因独立复核（差分探针，全部本人重跑）

用执行者留在 `D-R2/probe/` 的零 Lingxi 代码探针源（`zz_lanprobe.c` 重新编译至 /tmp 全新实例、`zz_lanprobe.py` 走 Apple 签名 `/usr/bin/python3`）：

| 探针 | 地址 | 结果 |
|---|---|---|
| 全新 clang adhoc 二进制（无 ALF 判定） | 127.0.0.1:47661 | ok（connect+accept+全交换，0.77s） |
| 同一二进制、同一进程形态 | 192.168.3.5:47662 | **stalled**：connect+write 成功、**server accept 永不触发**、recv 6s 超时（EAGAIN） |
| Apple 签名 /usr/bin/python3（同机同刻） | 192.168.3.5:47663 | ok（accept 触发、全交换，0.38s） |

ALF 全局状态（只读）：enabled、block-all 关闭。本机 en0=192.168.3.5、en1=192.168.3.10。
结论：**"ALF 按二进制实例（路径+cdhash）拦未授权应用的非回环入站流"的归因成立**——按应用而非按地址/路由/地址族过滤（回环 ok、同地址 Apple 签名 ok、无判定二进制 stalled）。执行者补充的"判定不跨 relink"（同路径旧 Allow 仍拦）与其 rerun-current-binary.log（294787929158efb0 逐字复现 RR1 签名）一致，本轮未重复 relink 实验。

环境项登记核对：`docs/rust-tauri/R05/PROGRESS_LEDGER.json` `environment_items[0]` = `R05-ENV-ALF-UNSIGNED-TEST-BINARY`，字段（id/summary/affected/decisive_evidence/not_a_code_regression/remediation_requires_user/executor_self_unblock/r04_precedent）符合既有口径。

## 4. workspace 窗口现状（与执行者记录的差异，如实报告）

执行者 17:57/18:2x 两轮：exit 101，唯一失败 r00（55.77s/55.11s stall，二进制 590de196dceb15ce）。
本人本轮（21:14 结束）：**exit 0，无任何失败**；r00 以 **170.25s held 后放行**的形态通过——与执行者 16:53 轮"用户当刻点 Allow，held>60s 后 ok"的形态一致，且二进制实例同为 590de196dceb15ce（未重链接）。合理解释：该实例的 ALF Allow 判定在本轮运行期间已存在/被授予（用户动作）。同时我的全新无判定探针二进制在 20:0x 仍于 192.168.3.5 stalled——ALF 拦截机制本身仍在，逐实例判定模型得到双向确认。
即：**执行者"仅剩 r00 环境项"的记录对其运行时刻属实；本轮环境项未再构成阻断，正式命令取得完整绿窗（exit 0）**。T06-C11B 的关闭属治理动作，由维护者按证据推进（本 REVIEW 即证据之一）。

## 5. 非阻断观察（不构成 mustFix）

1. RR2_ISSUE_MATRIX.json testEvidence 中"`(c) 修复前隔离跑=…model_purpose_not_granted`"措辞略松：该拒绝码实际落在 runs.db/源码路径（workerrpc.rs:174），隔离日志可见解为其上游表象（32×dispatch + failed.turn_budget_exceeded，本人复跑数值一致）。实质无误。
2. F43 行 remaining/status 仍按"BLOCKED 待用户动作"口径书写；本轮 exit 0 后该文案已滞后于事实，建议维护者下轮更新（不属于本验收的修改权限）。

## 6. 完整性声明

- 两次负例的临时改动均已按 sha256 精确还原（ad2b9d81…/584ad879… 与改前一致）；`git status` 修改文件集合与接管时相同（18 个），r00 测试文件始终零改动。
- 未执行 git commit/push；除本文件外未写任何仓库路径；运行输出全部在 /tmp。
