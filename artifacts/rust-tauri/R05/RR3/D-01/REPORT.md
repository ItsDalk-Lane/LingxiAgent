# RR3 D-01：本轮 r00 环境定位与精确操作准备

状态：**BLOCKED（当前非回环真实请求失败；系统操作仅准备）**。这是定位者自检记录，不是独立验收签字，不代表全 workspace 或 R05 通过。

执行者：`/root/rr3_d_impl_01`，全新上下文。全文读取 RR3_BRIEF、RR3_REVIEW_BRIEF、RR1/RR2 MASTER、RR2 D-R2/REVIEW-r1、D-R2/probe 源及日志、RR2 FINAL-01/STAGE_REVIEW；历史文件只读。未继承历史“仅 ALF”或同名旧绿窗的阶段结论。

## 当前事实与边界

- 候选 HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`。A/C 源码修复仍并行；这里不是总体冻结。
- macOS 27.0.1（26A434），arm64。cargo `1.98.1 (797e8a9bc 2026-08-05)` / rustc `1.98.1 (48a229cea 2026-09-01)`，全部使用绝对 rustup 代理 + `+1.98.1 --locked`。Node v24.16.0，npm 11.13.0。
- 没有设置 RUSTFLAGS/CARGO_ENCODED_RUSTFLAGS/CARGO_TARGET_DIR/RUSTUP_TOOLCHAIN/CARGO_PROFILE_TEST_OPT_LEVEL；最终 runner 若采用不同构建环境，必须从其真实 Cargo 产物重新核对身份。
- r00 测试源码与 HEAD 字节相等，SHA256=`3e72c2a2d16287549a2727a355642ec279c09595d42d6ec42d9c5585c8d2f453`；本工作包未修改生产代码、测试、系统策略或门槛。Cargo.lock SHA256=`259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3`。
- r00 输入包含合法合成 home、ManualClock、正式 ServiceState/bootstrap/run、真实 TCP；它是生产 crate 集成测试，不是外部 service 进程启动。合成数据/认证在测试内生成，不读取用户真实 home/凭证，不执行 LIVE。
- 成功标准：原非回环 LAN 断言实际完成，正式 r00 得到 1 passed/0 failed/0 ignored/0 filtered；未达到。本次只做一次正式运行和三组最小差分，无挑绿重试。

## 本轮真实测试产物

Cargo `--no-run --message-format=json` 精确返回的 executable：

`/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0`

- SHA256=`c5975a452505bd4f90fcf87509902f812148f809b21674302e6dbd34886daa6a`
- CDHash=`6eadd46c232408547f08e305c792f1c4c2614a94`
- CDHashFull=`6eadd46c232408547f08e305c792f1c4c2614a94482e343993cb869762f33aea`
- 有效 Mach-O arm64 / ad-hoc linker 签名；`codesign --verify --verbose=4` exit 0；TeamIdentifier 未设置。
- 开始及结束身份相等；08:33:12–08:33:20 的独立只读准备对象校验仍 MATCH（exit 0），并记录 systemChangesExecuted=false。
- r00 相关生产源/测试/锁/配置前后零变化；其输入子集 digest=`33789b4d9829f38298c5112aab865fbd3ce8e13051afa9a19cc339d7da0cf636`。完整 rust/scripts 输入快照在并行期间发生两个脚本变化（r02_run_output_regression.py、run_output_sinks.py），不是整体冻结，也不能用于最终整体验收绑定。
- 完整快照摘要：before=`48f2cfbc10de21bb5319a8704175c8e9f56d28fed2dc2899189b577b5521799f`，after=`ee1799a7d5ce57e82aee13069dbdb8b5f1da11bfbbd58d9e54d8199024099fb0`。完整逐文件清单及差集见 identity-before/after.json、input-comparison.json。

## 真实命令、计数、时间与退出

cwd 均为 `/Users/study_superior/Desktop/Code/LingxiAgent`；时间为本机 +08:00。每条 argv、开始/结束、实际退出、超时标记、日志 SHA256 均保存同名 JSON，不经管道吞退出。

| 名称 | 实际命令 | 开始→结束 | 实际 exit | 观察 |
|---|---|---|---:|---|
| prebuild-r00 | `/Users/study_superior/.cargo/bin/cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r00_management_leaves --no-run --message-format=json` | 08:27:24.246→08:27:24.451 | 0 | 已预构建/热目标，返回唯一对应 test executable；本命令未执行测试 |
| r00-formal-01 | `/Users/study_superior/.cargo/bin/cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r00_management_leaves -- --nocapture` | 08:27:53.849→08:29:05.336 | **101** | 0 passed/1 failed/0 ignored/0 measured/0 filtered；test 自身 54.62s，监督全程 71.486s |
| compile-probe | `/usr/bin/clang -Wall -Wextra -Werror -pthread artifacts/rust-tauri/R05/RR3/D-01/lan_probe.c -o artifacts/rust-tauri/R05/RR3/D-01/lan_probe` | 08:28:50.465→08:28:50.678 | 0 | 全新零 Lingxi 探针；没有手动重新签名 |
| probe-c-loopback | `/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/D-01/lan_probe 127.0.0.1` | 08:29:07.015→08:29:07.431 | 0 | 同二进制回环完整交换，accepted=1 |
| probe-c-lan | `/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/D-01/lan_probe 192.168.3.5` | 08:29:07.482→08:29:13.509 | **1** | connect+send 成功，recv EAGAIN；server poll 6000ms 返回0，accepted=0 |
| probe-apple-lan | `/usr/bin/python3 /Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/D-01/lan_probe.py 192.168.3.5` | 08:29:16.043→08:29:18.124 | 0 | 相同 IPv4 LAN、0.0.0.0 随机端口监听、4字节真实交换，accepted=True |
| prepared-identity-check | `/usr/bin/python3 /Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/D-01/verify_prepared_identity.py` | 08:33:12.990→08:33:20.306 | 0 | MATCH，仅只读身份检查 |

监督器为正式 r00 设180秒总限；本轮自然退出101，**supervisorTimedOut=false，无 TERM/KILL**。不把自然 FAIL 写成外层超时或被杀。监督器若超时会保留真实退出/信号并报告124；本轮没有发生该分支。进程及监听每秒只读采样保存 r00-formal-01-listeners.log，命令/各次采样退出均保留。

## 监听与真实失败

正式 r00 PID=`60881`，父 cargo PID=`60876`，进程开始=08:27:53。先观察回环 `127.0.0.1:50043`，后观察其 LAN 阶段 `TCP *:50220 (LISTEN)`；源码绑定=`0.0.0.0:0`，本次实际端口50220。

真实失败：`POST /lingxi/v1/web-auth/login` 到 `192.168.3.5:50220`，`write/read exchange` 的原20秒预算超时，读取0字节。测试保留真实网卡选择及 LAN 外来 Origin 拒绝断言，没有跳过或改为回环。

最小 C 探针监听 `0.0.0.0:50239`→回环成功；同文件新进程监听 `0.0.0.0:50241`→192.168.3.5 阻断；Apple Python 对照监听 `0.0.0.0:50288`→192.168.3.5 完整成功。探针先成功 bind/listen 再建立客户端，没有盲 sleep 等就绪；服务端 poll/recv、客户端读写及外层监督均有界。

C 探针 SHA256=`86d4f4f5fec48a66dfd38278331578e6764bf9edc2592e4315334294ab7b6b65`，CDHash=`d1c56ad1389b71f46a7b0f75f2cfa9be66fdb8f5`（linker ad-hoc）。`/usr/bin/python3` SHA256=`34129c71a01a74f7f3b2443521519b2e5447553fa187f5fcafaaf8c42cc192e2`，签名链 macOS Software Signing→Apple Code Signing Certification Authority→Apple Root CA，codesign verify exit0。两组只替代网络最小端点，不替代 Lingxi 入口，也不证明真实外部手机已测。

## ALF 只读状态与归因强度

identity-before/after.json 保留了每个只读命令的 argv/exit/raw output：

- `socketfilterfw --getglobalstate`：enabled (State=1)，exit0。
- `--getblockall`：disabled，exit0。
- `--getallowsigned`：built-in signed 及 downloaded signed 软件自动允许开启，exit0。
- `--listapps`：确切294787…绝对路径在列表中显示 Allow incoming connections，exit0。
- `--getappblocked '<本轮确切路径>'`：前后都返回 **permitted**，exit0；但上述同文件身份的 LAN 真实运行仍 FAIL。因此该字符串与旧路径记录不足以证明当前有效入站许可；不能声称已读取 ALF 内部 CDHash 绑定或已证明判定被删除。
- en0=192.168.3.5、en1=192.168.3.10；路由原始表保留于两次快照，非回环目标为本机真实 en0 地址。没有改代理、路由、pf 或防火墙。

本轮差分独立复现“应用相关的非回环入站过滤”：无 Lingxi 业务代码的同一 ad-hoc 端点回环绿/LAN红，Apple签名端点同LAN绿；r00实测同症状。这强烈支持 ALF/应用过滤这一环境归因，排除了 Lingxi HTTP 业务独有错误和同地址普遍不可达。**未执行系统策略开关差分，未直接读取内部有效 cdhash 规则，故不把排他因果证明或路径→CDHash判定模型写成已证明**。历史报告的对应强表述仅为历史，本报告不代其签字。

## 已完成准备、剩余条件与下一棒

精确操作与身份门槛在 PREPARED-ALLOW.md；仅针对当前绝对路径和上述 SHA256/CDHash 重新登记并允许入站。命令名称已从本机帮助核实为 `--remove` / `--add` / `--unblockapp`。没有执行这些命令、sudo、全局关闭防火墙、签名替换或其他权限操作。

必要外部条件：用户对经过最终构建身份核验的确切对象完成应用级系统操作，然后全新编号亲跑 r00 验证实际非回环请求通过；permitted 字符串不算通过。当前候选仍在并行修复，最终若重链接，所有对象身份与准备必须重新核对。单套件通过后仍须全新阶段审查亲跑完整 workspace/正式 R05 和前序闭包；D-01不得为整体验收放行。

文件只写 RR3/D-01。未 commit/push/branch/tag；未改总控文档。证据清单在 evidence-manifest.json，包含每个文件的 SHA256/大小；清单不包含自身，报告与准备文档包含于清单。下一棒为全新 D 审查者只读核对当前身份、原始差分/退出、操作准备与边界；系统解除未执行，保持 BLOCKED。
