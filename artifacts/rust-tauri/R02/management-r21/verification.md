# R02 管理 / 认证 / 安全审计定向验证

- 执行窗口：2026-09-28 09:49:00–09:49:46 UTC；原始时间见 `start-utc.txt`、`end-utc.txt`。
- 环境：本机 macOS；`cargo test --offline`，使用现有锁文件；未启动 TCP 服务。
- 候选：`start-source-sha256.txt` 与 `end-source-sha256.txt` 完全一致；列出本组三个源码、服务入口、Cargo.toml、Cargo.lock。
- `cargo test --offline --manifest-path rust/Cargo.toml -p lingxi-service --lib security_audit::tests -- --nocapture`：exit 0，3/3 PASS；原始输出 `security-audit.log`。
- `cargo test --offline --manifest-path rust/Cargo.toml -p lingxi-service --lib management::tests -- --nocapture`：exit 0，4/4 PASS；原始输出 `management.log`。
- `cargo test --offline --manifest-path rust/Cargo.toml -p lingxi-service --lib auth::tests -- --nocapture`：exit 0，18/18 PASS；原始输出 `auth.log`。
- 这些结果仅覆盖无端口定向单元行为。真 TCP 管理叶、TLS、完整阶段门禁与 Windows 运行仍须另取证；不据此标记 R02 PASS。

此前首次失败与复跑保存在 `../management-r20/`：`lib-build-first.log` 缺测试随机数依赖、`auth-tests-first.log` 旧测试未创建日志目录、`management-preflight-first.log` 旧测试断言日志文件不存在；均未覆盖或改写。后续源码修正和独立复跑另有原始文件。
