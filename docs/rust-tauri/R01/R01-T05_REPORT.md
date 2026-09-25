# R01-T05 执行报告（HTML/PDF 与办公处理原型）

- 任务：R01-T05；验收 R01-A09 / R01-A10（REQUIRED）
- 基线 HEAD：`5a8a8e24ab9e8e105da5132d16b175221a91d646`；分支 `codex/rust-tauri-migration`
- 执行代理：ZCode:R01-T05-exec-r1（全新独立执行；不 commit/push；判定归独立验收代理）
- 平台：macOS 27.0 arm64（M3 Ultra）；Chrome 153.0.8010.52；Electron 42.8.1（Chromium 142 系）；Rust 1.98.1（rust-toolchain.toml 锁定）
- 日期：2026-09-25

## 1. 交付清单

| 类别 | 路径 | 说明 |
|---|---|---|
| Spike 报告 | `docs/rust-tauri/R01/PDF_SPIKE_REPORT.md`（sha256 52d2af89…4fffb） | 契约矩阵/逐项验证/A09/A10/性能/许可 |
| ADR | `docs/rust-tauri/R01/ADR-003-document-renderer.md`（71351dea…6e13） | 结论：办公链可退出 Electron（条件见 ADR） |
| 候选原型 | `rust/crates/lingxi-browser-spike/src/pdf.rs`（1f449862…326c）+ `src/bin/spike_pdf.rs`（13aad66b…7788） | 复用 T04 cdp/launcher；零新增依赖（D-10） |
| 样本集 | `tests/migration/r01-t05/samples/`（S1–S6+decoy）+ `decoy/passwd` + `FONTS.md` | 生成器 `generate_samples.py --check` 可校验 |
| 工具链 | `tests/migration/r01-t05/{pdf_probe.swift, compare_outputs.py, analyze_matrix.py, run_old_chain.mjs, old_chain_harness.cjs, canary_server.mjs}` | PDFKit 探针/语义比较/矩阵判定/旧链驱动 |
| 一键重放 | `scripts/rust-tauri/r01-t05-replay.sh`（2e5d8752…7364） | 全流程：构建→样本校验→代理/canary→全场景→对比→性能 |
| 证据 | `artifacts/rust-tauri/R01/T05/`（SHA256SUMS.txt 632 项） | 含 a09/a10/s2/s3/s4/font-negative/perf/reverify/proxy.log |
| 决策登记 | DEPENDENCY_DECISIONS.md D-10；DEPENDENCY_RULES.json 模块条目职责补注 | 无新 crate、无新依赖、Cargo.lock 零变化 |

## 2. 逐场景：命令 / 预期 / 实际 / 退出码

统一前置：`env -u 全部代理变量`；`CARGO_TARGET_DIR=/tmp/r01t05-replay/cargo-target`；loopback-only 代理
127.0.0.1:18482（T04 修补版 proxy.mjs，非 loopback 一律 403）+ canary 127.0.0.1:18291；
浏览器均加 `--proxy-server` + `--proxy-bypass-list=<-loopback>`（T04 教训）；旧链 Electron
userData/cache/logs 全部重定向 /tmp。全部场景由 `r01-t05-replay.sh` 驱动（最终完整 run exit=0）。

| 场景 | 预期 | 实际 | 退出码 |
|---|---|---|---|
| A09 旧链 S1 | 产出 PDF | 74 页 994,832B，1040ms | 0 |
| A09 候选链 S1 | 产出 PDF | 74 页 1,436,971B，printToPDF 330ms | 0 |
| A09 语义+视觉断言（compare_outputs.py） | 全过 | **all_pass=true（35 项）**：页数等、140/140 行、无空白页、文本相似 0.99910、网格差 max 7.16、字体三族双链嵌入、图片 XObject=2=2 | 0 |
| S2 @page preferCSSPageSize=true ×双链 | A5 横版 | 两链 MediaBox=[0,0,594.96,420] | 0 |
| S2 preferCSSPageSize=false ×双链 | 各链实测语义 | 旧链 [0,0,842.88,595.92]（job 尺寸+CSS 方向）；候选 [0,0,594.96,420]（CSS 全优先）——KNOWN-DIFF 登记 | 0 |
| S2 break-before:page | 哨兵不在第 1 页 | 两链 marker_on_pages=[2] | 0 |
| S3 Letter+landscape+margins(inch) | MediaBox 792×612、边距内缩 | 两链 MediaBox=[0,0,792,612]，墨迹 bbox 逐值相同 [217,151,880,398] | 0 |
| S4 allowJavaScript=false ×双链 | JS 标记缺席 | 两链缺席，静态哨兵在 | 0 |
| S4 allowJavaScript=true ×双链 | JS 标记出现 | 两链出现 | 0 |
| 字体负对照 embedLingxiFonts=false | 三族不嵌入 | 仅系统回退字体（STSongti/PingFang 等） | 0 |
| A10 候选链 S5（JS 开） | 越权全拒、无泄漏、正常产出 | 7×DENY（/etc/passwd iframe/img/XHR、master.passwd、/etc/hosts、canary.invalid、loopback canary），输出无泄漏，exit=0 | 0 |
| A10 旧链 S5-decoy（参考） | 记录真实行为 | **泄漏**：decoy passwd 完整嵌入 PDF；loopback canary 被真实抓取（1 次命中，时间戳归属旧链窗口）；canary.invalid 经代理被 403 | 0 |
| A10 候选链 S6 无限脚本（5s 超时） | 超时、杀树、无伪产物 | exit=2「loadURL timed out after 5000ms」，无 s6.pdf，profile 已删，ps 残留为空 | 2（预期） |
| A10 旧链 S6（参考） | 记录真实行为 | exit=1（5342ms），无产物 | 1（预期非0） |
| A10 S6 JS 关闭（候选） | 死脚本惰性化正常产出 | 产出正常 | 0 |
| 性能 候选 16× / 旧链 8×（S1 直测口径） | 记录对照基线 | 候选 spawn→exit median 1644.3/p95 1663.5ms（printToPDF 312.5ms）；旧链 953.5/1021ms；R00 基线 812.1/835.6ms（239KB 样本，口径差异见 spike 报告 §6） | 0 |

矩阵判定汇总：`matrix-verdicts.json` = **28 VERIFIED / 0 FAILED**（analyze_matrix.py exit=0）。

### margins 单位考据（bisect 记录）

旧链（Electron 42.8.1）实测：margins {96} 与 {10} 在 A4 上均被拒（"margins must be less than or
equal to pageSize"），{5} 被拒、{4} 通过且墨迹左缘恰为 4in、{0} 通过——margins 数值按 **inch**
解释并直传 Chromium，校验亦按 inch 数值比较；electron.d.ts 注释 "in pixels" 与实测行为不符。
候选链按实测生产语义直传 inch（pdf.rs `margins_in` 注释固化证据），S3 两链墨迹 bbox 逐值一致。
迁移契约按**实测行为**冻结。

## 3. 改 rust/ 后强制校验（全部重跑于 fmt 后的最终源码）

| 校验 | 退出码 | 证据（artifacts/rust-tauri/R01/T05/reverify/） |
|---|---|---|
| `r01_t01_check_ownership.py`（正向） | 0 | t01-check-positive.log |
| 同 `--self-test`（N1–N15） | 0 | t01-self-test-N1-N15.log |
| `r01_t01_build_ownership.py --check` | 0 | t01-build-check.log（736 features/69 stores up-to-date） |
| `r01-t02-roundtrip.sh` | 0 | r01-t02-roundtrip.log |
| `r01-t02-handshake.sh` | 0 | r01-t02-handshake.log |
| `r01-t02-check-generated.sh` | 0 | r01-t02-check-generated.log |
| `cargo test --workspace --offline` | 0（37 passed: 11+7+19；另 lib 测试全过） | cargo-test-workspace.log |
| `cargo fmt --all --check` | 0 | cargo-fmt.log |
| `cargo clippy --locked --workspace --all-targets --offline -- -D warnings` | 0 | cargo-clippy.log |

## 4. 红线合规自查

- 未 commit/push/PR/tag/release；未改任务书目录、`.sync-audit`、`ORCHESTRATOR_PROGRESS.json`（该文件为预先存在的总控账本修改，本任务未触碰）。
- 生产目录零改动（desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json 均未触碰；office-pdf-helper.cjs / office-pdf-fonts.cjs 仅只读调用）。`git status --porcelain` 终态见 §6。
- 样本全部本地构造（PNG 逐像素程序生成；数学内容内联 SVG+CSS；无远端引用，S5 的远端/越权引用是故意拒绝探针）。
- 全部浏览器流量限 loopback：候选链 Fetch 拦截 + loopback-only 代理双边界；代理日志 286×REFUSED 证明 Chrome 后台外联（GCM/时间校检/组件更新等）全部被拒（T04 教训的闭环措施）；代理自验 200(loopback)/403(example.com) 留证 proxy-selftest.txt。
- 旧链运行不写用户目录（setPath 全重定向 /tmp/r01t05-replay/old-tmp-*），输出全部 /tmp 或 artifacts。
- 旧链真实 /etc/passwd 泄漏的开发期试运行产物已删除，未入证据链；提交证据一律用合成 decoy。
- Cargo.lock 零变化（无新 crate/依赖）；新 bin 挂在既有 prototype crate 下，DEP-07 生效。
- 预存失败未追修（审计封印 FAIL 等）；未跑全量 npm test（任务书明示不必）。

## 5. 问题 / 阻塞

- 无阻塞项。已知差异与限制全部登记于 PDF_SPIKE_REPORT §8（preferCSSPageSize=false 角例 KNOWN-DIFF、
  旧链 Ext-B 缺字、旧链无资源拦截、margins 实为 inch、仅 macOS arm64 实测、冷启动性能口径）。
- 提交提示：`artifacts/rust-tauri/R01/T05/` 下 `*.log` 命中根 .gitignore 的 `*.log` 规则
  （与 T04 reverify/proxy 日志同例，提交时需 `git add -f` 或沿用 T04 提交者做法）。

## 6. 最终 git status --porcelain
```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json   （预先存在的总控账本，本任务未触碰）
 M docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md  （D-10 追加）
 M docs/rust-tauri/R01/DEPENDENCY_RULES.json    （browser-spike 职责补注 spike_pdf）
 M rust/crates/lingxi-browser-spike/Cargo.toml  （description 更新）
 M rust/crates/lingxi-browser-spike/src/lib.rs  （pub mod pdf;）
?? artifacts/rust-tauri/R01/T05/
?? docs/rust-tauri/R01/ADR-003-document-renderer.md
?? docs/rust-tauri/R01/PDF_SPIKE_REPORT.md
?? rust/crates/lingxi-browser-spike/src/bin/spike_pdf.rs
?? rust/crates/lingxi-browser-spike/src/pdf.rs
?? scripts/rust-tauri/r01-t05-replay.sh
?? tests/migration/r01-t05/
```

## 7. 勘误（R1 独立验收后更正段；不回改上文历史结论）

> 来源：`R01-T05_REVIEW_R1.md` 发现 F3/F4/F5；修复轮次与证据见 `R01-T05_REPAIR_R1.md`。
> 本节为追加勘误（参照 R00 勘误先例），上文 §2/§3 的原始表述除 F3 两处计数按验收
> 要求更正为实测值外一律保留。

- **F3（计数笔误，已更正）**：§2 compare_outputs.py「41 项」实为 **35 项** checks
  （all_pass=true 本身属实）；`PDF_SPIKE_REPORT.md` 页首「623 项」实为 **632 项**
  （本报告 §1 写 632 正确）。两处已按实测值更正。
- **F4（cargo test 计数少报）**：§3 表「37 passed: 11+7+19」少计 lingxi-protocol 的 7 个；
  实测 **44 passed**（lingxi-browser-spike 11 + lingxi-kernel 7 + lingxi-spike 19 +
  lingxi-protocol 7）。少报而非虚增，不构成可信性问题；勘误以校准账本。
- **F5（A09 文本层差异构成说明，非缺陷）**：0.99910 的 0.0009 相似度缺口主要由旧链
  （Chromium 142）文本运行顺序导致的**数字串提取转置**构成（如源 HTML `庚42`+`40805`
  两格 → 旧链文本层 "4240085"、候选链正确的 "4240805"），属提取层伪差，位图层无对应
  差异（网格差同步极小）；另有 SVG 数学内容文本化差异与 𪚥 旧链缺字。两链 page-01 位图
  高度 2px 差（1686 vs 1684 @2x）为 MediaBox 1pt 舍入。均不影响 A09 判据。
- **F1/F2 处置指针**：WebSocket 边界缺口（F1）与超时 exit 码契约（F2）由 repair-r1
  修复并实测留证，详见 `R01-T05_REPAIR_R1.md`、`PDF_SPIKE_REPORT.md` §8 第 7–8 条、
  `ADR-003` 后续强制门禁节。
