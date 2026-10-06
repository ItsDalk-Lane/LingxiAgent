# R05 RR1 WP-T02 执行者报告（R1）— F03/F04/F05

- 执行者：R05-T02-执行者（首任，全新上下文，2026-10-04）。
- 基线：HEAD `d80737b6cb9186c8a18c0f35923aac00249d45c3`（未前进），工作树含 WP-T01 未提交候选（F01/F02/F29，已按矩阵核对未被本包回退）。
- 工具链：`/Users/study_superior/.cargo/bin/cargo`（rustc/cargo 1.98.1，仓库根 rust-toolchain.toml 锁定）。
- 边界遵守：未 commit/push/tag/PR；隔离数据目录 + 受控 loopback OAuth/HTTP 替身 + 合成凭证（dummy-*/MARKER-*）；未读取真实 home/密钥；冻结 HEAD 旧红在 `/tmp` git archive 隔离副本执行（已删除前先取日志，副本不再保留）。

## 1. 旧行为反例（先红，后修）

永久测试迁入 `rust/crates/lingxi-service/tests/r05_t02_credentials.rs`（mod `rr1_t02` + `rr1_f04`），断言与证据包 CN-F02/CN-F08 同强度；允许的接口适配仅两处并在下文登记。

| 反例 | 冻结 HEAD d80737b6 | 修复前候选树 | 修复后 |
|---|---|---|---|
| F03 旧 handle 换 key 后拿到新钥匙（CN-F02-1） | RED `Ok(Bearer("dummy-new-key"))` | RED 同 | GREEN |
| F03 revoke→删除→重建后 handle 复活（CN-F02-2，经真实 /models/reload 面） | RED `Ok(Bearer("dummy-rr1-old"))` | RED 同 | GREEN |
| F03 初始无 OAuth 热新增 OAuth 后 NotLoggedIn（store 未初始化） | RED `NotLoggedIn` | RED 同 | GREEN |
| F05 key 跨 512 截断边界泄漏前缀（CN-F08-1） | RED `secret-c` 残留 | RED 同 | GREEN（offset 0..600 全扫） |
| F05 percent/JSON 编码回显存活 | RED | RED 同 | GREEN |
| F05 未知工具名回显完整 key 进 kernel ProtocolError（CN-F08-2） | RED | RED 同 | GREEN |
| F04 六叶管理面后端入口（LA-*） | RED（login 面 404，六叶各断言点名） | 不适用（新面，实现即本包） | GREEN |
| F05 全链 home 零 marker（终态钉） | 绿（该链仅 error code 持久化） | 绿 | GREEN —— **无旧红主张，如实登记** |
| F03 handle 绑 principal | 不可编译（旧 API 无 principal） | 不可编译 | GREEN —— **契约缺口闭合腿，无旧红主张** |

红日志（exit 101）：`old-red-pre-fix-candidate.log`、`old-red-frozen-head-d80737b6.log`（7 红 + 1 对照）、`old-red-f04-frozen-head-d80737b6.log`（并入 frozen-head 主日志前单独留存）。

冻结副本的接口适配（断言原文未动）：`svc.reload(&next, 2)` → `svc.reload(&next)`（F02 后接口演进的反向适配）；principal 绑定腿按上述排除。F04 六叶红探针（`rr1_f04_frozen_red`）只进冻结副本，主树用完整 11 测试。

## 2. 修复摘要

- **F03**：`CredentialServiceInner.next_instance` 单调实例计数；`ProviderCell.instance` 从不跨生命周期复用；handle 注册表条目 (principal, provider, instance, generation, expiry)；`resolve_handle` 校验主体/实例/代次/过期；`install_tokens` 升级 (instance, generation) 双栅（刷新与登录共用）；`bootstrap` 无条件构造 store（文件懒创建）；revoke/logout 取消 pending 登录。
- **F04**：`credentials/login.rs`（新）一次性登录事务 + oauth_start/complete_code/poll_device/logout/oauth_models/add/remove；`oauth.rs` 抽出 `request_device_authorization`/`poll_device_token`（`run_device_code_flow` 组合原语义，既有 19 测试原样绿）+ `CallbackGrant::new`/`pending_transaction_parts`；`store.rs` tokens 改 Option + customModels 注册表（camelCase、v1 兼容、未知字段保留）；status 增 loggedIn/availableModels；management 7 条新路由（local-only + 真实认证），非 OAuth 显式 `CredentialError::NotOAuth`→409。
- **F05**：`sanitize_diagnostic`（先 scrub 后有界截断）；`scrub_materials` 增加确定性编码变体（percent 大/小写、unreserved 式、`\uXXXX`、serde 转义）；`scrubbed_excerpt` 改用新规则；`provider.rs` 宿主边界对全部 execute_route 结果（首试 + 401 重试，重试材料=新旧并集）的 Failed 消息与 usage invalid detail 先脱敏后 4096 字符截断；正常正文/流式 delta 不动。

## 3. 自检（本会话真实执行）

| 命令（均经 /Users/study_superior/.cargo/bin/cargo，1.98.1） | 结果 |
|---|---|
| `test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t02_credentials` | 36/36 绿（16 既有 + 20 新增），exit 0 |
| `test ... -p lingxi-service -p lingxi-adapters`（最终全量，`final-both-crates-full.log`） | 1079 passed / 0 failed，exit 0 |
| `fmt --manifest-path rust/Cargo.toml --all -- --check` | exit 0 |
| `clippy ... -p lingxi-adapters -p lingxi-service --all-targets -- -D warnings` | exit 0 |
| `run ... -p xtask -- check-contracts` | exit 0 |
| `run ... -p xtask -- check-boundaries` | exit 0 |

如实登记：一次中间全量跑（聚合管道只留汇总）报 `861 passed / 1 failed`，失败测试名未被该管道保留、无法指认；随后对同一命令的两次完整复跑（其一存证为 `final-both-crates-full.log`）均 1079 passed / 0 failed / exit 0。该单次瞬态失败未复现，按环境偶发登记（与 WP-T01 台账登记过的 workspace 偶发同类），不冒称其为零——独立审查者可用存证日志复跑核验。

## 4. 文件所有权与共享文件登记

- 独占（本包）：`lingxi-service/src/credentials/{mod.rs,login.rs,store.rs}`、`lingxi-adapters/src/models/{oauth.rs,credentials.rs}`、`tests/r05_t02_credentials.rs`。
- 共享改动（矩阵已登记回归范围）：
  - `management.rs`（与 T01 共享）：仅追加 F04 路由与 `credential_surface_error` 映射；未动 T01 的 reload 代次接线。
  - `models/dispatch.rs`（与 T05 共享）：仅 `scrubbed_excerpt` 次序修复 + 常量；错误 body 预算/大小约束（F16）未动，归 WP-T05。
  - `models/provider.rs`（与 T01/T03 共享）：仅结果脱敏包装 + `NotOAuth` 映射；能力检查与重试逻辑未动。
  - `models/openai_completions.rs`：未改（F05 修复在宿主边界统一处理，避免逐族补丁）。
- 台账：`RR1_ISSUE_MATRIX.json` F03/F04/F05 → SELF_CHECKED；`RR1_PROGRESS.md` 第 2 行更新。

## 5. 剩余与移交

- F03/F04/F05 待新独立审查者实测验收（INDEPENDENT_PASS 后方可 CLOSED）。
- F05×F22 联合：usage invalid detail 的宿主边界脱敏已落（F05 半边）；严格数值解析/字段路径诊断归 WP-T07。
- F04×F25 联合：六叶逐叶断言已在本套件；stage gate 的六叶误放行修复归 WP-T08。
- 环境限制如实登记：本机 macOS arm64（审查基线平台 Linux x86_64，Rust 版本一致 1.98.1）；未做真实付费供应商/真实 OAuth（按边界禁止，也不属离线义务）。
