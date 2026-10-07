# D-REVIEW-01 精确允许入站准备（未执行）

状态：PREPARED；required r00 gate = BLOCKED/FAIL。本文件和 MATCH 校验只说明准备对象正确，不说明环境已经解除。

本轮唯一对象：

`/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0`

- SHA256：`9f7489029c91d1c232e3c204236854bd53c69cee2fef33d6fd778a27f44696d3`
- CDHash：`d9676388f524872f1269c13f61f413458c0e9e52`
- CDHashFull：`d9676388f524872f1269c13f61f413458c0e9e5246adbc3b7ccb9da04054687d`
- 有效 Mach-O arm64、linker ad-hoc 签名，TeamIdentifier 未设置；完整签名与验证在 identity-before/after.json。
- CargoJSON 唯一对应 lingxi-service r00 test artifact，fresh=false；预构建、运行中和结束后 SHA/CDHash 相等。
- 当前 HEAD `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b` 加未提交修改；355 个 Rust/脚本/锁/工具链输入摘要 `0f945b9884ee73957a6db3fe19c3d6b4a6b87672feef7c3fd1b46998f2f92b75`。

## 只读前置校验

在仓库根运行：

```sh
/usr/bin/python3 artifacts/rust-tauri/R05/RR3/D-REVIEW-01/identity_check.py
```

本轮已实际运行，exit0，status=MATCH，systemChangesExecuted=false。脚本核对绝对路径、非链接产物、SHA/CDHash、有效签名和本轮来源输入；任何不一致输出 STALE/BLOCKED 并非零退出。校验至操作之间仍存在时间窗口，需保持该对象不被重构建；本准备不授予操作授权。

## 已准备的最小操作（本审查不执行）

本机 `socketfilterfw --help` 返回255并打印完整帮助；确实支持 `--remove <path>`、`--add <path>`、`--unblockapp <path>`。255是该次实际退出，未改写成0。

已有同路径 Allow/permitted 与本轮实际局域网 FAIL 并存，故拟仅移除该路径旧应用条目、重新登记当前文件、允许该应用入站。管理员命令逐条执行，前一条成功再进行后一条；执行前须由总控集中处理对应系统授权。本审查没有请求授权，也没有执行以下命令。

```sh
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --remove '/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0'
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add '/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0'
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp '/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0'
```

等价系统界面操作：系统设置 → 网络 → 防火墙 → 选项，仅将上面确切路径的旧应用条目移除、添加当前文件并设置「允许传入连接」。不操作其他同名程序、探针或整个目录；不全局关闭防火墙，不重签，不改变路由或权限规则。

## FINAL 对象与复验边界

本准备对象是本轮最终 Rust 生产源（含 F46）生成的 r00 集成测试程序，不是 lingxi-service 正式服务程序。当前 FINAL 尚未执行，**不能证明未来 FINAL 实际程序就是本对象**。

当前 R05 stage map 的 workspace 命令为 cargo test --manifest-path rust/Cargo.toml --workspace --locked；运行器从仓库根启动并继承环境。FINAL 使用的工具链、PATH、构建环境、target/feature/profile或依赖构建发生变化，都可能再次重链接。总控/新 FINAL 必须从其真实 CargoJSON 精确取得目标并重新核 SHA/CDHash；只有对象与本准备完全相同且来源输入相等才可消费本准备。文件名相同也不能沿用；D-01 的 c5975a45… 准备已过期。

系统操作后，用新编号保存身份、ALF状态、监听与真实运行结果。原命令不改 LAN/20s/断言：

```sh
/Users/study_superior/.cargo/bin/cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r00_management_leaves -- --nocapture
```

实际 1 passed/0 failed/0 ignored/0 filtered、自然exit0才是该套件通过；permitted/MATCH 不算。若仍失败，保持 BLOCKED并继续真实过滤诊断。单套件通过后仍需新 FINAL 完整规定链，本准备不代表 A/C/F46、workspace、R05 或 R06_READY 放行。总控先完成其他独立工作，再集中处理系统动作；本审查到此仅交准备。
