# 管理服务定向验证 R19

当前目录仅记录本轮实际运行；完整 R02 阶段门禁和独立验收另行判定。

## 旧 Node 设备凭证哈希独立向量

- 命令：`node -e 'const c=require("node:crypto"); const s="hana_dev_unit_test_secret"; const salt="unit-test-salt"; process.stdout.write(c.scryptSync(s,salt,32).toString("base64url")+"\n")' > artifacts/rust-tauri/R02/management-r19/device-node-vector.log 2>&1`
- 起止时间：2026-09-28 08:47:14 至 08:47:34 UTC（工具调用前后时间，命令本身约 0.001 秒）。
- 退出状态：0。原始输出：`device-node-vector.log`。
- 运行前源码摘要：`auth.rs` 2d33a7c7c10c45ce6760ff44962c1f5ce25233a82ff62afd744c02b536c41d58；`management.rs` 2cffde38e35cb700062e9e0dfd6ec3295e9a01af58dccced586b4ab66f0b8ee9；`lib.rs` 9634cf91cf482cf68eff03a04a397458c859d631c1dc74f25b79d80aa622e62e；`Cargo.toml` cec902952a04a6bda6296a0f406f12d59ccdb286c13a99282f1aa808751f1096；`Cargo.lock` 6a88bfe3cc3102258f5a29b5b20f9709798c26f862fd681d9eff6f7b85e16f98。
- 这只验证旧 Node 的预期值，不等于 Rust 实现已经运行通过。

## Rust 定向检查

所有命令在当前工作区运行，使用 `--offline`、本机合成测试根和当前 Rust 锁文件。下列起止为调用前后的 UTC 采样，涵盖命令时间；原始终端输出分别保存在同名日志。检查间有源码继续修改，不能把早期通过记录直接绑定到最终候选。

| 检查 | 起止 UTC | 退出 | 原始日志及结果 |
| --- | --- | --- | --- |
| `cargo check --manifest-path rust/Cargo.toml --offline -p lingxi-service` | 08:49:46–08:50:07 | 0 | `cargo-check.log`；编译通过，管理网络读取当时有未接线警告 |
| `cargo test --manifest-path rust/Cargo.toml --offline -p lingxi-service --lib auth::tests` | 08:50 后–08:50:53 | 0 | `auth-lib.log`；16/16，通过时尚无标准审计链 |
| `cargo test --manifest-path rust/Cargo.toml --offline -p lingxi-service --lib management::tests` | 08:50:53 后–08:51:16 | 0 | `management-lib.log`；3/3，通过时尚无标准审计链 |
| `cargo test --manifest-path rust/Cargo.toml --offline -p lingxi-service --lib static_web::tests` | 08:51:16 后–08:51:43 | 0 | `static-web-lib.log`；2/2 |
| `cargo check --manifest-path rust/Cargo.toml --offline -p lingxi-service` | 09:02 前–09:02:18 | 0 | `audit-cargo-check-first.log`；已包含标准审计生产代码 |
| `cargo test --manifest-path rust/Cargo.toml --offline -p lingxi-service --lib auth::tests` | 09:03:44–09:04:10 | 0 | `audit-auth-lib-first.log`；17/17，含标准日志故障后重启补写、去重、无明文 secret |
| `cargo test --manifest-path rust/Cargo.toml --offline -p lingxi-service --lib management::tests` | 09:04:10 后–09:04:47 | 0 | `audit-management-lib-first.log`；3/3，含密码迁移审计日志无明文密码 |

09:03:44 运行前关键源码 SHA-256：`auth.rs` 0a4a480e328ce811ef49e7636ec2639ad395675ac5a966a546207b494f59b237；`management.rs` 5a3caf3e78854a69da7bbfb2f48bc86c3c76d2835f0e9b9a3fd9d754a2b10dbe；`security_audit.rs` e52fa8016a13a26acd617051fbd26faf218d58702d48331fe0995e6816b2fbca；`lib.rs` 2db7c3078b9f0737317fb48af9025cf4fbda44a9af67b4e3e571777447d276c3；`Cargo.toml` cec902952a04a6bda6296a0f406f12d59ccdb286c13a99282f1aa808751f1096；`Cargo.lock` af34845f58995d0267daf8bcfadeb6c6c3349bbeea2d7f138553b23c87251e9f。

先前 `management-r18/management-lib-first.log` 的 scrypt 离线依赖失败和主代理 `tls-r17/tls-test-first.log` 的审计摘要编译失败都原样保留；本轮通过没有覆盖或改写它们。真实 HTTP、HTTPS、34 叶、完整门禁及独立验收不在本表标为通过。

## 后续同根因修复与独立原件

- `fresh-management-lib-first.log`：fresh reload 改动后旧单测夹具没有先写初始文件，退出 101，2 个测试失败。这是测试夹具问题，不等于产品读取应退回旧内存；修夹具后 `fresh-management-lib-second.log`、`fresh-management-lib-third.log` 各退出 0，3/3 通过。第三轮覆盖外部撤销、坏库拒旧身份和失败不覆盖。
- `auth-final-candidate.log`：09:19:22–09:19:45 UTC 采样，退出 0，18/18；输入包含真正非法的 `deviceKind=invalid-kind`，核设备、凭证和安全审计均不新增。`unknown` 在旧 Node 枚举中合法，不能用作非法输入。
- `management-final-candidate.log`：09:19:45 后–09:20:10 UTC 采样，退出 0，3/3。
- `audit-id-final.log`：审计 eventId 改为稳定来源/序号/动作/目标/时间后，单项故障恢复、无重复与无 secret 检查退出 0，1/1。
- `management-bounded-file.log`：管理配置统一限正常文件、16 MiB、拒符号链接后，退出 0，3/3。
- 本目录中 `audit-auth-lib-first.log` 与 `audit-management-lib-first.log` 对应更早代码，不能替代上述新增修复后的结果；每轮源文件摘要以工具记录和下面后续快照为准。

09:19:22 UTC 的运行前 SHA-256：`auth.rs` 9d2d403e7946ce240d38b4c39188cbeee26fc279a31c5a27655fb0090fa3ad2b；`management.rs` 03d9060f0b9ede73666cb780369dc204cfb7010faa56630327d4d469496fd2bf；`security_audit.rs` bea4eca45ee6e1b5bf9a761a4cd58cd4fb5b4bdb20e3e12f063988849a704caa；`lib.rs` 2db7c3078b9f0737317fb48af9025cf4fbda44a9af67b4e3e571777447d276c3；`Cargo.toml` cec902952a04a6bda6296a0f406f12d59ccdb286c13a99282f1aa808751f1096；`Cargo.lock` af34845f58995d0267daf8bcfadeb6c6c3349bbeea2d7f138553b23c87251e9f。其后管理文件读取边界又做了一次源码修改并运行 `management-bounded-file.log`；最终候选摘要必须另行冻结。

安全审计限制：`device-credentials.json.audit` 与 `management.json.audit` 是持久意图；`${home}/logs/security-audit.jsonl` 是旧服务的标准审计证据。日志写故障后产品操作可能已提交，代码发出 `audit_pending=true` 安全错误并在重启补投影；这种故障分支未达到“同步写标准日志”的严格解释，不应宣称该原断言在所有失败条件下 PASS。旧 Node 设备注册簿和账号文件的实际数据导入尚未实现，合成新库测试不证明真实迁移。
