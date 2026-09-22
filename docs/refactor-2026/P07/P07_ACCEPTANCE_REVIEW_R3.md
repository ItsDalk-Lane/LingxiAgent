# P07 收口复验收报告（P07_ACCEPTANCE_REVIEW_R3，FIXR1 后独立复验收）

日期：2026-09-22｜验收者：独立复验收子代理（全新上下文，与执行者/首轮验收者/FIXR1 修复者均无共享会话）｜只读验收：未修改任何生产代码/测试/文档；唯一写入为本报告与 `artifacts/refactor-2026/P07/logs/ACCEPT-R3-*` 证据；负向构造均在系统临时目录内完成副本注入并已清理，`git diff HEAD` 全程为空。

## 1. 结论与终判

**P07 阶段终判：PASS**（判定对象 = 含 FIXR1 修复轮后的阶段整体；执行轮 PASS_WITH_BLOCKED_ITEMS 的实质性结论经两轮独立验收 + 修复轮复核后维持）。

- FIXR1 两项修复（F-A/F-C）**独立核实全部闭合**，未发现新问题。
- BLOCKED 项按任务书口径单独列明（§6）：P04-T07-2 真供应商冒烟、P06 真模型行为评测（继承，无凭证/费用授权，与本阶段新增无关）。
- 已登记给 P08 的 F-B（工具输出复跑覆盖样本、samples/ 未入清单）不在本轮闭合范围，状态不变。
- 除上述已知 BLOCKED 与 F-B 外，**无其他未闭合项**。

## 2. F-A 闭合独立核实（process.exit → exitCode）

**实现核查**（`artifacts/refactor-2026/P07/tools/bench-soak-resources.mjs:285-293`）：try 块末尾改为 `process.exitCode = …` 赋值（:289），finally 执行 `cache.dispose()` + `fs.rmSync(lingxiHome, {recursive, force})`（:290-293）。不再存在 `process.exit()` 调用。

**三条退出路径独立实测**（全部为本轮亲测，非引用 FIXR1 日志）：

| 路径 | 方法 | 结果 |
|---|---|---|
| 成功路径 | 本人运行修复版 `--batches 6`（--out 落 logs/ACCEPT-R3-soak.json） | exit 0，verdict=NO_REPRODUCIBLE_UNBOUNDED_GROWTH；运行前 tmpdir `hana-p07-soak-*` count=0 → 运行后 count=0（macOS 真实 tmpdir 实数） |
| 判定失败路径 | 本人复跑 FIXR1 负向注入副本（logs/P07-FIXR1-negprobe-inject.mjs，先 diff 核实与工具本体仅差「禁用外部追加」一处 + 头注释） | exit 1（退出码语义保留），运行后 tmpdir count=0（失败路径 finally 清理仍执行） |
| 中途抛出路径 | 本人构造反例 CE-R3：系统临时目录副本在批循环 b===2 处插入 throw | exit 1（未捕获异常语义），tmpdir count=0（finally 清理在异常传播前执行）；临时副本与输出文件已删除 |

结论：F-A 修复彻底——退出码语义（成功 0 / 失败非 0）未破坏，清理在所有路径可达。

## 3. F-C 闭合独立核实（断言承重 + 机制叙述）

**生产源码链核实**：
- `server/history-read/types.ts:47`：`HistoryProbeVerdict = "valid" | "append_candidate" | "branch_view_stale" | InvalidationReason`——判定集确无 `"fresh"`，旧断言 `!== "fresh"` 恒真的事实成立。
- `server/history-read/cache.ts:36-45`：`historyDirectoryCacheKey` 仅由 runtimeId/studioId/sessionId（或 path）构成，**不含 size/mtime** → 外部追加后 key 不变，soak 的 probe 是对同 key 的真实命中校验（排除「key 漂移导致断言空转」路径）。
- `server/history-read/cache.ts:158-198`（probe 判定序）：`valid` 仅在 size 相等且 mtime/ctime/revision/head 全等分支返回（:182-191）；size 增长必然落入分支 6（epoch 未变 → `append_candidate`，epoch 已变 → `untrusted_mutation`）——**追加场景不可能判 valid**。
- `server/history-read/index.ts`（tryDirectoryOnce）：`cache.get` 命中后仅 `verdict === "valid"` 直接复用目录（:336-338 附近）；`append_candidate` 走增量重建、`branch_view_stale` 走分支视图重建、其余失效判定落全量重建——「读路径仅 valid 直接命中」属实，非 valid 即不会返回追加前旧快照。

**断言承重**（工具 :187-191：`if (verdict === "valid") probeAppendDetected = false`）：
- 正跑：本人 6 批 + FIXR1 12 批原始规格复跑 + 执行轮样本三方对照，全批 `probe_verdict_after_append = append_candidate`、`probe_append_detected = true`；本人复跑逐批 rebuilds=12/hits=6/evictions 递增序列与执行轮原样本、FIXR1 复跑样本**逐位一致**（0 mismatch），invalidations 两两均为 `{}`——FIXR1「判定逻辑不受影响、逐位一致」声称属实。
- 负向注入：本人复跑禁用追加副本 → probe 判 valid → 断言变红 exit 1（同时覆盖 F-A 失败路径退出码）；注入副本与工具本体 diff 仅 1 行实质差异（appendFileSync → `void appendPath`），注入最小。

**三份文档机制叙述**：SOAK_RESOURCE_REPORT §1（已就地更正）+ §7（更正节留痕，含「原文是什么」）、P07_REPORT（§71 回退句更正 + §74-90 FIXR1 节）、HISTORY_COST_REPORT §4（一句话改写并标注「P07-FIXR1 更正，原『判非 fresh → invalidate → 下一批重建』不准确」）。grep 全 docs+tools+samples：现行叙述无「追加必失效→下批重建」残留；`probe_invalidated`/`"fresh"` 现存引用全部为历史性/更正性叙述（描述旧断言为何废弃），执行轮 `samples/soak-resources.json` 保留旧字段属「历史证据不改写」的正确处置；新证据链（ACCEPTANCE_MAP A05 evidence → logs/P07-FIXR1-soak-original.json；command_ids P07-FIXR1-soak-original/negprobe-inject-run/negprobe-verify 均在 command-log.jsonl 实际存在）无悬空引用。

**反例（针对修复内容）**：
1. 「失败路径提前抛出时清理不执行」→ 实测不成立（§2 中途抛出路径 count=0）。
2. 「新断言可被无关因素满足」→ 分析两条潜在退化路径：(a) key 漂移——被 cache key 构成排除（见上）；(b) 若追加对象已被 LRU 淘汰，probe 返回 `directory_invalid`（cache.ts:161），断言仍通过但退化为「不在缓存」的弱证明——在当前固定负载下 appendPath=批内最后构建且二次访问的最新 LRU 条目必驻留（三方实测全批 append_candidate 佐证），且 `probe_verdict_after_append` 字段留档实际判定、此类退化可被直接识别（verdict 将显示 directory_invalid 而非 append_candidate）。残余限制不影响当前证据效力，登记为信息级。
3. 「probe 判 branch_view_stale 时断言语义是否仍正确」→ 该判定仅存在于 size 相等分支（cache.ts:190），追加（size 增长）不可能触发；且即便出现，读路径走 rebuildBranchView 而非直接命中旧目录，断言「非 valid 即不返回旧快照」的保护语义与读路径分支一致。

## 4. FIXR1 范围纪律

- `git diff HEAD` 全程为空（0 文件）；git status 仅 4 项未跟踪（2 新测试 + docs/ + artifacts/ P07 目录）。f1 patch `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` sha256 `25fb315f…` 与 HEAD blob 逐位一致（本轮所有命令运行后复验仍一致——typecheck/lint/定向测试均无重写副作用，未跑全量故未触发现象）。
- HISTORY_COST_REPORT §4 更正**不属超范围**：首轮验收 P07_ACCEPTANCE_REVIEW §4 F-C 行的位置列原文即列「HISTORY_COST_REPORT §4、ACCEPTANCE_MAP A05、SOAK_RESOURCE_REPORT §1」三处；FIXR1 恰改这三处（另加 P07_REPORT/P07_RESULT 的修复轮自身记录），改动最小（一句话 + 标注）且已在 fix_rounds changes 中逐项披露。
- command-log.jsonl 共 46 条，其中 `P07-FIXR1-*` 恰 10 条，与 ACCEPTANCE_MAP/P07_RESULT fix_rounds verification（各 10 项）、P07_REPORT §90 复核清单（10 个）三方一致；negprobe-inject-run 如实记 `exit_code=1 status=FAIL`（预期失败未冒充 PASS）；每条含 source_sha=52dbb45f9（正确 HEAD）、argv、起止时间与双向 digest。
- 自纠留痕：空行清除有物证（`P07-FIXR1-manifest-check.err` 留有首次校验时 `shasum: WARNING: 1/2 lines improperly formatted`，终态 final 校验无警告、现清单空行数 0）；「计数 11→10」在仓库文档中无字面叙述（FIXR1 自报层面行为），但终态四处计数自洽（command-log 10 = 两份 verification 10 = P07_REPORT 10），无需动作。

## 5. 双清单独立复算与定向抽查

- **EVIDENCE_SHA256 独立复算**（仓库根 `shasum -a 256 -c`）：docs 清单 12 条 OK + logs 清单 106 条 OK = **118/118 全 OK、0 FAILED**，与 FIXR1 manifest-check-final（FINAL_MANIFEST_CHECK_EXIT=0）声称一致；final 输出条目集合与当前清单条目集合双向差集为空。清单头注自指排除口径（manifest-check.out/.err 与 -final.out 不入清单）沿 P06-FIXR 惯例。
- **定向抽查**：2 个新测试复跑 `tests/p07-startup-lazy-init.test.ts` + `desktop/src/react/__tests__/chat-performance/p07-stream-buffer-wallclock.test.ts` → **5/5 通过**（wallclock 15.6ms / 3.195M handle/s / flush=2 / probe 8.6ms，与首轮验收值一致）；`npm run typecheck` exit 0；`npm run lint` exit 0（0 errors 33 warnings，既有状态）。未跑全量 npm test（f1 patch 约束，首轮已全量对账 14797 绿）。

## 6. 边界与未验证（如实）

- 未复跑：全量 npm test（f1 patch 已知重写现象，两轮已对账）、真实浏览器开合循环、Electron 窗口级启动、W4-S3 进程树取消、bundle 签名构建（均沿用首轮验收结论与执行轮登记的限制，本轮无新疑点故不重复昂贵检查）。
- BLOCKED（继承，按任务书口径单列）：P04-T07-2 真供应商冒烟（无凭证/费用授权）；P06 真实模型工具行为评测（同因）。本阶段性能测量未消耗真实账户。
- F-B 维持登记给 P08（结构性：bench 工具固定输出路径复跑覆盖 + samples/ 目录未纳入 EVIDENCE_SHA256；本轮核实清单头注与 fix_rounds 均如实注明「tools/samples 仍不在清单内，F-B/P08 范畴」）。
- 信息级（无需动作）：soak 断言在「追加对象已被淘汰」负载变体下会退化为弱证明（§3 反例 2b，probe_verdict_after_append 字段可识别）；P06 fix_rounds 计数瑕疵等遗留登记项本轮未触碰（与 NEXT_STAGE_HANDOFF §遗留登记一致）。
- 本报告不承诺绝对无缺陷；以上为本轮抽样核验范围内的事实。

## 7. 本轮产物清单（同批写入）

- docs/refactor-2026/P07/P07_ACCEPTANCE_REVIEW_R3.md（本文件）
- artifacts/refactor-2026/P07/logs/ACCEPT-R3-{tmpdir-before.out, soak.out/.err/.json, tmpdir-after.out, negprobe-soak.json, negprobe-run.out/.err, new-tests.out, typecheck.out, lint.out}
- 两份 EVIDENCE_SHA256.txt 按惯例追加本轮条目（既有行未改动）。
