# R06-T06 测试守卫（内存/时间）：说明与使用

> 适用：`rust/` workspace 的全部 `cargo test` 面（133 个测试二进制，含被测试 spawn 的
> 服务/夹具子进程）。实现：`rust/crates/lingxi-test-guard/`。

## 1. 背景（为什么存在）

2026-10-10 事故：`r06_t06_memory` 测试进程被一个无终止循环拖入无界字符串累积，
约 61 分钟内常驻内存涨到 ~176GB，耗尽 96GB 机器内存，整机卡死被 macOS 看门狗重启。

- **根因**：`lingxi-service/src/memory.rs` 的 `urldecode()` 循环里 catch-all 分支
  `b => out.push(b as char)` 不推进游标 `i`——任何含普通字节的输入（如
  `agentId=agentA`）都会无限循环、无限追加。修复：逐字节解码、每个分支都推进游标
  或终止，结构上有界（另附 3 个回归单元测试，见 `memory.rs` `mod tests`）。
- **修复前实测**（同一二进制、`--test-threads=4`）：12 秒涨到 4.3GB（外部监控 4GB
  处截杀）。
- **修复后实测**：连续 3 次运行，峰值 ~32MB、单次 ~0.4s，7/7 通过
  （证据 `artifacts/rust-tauri/R06/T06/09_test_guard/fix_after_r06_t06_memory.txt`）。

守卫是第二道防线：即使未来再引入类似的失控，进程也会在阈值处被 **abort**，
而不是把机器拖死。

## 2. 机制（默认对全部测试生效）

- `lingxi-test-guard` crate 用 `ctor`（预 main 钩子）在**测试进程启动时**安装一个
  后台采样线程 `lingxi-test-guard`；任何链接了被测 crate 的测试二进制自动覆盖，
  无需在测试里手写调用。
- 采样周期 50ms，读取**本进程真实常驻内存**：
  - macOS：`task_info(MACH_TASK_BASIC_INFO).resident_size`（libc，无新依赖）；
  - Linux：`/proc/self/statm` 第 2 字段 × `sysconf(_SC_PAGESIZE)`；
  - 其它平台：内存采样不可用 → 显式退化为仅时间上限（打印在报告里，绝不静默）。
- 超限动作：无锁 `write(2)`（附同目标去重逻辑）打印报告 →
  `std::process::abort()`。**不用 panic**：后台线程 panic 不会终止测试主线程，
  泄漏进程会继续涨；abort 保证进程真正停止。
- 报告包含：原因（内存或时间）、当前值与阈值、进程名/PID、已耗时、
  **当前存活的测试线程名**（libtest 用测试函数名命名每个测试线程——这即
  "哪个测试在泄漏"），以及一键复现/调整指引。

## 3. 阈值与开关（环境变量）

| 环境变量 | 默认 | 语义 |
|---|---|---|
| `TEST_MEM_LIMIT_MB` | `8192`（8 GiB） | 单测试进程常驻内存上限；`0` = 关闭内存检查 |
| `TEST_TIME_LIMIT_SEC` | `600`（10 分钟） | 单测试进程墙钟上限；`0` = 关闭时间检查 |
| `LINGXI_TEST_GUARD` | 未设置 = 开启 | `0`/`off`/`false`/`no` = 整个守卫关闭 |
| `TEST_GUARD_VERBOSE` | 未设置 = 静默 | `1`/`true`/`yes`/`on` = 启动时打印生效阈值 |

取值非法（如 `8GB`、负数）时**不静默**：stderr 打印告警并回落默认值。

默认值的依据（本机全量实测，134 个测试二进制逐一二进制运行）：正常用例峰值
远低于 1.5GB（事故现场记录：除泄漏进程外全部进程 < 1.5GB）；8GiB 阈值相当于
正常峰值的 5 倍以上富余，命中即代表真实失控。时间 10 分钟同理（绝大多数测试
秒级完成）。时间默认值上市时曾为 300s，在收尾验证中命中既有最慢合法用例
`r04_t08_tool_matrix::approval_face_share_cases`（单跑恒需 ~300.5s：T05 时代
全量负载下 300.57s，候选 hooks-on 300.48s / hooks-off 300.46s——固有耗时，
与 T06 改动无关），属于对已知合法用例的误伤；上调为 600s = 已知最慢合法用例
的 ~2 倍富余，同时仍远小于事故现场的 61 分钟失控，触发即代表真实失控
（证据：`artifacts/rust-tauri/R06/T06/09_test_guard/guard_time_default_incident.txt`）。

## 4. 触发后的现象

- 测试进程以 **SIGABRT（信号 6，退出码 134）** 终止；`cargo test` 报
  `process didn't exit successfully ... (signal: 6, SIGABRT)` 并标记该测试失败。
- stderr（必要时 stdout）出现报告块：

```
=== LINGXI_TEST_GUARD: ABORTING TEST PROCESS TO PROTECT THE MACHINE ===
  reason  : resident memory 0.84 GiB (...) exceeded the 0.50 GiB (...) limit (TEST_MEM_LIMIT_MB)
  process : zz_temp_guard_demo-... (pid 15789)
  memory  : now 0.84 GiB (...) / limit 0.50 GiB (...)
  elapsed : 0.05s / time budget off
  live test threads (libtest names each thread after its test function):
    - main
    - temp_guard_demo_unbounded_growth_trips_memory_limit
  next    : reproduce single-threaded (`cargo test <name> -- --test-threads=1 --nocapture`);
            adjust or disable: TEST_MEM_LIMIT_MB=16384, TEST_MEM_LIMIT_MB=0,
            TEST_TIME_LIMIT_SEC=0 (0 = off), LINGXI_TEST_GUARD=0 (off entirely)
===========================================================================
```

真实触发演示（临时自杀用例，跑完即删）：`artifacts/rust-tauri/R06/T06/09_test_guard/`
的 `demoA_memory_trip.txt`（512MB 限下 0.05s 内 abort）与 `demoB_time_trip.txt`
（2s 限下 2.03s abort）。

## 5. 覆盖方式（为什么选 ctor + feature 边）

| 方案 | 结论 |
|---|---|
| 自定义 test harness（`harness = false`） | 需改写 115+ 个测试文件，侵入过大——否决 |
| nextest setup / 外部脚本 | 无法在测试进程内安装监控（且仓库当前未用 nextest）——否决 |
| **ctor 预 main 钩子 + dev-dependencies feature 边** | 采用：零测试文件改动（少数不引用 crate 的二进制加一行 `use`）、release 图零链接 |

- 覆盖手段：`lingxi-test-guard` 只经各 crate 的 `test-guard` feature / dev-dependency
  边进入测试构建。`lingxi-kernel` 依赖图的测试二进制经 kernel 的 `test-guard`
  feature 自动装；不链接 kernel 的少数二进制（protocol/spike/夹具/bin 测试面、
  6 个 spawn-only 集成测试）有一行显式 `use lingxi_test_guard as _;`。
- 覆盖证据：`coverage_scan.txt`——全部 133/133 测试二进制含守卫符号（nm）。
- 零开销证据：`release_zero_overhead.txt`——普通 `cargo build` 与 `cargo build --release`
  的全部产物守卫符号为 0（守卫只在测试构建存在）。
- **已知边界**：完全不引用被测 crate 的合成测试二进制不会被 rustc 链接守卫
  （rustc 不链接未使用的 crate）。新增此类测试时，加一行
  `use lingxi_test_guard as _;` 即可（dev-dependency 已在其 crate 中）。
- **子进程覆盖（实测口径）**：`cargo test` 构建的 `lingxi-service` 服务二进制
  （CARGO_BIN_EXE_，被 closed_loop 系测试 spawn）经 kernel 链接同族 feature
  传递，实测守卫 armed（`TEST_GUARD_VERBOSE=1 ./target/debug/lingxi-service
  --help` 打印 armed 行）；`r04_t07_fixture` 等不经 kernel 链接的独立小夹具
  不带守卫（其操作面是单次受控操作，无累积路径）。普通 `cargo build` 与
  release 产物不带守卫（见零开销证据）。需要整体关闭时用
  `LINGXI_TEST_GUARD=0` 运行测试。
- **构建波次不在守卫范围内（实测）**：守卫只覆盖测试进程**运行期**。全量
  重建（如源码触碰后的 `cargo test`）会并行编译/链接大量带调试符号的测试
  二进制，属于独立的构建内存风险。本机实测（2026-10-10 复盘）：默认并行度
  下重建 20 秒构建内存即到 5.3GB 且不收敛（27 个并行 rustc）；
  `cargo test -j 8 ...` 时同一波次峰值 3.9GB 且完成。内存紧张时用
  `cargo -j 8 test ...` 限制并行度，或分批构建（例如 `-p lingxi-service`）。
  证据：`artifacts/rust-tauri/R06/T06/09_test_guard/build_wave_*.txt`。

## 6. 约束遵守

- 优先根因修复：泄漏本体（`urldecode`）已修并有回归测试；守卫只做兜底。
- 依赖：唯一新增第三方 crate 是 `ctor 0.8.0`（成熟、维护良好；锁定于
  `rust/Cargo.lock`，登记于 `docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md` D-12）；
  `libc 0.2.189` 复用已有锁定版本。
- unsafe 仅 4 处（`task_info`/`task_threads`/`pthread_getname_np`/内联 `write`），
  均为 libc FFI 调用，逐处注明 SAFETY；不使用未注明理由的 unsafe。
- 守卫自身有 12 个单元测试：阈值解析（默认/覆盖/0 关闭/非法告警/溢出回落）、
  越限判定优先级、真实 RSS 采样合理性、线程名枚举、fd 去重、默认安装证明、
  幂等安装。
