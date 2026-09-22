# P07 独立验收报告（P07_ACCEPTANCE_REVIEW）

日期：2026-09-22｜验收者：独立验收子代理（全新上下文，与执行者无共享会话）｜只读验收：未修改任何生产代码/测试/文档；负向注入均在验毕立即 `git checkout` 还原并复核零残留（`git diff HEAD` 全程为空）。

## 1. 结论

**PASS（接受执行者 PASS_WITH_BLOCKED_ITEMS 的阶段结论）**，附 3 项低严重度发现（F-A/F-B/F-C，见 §4）：均为基准工具或报告叙述层面，不动摇任何场景判定、不涉及生产代码缺陷；建议随 P08 修复（工具/文档级，最小改动）。

BLOCKED 项确认为继承（P04-T07-2 真供应商冒烟、P06 真模型行为评测：无凭证/费用授权），与本阶段新增无关，登记一致。

## 2. 核验范围与方法

- HEAD `52dbb45f9cc4eaea5b2194780a57a8899b30f21a`（验收时确认）；分支未切换。
- 通读：P07 任务书、01 通用执行约束、92 模板、P07 全部 12 份交付文档 + ACCEPTANCE_MAP.json + P07_RESULT.json + command-log.jsonl（36 条逐条）。
- 独立复测（工具=artifacts/refactor-2026/P07/tools/ 与 P00 工具；输出全部写入 artifacts/refactor-2026/P07/logs/ACCEPT-R2-*）：

| # | 复测项 | 执行者声称 | 独立复测 | 一致性 |
|---|---|---|---|---|
| R1 | W1 启动 ×10（P00 工具） | median 1301 / p95 1302 / ready 10/10 | **median 1301 / p95 1302 / ready 10/10** | 完全一致 |
| R2 | bench-compare W1 判定（P00 vs P07 样本） | IMPROVED -12.0% | **IMPROVED, delta -12%** | 完全一致 |
| R3 | trim 微基准（复跑后原样本已备份还原，sha 复核一致） | 0.377 / 11.144 / 40.014 µs/append | **0.231 / 11.181 / 38.477** | cap5000 差 0.4%，量级一致 |
| R4 | soak 12 批（--out 重定向，未触碰原样本） | NO_REPRODUCIBLE_UNBOUNDED_GROWTH；rss -1.0%/+0.3% | **NO_REPRODUCIBLE_UNBOUNDED_GROWTH；rss +0.8%**（第 3 次独立运行） | 判定一致 |
| R5 | W3 Layer1 吞吐 | 116k / 141k / 90k ev/s | **115.6k / 139.0k / 89.0k** | 差 ≤1.5%（噪声区） |
| R6 | UI wallclock 测试复跑 | 15.6ms / 3.2M handle/s / flush=2 / 探针 8.7 | **15.7ms / 3.19M / 2 / 8.8** | 一致 |
| R7 | 全量 npm test | 4 红=F1 同组 / 14797 绿 / 1 expected fail / 15 skipped | **同名 4 红 / 14797 / 1 / 15** | 完全一致 |
| R8 | npm run typecheck | 0 | **0** | 一致 |

- 基线对账：P06 全量 14792 绿 → P07 14797 绿 = +5，恰为本阶段新增 5 例（3 lazy-init + 2 wallclock）；4 红测试逐名比对（post-verification-audit-seal 旧坐标 / round3 manifest / R10-03 / R10-04）与 P05/P06 相同，未扩大。
- f1 patch：本人复跑全量后被重写（复现了已知现象），已 `git checkout` 还原，sha256 `25fb315f…` 与执行者声称一致。

## 3. 逐项核验结果（对应验收提示词 7 个关注点）

### 3.1 「生产代码零改动」— 属实
`git diff HEAD` 完全为空（0 文件）；`git status` 仅 4 项未跟踪：`artifacts/refactor-2026/P07/`、`docs/refactor-2026/P07/`、`tests/p07-startup-lazy-init.test.ts`、`desktop/src/react/__tests__/chat-performance/p07-stream-buffer-wallclock.test.ts`。无任何 tracked 文件改动 ⇒ 无断言削弱、无测试删除、无既有审计证据修改，P05/P06 锁定面（session-stream-store.ts、history-read/*、use-stream-buffer.ts、canonical 装配等）零触碰由空 diff 直接证明。

### 3.2 测量可复现性 — 8 项复测全部成立（见 §2 表）
样本与报告口径抽查：W2 热页 p50/p95（phase A 0.894/1.208、0.710/0.879；phase B 0.9125/0.715）、fullFileReadCalls max=0、jsonlParse 103/页、冷首页 10.77/72.63、P00 基线（1.108/0.824、11.25/84.1）均与 summary JSON 逐位一致；fixture sha256 交叉核对内嵌于样本且与 fixture audit 一致。P00 基线 provenance 经 P00 command-log 核实为 `92c6646c5818`（重构前）。

### 3.3 「-12%/-18%」归因 — 诚实
- W1 -12%：P00 基线（92c6646c，2026-09-21）与 P07 复测（52dbb45f9，2026-09-22）同机/同 Node v24.16.0/同 lockfile `a9735825…`（两处样本 env 字段核实）。P07_PERFORMANCE_COMPARISON §前置声明明确「不包含任何由 P07 代码改动带来的优化宣称」，HOTSPOT_REPORT §2 亦标注「属 P01–P06 累积效果+机器波动区间，不作为本阶段优化宣称」。10/10 ready 且样本极紧（1298–1302ms），-12% 超出 P00 已建的批间波动区（±3.2%），为真实系统性差异，归因叙述与 git 事实（P07 零 diff）自洽。
- bundle -18%：源码形态 1301 vs bundle 1072（首轮日志）/1041（r2，样本文件）同环境同口径；两值在 §3 表格并列，未隐藏。限制（W1 终点=server-info.json 非窗口级）已如实登记。

### 3.4 热点登记 — 证据成立、延后合理
- `server/session-stream-store.ts:144` `trimEvents` 确为 `splice(0,1)` 头部移除（源码核对）；微基准复跑证实饱和后 11.1µs（cap5000）/38.5µs（cap20000）vs 未饱和 0.23µs；W3L1 n=20000 吞吐回落（141k→90k）与微基准互证成立。
- 「P05 锁定面零触碰」与空 diff 互相印证 ✓。最小修复方案（头偏移游标+周期压实+等价测试+复跑微基准）确为最小且保持 seq/reset/truncated 语义，按 T01.4 登记延后并写入 NEXT_STAGE_HANDOFF §4（授权前不改）——符合任务书「最多三个热点」与「无安全热点不制造需求」的约束（H2 验证结论不改、H3 无第三热点）。

### 3.5 新测试质量 — 走真实入口、断言承重
- `tests/p07-startup-lazy-init.test.ts`：逐字提取生产 `server/index.ts startBridgeManager` 源码并在 vm 执行（提取失败=测试失败，防漂移）；A03 三例与生产实现（:860-899）逐行核对形状一致（single-flight/失败收口 null/getState 三态/finally 清 promise）。
  - 负向注入 1：临时删除生产源码中 `if (bridgeManagerInitPromise) return bridgeManagerInitPromise;` → **用例 1 变红（loader.calls=2）**，用例 2/3 仍绿（符合预期差异面）。已还原，diff 零残留。
- `p07-stream-buffer-wallclock.test.ts`：驱动真实 `streamBufferManager`（use-stream-buffer.ts 单例）+ 真实 store；断言含 50k delta 全量长度相等（末帧不丢）、flush 上界、markdown_parse=0、A06 形状 TAIL_AFTER_FILE/文件块保留。
  - 负向注入 2：临时在 canonical 分支丢弃 `assistant_segment_delta` → **两用例均变红**（长度断言与 startsWith 断言分别捕获）。已还原，diff 零残留。
- A02 锚点核实：tests/server-startup-diagnostics-contract.test.ts:491「keeps bridge platform dependencies out of the server readiness path」存在且断言 ready 写入 < setImmediate < startBridge 顺序；生产侧 503 `server_starting` 门（index.ts:387-402）、bind(:400)先于 engine.init(:445)、路由挂载后 activeFetch 切换(:1056) 均核实。
- 备注G（透明性）：A06 用例通过时 stderr 出现「unresolved assistant segment finalized with fallback」——该终态保留走的是生产设计的 fallback 终结路径（finalizeRun 投影诊断），结果锚点（内容不丢）有效，非测试缺陷。

### 3.6 全量基线对账 — 成立（见 §2 R7 与基线对账段）

### 3.7 反例构造（≥2 实做，共 4 组）
1. **异常退出/工具状态残留（CE-1，坐实 F-A）**：macOS 真实 tmpdir 下 SIGKILL soak 中途 → 泄漏 `hana-p07-soak-*` 目录；进一步发现**正常成功退出同样泄漏**（`process.exit()` 位于 try 内，跳过 finally 的 rmSync）——实测系统 tmpdir 累积 8 个 ≈12MB 目录（含执行者 2 次 + 本人复测/杀进程）。已全部清理，现零残留。
2. **缓存失效时序（CE-2）**：隔离临时目录内以真实 `HistoryDirectoryCache` 构建后分别施加 外部追加 / 同长度原地重写 / 截断 / 原子替换：probe 判定分别为 `append_candidate` / `untrusted_mutation` / `snapshot_changed` / `file_identity_changed` —— **生产 probe 语义正确**（不返回旧内容：读路径仅在 verdict==="valid" 直接命中，append_candidate 走增量重建，index.ts:322-345 核实）。但该实测同时暴露 soak 断言问题（F-C）。
3. **负向注入 ×2**（见 3.5，均已还原）。
4. **并发基准数字稳定性**：W3L1/W1/trim/soak 复测与执行者值差 ≤1.5%/0%/0.4%/判定一致；全量测试两次运行计数完全相同。

## 4. 发现（不阻塞，建议 P08 一并处理）

| # | 严重度 | 位置 | 问题 | 最小修复 |
|---|---|---|---|---|
| F-A | 低（工具缺陷+报告不实一句） | artifacts/refactor-2026/P07/tools/bench-soak-resources.mjs:273-276 | `process.exit()` 在 try 内，finally 的临时目录清理**每次运行（含成功）都不执行**，泄漏 ≈12MB/次；与 P07_REPORT §回退「soak/bench 全部临时目录用后即删」不符（其余工具无此问题，w3l2/w4/bench-startup 清理已核实有效） | 以 `process.exitCode = code` 替代 `process.exit(code)`（让 finally 自然执行），或把 rmSync 移到 exit 前；P07_REPORT 该句在合法触碰时更正 |
| F-B | 低（证据保留机制） | tools/bench-stream-throughput.mjs、bench-stream-model-layer.mjs、bench-startup-bundle.mjs 等固定输出路径 | 工具每次复跑覆盖同一样本文件；执行者对 W3L1 跑 2 次、W3L2/W4 跑 3 次，报告引用首轮精确值（66,180/85,791/100,387 与 116,513/141,115/90,124），而留存样本 JSON 仅含最后一轮——「保留全部样本」仅在 .out 摘要层成立。另：本人复跑 W3L1 时覆盖了 stream-throughput.json（数值等价 115.6k/139.0k/89.0k，见 ACCEPT-R2-w3-layer1.out；原 r2 内容无哈希锚定、无法字节级复原，如实披露） | P08 复用时为输出路径加 run-id 后缀或改 `--out` 必填；两份 EVIDENCE_SHA256 清单补 pin samples/ 目录（当前仅覆盖 docs 11 项与 logs 73 项） |
| F-C | 低（断言空转+叙述不精确） | tools/bench-soak-resources.mjs probe_invalidated 计数；HISTORY_COST_REPORT §4、ACCEPTANCE_MAP A05、SOAK_RESOURCE_REPORT §1 | 断言 `verdict !== "fresh"` 恒真（HistoryProbeVerdict 联合类型无 "fresh"，"valid" 也会被计为"已失效"）——12/12 计数**不构成证明**；实测外部追加的判定为 `append_candidate`（保留目录、非 invalidate），「追加→invalidate→下批重建」的机制叙述不准确（下批重建实际由 12 会话>8 槽 LRU 颠簸驱动）。**生产语义不受影响**（A05 实质由既有 history-read-directory-cache/counters 测试锁定，且已在定向 15 文件中复跑绿；CE-2 实测四类变更判定全部正确） | 断言改为 `!["valid","append_candidate","branch_view_stale"].includes(verdict)` 之类精确判定；三处文档在合法触碰时修正机制描述 |

信息级备注（无需动作）：(i) bench-compare 对 W1 的 IMPROVED -12.0% 判定未作为命令留档（command-log 仅 --selftest）——本人复跑产出完全相同判定（ACCEPT-R2-bench-compare-w1.out），实质为真、仅日志完整性小缺口；(ii) scripts/benchmark-history-read-directory.mjs:463 硬编码 gitHead/branch（P00 起即错，非本阶段引入），权威 SHA 以 command-log source_sha 为准；(iii) EVIDENCE_SHA256 未覆盖 samples/（P06 同口径，P05 曾覆盖 artifacts，见 F-B 建议）。

## 5. 边界与未验证（如实）

- 真实浏览器开合循环、Electron 窗口级启动（window.show→可交互）、W4-S3 进程树取消本人未复跑（GUI 未授权/S3 单样本已由 r3 日志与 p02-cancellation-edges 同形状测试覆盖）；执行者均已如实登记为限制。
- bundle 形态 -18% 未重新构建（需 LINGXI_SIGN_KEY 的 seed 签名步骤按设计 exit 1；bundle/index.js 产出与首轮计时以日志+dist-server 存在性核实）。
- 真供应商冒烟/真模型评测维持 BLOCKED（继承，无凭证/费用授权）；四平台 CI 属 P08。
- 本报告不承诺绝对无缺陷；以上为抽样核验范围内的事实。

## 6. 验收轮产物清单（本报告同批写入）

- docs/refactor-2026/P07/P07_ACCEPTANCE_REVIEW.md（本文件）
- artifacts/refactor-2026/P07/logs/ACCEPT-R2-{startup-b1.json/.out/.err, bench-compare-w1.out, trim-micro.out/.err, soak.json/.out/.err, w3-layer1.out, full-test.out}
- 两份 EVIDENCE_SHA256.txt 已按 P06 惯例追加本轮条目（既有行未改动，# 注释行 shasum -c 兼容）。
