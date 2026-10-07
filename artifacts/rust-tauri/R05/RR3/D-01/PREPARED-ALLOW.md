# 本轮精确允许入站操作准备（尚未执行）

对象只限 `/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0`。

- SHA256：`c5975a452505bd4f90fcf87509902f812148f809b21674302e6dbd34886daa6a`
- CDHash：`6eadd46c232408547f08e305c792f1c4c2614a94`
- 签名：有效 linker ad-hoc，未设置 TeamIdentifier；不是 Developer ID 签名。
- 本轮已读到该路径旧记录为 Allow/查询 permitted，但同一当前文件的非回环真实请求仍超时；不能把旧记录当成当前有效允许。

本轮仅准备操作。子代理没有执行任何系统策略修改。总控先完成其他可独立事项，再集中向用户说明本操作。

## 操作前身份核验

在仓库根运行下方只读校验。只有 `status=MATCH` 才能使用这份准备；若为 STALE、文件丢失或最终候选重链接，先重新构建并重新核对路径、SHA256、CDHash，生成新准备，禁止沿用旧窗口或旧文件名。

```sh
/usr/bin/python3 artifacts/rust-tauri/R05/RR3/D-01/verify_prepared_identity.py
```

## 用户执行的最小范围操作

系统设置 → 网络 → 防火墙 → 选项：找到上面的**确切绝对路径**，移除其旧条目，再把经过身份核验的同一路径当前文件添加，设置「允许传入连接」。仅处理此应用条目。不要关闭整个防火墙，也不要把其他 r00 文件名、探针或整个目录添加为允许。

等价管理员命令如下，**仅已准备，未执行**。移除旧条目是为了重新登记当前对象，范围只限这一个路径；每条必须成功才进入下一条。命令名称已用本机 `socketfilterfw --help` 核实，使用 `--unblockapp`。

```sh
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --remove '/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0'
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add '/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0'
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp '/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0'
```

这是待验证的精确解除操作，尚不能保证它必然解除阻断；如果仍失败，保持 BLOCKED，继续取得系统过滤的有效诊断，不降门槛。

## 操作后复验

1. 再跑只读身份校验，确认没有重链接；记录 `--getglobalstate`、`--getblockall`、`--getappblocked '<确切路径>'`，保持全局防火墙开启和 block-all 未变。
2. 用新证据编号受监督运行：`/Users/study_superior/.cargo/bin/cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r00_management_leaves -- --nocapture`，记录实际 PID、监听、请求地址端口、完整计数、退出/超时/被杀。1 passed/0 failed/0 ignored/0 filtered 才是该套件通过；查询 permitted 本身不是通过。
3. 最终集成冻结后重新取得 Cargo 实际可执行文件身份。全新独立阶段审查者亲跑完整 workspace 和 R05→R04→R03→R02 规定门禁。单 r00 成功不自动代表阶段放行。

新的端口由测试选择，不能固定为本轮 `50220`；该端口只是本轮证据。若任一构建将目标重链接，本文件操作对象身份即过期。
