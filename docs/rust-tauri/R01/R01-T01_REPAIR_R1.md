# R01-T01｜修复记录 REPAIR R1（对应独立验收 R01-T01_REVIEW_R1.md 的 F1/F2/F3）

- 修复者：ZCode:R01-T01-repair-r1（全新修复代理；未参与 T01 执行与 R1 验收）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `328cc8bb5a807bdaad520b459907fb1fa4e10dca`（开工实测一致）
- 范围：只改 T01 交付物（校验器 + ADR/执行报告中与修复直接相关的段落）；未改生产代码、
  任务书目录、.sync-audit、ORCHESTRATOR_PROGRESS.json、验收报告原文；未 commit/push。
- 环境：macOS 27.0 arm64；Python 3.14.3；cargo 1.93.0。全程离线：所有命令以
  `env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY`
  摘除死代理，cargo 一律 `--offline`，`CARGO_TARGET_DIR` 指向 /tmp 隔离目录。
- 结论：**READY_FOR_REVIEW**（PASS/FAIL 归总控另派的全新复验代理判定）。

## 1. F1（高）依赖方向校验器对连字符内部依赖名失效

**根因**：`transitive_deps()` 以 `resolve.nodes[].deps[].name` 建邻接——cargo 在该字段
把连字符包名改写成下划线 crate 标识符（本次修复中实测复核：
`deps[].name = ['lingxi_adapters','lingxi_protocol']` 而
`packages[].name = ['lingxi-adapters','lingxi-kernel','lingxi-protocol']`，证据
`inject-p1a-cargo-metadata-depnames-repair-r1.log`）。后果三重：DEP-03/DEP-04 的
连字符 forbidden 模式永不命中；传递闭包按 `name_of[dep_id] == name` 展开时在第一个
连字符包处断裂；D2 的 `deps & workspace_names` 交集恒空、白名单恒真。

**修复内容**（`r01_t01_check_ownership.py`）：

1. `transitive_deps()` 改为以 `deps[].pkg`（包 ID）建邻接、以包 ID 求闭包，闭包完成
   后再映射回 `packages[].name` 做模式匹配与报告（同时消除同名多版本歧义）。
2. D1 模式匹配双侧经 `norm_dep_name()`（小写 + `_`→`-`）归一，连字符/下划线两种
   形态互通。
3. 按验收建议补 D5 对偶检查：status≠exists 的模块若 crate 已落盘
   （`crate_path/Cargo.toml` 存在）或已进入 cargo workspace，即违例——堵死
   "planned 模块被静默实体化后整体跳过执法"的辅助攻击路径（B1 工作区成员形态）。
4. 新增负向自测 N8（kernel 直连连字符内部 crate，dep 名用真实 cargo 输出形态
   `lingxi_adapters` → 期望 D1/DEP-03）、N9（kernel→lingxi-adapters→tauri 传递桌面
   依赖 → 期望 D1）、N10（伪造 workspace 成员 + kernel 白名单规则 → 期望 D2），
   三者均断言"以匹配规则 ID 拒绝"。

**真实注入复证**（/tmp/r01-t01-repair-r1 隔离副本，非合成 metadata；修复后校验器）：

| 场景（对应验收反例） | 注入方式 | 期望 | 实际退出码/规则 | 证据日志 |
|---|---|---|---|---|
| P1a（B1 直连形态） | kernel Cargo.toml 加 `lingxi-adapters = { path = "../../../external/lingxi-adapters" }`（外部 path crate，自带 `[workspace]` 脱离工作区） | 拒绝 | **1 / D1 DEP-03**（`forbidden ['lingxi-adapters']`） | inject-p1a-kernel-direct-lingxi-adapters-repair-r1.log |
| P1b（B1 工作区成员形态） | crate 建于 `rust/crates/lingxi-adapters` 并加入 workspace members + kernel path 依赖 | 拒绝 | **1 / D5**（planned 模块实体化即违例，先于 D1 触发） | inject-p1b-planned-module-materialized-repair-r1.log |
| P2（B2 传递形态） | kernel→lingxi-adapters→tauri（两个外部伪造 crate） | 拒绝 | **1 / D1 DEP-02**（传递闭包穿透连字符包检出 tauri） | inject-p2-kernel-adapters-tauri-transitive-repair-r1.log |
| P3（B3 白名单绕过） | 伪造 workspace 成员 forged-internal + kernel 依赖 + 副本中 DEPENDENCY_RULES 加 `allowed_module_deps:["lingxi-protocol"]` 规则 | 拒绝 | **1 / D2 DEP-T10**（`disallowed workspace modules ['forged-internal']`） | inject-p3-forged-member-whitelist-repair-r1.log |

驱动脚本原始输出：inject-scenarios-driver-repair-r1.log（P1b/P3 段）与
inject-p1a-p2-redo3-driver-repair-r1.log（P1a/P2 段；前两轮驱动脚本自身有
`set -u` 未绑变量与嵌套 workspace 根两处脚本错误，已修正重跑，被取代的驱动日志
未保留，各场景最终证据日志均为修正后的真实运行）。干净副本正向回归：
inject-clean-copy-positive-repair-r1.log（RESULT: OK，退出 0）。

## 2. F2（低）authoritative store 写者唯一为 rust-service 未被机器执法

**根因**：O6 只禁 worker/ui 类 owner 拥有 store，host/build 类不受限；
`target_writer_process` 字段无任何规则校验。

**修复内容**：校验器新增 O8——classification.kind 为 authoritative 的 store，
owner 必须是 core/service/adapters 类（host/build/ui/worker/xtask/cli 一律拒绝），
且 `target_writer_process` 必须以 `rust-service` 开头（单一写者契约）。负向自测
N11：把 session-jsonl 改映射给 tauri-host.desktop-host + tauri-host 写者 → 期望
O8 拒绝。ADR-001 §5 store 分配段补记 O8 执法约定。

**真实复证**（P4，对应验收 A9）：/tmp 副本中改生成器闭表
`session-jsonl → tauri-host.desktop-host + TAURI_HOST`，重跑生成器成功（退出 0，
inject-p4-regenerate-repair-r1.log，生成器自身不拦 host 类——执法在校验器），
再跑修复后校验器 → **退出 1 / O8**
（inject-p4-authoritative-store-to-host-repair-r1.log）。修复前同注入退出 0
（验收报告 §3 A9 实测记录）。

## 3. F3（低）换 fact_id 的 shadow 关键事实可过校验

**根因**：校验器只查 11 条必需事实的"必需子集"，不查词表外多余条目。

**修复内容**：02 §3 的 11 条关键事实锁为精确集合（`LOCKED_CRITICAL_FACTS`）：
缺失即 O4 拒绝（原行为保留），词表外条目同样 O4 拒绝，报错明示"新增/改名关键
事实属治理变更，须修订 ADR-001"。策略写明：拒绝（不是告警）——机器边界内不
允许语义影子条目存在；合法演进路径是 ADR 修订后同步更新锁定集合。负向自测
N12：新增 `run_terminal_state_shadow`（adapters.storage 为第二语义负责人形态）
→ 期望 O4 拒绝。ADR-001 §8 失效信号段补记该治理约定。

**真实复证**（P5，对应验收 A10）：/tmp 副本的 OWNERSHIP_TARGET 注入 shadow 事实
条目后跑修复后校验器 → **退出 1 / O4**
（`critical fact entries outside the locked 02 §3 set: ['run_terminal_state_shadow']`，
inject-p5-shadow-critical-fact-repair-r1.log）。

## 4. 修复后全量重跑（仓库内真实文件，全部实际运行）

| 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|
| `python3 -B docs/rust-tauri/R01/r01_t01_build_ownership.py --check` | 0 | UP_TO_DATE features=736 stores=69（数据文件零改动，无需重生成） | generator-check-repair-r1.log |
| `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py --self-test` | 0 | 正向 O1-O8/D1-D5 全 PASS；负向 N1-N12 全部以匹配规则 ID 拒绝 | check-ownership-positive-and-selftest-repair-r1.log |
| `cargo build --manifest-path rust/Cargo.toml --workspace --offline`（CARGO_TARGET_DIR=/tmp/r01-t01-repair-r1-target） | 0 | 编译通过 | cargo-build-repair-r1.log |
| `cargo test --manifest-path rust/Cargo.toml --workspace --offline` | 0 | 12/12（kernel 7 + protocol 5），无 ignored | cargo-test-repair-r1.log |
| `cargo metadata --format-version 1 --offline` | 0 | 解析图仍仅 lingxi-kernel/lingxi-protocol 两包 | cargo-metadata-repair-r1.json |
| `cargo fmt --all -- --check` | 0 | 无 diff | cargo-fmt-check-repair-r1.log |
| `--emit-negative-fixtures` + N1/N2 夹具校验 | 0 / 1 / 1 | 夹具生成成功；双 owner 与"二者同步"绕过均 O4 拒绝 | emit-negative-fixtures-repair-r1.log、check-ownership-negative-{dual-owner,sync-bypass}-repair-r1.log |

未执行：全量 npm test（范围外；已知预存审计封印失败与本修复无关）、跨平台编译
（归 R01-T03/R09/R10）。未删除或弱化任何既有负向用例（N1-N7 原样保留且仍拒绝），
校验规则只加严未放宽。

## 5. 文件清单与 SHA-256（修复者复算）

修改（3 个，均为 T01 交付物）：

| 文件 | SHA-256 |
|---|---|
| docs/rust-tauri/R01/r01_t01_check_ownership.py | 3c73de7bdab9ea9cfb8bc9682deb37cfc8a1fb392d371e659832b055bcd36728 |
| docs/rust-tauri/R01/ADR-001-ownership.md | 5fd0f4f03736b682c03d584f48c47195f47b01a27378c88f391a59c8c239ef83 |
| docs/rust-tauri/R01/R01-T01_REPORT.md | 7209294051d49f4aeb36385781770969f64b0daa3c1e684836e4d6dfab35f6b8 |

新增（本文件 + 证据日志，均带 repair-r1 标记）：

| 文件 | SHA-256 |
|---|---|
| docs/rust-tauri/R01/R01-T01_REPAIR_R1.md | （自引用，以落盘后复算为准） |
| artifacts/rust-tauri/R01/T01/generator-check-repair-r1.log | 5b62621f993eb9334fce5cb3cd9f0485ef3ecd2d1a06927d61cfff1e739eff55 |
| artifacts/rust-tauri/R01/T01/check-ownership-positive-and-selftest-repair-r1.log | bc9a5d5621ff9fcacf61a6f0e65a1f0596d230d840db428544aa641ec98f4538 |
| artifacts/rust-tauri/R01/T01/cargo-build-repair-r1.log | a048aea99565a826468aca8e73423a10ebfe5917769cb28f02763fa87f5d4ab6 |
| artifacts/rust-tauri/R01/T01/cargo-test-repair-r1.log | 938dfa7ab8d9913a6188de88019d99fbd1b51fda383feb74f4f5064a53a63c6a |
| artifacts/rust-tauri/R01/T01/cargo-metadata-repair-r1.json | 3cf2d853223cdede9ae00f4a29fdb1fc4f9c01be96545178ed3c516f614f1052 |
| artifacts/rust-tauri/R01/T01/cargo-metadata-repair-r1.stderr.log | e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 |
| artifacts/rust-tauri/R01/T01/cargo-fmt-check-repair-r1.log | e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 |
| artifacts/rust-tauri/R01/T01/emit-negative-fixtures-repair-r1.log | 5ad1d4f5da1a62ae0fcf420fe5d75995d5e1c8e59cbae38823fd30151e36e9fb |
| artifacts/rust-tauri/R01/T01/check-ownership-negative-dual-owner-repair-r1.log | 8f73a586ce3587bfb519fa078dac6028a1abab7c412595f792df43c5f2bf8671 |
| artifacts/rust-tauri/R01/T01/check-ownership-negative-sync-bypass-repair-r1.log | 6aebbd9331fc83907933cc5f234e9d99713f757ca75792be9a911b13dff1dec9 |
| artifacts/rust-tauri/R01/T01/inject-p1a-cargo-metadata-depnames-repair-r1.log | 371c72b68f644d2a2518d9f64f3a56fea48b1766053891a74982e89f27bd2c0c |
| artifacts/rust-tauri/R01/T01/inject-p1a-kernel-direct-lingxi-adapters-repair-r1.log | edd90dabf7e1447d7772fea7e63782cccc132931e159dbc41096a6ad39775a8b |
| artifacts/rust-tauri/R01/T01/inject-p1b-planned-module-materialized-repair-r1.log | 211640fe87af8c5f559663b9871dd347874c4c123102dc0f7f3dc4eb8234744c |
| artifacts/rust-tauri/R01/T01/inject-p2-kernel-adapters-tauri-transitive-repair-r1.log | 4ae48046fec85a964ec80de652ede28febaddbedf45ef448c0a24034499418ad |
| artifacts/rust-tauri/R01/T01/inject-p3-forged-member-whitelist-repair-r1.log | 00bf95b995820e93693efbfadc3f8a6017374f4069a5df5d1e01ac3df0b96064 |
| artifacts/rust-tauri/R01/T01/inject-p4-regenerate-repair-r1.log | e70f89a6f8d8903d5285bc8fd967ca446057a0599d338e9c35e8206ba75504d1 |
| artifacts/rust-tauri/R01/T01/inject-p4-authoritative-store-to-host-repair-r1.log | bafb0a6d30ea571fd97f3984b8628349d37773e413df4de37c2761c4d147bd81 |
| artifacts/rust-tauri/R01/T01/inject-p5-shadow-critical-fact-repair-r1.log | 0d370ad170f4e8c5988351e477debba34dd5d1b8e96960d9cd2faaa67b856815 |
| artifacts/rust-tauri/R01/T01/inject-scenarios-driver-repair-r1.log | c8f9b44b5459d245669021d8bf5aa453915249ba7d42c70c30afa306195f50ca |
| artifacts/rust-tauri/R01/T01/inject-p1a-p2-redo3-driver-repair-r1.log | bc97db7666df4209e983f92cd134c866bbe23797f1b10e62b40a56d33c73504e |
| artifacts/rust-tauri/R01/T01/inject-clean-copy-positive-repair-r1.log | a8cdddaf0b5b087bf842400131d629d829b4f3e9519d63492826b1cba5c52302 |

未改动核验（与验收报告 §2 哈希逐一相符）：DEPENDENCY_RULES.json
（001d271c…）、OWNERSHIP_TARGET.json（6dfbdd6c…）、r01_t01_build_ownership.py
（176b0435…）、rust/Cargo.toml（593cf941…）、rust/Cargo.lock（bbbeb538…）、
rust/crates/** 全部源文件。验收报告原文 R01-T01_REVIEW_R1.md 未触碰
（99e8a125… 为修复者复算值，仅作引用）。

## 6. 最终 git status --porcelain

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json
?? artifacts/rust-tauri/R01/
?? docs/rust-tauri/R01/
?? rust/
```

（ORCHESTRATOR_PROGRESS.json 为总控账本预存修改，本修复未触碰；其余三项为
T01 任务树，本修复仅在其中改/增上述文件。未 commit、未 push。）
