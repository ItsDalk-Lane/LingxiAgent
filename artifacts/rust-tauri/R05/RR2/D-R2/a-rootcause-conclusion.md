# F43 (a) r00_management_leaves 非回环访问失败 — 真实根因核对（2026-10-06，D 工作包亲测）

## 复跑（当前环境、当前树）

- 命令：`/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r00_management_leaves -- --nocapture`
- 结果：FAILED（exit 1），51.78s。逐字签名与 RR1 相同：
  `request to 192.168.3.5:<port> stalled during write/read exchange (POST /lingxi/v1/web-auth/login, 0 bytes read so far)`（`rerun-current-binary.log`）。
- cargo 无需重编译（Finished 0.25s），执行的是当前树元数据对应二进制 `rust/target/debug/deps/r00_management_leaves-294787929158efb0`；
  该路径在 ALF 判定表中有历史 Allow，但**仍被拦截** → 判定按 (路径, 代码目录哈希) 生效，该路径的二进制在判定授予后又被重链接过。

## 差分探针（零 Lingxi 代码，`probe/`）

| 探针 | 地址 | 结果 |
|---|---|---|
| clang adhoc 签名二进制（无判定） | 127.0.0.1 | ok（connect+accept+数据交换完成） |
| clang adhoc 签名二进制（无判定） | 192.168.3.5 (en0) | connect+write 成功、**accept 永不触发**、recv 超时 |
| clang adhoc 签名二进制（无判定） | 192.168.3.10 (en1) | 同上 stalled |
| clang adhoc 签名二进制（无判定） | fe80::…%en0（IPv6 链路本地） | 同上 stalled |
| 同一二进制 `codesign --force --sign -` 后 | 192.168.3.5 | 仍 stalled（adhoc 重签无效） |
| Apple 签名 /usr/bin/python3（对照） | 192.168.3.5 | **ok**（同机同地址同刻完成全交换） |

ALF 状态（只读）：enabled；block-all 关闭；auto-allow built-in/downloaded signed 均开（adhoc 无信任链，不适用）。
判定表 63 条，几乎全是历次重链接后的 `r00_management_leaves-<hash>`（用户逐个点过 Allow）。

## 结论（排除法，均有探针记录）

1. **不是 DNS/主机名**：直接用 IP，无解析。
2. **不是地址选择/路由/TUN**：本地子网路由走 en0/en1 直连（utun1024 只吃 default 语义路由）；同一地址 Apple 签名对照二进制全交换成功；卡点在 accept（socket 过滤层），不在路由。
3. **不是 IPv6 可绕**：链路本地 IPv6 同样 stalled（按应用而非按地址族过滤）。
4. **adhoc codesign 无效**：linker 本就 adhoc；显式重签后仍 stalled。
5. **判定不可跨重链接复用**：同路径旧 Allow 对重链接后的二进制不生效。
6. **回环豁免**：loopback 全部通过（与失败测试内 127.0.0.1 段全部成功一致）。

**根因 = macOS 应用防火墙（ALF/socketfilterfw）按二进制实例（路径+cdhash）挂起未授权应用的非回环入站流**；与 R05-ENV-ALF-UNSIGNED-TEST-BINARY 环境项一致，本轮在当前环境完整复证。

## 正当测试基建无解（不削弱断言、不动系统防护的前提下）

- 服务监听 socket 属于测试二进制进程本身，无法绕开 per-app 过滤；
- 已验证：改地址族、adhoc 重签、复用带判定的路径均无效；
- 生产语义（connectionKind=lan 依赖对端非回环地址）必须保留，不接受降级为回环。

**解锁动作（需用户，执行者无 sudo、不擅改系统防护）**：对当前二进制 `rust/target/debug/deps/r00_management_leaves-<当前hash>` 点一次 ALF Allow（或 `sudo socketfilterfw --add <该二进制>`，或临时关防火墙，或 Developer ID 签名）。每次重链接会再次失效——环境项登记继续有效。

## 本工作包对 (a) 的处置

不修改 `r00_management_leaves.rs`（任何修改只会触发重链接并刷新被拦二进制，无法变绿）；状态如实按环境受阻登记。
