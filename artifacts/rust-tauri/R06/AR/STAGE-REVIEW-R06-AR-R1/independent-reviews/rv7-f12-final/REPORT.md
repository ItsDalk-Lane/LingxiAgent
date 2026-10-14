# R06 收口终审（RV7）——R06-AR-F12 第三轮修复后终态

- Reviewer：RV7（全新独立实例，不复用 RV5/RV6/执行者结论）
- 复审对象：基线提交 `180abc207`（fix: RV6-F12-01 complete append fix (7/7 sites) + same-code regression gate + RV6-F12-02 stats error）
- 工作树：`git worktree add /tmp/lingxi-rv7 180abc207`；cargo 1.98 `--locked`，CARGO_TARGET_DIR=/tmp/rv7-target，合成环境 /tmp/rv7-env
- **总判定：F12 = PASS**（RV5-F12-01 义务 7/7 完成、RV6-F12-01 两处残留消除、RV6-F12-02 已修、回归闸在位且经负控证明有效、全套件亲跑绿；RV5/RV6 已验锚点抽查全部成立。1 条非阻塞观察 RV7-F12-O1 见 §3。）

## 1. 修复核验（源码级，逐调用点）

### 1.1 RV5-F12-01 义务 7/7 全部为 append 语义
全 rust 树 `fail_build` 共 1 定义（lingxi-adapters storage/knowledge.rs:3275）+ 8 调用点（service knowledge.rs）+ 2 测试断言（preservation:564、consumers:382 回归闸），TS 无对应物。8 调用点 = process_job 尾部 :2338（历史既有 append，RV5 认可的口径基准）+ 义务 7 处：

| # | 新行号 | 路径 | 形态 |
|---|---|---|---|
| 1 | :2607-2613 | provider 失败 | `error.code.clone()` + append，Ok 时 `return Err(error)` |
| 2 | :2621-2627 | validate_embedding_batch | 同上 |
| 3 | :2639-2645 | identity 漂移（主错误 `main`） | 同上 |
| 4 | :2687-2693 | upsert_batch 失败（RV6 残留①） | 同上，本轮修复 |
| 5 | :2707-2713 | accounting_error（RV6 残留②） | 同上，本轮修复 |
| 6 | :2725-2733 | complete_build_plan | 同上 |
| 7 | :2738-2746 | complete_job | 同上 |

七处统一「保留主错误码 + `; cleanup fail_build rejected (cache status unchanged): {cleanup}`」，`grep -rn "fail_build" rust/` 全量比对：**零 `?` 替换残留**（全部 `if let Err` 形态）。

### 1.2 RV6-F12-02 已修
:2597-2605：provider 错误分支内 `set_job_embedding_stats` 失败改为「保留 provider 主错误码 + append `; embedding stats persistence failed: {stats_err}`」，与 fail_build 家族同口径。

### 1.3 RV6 修复项(3) 回归闸在位且经负控证明有效
tests/r06_ar_f12_cache_consumers.rs:375-386：升级用例非 provider 腿（`vector_ingestion_future_write_rejection_retains_paid_batch_and_error_code`，流经 :2685 upsert 主错误 + :2687 清理拒写，主/清理同码 KNOWLEDGE_SCHEMA_NEWER）断言错误串 `contains("cleanup fail_build rejected")`——追加后缀是同码场景下唯一可区分信号。

**负控亲验**（防断言永真）：临时将 :2687 还原为 `self.vectors.fail_build(&current.variant_id)?;` 替换语义后，该测试**精确红在 :378**，实测错误串变回 `KNOWLEDGE_SCHEMA_NEWER: ...; variant failure status unavailable: KNOWLEDGE_SCHEMA_NEWER: ...`（无追加后缀，主错误码仅因同码巧合幸存）——精确复现 RV6-F12-01 缺陷形态。还原后复跑 6/6 绿，worktree 干净（`git status --porcelain rust/` 为空）。

## 2. 亲跑测试（全部在 /tmp/lingxi-rv7 @ 180abc207 独立执行，`--locked`）

| 套件 | 命令要点 | 结果 |
|---|---|---|
| r06_ar_f12_cache_consumers（service） | `LINGXI_HOME=/tmp/rv7-env/f12 cargo test -p lingxi-service --test r06_ar_f12_cache_consumers` | **6 passed / 0 failed**（含回归闸腿与 provider 腿 SYNTHETIC_PROVIDER_ERROR + "cache status unchanged" 断言；数据保护断言 64 行保留、status=building、version=999、2 次调用全过） |
| r06_ar_f12_cache_preservation（adapters） | `LINGXI_HOME=/tmp/rv7-env/f12a … -p lingxi-adapters` | **22 passed / 0 failed**（c01-c04 全族） |
| r06_original_knowledge_proofs（service） | 相邻面 | **12 passed / 0 failed** |
| r06_t07_knowledge_tools（service） | 相邻面 | **6 passed / 0 failed** |
| 负控（临时还原 `?` 语义） | consumers 单测 | **预期红**：`vector_ingestion_future_write_rejection...` FAILED at :378，错误串缺追加后缀；还原后复跑 **6/6 绿** |

## 3. 新观察（RV7-F12-O1，minor，不阻塞）
`accounting_error` 自 :2568（本 batch dispatch 结果）即在手，而 :2699 `set_job_embedding_stats...?` 与 :2700-2704 `set_job_progress...?` 位于 :2705 accounting 检查**之前**——若 accounting_error 已 Some 且这两处 facts 持久化失败，`?` 返回持久化错误，accounting 主因被替换（:2618/:2684 的 stats `?` 同窗口同性质，但属成功路径常规持久化首错语义更强）。与 RV6-F12-02（provider 分支 stats `?`）同构，但不在其字面范围（RV6-F12-02 仅点名 :2597；RV5 义务限定 fail_build 家族）。按 RV6 对 RV6-F12-02 的先例（minor、不单独阻塞 F12 关闭）登记为建议项：accounting 在手窗口内的 stats/progress 持久化失败宜同口径 append 或记录豁免理由。

## 4. RV5/RV6 已验锚点抽查（全部成立）
- `cache_write`（lingxi-adapters storage/knowledge.rs:636-646）：`transaction_with_behavior(Immediate)` 事务内 `check_user_version`——W12-R01-F01/F02 机制核心 ✓
- `set_variant_status`（:2561-2563）：经 `cache_write(INDEX_SCHEMA_VERSION)` 版本闸 ✓
- `complete_build`（:3228-3242）：schema 预检先于 `checkpoint_entries`，F12 分类顺序注释在源码 ✓
- TS `vector-index-adapter.ts:1114`：best-effort 只吞 `SQLITE_(BUSY|LOCKED|READONLY|IOERR|FULL|CANTOPEN)`，版本拒绝可见；last_used_at 在 `assertWritableSchema` 后的 `.immediate()` 事务内（:1095-1115）✓
- TS `ann-index-store.ts:16-50 openKnowledgeCacheDatabase`：只读守卫先行 `assertCacheSchema`；非 WAL 库在 EXCLUSIVE 事务内重验版本后才切 WAL；`initialize(db)` 最后执行——W12-R02 初始化侧写 ✓

## 5. 环境限制与未执行项
- 未重放：TS 三套（RV5 已 30/30）、RV5 探针二进制（counter-old-status / race-v2 / open-window-v2，任务允许锚点抽查替代，无反证）、Windows/Linux、正式打包、真实供应商。
- F15 不在本轮范围（RV5 已 PASS）。
- 判据核对：任务书三项条件（7/7 修复 ✓ + 回归闸 ✓（含负控）+ 全套件绿 ✓）全部满足，无前两轮义务残留 → PASS。
