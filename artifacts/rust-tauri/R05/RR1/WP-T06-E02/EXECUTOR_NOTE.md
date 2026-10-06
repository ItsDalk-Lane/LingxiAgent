# WP-T06 R2（F37）执行记录 — R05-T06-修复-r1（2026-10-05）

F37（LOW，reviewAppended 测试电池缺口）：F17 的『父目录组件授权后被换为符号链接』腿无永久仓库测试。
本轮为纯测试追加（零生产改动）：唯一改动文件
`rust/crates/lingxi-service/tests/r05_t06_rr1_media_resource.rs`（R1 新建、未跟踪），追加 2 条永久腿：
- `rr1_f37_parent_dir_swapped_to_outside_symlink_is_refused_by_real_target`（防宽）：先在父目录为真实目录时
  authorize 通过，再把父目录换为指向授权根外 `outside-decoy` 的符号链接 → `read_image_reference` 与
  `read_host_audio` 均 `ErrorCode::Forbidden`，且拒绝消息不含越权内容（`OUTSIDE_REDIRECT_SECRET` 不外溢）。
- `rr1_f37_parent_dir_swapped_to_inside_symlink_reads_the_real_target`（防紧）：同样 authorize 后换父目录为
  指向授权根内 `relocated` 目录的符号链接 → 两个读取器都读到真目标内容
  （`REAL_TARGET_INSIDE_PNG/WAV`，非被移走的陈旧原文件），image 侧同时断言 mime/filename。

工具链：全程 `/Users/study_superior/.cargo/bin/cargo`（rustup 代理 → rust-toolchain.toml 锁定 1.98.1），
`--locked`，未动依赖。本机 macOS arm64。

## 自检（命令原文与退出码）

| 检查 | 命令 | 退出码 | 结果 |
|---|---|---|---|
| 指定电池 | `/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t06_rr1_media_resource` | 0 | 17/17（既有 15 + F37 双腿），`green-battery-f37.log` |
| 邻近电池 | 同上 `--test r05_t06_operations` | 0 | 7/7，`f37-reg-operations.log` |
| 邻近电池 | 同上 `--test r05_t06_rr1_system_speech` | 0 | 10/10，`f37-reg-speech.log` |
| clippy | `/Users/study_superior/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-service --all-targets -- -D warnings` | 0 | `f37-reg-clippy.log` |
| fmt | `/Users/study_superior/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | `f37-reg-fmt.log` |

## 变异验证（隔离 /tmp 副本，仓库工作树零接触）

副本纪律：`rsync -a --exclude target --exclude .mimosa rust/ → /tmp/r05t06-f37-mut/rust/`，独立
`CARGO_TARGET_DIR=/tmp/r05t06-f37-mut-target`；每次变异只改副本的
`crates/lingxi-service/src/operations.rs::read_authorized_bytes_bounded`，事后 `rsync` 复原 + `diff -q`
逐字节一致；仓库同名生产文件 SHA256 变异前后均为
`e42c465525e0e777bbe9ef65cf9819bd9ac57023fa31e971919be80fdbc9f42c`（未触）。副本基线 17/17 绿。

- **mut1（防宽，`mut1-anti-widening-f37.log`）**：把 `read_authorized_bytes_bounded` 退化为 pre-F17 形态
  （不做真目标授权判定、按词法路径 follow-open 读取）→ exit 101，恰 2 红：新腿 (a) 在其
  `expect_err` 断言点红（panic 实见返回 `Ok(Bytes{ bytes: OUTSIDE_REDIRECT_SECRET_PNG })`——越权内容
  被读出）、既有叶符号链接腿同点红（同一诚实性防线家族）；腿 (b) 与其余 15 腿全绿。
- **mut2（防紧，`mut2-anti-tightening-f37.log`）**：在函数头插入『锚目录之下任何符号链接组件一律
  Forbidden』的过紧策略 → exit 101，恰 1 红：腿 (b) 在其 `expect` 断言点红（panic 实见
  `Forbidden: MUT2: … contains the symlink component …/ws/nested`）；腿 (a) 与其余 16 腿全绿。

## 冻结 HEAD 旧红探针（`oldred-frozen-head-f37-probe.log`）

`git archive d80737b6 rust → /tmp/r05t06-f37-oldred`（隔离副本、独立 target），叠加最小自包含探针
`zz_f37_oldred_probe.rs`（两条 F37 腿原文，仅用 HEAD 已有 pub API；不入正式树）：
- 腿 (a) 在冻结 HEAD **绿**（R04 的 authorize 真目标判定本就拒绝授权根外换链——如实记录：腿 (a) 是
  前瞻契约腿，其可红性由 mut1 证明）；
- 腿 (b) 在冻结 HEAD **红**（exit 101：authorize 通过后 `std::fs::read` 按进程 cwd 解析裸相对路径，
  `cannot read the reference image nested/leaf.png: No such file or directory`——F17 的 P1 类读取分歧）。

## 覆盖说明（如实）

`read_authorized_bytes_bounded` 的 `DirFd::open` ELOOP→churn≤8 重解析分支只在『authorize 与 open 之间
组件被换』的真实竞态窗口内可达（authorize 的 canonicalize 已把 scope.path 解析为全真路径，确定性测试
无法在不加生产钩子的前提下注入该窗口）。本轮双腿钉住的是该防线与 authorize 真目标判定合成的可观测
契约的双向行为（防宽：mut1 红；防紧：mut2 红；腿 (b) 另对冻结 HEAD 构成真旧红），与 F37.remaining
要求的双向断言一致；竞态窗口本身的确定性注入属生产行为改动，超出本 LOW 测试缺口范围，未做。

## 残留清理

/tmp/r05t06-f37-mut、/tmp/r05t06-f37-mut-target、/tmp/r05t06-f37-oldred、/tmp/r05t06-f37-oldred-target
已删除（本轮收尾时）；探针文件未入仓库。
