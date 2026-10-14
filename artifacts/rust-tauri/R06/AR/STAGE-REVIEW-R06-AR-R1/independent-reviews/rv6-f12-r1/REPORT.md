# R06 收口独立复审（RV6）——R06-AR-F12 修复后终态

- Reviewer：RV6（全新独立实例，不复用执行者或 RV5 结论）
- 复审对象：基线提交 `7302a03cd`（fix: RV5-F12-01 preserve main error when fail_build cleanup is rejected）
- 工作树：`git worktree add /tmp/lingxi-rv6 7302a03cd`；cargo 1.98 `--locked`，CARGO_TARGET_DIR=/tmp/rv6-target，合成环境 /tmp/rv6-env
- **总判定：F12 仍为 FAIL（RV6-F12-01：修复不完整——RV5 同族清单 7 处中 2 处残留 `?` 替换语义）；基线红已消除、已改 5 处正确、其余 RV5 结论复核全部成立。**

## 1. 修复核验（源码级，逐调用点）

### 1.1 已修复的 5 处（正确）
`rust/crates/lingxi-service/src/knowledge.rs`（新行号）：`:2599-2605`（provider 失败路径）、`:2613-2619`（validate_embedding_batch）、`:2631-2637`（identity 漂移，主错误重构为 `main`）、`:2703-2711`（complete_build_plan）、`:2716-2724`（complete_job）。五处均为「保留主错误码（`error.code.clone()`）+ 追加 `cleanup fail_build rejected (cache status unchanged): {cleanup}`」，与 process_job 尾部（`:2338-2342`）append 哲学一致。全 rust 树 `fail_build` 仅此定义（lingxi-adapters storage/knowledge.rs:3275）+ 8 个调用点（service knowledge.rs）+ 1 个测试断言，TS 侧无对应物；除下述 2 处外无其他遗漏。

### 1.2 发现 RV6-F12-01（FAIL）：同族 2 处未修，`?` 替换主错误缺陷仍在
RV5 修复项 (1) 明确要求：`embed_planned_job` 错误路径中「主错误已有、fail_build 仅清理」的分支（旧 :2599 及同族 :2607/:2616/**:2657/:2670**/:2682/:2687）不得用 `?` 替换主错误。本次修复只改了 5 处，以下 2 处残留（新行号，即 RV5 清单中的旧 :2657/:2670，父提交行号已逐一比对确认映射）：

```rust
// :2677-2680 —— upsert_batch 失败路径：主错误 error 已有，fail_build 仅清理
if let Err(error) = self.vectors.upsert_batch(&current.variant_id, &entries) {
    self.vectors.fail_build(&current.variant_id)?;   // ← 清理被拒时替换 upsert 主错误
    return Err(error);
}
// :2690-2693 —— accounting_error 延迟主错误路径
if let Some(error) = accounting_error {
    self.vectors.fail_build(&current.variant_id)?;   // ← 同上，accounting 主因丢失
    return Err(error);
}
```

- 缺陷类与 RV5-F12-01 完全相同（错误可追溯性/分类组合；非数据保护——字节/状态在两条路径同样实测保留）。
- 清理拒写场景真实可达：`fail_build` 内部经 `cache_write(VECTOR_SCHEMA_VERSION)`（storage/knowledge.rs:3277），future-schema 库上必被 `KNOWLEDGE_SCHEMA_NEWER` 拒绝。
- **冻结套件无法暴露该残留**：`vector_ingestion_future_write_rejection_retains_paid_batch_and_error_code`（provider_error=false 分支）恰好流经 :2678，但该场景下 upsert 主错误与清理错误**同码**（均为 KNOWLEDGE_SCHEMA_NEWER），断言只查该码存在，替换不可观测；:2691（accounting_error）在冻结套件中无任何覆盖。因此测试全绿不构成对残留的反证。
- 修复提交无正文、无任何「只改五处」的范围说明；artifacts 下无 F12 修复证据目录（仅 f15-repair）。且修复本身覆盖了非 provider 路径（complete_build_plan/complete_job 两处），证明执行者按类别规则修，而非按 provider 场景修——在 7 处中停在第 5 处，判定为遗漏而非有意收窄。
- 提交自身宣称的不变量（"preserve main error when fail_build cleanup is rejected (append, not replace)"）在上述 2 处不成立。

### 1.3 次要观察（RV6-F12-02，建议同口径，不单独阻塞）
`:2597` provider 错误分支内 `self.facts.set_job_embedding_stats(&job.id, &stats)?;` 位于 `return Err(error)` 之前——stats 持久化失败时同样会替换 provider 主错误。此调用不属于 RV5 枚举的 fail_build 家族（facts 库、另一错误面），process_job 尾部对同类失败采用 append（:2331 "persisted statistics unavailable"），建议随 RV6-F12-01 一并同口径处理或显式记录豁免理由。

## 2. 亲跑测试（全部在 /tmp/lingxi-rv6 @ 7302a03cd 独立执行）

| 套件 | 命令要点（均 `--locked`，CARGO_TARGET_DIR=/tmp/rv6-target） | 结果 |
|---|---|---|
| r06_ar_f12_cache_consumers（service，原基线红） | `LINGXI_HOME=/tmp/rv6-env/f12 cargo test -p lingxi-service --test r06_ar_f12_cache_consumers` | **6 passed / 0 failed**，含原红项 `vector_ingestion_provider_failure_records_cleanup_version_rejection`（断言 SYNTHETIC_PROVIDER_ERROR + "cache status unchanged" + 保留 64 行 + status=building + version=999 + 2 次调用全过）——**基线红消除** |
| r06_ar_f12_cache_preservation（adapters） | `LINGXI_HOME=/tmp/rv6-env/f12a … -p lingxi-adapters --test r06_ar_f12_cache_preservation` | **22 passed / 0 failed**（与 RV5 一致） |
| 相邻知识面（service） | `… -p lingxi-service --test r06_t07_knowledge_tools --test r06_original_knowledge_proofs` | **12 + 6 passed / 0 failed** |

## 3. RV5 其余结论抽查（锚点级，全部吻合）
- `cache_write`（storage/knowledge.rs:637-646）：`BEGIN IMMEDIATE` 事务内 `check_user_version`——W12-R01-F01/F02 机制核心 ✓
- `set_variant_status`（:2561）：同样经版本闸 `cache_write(INDEX_SCHEMA_VERSION)`——旧句柄不可写 future 库 ✓
- `complete_build`（:3229-3241）：schema 预检先于 `checkpoint_entries`，分类顺序注释在源码 ✓
- `open_cache`（:531 起）：guard sidecar + try_lock_shared 先行，恢复路径受闸 ✓（抽查）
- TS `vector-index-adapter.ts`：last_used_at 更新在 `assertWritableSchema` 后的 immediate 事务内；best-effort 只吞 `SQLITE_(BUSY|LOCKED|READONLY|IOERR|FULL|CANTOPEN)`，版本拒绝必须可见（W12-R01-F02）✓
- TS `ann-index-store.ts` `openKnowledgeCacheDatabase`：只读守卫先验版本，非 WAL 库仅在 EXCLUSIVE 事务重验后才切 WAL（W12-R02 初始化侧写）✓

remaining_scope 六条与三个历史 blocker 的源码机制锚点全部复核成立；RV5 的探针级亲跑证据（counter-old-status、race-v2、open-window-v2）本轮未重放（任务允许抽查锚点替代），无反证。

## 4. 环境限制与未执行项
- 未重放：TS 三套（vector/fulltext/ann，RV5 已 30/30）、RV5 探针二进制、Windows/Linux、正式打包、真实供应商。
- F15 不在本轮范围（RV5 已判 PASS，本轮未触碰）。

## 5. 总判定与修复项
**F12：FAIL（唯一阻断项 RV6-F12-01）**。基线红已消除、五处已改实现正确、RV5 其余结论全部成立；但 RV5-F12-01 修复义务（同族 7 处「主错误已有、fail_build 仅清理」分支禁用 `?` 替换）仅满足 5/7。

修复项（完成后可复审关闭）：
1. [阻断] `knowledge.rs:2677-2680`（upsert_batch 失败）与 `:2690-2693`（accounting_error）改用与其余 5 处一致的「保留主错误码 + 追加清理拒写详情」模式。
2. [建议] `:2597` stats 持久化失败不得替换 provider 主错误（同口径 append 或记录豁免理由）。
3. [建议] 为 1 补一条可区分主/清理错误码的回归断言（现有冻结套件同码场景无法暴露替换），防同类回归。
