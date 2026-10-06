# R05 RR1 叶断言偏差登记册（RR1_LEAF_DEVIATIONS）

- 维护纪律：本登记册由实现/修复工作包追加条目，登记**实现与原 R00 叶断言文本之间的有意偏差**及理由；
  F25（叶份额重推导）与 F26（CID 完整性）在重推导时**必须消费本登记册**——对登记在册且理由成立的偏差，
  按登记的语义等价类或显式偏差记录处理，**不得误杀正确实现，也不得静默对齐（悄悄改叶文本或放室断言）**。
- 登记不是放行：每个条目必须给出偏差的权威依据（总控文本）、实现位点、钉住测试与理由；无关偏差不得混入。
- 本登记册**不修改** `docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json` 的任何叶文本。

## 条目 1：LA-CFEC64F68DDE 非 OAuth 拒绝边界 404 → 409

| 字段 | 内容 |
|------|------|
| 登记 F-ID | F31（RR1_ISSUE_MATRIX.json；登记人 WP-T02-R2，2026-10-04；行为实现于 WP-T02-R1 F04） |
| 叶 | `R00-T02-LA-CFEC64F68DDE`（F-D11-…-OAUTH-PROVIDER-CUSTOM--CFEC64） |
| 原 R00 断言文本 | `docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json`："拒绝/边界：非 OAuth provider **404**；不应错误扩大写入或披露范围" |
| 实现行为 | **已知**非 OAuth provider（如 apiKey/authHeader）访问 OAuth 专属面（`/lingxi/v1/models/oauth/{p}/models`、add/remove、login/callback/poll、logout）→ **409 CONFLICT**，reason=`oauth_only_surface`，cause=`models.oauth_only_surface`，body 点名 OAuth-only 规则；**未知 provider 仍 404** |
| 实现位点 | `rust/crates/lingxi-service/src/management.rs`（`credential_surface_error` 的 `CredentialError::NotOAuth` 分支）；错误源 `rust/crates/lingxi-service/src/credentials/login.rs` `not_oauth()` |
| 钉住测试 | `rust/crates/lingxi-service/tests/r05_t02_credentials.rs` `rr1_f04_oauth_model_listing_and_non_oauth_rejection`：aux（apiKey）GET `/lingxi/v1/models/oauth/aux/models` 断言 **409** 且 body 含 "oauth"；ghost（未知 provider）断言 **404**。同类 409 拒绝腿另见 device/logout/non-OAuth-login 等测试 |
| 权威依据 | 总控 `RR1_MASTER_PROMPT_2026-10-04.md` §4 F04 六叶清单将该叶表述为"列出指定 OAuth provider 模型，**非 OAuth 明确拒绝**"——**未规定状态码**；同时要求"错误/过期/重复 state、PKCE、不合法回调……非 OAuth……拒绝"显式化。404 语义上表示"资源不存在"，会把"provider 存在但面不适用"伪装成"provider 不存在"，既掩盖已知 provider 的存在，也与 R05 管理面既有惯例（无效输入显式 409，见 `credential_surface_refused`）冲突 |
| 偏差理由（一句） | 404 会隐藏已知 provider 的存在（信息披露方向反而更差），409+oauth_only_surface 是显式、可诊断的拒绝，满足叶的语义主句"非 OAuth 明确拒绝；不应错误扩大写入或披露范围" |
| F25 处理指令 | 重推导本叶份额时按**语义等价类**断言"非 OAuth provider 的明确拒绝（不空列表、不 200、不扩大披露）"，或在严格状态码断言旁**携带本条目**作为登记偏差；两种处理都合法，**静默改叶文本、静默对齐 404、或因状态码不同判实现失败，均为错误处理** |

## 条目状态

- 2026-10-04 WP-T02-R2 建册并登记条目 1（F31 行为本体在 WP-T02-R1 已实现并被独立审查确认行为符合总控；本登记消除 F25 重推导时的文本冲突）。
- 2026-10-05 **F25 已消费条目 1**（WP-T08-R1）：LA-CFEC64F68DDE 按 full_original_behavior 重新钉住时，其 A2 案例的证据=具名测试
  `rr1_f04::rr1_f04_oauth_model_listing_and_non_oauth_rejection`（该测试同时断言已知非 OAuth 409+oauth_only_surface 与未知 provider 404），
  即采用登记的处理指令前一种合法形态——语义等价类"明确拒绝"由具名测试钉住，且该叶在阶段图（R05.json full_original_behavior 叶的
  evidenceRequired）与本文档双向携带偏差说明。R00 叶文本未被修改，断言未放宽，未按 404 误杀实现。
- 2026-10-06 **状态变更：偏差（就列表而言）已消除**（RR2 WP-E，按 `RR2_MASTER_PROMPT_2026-10-06.md` §四E / F31 执行）：
  GET `/lingxi/v1/models/oauth/{provider}/models` 对**已知非 OAuth provider 恢复为 404**（与未知 provider 同一最小 body：
  reason=`model_provider_unknown`，不披露凭证类型/不泄露 provider 存在性差异，零凭证写入），与原 R00 叶文本逐字对齐。
  钉住测试 `rr1_f04_oauth_model_listing_and_non_oauth_rejection` 同步改为：非 OAuth 列表=404（含零披露/零写入断言）、未知=404（两 body 除
  requestId 外逐字一致）、合法 OAuth 列表=200+去重并集清单。**其他 OAuth 面接口（login/callback/poll/logout/add/remove model）维持
  409+oauth_only_surface 不变**——本条目登记的偏差范围自即日起仅覆盖那些非列表接口；其偏差登记原文上方保留，作为历史记录与后续重推导的
  依据。阶段图 `rust/crates/xtask/src/stage_maps/R05.json` 该叶 evidenceRequired 的偏差说明已最小同步。R00 叶文本仍未被修改。
