# R18-N02 Gate 身份链追加修复

本目录只记录独立复核后的追加候选；[首批已通过原件](../r18-gate-binding/RESULT.md)保留为旧候选观察，不能替代本轮。

## 同根入口和失败分支

1. `verify.rs` 的命令目录创建、stdout/stderr 日志创建、程序启动等内部错误，现进入命令结果的 `internalError`，保留原错误并标 FAIL，之后照常拍检查点和收尾快照。等待子进程报错时先运行已有的进程组清理并记录 `cleanup`，避免直接 `?` 丢失进程归属。其他提前中断在新鲜证据根内写 `gateAbort`/`overall=FAIL`、前后清单；已有非空证据根拒绝覆盖。
2. `candidate.rs` 对 Git 枚举的每个文件，Unix 逐级用 `openat` + `O_NOFOLLOW` 持有目录和文件句柄，并对比路径前后设备号/inode；父目录换 symlink、文件被目录替换、缺失或路径替换均拒绝。Windows 使用 `OPEN_REPARSE_POINT` 句柄及 volume/file ID 核对父目录与文件，已按本地 `windows-sys` 0.61.2 API 静态审查；Windows 目标未安装，尚无真实编译/运行结论。本次证据目录只按原始字面路径排除，不使用解析后的 symlink 目标作排除范围。
3. `runner_identity.rs` 在正式 Gate 命令前后逐字节对照编译进 xtask 的 main/verify/stage_map/candidate/identity 程序、R02.json 阶段图及相关 Cargo/toolchain 配置与当前磁盘原文；未知或缺失的源文件也拒绝。旧二进制执行新磁盘阶段图会在执行命令前 FAIL，并在本轮结果中给出 `runnerSourceBinding`。这种检查只能约束包含此代码的新二进制；更早的旧二进制不会自带该检查，独立验收必须要求 `runnerSourceBinding.status=PASS` 并使用当前候选的正式构建/调用记录。

## 检查与原件

- `check-final-b.json`、`clippy-final-b.json`、`xtask-test-final-b.json`：无端口编译/严格检查/单元测试退出 0；当时 xtask **66/66**。这些记录包含真实命令、UTC 起止、退出码、各相关文件检查前后 SHA-256、原始 stdout/stderr。
- 后续又收紧了证据排除路径，`fmt-xtask-final.json`、`candidate-tests-final.json`（7/7）、`runner-identity-tests-final.json`（1/1）、`spawn-error-test-final.json`（1/1）、`log-error-test-final.json`（1/1）、`diff-check-final.json` 均退出 0。`check-final-c.json` 与 `clippy-final-c.json` 也退出 0。
- `xtask-test-final-c.json` 是后来 **65/66 FAIL** 的原始记录：并行的 R02.json 叶证据修改使旧的 LA-200D4E5D52C9 泛化认证负控失败。`fmt-final-c.json` 退出 1 指向并行编辑的 `lingxi-service/tests/tls_web_login.rs`。这两项没有改写为通过，相关组和主执行者会在源码稳定后复查。
- `check-1.json`、`xtask-test-1.json`、`xtask-test-2.json`、`clippy-final-a.json` 保存本组开发中的真实首次编译、断言及严格检查失败，后续修复结果另列，不覆盖原件。`internal-error-tests-final.json` 过滤器实际匹配 **0 项**，不能作为通过证据，已改用上面两个精确负控。

完整 `verify-stage R02`、真服务、全 npm、A16、Windows 编译/运行均未由本组执行。本组静态修复与定向无端口检查不构成 R02 阶段 PASS。
