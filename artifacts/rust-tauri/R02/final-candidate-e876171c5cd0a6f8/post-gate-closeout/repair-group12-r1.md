# R02 最终收口修复 — 根因组 12 R1：`test_mode_home_name` 纳秒碰撞偶发

- 子代理：`R02-REPAIR-GROUP-12-R1`
- 时间（UTC）：2026-09-28T20:11Z（验证完成时刻）
- 唯一改动文件：`rust/crates/lingxi-service/src/config.rs`（+23 / −9）
- 未 commit / 未 push；未弱化任何断言。

## 根因

`test_mode_home_name(pid)`（原 config.rs:817-823）仅用
`SystemTime::now().as_nanos()` 生成"唯一"名。背靠背两次调用可落在同一纳秒
（时钟分辨率/负载），今日 19:37 与 bisect5 G2 两次真实复现即此：
`config::tests::test_mode_home_name_is_unique_and_prefixed` 中
`assert_ne!(a, b)` 失败。同缺陷也是产品健壮性问题：同纳秒创建的两个
test-mode home 同名同根冲突。

## 修复

`test_mode_home_name` 增加进程内单调唯一源：

```rust
static COUNTER: AtomicU64 = AtomicU64::new(0);
let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
...
format!("lingxi-service-test-{pid}-{seq}-{nanos:x}")
```

- 进程内计数器单独保证唯一（同纳秒不再碰撞）；pid + 纳秒保留跨进程区分。
- 函数签名不变；零新依赖。
- 消费者取证（全仓 grep `test_mode_home_name` / `lingxi-service-test`）：
  仅 config.rs:849 `temp_base.join(...)` 作为路径组件使用 + 测试前缀断言；
  无任何代码解析名称格式（文档散文描述格式，无解析器），前缀
  `lingxi-service-test-` 保持兼容。
- 测试加固：`test_mode_home_name_is_unique_and_prefixed` 从 2 次调用改为
  紧凑循环生成 N=512 次并 HashSet 去重断言（每次断言前缀），严格强于原断言。

## 验证（环境：`PATH="$HOME/.cargo/bin:$PATH"`，
`CARGO_TARGET_DIR=/tmp/rust-target-r02-final`，`--locked --offline`）

| # | 命令 | 次数 | 结果 | exit |
|---|------|------|------|------|
| a | `cargo test --manifest-path rust/Cargo.toml --locked --offline -p lingxi-service --lib test_mode_home_name -- --nocapture` | 50（最终文件状态，fmt 收尾后重跑；日志 `/tmp/r02-final/g12-loop-final-{1..50}.log`） | 50 PASS / 0 FAIL | 每轮 0 |
| b | `cargo test --manifest-path rust/Cargo.toml --locked --offline -p lingxi-service --lib` | 1（最终文件状态，日志 `/tmp/r02-final/g12-fulllib.log`） | 184 passed / 0 failed | 0 |
| c1 | `cargo fmt -p lingxi-service -- --check` | 1 | 无 diff | 0 |
| c2 | `cargo clippy -p lingxi-service --all-targets --locked --offline -- -D warnings` | 1 | 零告警 | 0 |

注：首轮 50 次循环与全量跑在 rustfmt 换行收尾前完成（逻辑等价），随后在
最终文件状态全部重跑上表所记结果，证据绑定最终代码。
