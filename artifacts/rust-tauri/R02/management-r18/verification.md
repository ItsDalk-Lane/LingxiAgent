# R18 管理服务定向检查原始记录

本目录保留每次命令原始输出。以下是历史候选观察；源码在第二次单测后继续修改，不能据此称当前候选通过或 R02 阶段通过。全部时间为 UTC。

| 命令 | 起止 | 退出 | 原始输出 | 结论 |
|---|---|---:|---|---|
| `cargo test -p lingxi-service --offline --lib auth::tests -- --nocapture` | 2026-09-28 07:53:41–07:54:39 | 101 | `auth-lib-first.log` | 我新增管理测试使用 `.unwrap()`，而 `EndpointError` 未实现 Debug，编译失败；原件保留。 |
| 同一命令 | 2026-09-28 07:57:13–07:58:20 | 0 | `auth-lib-second.log` | 12/12 身份与配对单测通过；此后源码仍有新增修改，须复验。 |
| `cargo test -p lingxi-service --offline --lib management::tests -- --nocapture` | 2026-09-28 07:58:34–07:59:02 | 101 | `management-lib-first.log` | 父任务刚添加的 `scrypt` 依赖尚未缓存，离线解析失败；不是源码测试通过。 |

第二次 `auth::tests` 开始前的 SHA-256 摘要：

```text
45bce455198b5200a6d8e5c283557006aa593742ab3223420643ddf054b2d085  rust/crates/lingxi-service/src/auth.rs
5b92a1c7817f648cacc35972fe4184d1f8f2d3989b3bb058e75e3c8383c4d249  rust/crates/lingxi-service/src/management.rs
8923e757c09d17fdf3e826751d0447a4fd17c618d69fc0bdc8f0ca0ff5beb8a8  rust/crates/lingxi-service/src/static_web.rs
2f26c618e3f5a42f41cb639a4e9f231b2ba00c0c9194213cbdcbdc3506c9f8ca  rust/crates/lingxi-service/src/paths.rs
9634cf91cf482cf68eff03a04a397458c859d631c1dc74f25b79d80aa622e62e  rust/crates/lingxi-service/src/lib.rs
b9bf73c87f61c54d89fc746ccdcb2163f3edfe03b580e66b5fe1d8a5cc7eccef  rust/crates/lingxi-service/src/serve.rs
f786b79624ece09582cae42ad9c0cc3a79fbe6496d7dbbc57b73e9171e0b8708  rust/crates/lingxi-service/tests/auth_matrix.rs
c5d7e631609267562ebe333c1ecaa9b1b8c51e65a528dc7ede8e473b3615e0e7  rust/crates/lingxi-service/Cargo.toml
6a88bfe3cc3102258f5a29b5b20f9709798c26f862fd681d9eff6f7b85e16f98  rust/Cargo.lock
```
