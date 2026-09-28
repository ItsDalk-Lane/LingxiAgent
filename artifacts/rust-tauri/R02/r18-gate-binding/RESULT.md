# R18 Gate 候选内容绑定：静态修复与无端口验证

- 修复范围：`rust/crates/xtask/src/main.rs`、`verify.rs`、`candidate.rs`、`candidate/tests.rs`、`xtask/Cargo.toml` 和 `rust/Cargo.lock`。只改正式 `verify-stage` 的候选身份链及其单元测试；R00 原件、历史记录未改。
- 内容清单：Git 已跟踪文件和非忽略新增文件；按原始路径字节排序，逐个记录类型、实际文件 SHA-256、Unix 权限，聚合 SHA-256。正式运行前、每条注册命令结束后、整轮结束后比较。仅排除本次 `--evidence` 输出子树；输出在仓库外时不排除任何源码。
- 防绕过：缺失的 Git 枚举文件、文件被目录替换、候选符号链接、`--evidence` 或其祖先为符号链接、证据目录就是仓库根目录，均在开工快照阶段报错。无法读取/枚举/写入摘要同样失败。路径使用十六进制原始字节身份，避免非 UTF-8 路径被替换字符合并。
- 结果：`verify-stage-result.json` 中增加 `candidateSourceBinding`，写入前后摘要、完整清单路径及清单本身的 SHA-256、逐命令检查点、排除项、实际结束 HEAD 和失败原因。源码或 HEAD 漂移、任一快照错误时 `overall=FAIL`；原命令/场景的失败字段原样保留。两份完整清单分别在本次证据目录的 `candidate-source-before.json` 与 `candidate-source-after.json`。

## 已执行检查

`fmt-final4.json`、`xtask-check-final4.json`、`xtask-clippy-final4.json`、`xtask-test-final4.json` 均记录原始命令、UTC 起止、退出码、检查前后六个相关源码/锁文件的 SHA-256，以及 stdout/stderr 原始日志；四项退出码均为 0，检查前后源码摘要一致。`xtask-test-final4` 为 **62/62 PASS**。`git diff --check -- rust/Cargo.lock rust/crates/xtask` 退出 0。

首次直接单元测试 58/59，失败原因是 macOS 拒绝创建非法 UTF-8 文件名；该首次命令的终端输出未保存为文件，不能冒充已有原始日志。后来 `xtask-test-final3.json` 保留另一项真实失败及原日志：并行测试临时 Git 目录同名碰撞，61/62；修复后最终 62/62。

完整 `verify-stage R02`、真服务、全 npm、A16 均未因本组修复执行；这份无端口结果不能作为 R02 阶段 PASS。Windows 目标未安装，此处 Windows 分支仅静态审查，未编译或运行。单条命令内部曾修改源码又在该命令结束前恢复的瞬态行为，离散检查点无法证明；本实现保证开工、每条命令结束及收尾三个层次的字节一致性。
