# N06：Windows 安全审计日志路径替换风险

## 原缺陷与同类入口

旧 Windows 分支先用 `symlink_metadata` 检查 `logs` 和 `security-audit.jsonl`，再按路径调用 `OpenOptions::open`。若目录或文件在两次调用之间被替换，检查对象与实际写入对象不同。管理和设备注册簿的所有安全审计均经 `security_audit::project`，因此启动补投影、正常写入和故障重试共用这个缺陷入口。

## 当前静态修正

- 数据根以 Windows NT 绝对路径打开，`OBJ_DONT_REPARSE` 拒绝路径中的重解析点；随后以已打开的目录句柄作为 `RootDirectory`，逐层打开 `logs` 和固定文件名。这避免了检查后重新按可变路径解析。
- 每次打开后仍检查句柄对应对象的重解析属性、目录/普通文件类型；日志文件还要求链接数为 1。打开期间不共享删除权限，数据根与日志目录句柄保持到日志同步写盘结束。
- 覆盖旧路径检查的所有调用方，无需改变审计事件格式、上限或补投影顺序。`windows-sys 0.61.2` 原已在锁文件；本轮只增加服务包对该版本的 Windows 目标依赖。
- 新增 Windows 专项测试源码：数据根、目录、文件链接及硬链接拒绝，和打开句柄期间路径不可替换。该测试**尚未运行**。

实现依据：[Microsoft `NtCreateFile` 的目录句柄相对路径和打开选项](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntcreatefile)、[Microsoft `OBJECT_ATTRIBUTES` 的 `OBJ_DONT_REPARSE`](https://learn.microsoft.com/en-us/windows/win32/api/ntdef/ns-ntdef-_object_attributes)。

## 本轮真实验证

| 检查 | 结果 | 原始证据 |
|---|---|---|
| macOS `cargo check --offline --locked --manifest-path rust/Cargo.toml -p lingxi-service` | PASS，exit 0 | `revised-host-check.log` |
| macOS `cargo test --offline --locked --manifest-path rust/Cargo.toml -p lingxi-service --lib security_audit::tests -- --nocapture` | PASS，3/3，exit 0 | `revised-host-tests.log` |
| `cargo check --offline --locked --manifest-path rust/Cargo.toml -p lingxi-service --target x86_64-pc-windows-msvc` | BLOCKED，exit 101；本机 Homebrew Rust 只有 `aarch64-apple-darwin`，缺 Windows `core/std`，在编译本包前停止 | `revised-windows-target-check.log` |
| Windows 专项链接、硬链接、替换测试 | 未执行；无 Windows 运行环境 | 测试源码见 `security_audit.rs` |

当前候选检查窗口：`revised-start-utc.txt` 至 `revised-end-utc.txt`。开始/结束摘要文件完全一致；`revised-start-sha256.txt` 保存本轮修正的三个文件摘要。此前候选的日志（`final-*`、`cargo-check-windows-target-first.log`）均保留，没有覆盖。

## 剩余风险和恢复条件

**N06 当前只能判为“静态修正，Windows 编译与运行 BLOCKED”。** 必须在带 Windows Rust 目标标准库的环境编译，并在 Windows 主机运行上述专项测试，检查普通写入、链接/硬链接拒绝、目录替换、旧日志重放与失败后恢复，才可把 N06 关为 PASS。本机 macOS 结果不能替代。

Windows 数据根和 `logs` 的访问控制列表未在本轮重设；项目 `paths.rs` 的非 Unix 分支原本也只创建目录。该旧有权限问题与 N06 路径替换为不同根因，需在全范围问题表单列，不因本轮 nofollow 修改而宣称解决。
