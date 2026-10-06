# R05 RR2 WP-E（F31）独立验收 REVIEW-r1

日期：2026-10-04（审查者本地时钟）｜验收者：全新上下文独立审查智能体（未参与实施）
对象：工作树未提交改动（HEAD ad5ec4e98，分支 codex/rust-tauri-migration）中 WP-E 声称的 F31 修复
权威：RR2_MASTER_PROMPT_2026-10-06.md §四E；R00 叶 R00-T02-LA-CFEC64F68DDE 原文（只读）
cargo：/Users/study_superior/.cargo/bin/cargo

## Verdict：PASS（F31 关闭，无 mustFix）

## 1. 行为面亲跑

| 命令 | 退出码 | 结果 |
|---|---|---|
| cargo test --manifest-path <repo>/rust/Cargo.toml --locked -p lingxi-service --test r05_t02_credentials | 0 | 38 passed / 0 failed |
| 同上 `-- --exact rr1_f04::rr1_f04_oauth_model_listing_and_non_oauth_rejection` | 0 | 1 passed / 0 failed |

断言内容核验（读测试源 + 绿跑通过）：
- 已知非 OAuth（aux, apiKey）GET `/lingxi/v1/models/oauth/aux/models` → 404，且 body 小写不含
  "apikey"/"authheader"、不含秘密 `sk-rr1-f04-aux`；
- 未知（ghost）→ 404，两 body 剥离 `details.requestId` 后 `assert_eq!` 逐字相等（无存在性 oracle）；
- 被拒前后 `{runtime_dir}/credentials.json` 字节 `assert_eq!`（零写入）；
- 合法 OAuth（main）列表 200 + 去重并集 {stub-model, custom-list-model}（len=2）保留。

## 2. 范围核验（读 git diff）

- management.rs 共 3 hunks：仅 `list_oauth_models` 的 match 臂把 `NotOAuth` 并入 `NotConfigured`
  的最小 404（reason=model_provider_unknown, cause=models.provider_unknown）；`credential_surface_error`
  的 `NotOAuth→409` 分支代码逐字未动（仅注释更新）；无其他接口改码（非注释代码行 diff 仅此一处）。
- r05_t02_credentials.rs 共 6 hunks，全部位于具名测试（fn 区间 3751–3898）内或其 doc 注释；
  套内其余 13 处 `status, 409` 断言（login/callback/state/device/denial/revoke/add/remove 的
  non-OAuth 拒绝腿）逐字未动，未削弱。
- login/callback/poll/logout/add/remove 面仍走 `credential_surface_error`（路由表 613/617 行核实：
  POST add、DELETE remove、logout 等未改映射）。

## 3. 对齐核验

- `git status --short docs/rust-tauri/R00/` 与 `git diff --stat docs/rust-tauri/R00/` 均为空：R00 叶文本
  零改动；叶原文断言"非 OAuth provider 404；不应错误扩大写入或披露范围"与现实现一致。
- RR1_LEAF_DEVIATIONS.md 条目 1 追加的 2026-10-06 状态变更（偏差就列表而言消除、历史登记原文保留、
  其他 OAuth 面仍 409）与实际 diff/行为逐点相符——如实。
- stage_maps/R05.json 仅 1 hunk：该叶 evidenceRequired 更新为"历史偏差 + 2026-10-06 RR2 恢复 404，
  其他 OAuth 面维持 409"，与事实一致。
- F25 侧：R05_SCOPE_MATRIX.json（该叶 r05_share 无 409 语义）与 r05_leaf_case_map.tsv
  （a1/a2 行→具名测试名，名字未变）均零改动且确无需改。

## 4. 隔离变异（/tmp/e-r2-review-mut，主树零接触）

- 副本基线（未变异）：具名测试绿，exit 0（baseline-green.log）。
- 变异：把列表 404 改回 409（将 `NotOAuth` 移出 404 臂 → 落回 `credential_surface_error` 409 分支，
  精确复现 RR1 行为）→ 具名测试红，exit 101，panic 于断言腿 `left: 409 / right: 404`
  （red.log）。
- 还原：cp 主树原 management.rs 覆盖副本 → 同测试绿，exit 0（green-restored.log）。
- 审查者日志：/tmp/e-r2-review-exact.log、/tmp/e-r2-review-mut/{baseline-green,red,green-restored}.log。

## 5. r00_management_leaves 断言兼容性（只读）

- 该文件 grep 无 oauth/models/oauth 引用；其 404 断言（1289/1404 行）针对 missing registry，
  与本改动无交集。恢复 404 后与该文件期望无冲突（实施者 before/after 两跑失败集合一致的环境性
  stall 属 D 的 F43 面，非本 WP 引入；本审查未重跑该重型套件，以读断言核验为限）。

## 判定要点核对

- 范围越界改码：无（仅列表面 + 登记文件；stage_map.rs 属 WP-C，非 E）。
- 存在性 oracle 引入：无（两 404 body 除 requestId 外逐字一致；body 不含 provider 名/凭证类型/秘密）。
- 叶文本被改：无（R00 零 diff）。
- 登记册未同步：无（登记册条目 1、stage_map R05.json、矩阵 F31 行三方同步且如实）。

结论：F31 验收通过。无需修复项。
