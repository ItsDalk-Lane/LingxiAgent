# R02 Windows 私有文件权限组：静态核对（2026-09-28）

## 范围与分类

这是 R02 多平台原范围中的旧缺口：此前 Windows `ensure_private_dir`、`atomic_write_private`、SQLite/备份的 `tighten_owner_only` 分支未设置私有 ACL。新代码只从静态阅读与 macOS 宿主编译得到检查结果；Windows 编译与运行仍为 **BLOCKED**，不得据此关闭 Windows 验收。

本组覆盖数据根、runtime/tmp/data/logs 目录，local token、设备/凭证注册簿、管理状态、实例锁和记录、标准审计日志、轮转日志，SQLite 主文件和 WAL/SHM、备份 partial/DB/manifest、恢复目标。既有 home 和用户指定的既有备份目录只核权限与所有者；若不安全，明确拒绝，不修改整个用户目录。新建目录和文件在写入内容前设置当前进程用户与 SYSTEM 的受保护 DACL。实际服务的 `InstanceGuard` 保留 home/runtime 与祖先句柄；备份/恢复期间保留目标目录与祖先句柄；原子写期间也保留父目录句柄。

## 本地绑定核对

- 以已锁定的 `windows-sys 0.61.2` 本地源为准，逐项找到了本组调用的 **17 个 Win32 函数**以及 **41 条常量/类型声明**。原始声明见 `windows-sys-0612-api-signatures-final.txt`、`windows-sys-0612-constants-final.txt`。
- `CreateFileW`/`CreateDirectoryW` 的路径、访问位、分享位、`SECURITY_ATTRIBUTES` 参数；`GetSecurityInfo`/`SetSecurityInfo` 的句柄、DACL、owner 参数；`GetTokenInformation` 的 `TokenUser`/`TokenOwner`；`ConvertSidToStringSidW` 与 `LocalFree` 配对；`GetAce`/`GetAclInformation`/`EqualSid`/`IsValidSid` 的声明均与源码调用形状一致。新增 Windows features 与本地生成绑定名一致。
- 此项只是类型签名核对。缺少 Windows 标准库 target，`#[cfg(windows)]` 实际分支未被本机编译；Win32 真实权限行为仍需 Windows 机验证。

## 已执行检查及原件

- 使用 `/Users/study_superior/.cargo/bin/cargo`，版本 1.98.1；仅安装 `aarch64-apple-darwin` target。版本原件：`rustup-1981-version.txt`。
- 当前源码的 `cargo check --offline --locked`（service + adapters）exit 0：`rustup-1981-final.meta.txt`、`rustup-1981-final-check.*.log`。
- `paths::tests` 5/5、`backup_restore` 11/11（macOS）exit 0：同一 meta 及 `rustup-1981-final-paths.*.log`、`rustup-1981-final-backup.*.log`。
- `rustfmt --check` exit 0：`rustup-1981-fmt-check.*`。当前相关文件 SHA-256：`rustup-1981-final-source-sha256.txt`。
- 首次定向测试误在仓库根运行，因找不到 `Cargo.toml` exit 101；原件保留为 `host-targeted-tests.*`，随后 `host-targeted-tests-rerun.*` 正确指定 manifest 并通过。此前 Homebrew 1.93 宿主检查是历史中间观察，最终以 rustup 1.98.1 命令为准。

## 未闭合风险 / Windows 恢复条件

1. Windows target 未安装，本机不能编译或运行 `windows_acl.rs` 和 Windows 专项测试；需要已有合规 Windows 环境按锁定工具链编译并执行新建、旧目录宽权限拒绝、所有者、硬链接、路径替换、ACL 设置失败、备份/恢复/SQLite/日志实际写入测试。不能拿 macOS PASS 替代。
2. 既有 Windows Lingxi 数据根若沿用宽松 ACL，将被明确拒启；须在真实 Windows 环境核对既有安装目录的 ACL 与迁移合同。当前代码不自动修改用户原有共享目录，这一兼容性影响尚未验收。
3. 权限守卫覆盖正式 `acquire` → 服务启动链；单独调用库的测试构造器并不持有长期实例守卫。正式阶段验收应覆盖真实启动链。

因此本组是“源码已修、macOS 定向检查通过、Windows 动态验收 BLOCKED”，不是 R02 PASS。
