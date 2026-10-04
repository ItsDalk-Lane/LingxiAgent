# ALF 环境项决定性取证（2026-10-02，R05-T01）

## 结论
r00_management_leaves 的失败不是代码回归：macOS Application Firewall（ALF）对无有效签名
（adhoc）的测试二进制按 signing identifier 逐个要求入站许可；无 verdict 的二进制，ALF 在
内核层完成 TCP 握手但从不把连接交付给 accept()。客户端观测：connect 成功 → write 成功
（内核缓冲）→ 读 0 字节直到测试的 20 秒有界 stall panic。这与应用层代码无关。

## 决定性实验：裸探针（零 Lingxi 代码）
zz_probe2.rs（见同目录源码）：std::net 裸 TcpListener 绑 0.0.0.0，先 loopback 自连（对照），
再 LAN 自连（192.168.3.5）。

运行 1（16:27:12 local 前后）：
```
probe pid=22114 listening 0.0.0.0:55922
LOOPBACK OK from 127.0.0.1:55923: [112, 105, 110, 103]
LAN CONNECT OK (kernel-level handshake completed)
LAN WRITE OK (bytes accepted by kernel)
LAN STALL: connect+write completed at kernel level but the connection never reached accept() in 10s
probe exit: 2
```
运行 2（16:31 前后）：pid=22224 port=56524，同样 LOOPBACK OK / LAN CONNECT OK / LAN WRITE OK / LAN STALL。

ALF 日志同时点（log show --predicate 'process == "socketfilterfw"'）：
```
2026-10-02 16:27:25 socketfilterfw: Handle flow with processPID: 22114 and bundleid: zz_probe2
2026-10-02 16:27:25 socketfilterfw: Enqueuing new inbound flow for zz_probe2 with identifier: zz_probe2
```
→ 探针二进制的入站流被 ALF 排队后无人应答。探针不含任何 Lingxi 代码，应用层被排除。

## 失败测试二进制的 ALF 日志对照
```
16:06:30/16:06:37  r00_management_leaves-da34472139e195c5 (pid 16998)  Enqueuing new inbound flow   ← verify-R04-rerun rust_test_workspace
16:14:29/16:14:36  r00_management_leaves-da34472139e195c5 (pid 17385)  Enqueuing new inbound flow   ← verify-R04-rerun 同上套件内重试/后续
16:17:42/16:17:49  r00_management_leaves-8c4c14e6013d6767 (pid 18747)  Enqueuing new inbound flow   ← verify-R04-rerun 内嵌 R03 workspace test
16:18:42/16:18:46  r00_management_leaves-8c4c14e6013d6767 (pid 19423)  Enqueuing new inbound flow
```
历史对照（前序会话取证，r00-firewall-alf-evidence.log）：基线 worktree 的二进制
r00_management_leaves-812df2ee793b5dda 命中历史 Allow 规则
（"Found known filter app by signing identifier … return known verdict: 1"）→ 同一测试在
基线树上 PASS（r00_management_leaves_baseline_head_pass.log，33.12s）。

## 为什么「基线 PASS / 我的树 FAIL」不是代码回归
测试二进制文件名含 cargo metadata hash。我的树改动了 lingxi-service lib → 测试二进制重链 →
文件名 hash 变化（812df2ee… → 8c4c14e6…/da344721…）→ ALF 视为全新未签名 app 要求新 verdict。
ENTRY-01/02 跑的是未改动的基线树 → 复用了已被用户 Allow 的旧 hash 二进制。

## 为什么应用层被排除（无需改代码的证据）
1. 失败测试中全部 loopback 请求成功——包括同一 web-auth/login handler 在 loopback 阶段的
   多次成功调用，以及 LAN 模式服务器（0.0.0.0 绑定）上 loopback 地址的凭证签发请求
   （测试 2327-2335 行）紧邻失败的 LAN 请求之前成功返回 200。handler/锁/初始化挂起
   对 loopback 与 LAN 必然同等生效；实际只有 LAN 地址 stall。
2. 我的 lingxi-service diff 对 bind/accept/TcpListener/NetworkMode 路径零改动
   （git diff grep 验证，见下）。
3. 裸探针在两棵树上同样 stall（零 Lingxi 代码）。

## 对总控三条排除项的回应
(a) python 裸 socket 自连成功：python3 是系统签名/已被允许的进程，有 ALF verdict；adhoc 测试
    二进制没有。(b) TCP connect 成功：ALF 在内核层完成握手但扣留交付，connect 成功正是 ALF
    拦截下的预期现象。(c) 测试文件未改动：二进制因 lib 变化重链，signing identifier 改变。

## 环境状态
```
Firewall is enabled. (State = 1)
Firewall has block all state set to disabled.
Automatically allow built-in signed software ENABLED.
Automatically allow downloaded signed software ENABLED.
```

## 无法自解的解除途径（需用户/管理员）
1. ALF 弹窗出现时对该测试二进制点 Allow（每次重链后需重新允许）；本会话 kimi-cu 服务不可用、
   osascript 无 AX 授权，均无法代为点击。
2. sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add <binary>（并按需 --unblockapp）。
3. 跑门禁期间临时关闭 macOS 防火墙（系统设置 → 网络 → 防火墙）。
4. 用 Developer ID 证书签名测试二进制（自动允许覆盖有效签名 app）。

## R04 先例
docs/rust-tauri/R04/R04-T05_REPORT.md、R04-T07_REPORT.md 已登记同类环境项：
「代码变化 → 测试二进制 hash 变化 → 防火墙视为新未签名 app」。
