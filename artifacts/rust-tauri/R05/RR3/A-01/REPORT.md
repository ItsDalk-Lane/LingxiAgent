# RR3 A-01 实施交接（F42 A1 + A2）

状态：IMPLEMENTED / SELF_CHECKED。本文件由实施者编写，不是独立验收；没有自签 ACCEPTED / R06_READY。未提交、推送、建分支或改总控台账，历史证据原样保留。当前交接对象仅 A，F46 logging 属 C 后续生产修复。

## 输入与成功标准

已读 RR3_BRIEF、RR1/RR2 master、RR2 brief/矩阵/进度/交接、R05 现行报告/映射/负测/性能/交接及 brief 指定 RR2 原始证据。HEAD `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`。锁定配置实际在仓库根 `rust-toolchain.toml`，使用 `/Users/study_superior/.cargo/bin/cargo` 与 rustc 1.98.1、`--locked`；Node v24.16.0/npm 11.13.0、Python 3.14.3、macOS 27.0.1 arm64。

成功标准：绑定前固定有真实 OS fd 依据的本次输出集；各祖先日志增长不影响候选，旧证据与其他源码仍被绑定；源/副本用同一输出集；禁止不充分的目录归属、非法根、符号链接及被 Git 登记的输出文件；发现器错误不得当成空集合；`!stable` 独立强制 FAIL 保留。

RR2 原始失败汇总见 `rr2-raw-binding-failures.json`：R04 8/8 checkpoint 不稳，只变父 `r04_regression_gate/stdout.log`；R03 15/15 不稳，只变父 `r03_regression_gate/stdout.log`。历史 R05 stable=true 不改变其 overall FAIL。

## 改动与归属

- `rust/crates/xtask/src/candidate.rs`：Scope 一次发现并固定当前进程及祖先精确 FILE 输出；完整记录排除项，不排除祖先目录。后续快照拒绝被 Git 登记、符号链接、非普通文件和 inode 被替换的输出；其他 tracked/非忽略 untracked 仍逐项绑定。
- `rust/crates/xtask/src/main.rs`：先完成证据目录新鲜性检查，再创建 Scope；保留不稳定时强制 FAIL。`verify.rs` 未改。
- `rust/crates/xtask/src/runner_identity.rs`：编译内嵌发现器字节纳入 runner 身份，发现器源变更而旧 runner 未重建时拒绝；永久测试覆盖此失配。
- `rust/crates/xtask/src/candidate/tests.rs`、`verify/runner_tests.rs`：真实子进程 fd、四祖先 FILE、合法脏树/日志增长/内容变化/inode 替换，以及生产编排的标准四层布局自检。
- `scripts/rust-tauri/run_output_sinks.py`：唯一共享发现器（总控已登记 A 独占）。目录从叶向上逐层证明；子 FILE 不能证明子目录整体，未获证的子目录不能被祖先吸收。ps/lsof 故障抛出；lsof 仅允许 0 或无 stderr 的 1（无 fd 匹配）。
- `scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`：生产 discover 包装调用共享发现器；加强根与 FILE 校验；E0s 永久接入实际发现器回归并保存证据。binder/cmp、classifier/E5 语义未削弱。
- `scripts/rust-tauri/r02_run_output_regression.py`：新永久真实 OS fd 反例与对照。提取生产 shell discover/binder/validators，在隔离 Git 工作区执行，真实父/子 fd，非手传理想 FILE。

## 红绿证据与实际执行数

最后有效完整自检为 `final-selfcheck-02/`，实际 argv、时间、退出码、预期码、日志 hash 全在 `commands.json`；输入/lock/toolchain/schema 配置/stage-map/发现器/二进制 hash 在 `inputs.json`、收尾一致性与测试二进制 hash 在 `../delivery-inputs.json`。所有 A 输入与最后自检相等。

| 检查 | 实际结果 | 证据 |
| --- | --- | --- |
| 原发现器永久反例 | exit 1，命中旧 untracked JSON 被 DIR 吞掉的断言 | `final-selfcheck-02/discovery-old-red.log`、其 `old-untracked-parent-child/sinks.txt` |
| 修后真实发现器 | 4/4，ignored 0 / filtered 0；旧 untracked 父子、仅子 sink、旧 tracked 后代、无旧文件四组全部通过 | `final-selfcheck-02/discovery-green/results.json` 及各组 before/after/source-copy bindings |
| 归属负对照 | 7 非法根拒绝；tracked sink 拒绝；lsof 故障 exit 1 且明确异常 | 同上 `results.json` |
| 变化检出 | 旧证据变化、新源码、source/script/config/stage-map 变化、删除/重命名检出；source/copy 绑定对称 | 同上四组实际 binder 记录 |
| xtask 全套 | 121 passed / 0 failed / 0 ignored / 0 filtered，exit 0 | `final-selfcheck-02/xtask-all.log` |
| 标准嵌套，仓库内证据 | 7/8/15/20 checkpoint，全部 stable=true，changedPath 空，commands/overall PASS | `final-selfcheck-02/nested-internal/`、`nested-checkpoint-index.json` |
| 标准嵌套，仓库外证据 | 同上 7/8/15/20；独立过滤运行 1 passed / 0 ignored / 120 filtered，exit 0 | `final-selfcheck-02/nested-external/`、`nested-external.log` |
| fmt / xtask clippy all-targets -D warnings / locked build / bash -n / diff-check | 各 exit 0 | `final-selfcheck-02/{fmt,clippy-xtask,build-xtask,shell-syntax,diff-check}.log` |
| 生产 E0s 段提取自检 | exit 0；原 62 classifier fixtures + 新 4 实际 fd 对照 + 7 非法根 | `e0s-extracted.sh`、`e0s-01.log`、`e0s-01/f42-real-fd/` |

每层完整记录名为 `verify-stage-result.json`，checkpoint 字段是 `candidateSourceBinding.checkpointAfterEveryCommand`。两种模式全部保存 before/after、command stdout/stderr；索引同时保存各层证据 hash，不以最终一条 stable 代替逐层证明。

## 失败轮与替身边界

全部失败历史保留，不能计入产品红证：`green-discovery` 第一轮的仅子 fd 对照误增长了非 sink 的父文件，绑定正确检出，修正夹具后 `green-discovery-2/-3` 通过；`nested-layout-01` 把内层证据根误指向已写入命令日志的目录，正确触发新鲜性拒绝，修正为真实 `R03_REGRESSION` 布局后 `nested-layout-02` 通过；`discovery-final-01` 回归脚本提取字符串语法错误，修正后 `-02/-03` 通过。初期 `red-discovery` 与最后 old-red 都是明确 F42 目标失败。

`final-selfcheck-01` 虽通过，但之后补了 lsof 错误拒绝，不作为交接有效最终同输入证据；`final-selfcheck-02` 是其后重新 fmt/clippy/build/all-tests/discovery-red-green 的有效轮。`xtask-candidate`、`xtask-all`、`xtask-final-01` 和早期 nested/external 记录均为过程证据。

真实 fd 回归只使用临时 Git 工作区；当前 `.gitignore` 原样复制，生产 discover/binder/validator 不替换。只有旧算法红证指定历史 `discover-before.py`；只在隔离负对照中模拟 lsof 命令错误，发现器保持生产实现。各 SOURCE/copy 输出集合来自实际发现器。临时候选副本的 `.git` 仅在其绑定证据保存后清理，未删除用户/历史内容。

嵌套自检调用真实生产 Scope、`verify_stage_with_checkpoint` 与 `run_command`，真实子进程和标准 R05→R04→R03→R02/A16 布局；映射为测试 RX，叶命令为有界 printf/测试子进程。它证明逐层绑定/输出归属，**不是正式 R05/R04/R03/R02 业务关卡**。受控报告没有正式入口生成的 runnerSourceBinding；实际编译根 runner 身份检查与源失配拒绝由全套测试覆盖。源代码没有注入生产主树。

## 绑定、限制与独立验收入口

`working-tree-observation.json` 保存含全部 tracked/非忽略 untracked 的收尾工作树观察（仅排除 A-01 自身证据子树）：38860 文件，digest `0b41063896068a2ad9c6ed6b00aa776a102f5a0252302b7a12085347d337b44f`。这不是冻结候选，期间 B/C/总控并行写入，不能当正式稳定性证据。完整候选/runner 身份与冻结证明应由独立阶段审查者保存。

本轮未执行完整 workspace/正式 stage gate，也未验证 Linux/Windows；实际 OS 发现器运行平台为 macOS。没有将环境 FAIL 改写成 stable FAIL/或 PASS。A 可移交全新独立审查；完整主树全链必须先由总控安排写入静默窗口。

独立验收先用全新证据目录，确认未存在后顺序执行（cwd 仓库根）：

```bash
python3 scripts/rust-tauri/r02_run_output_regression.py --evidence artifacts/rust-tauri/R05/RR3/A-REVIEW-01/discovery
LINGXI_NESTED_ARCHIVE="$PWD/artifacts/rust-tauri/R05/RR3/A-REVIEW-01/nested-internal" /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p xtask
LINGXI_NESTED_EXTERNAL=1 LINGXI_NESTED_ARCHIVE="$PWD/artifacts/rust-tauri/R05/RR3/A-REVIEW-01/nested-external" /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p xtask rr3_real_fd_standard_nested_layout -- --nocapture
```

最终全新阶段审查者在其他包验收与冻结后亲跑原全套门禁（必须递归检查每层 checkpoints、stable、runnerSourceBinding，不仅此单命令）：

```bash
/Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/RR3/FINAL-01/verify-R05
```

独立审查结果归其新目录与总控，不由本实施者写验收结论。`artifact-digests.json` 是 A-01 证据交接清单，排除清单自身，保留所有失败轮。
