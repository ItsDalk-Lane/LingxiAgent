# R05 RR2 WP-E（F31）实施证据——OAuth 列表面非 OAuth 恢复 404

日期：2026-10-06｜执行：WP-E 实施智能体（全新上下文）｜权威：RR2_MASTER_PROMPT_2026-10-06.md §四E、RR2_BRIEF.md
候选：ad5ec4e98 分支 codex/rust-tauri-migration（工作树含 A/B/C/D 未提交改动，本 WP 未触碰）
cargo：一律 /Users/study_superior/.cargo/bin/cargo（rustup 1.98.1）

## 改动（本 WP 独占/登记文件）

1. rust/crates/lingxi-service/src/management.rs
   - `list_oauth_models`：新增列表路由专属 match 臂——`CredentialError::NotOAuth` 与 `NotConfigured`
     合并映射为与未知 provider **完全相同的最小 404**（reason=`model_provider_unknown`、
     cause=`models.provider_unknown`；刻意不格式化 `err`，避免泄露 auth kind 与"已知 provider 存在"）。
   - `credential_surface_error` 的 NotOAuth 分支：行为零改动（仍 409+oauth_only_surface），仅更新注释
     说明列表路由不再走该映射（RR2 F31 carve-out）。其余 OAuth 面（login/callback/poll/logout/add/remove）
     语义逐字未动。
2. rust/crates/lingxi-service/tests/r05_t02_credentials.rs（仅 `rr1_f04_oauth_model_listing_and_non_oauth_rejection`）
   - 非 OAuth 列表：409→404；新增断言 body 不含 apiKey/authHeader 字样、不含秘密 `sk-rr1-f04-aux`；
   - 未知 provider：404；新增断言两 body 除 per-request requestId 外逐字一致（无存在性 oracle）；
   - 新增零写入断言：被拒前后 `{runtime_dir}/credentials.json` 字节不变；
   - 合法 OAuth 列表=200+去重并集清单断言保留；套内其余 409 面断言（device/logout/add/remove/non-OAuth-login
     等）逐字未动。
3. docs/rust-tauri/R05/repair-current/RR1_LEAF_DEVIATIONS.md：条目 1 追加 2026-10-06 状态变更
   （偏差就列表面消除；历史登记原文保留；其他 OAuth 面仍按登记维持 409）。
4. rust/crates/xtask/src/stage_maps/R05.json：R00-T02-LA-CFEC64F68DDE 的 evidenceRequired 偏差说明
   最小同步为"历史偏差+2026-10-06 RR2 恢复 404"（F25 消费点；JSON 复验有效）。
   - F25 侧核对结论：R05_SCOPE_MATRIX.json 该叶 r05_share 不含 409 语义（无需改）；
     r05_leaf_case_map.tsv a1/a2 行=案例→具名测试名映射，测试名未变（无需改）。
5. docs/rust-tauri/R05/repair-current/RR2_ISSUE_MATRIX.json（F31→IMPLEMENTED）、RR2_PROGRESS.md（状态表+轮次日志）。

## 未改（所有权核验）

- rust/crates/lingxi-service/tests/r00_management_leaves.rs（D 所有）：未编辑；grep 无 oauth/models/oauth 引用，
  本 WP 改动不影响其期望（原叶期望 404，恢复后一致）。
- R00 叶文本（docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json）：只读，零改动。
- credentials/login.rs `not_oauth()`：零改动（错误源保持，映射差异只在 management 路由层）。
- A/B/C/D 的未提交改动文件：零触碰（git status 前后集合一致，仅新增本 WP 文件的 M/?? 状态）。

## 自检命令与退出码（真实执行）

| 命令 | 退出码 | 结果 |
|---|---|---|
| cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t02_credentials | 0 | 38 passed / 0 failed（green-r05t02-credentials-full.log） |
| （改动前基线）同上 --test r00_management_leaves | 101 | 1 环境性失败：stall @ POST /lingxi/v1/web-auth/login（192.168.3.5 非环回自地址，macOS 防火墙/TUN 拦截迹象）＝D 的 F43 环境面（baseline-r00-management-leaves-before-e-changes.log；首跑经 tail 管道 shell 报 0，cargo 实际 101，日志内 test result FAILED 为证） |
| （改动后）同上 --test r00_management_leaves | 101 | 失败集合与基线完全一致（同一测试、同一断言点、同一环境成因），无新增红项（after-r00-management-leaves-post-e-changes.log） |
| cargo fmt --check（rust/ 全 workspace） | 0 | 干净（首次 run 报 1 处 arm 折叠差异→已按 rustfmt 建议改写→复跑 0） |
| cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-service --tests -- -D warnings | 0 | 零警告（clippy-lingxi-service-tests.log） |
| cargo test --manifest-path rust/Cargo.toml --locked -p xtask（stage_map 同步后复验） | 0 | 117 passed（green-xtask-tests-post-stagemap-sync.log；含 C 已改的 stage_map.rs 镜像测试，全绿） |

## 针对性断言（自检 §2）

合法 OAuth 清单 200（登出态 loggedIn=false/0 模型 + 登录态去重并集 {stub-model, custom-list-model}）；
已知非 OAuth（aux, apiKey）列表 404；未知（ghost）404；被拒请求零凭证写入（store 字节不变）+
零秘密披露（body 无 apiKey/authHeader/秘密字样，且与未知 provider 404 body 同形）。

## 变异自证（/tmp/r05-rr2-e-mut 隔离副本，主树零接触）

- 变异：把列表 404 改回 409（将 NotOAuth 移出 404 match 臂 → 落回 credential_surface_error 的 409 分支，
  即精确复现 RR1 行为）。
- 结果：--exact 跑 `rr1_f04::rr1_f04_oauth_model_listing_and_non_oauth_rejection` 红，exit 101，
  panic 于新断言腿 `left: 409 / right: 404`（red.log；mutated 409 body 顺带展示了旧实现披露
  `uses auth kind "apiKey"` —— 正是最小 404 所避免的）。
- 还原：cp 主树原 management.rs 覆盖隔离副本 → 同测试绿，exit 0（green-restored.log）。
- 注：变异首版脚本曾造成括号不平衡（编译错），按纪律弃用该结果、从未把它计为红证据，重做后的红是
  行为红非编译红。

## 证据文件清单（本目录）

- baseline-r00-management-leaves-before-e-changes.log
- after-r00-management-leaves-post-e-changes.log
- green-r05t02-credentials-full.log
- clippy-lingxi-service-tests.log
- green-xtask-tests-post-stagemap-sync.log
- red.log / green-restored.log（隔离变异红/还原绿）
- REPORT.md（本文件）
