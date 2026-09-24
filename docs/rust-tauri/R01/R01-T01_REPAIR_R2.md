# R01-T01｜修复记录 REPAIR R2（对应独立复验 R01-T01_REVIEW_R2.md 的 G1/G2/G3，G4 评估不改）

- 修复者：ZCode:R01-T01-repair-r2（全新修复代理；未参与 T01 执行、R1 验收、R1 修复、R2 复验）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `328cc8bb5a807bdaad520b459907fb1fa4e10dca`（开工实测一致）
- 范围：只改校验器单文件 `docs/rust-tauri/R01/r01_t01_check_ownership.py`（含其内置自测 N13/N14/N15）。
  未改生产代码、任务书目录、.sync-audit、ORCHESTRATOR_PROGRESS.json、既往验收/修复报告原文；
  DEPENDENCY_RULES.json / OWNERSHIP_TARGET.json / ADR-001 / rust crate 全部未动（§6 哈希对照 R2 报告值逐一相符）。
  未 commit/push。
- 环境：macOS 27.0 arm64；Python 3.14.3；cargo 1.93.0。全程离线：所有命令以
  `env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY`
  摘除死代理（127.0.0.1:7890 已死），cargo 一律 `--offline`，`CARGO_TARGET_DIR` 指向 /tmp 隔离目录。
- 结论：**READY_FOR_REVIEW**（PASS/FAIL 归总控另派的全新复验代理判定）。

## 1. G1（中，R2 FAIL 依据）DEP-07 对未注册 workspace 成员不执法

**根因**：`check_dependencies()` 的全部 D 规则以 `rule["applies_to_modules"]` ∩ 注册表
`status=exists` 模块为执法锚点；cargo `workspace_members` 与 `module_registry` 的差集从不被
检查。R1 修复补的 D5 对偶检查只覆盖"已注册但 status=planned 的模块被实体化"一个方向，未覆盖
"完全未注册的成员"。未注册成员被 `cargo build --workspace` 真实编译而 DEP-07 声明的
workspace 级桌面依赖禁令对它不执法——声明执法 ≠ 实际执法。

**修复内容**（校验器）：D5 反向闭合——规则循环之后新增检查：cargo metadata
`workspace_members` 的每个包名必须存在于 `module_registry`，未注册成员即违例（D5）。与既有
D5 双向闭合"注册表 ≡ workspace 现实"。**放置在规则循环之后**是有意为之：既存负向用例 N10 的
场景（伪造 workspace 成员 + kernel 白名单规则）现在同时构成 D5 反向违例与 D2 违例，后置放置
保证 D2 先触发、N10 仍以原匹配规则 ID（D2）拒绝——既有负向用例零改动、零弱化。报错明示
"未注册成员逃逸全部 applies_to keyed 规则而 cargo build --workspace 仍编译它；须登记注册表
并使依赖规则生效"。

**自测**：新增 N13——合成 metadata 注入未注册成员 stealth-crate（依赖 tauri、无任何已注册模块
依赖它）→ 期望且实际以 **D5** 拒绝。

**真实注入复证**（/tmp/r01-t01-repair-r2/g1-stealth 隔离副本，非合成 metadata）：rust/Cargo.toml
members 加入 `crates/stealth-crate`，新建该 crate（`tauri = { path = <外部伪造 crate> }`），不
登记进 module_registry。`cargo build --workspace --offline` 真实编译 stealth-crate 与 tauri
（日志可见 `Compiling tauri` / `Compiling stealth-crate`，桌面栈真实进入 headless workspace 构建，
注入成立）；跑修复后校验器 → **退出 1 / D5**
（`cargo workspace members missing from module_registry: ['stealth-crate']`）。
证据：inject-g1-unregistered-workspace-member-repair-r2.log。

## 2. G2（低）声明但未激活的 optional 禁止依赖逃逸 resolve 图

**根因**：cargo metadata 的 resolve 图只含**已激活**边；`optional = true` 且无 feature 激活的
禁止依赖声明不进入 resolve 图，包 ID 闭包天然看不到它（本轮复证实测：注入后 resolve nodes 仅
lingxi-kernel/lingxi-protocol，packages[] 无 tauri 条目——resolve 图盲是真的）。DEP-02 的契约
措辞是"不能依赖"，manifest 级声明即违约。

**修复内容**（校验器）：对已注册 exists 模块增加 **manifest 级声明扫描**——`packages[]`
（取 workspace 成员包）的 `dependencies[]` 列表涵盖全部声明依赖（含 optional/target-specific），
其 `name` 字段是真实包名（`rename` 只出现在独立的 `rename` 字段，rename 不可伪装声明），逐一
比对 `forbidden_dep_patterns`（双侧 `norm_dep_name()` 归一，与传递闭包检查同策略）。命中即
D1 拒绝，报错明示"声明即违约，即使 optional/未激活、不在 resolve 图"。

**自测**：新增 N14——合成 metadata 中 kernel 的 packages[].dependencies 追加
`tauri（optional=true）`，resolve 图不动 → 期望且实际以 **D1 / DEP-02** 拒绝。

**真实注入复证**（/tmp/r01-t01-repair-r2/g2-optional 隔离副本）：kernel Cargo.toml 真实追加
`tauri = { path = …, optional = true }`；`cargo metadata --offline` 实测 resolve 图仍只有
lingxi-kernel/lingxi-protocol 两节点、packages[] 无 tauri（盲态成立），而 kernel 的 manifest
声明列表含 `('tauri', True)`。跑修复后校验器 → **退出 1 / D1 DEP-02**
（`declares forbidden dependencies ['tauri'] … in its manifest`）。
证据：inject-g2-optional-inactive-tauri-repair-r2.log。

## 3. G3（低）O8 写者前缀匹配，`rust-service-fork` 可过

**根因**：O8 写者判定用 `writer.startswith("rust-service")`，前缀仿冒串
`rust-service-fork (旁路写进程)` 可通过。

**修复内容**（校验器）：写者改为**锁定词表精确匹配**——常量
`RUST_SERVICE_WRITER = "rust-service (lingxi-service 组合根进程，唯一业务数据写者)"`，即生成器
`r01_t01_build_ownership.py` 中 `RUST_SERVICE` 当前发出的唯一规范串（当前 42 项 authoritative
store 全部使用该串，逐行实测核验）。authoritative store 的 writer 不等于该串即 O8 拒绝；改规范
串属生成器+数据变更。与 ADR-001 的"target_writer_process 必须为 rust-service，由校验器 O8
机械执法"表述一致（精确匹配是"必须为 rust-service"的严格化，方向一致，ADR 无需改）。

**自测**：新增 N15——session-jsonl 保持 authority 类 owner（O8 第一半通过），writer 改
`rust-service-fork (旁路写进程)` → 期望且实际以 **O8** 拒绝。

**真实注入复证**（/tmp/r01-t01-repair-r2/g3-writer-fork 隔离副本）：副本生成器的 `RUST_SERVICE`
常量改为 `rust-service-fork (旁路写进程)` 并重生成 OWNERSHIP_TARGET.json（生成器退出 0，执法
在校验器；owner 类保持 authority 不变，O7-drift 实测 PASS 故 O8 是唯一可能失败项）。跑修复后
校验器 → **退出 1 / O8**（`authoritative store agent-authored-records has writer
'rust-service-fork (旁路写进程)' … exact match, not a prefix`）。
证据：inject-g3-rust-service-fork-writer-repair-r2.log。

## 4. G4（提示）归一化子串匹配边界——评估结论：不改

- 误杀方向成立（`taos` 被子串模式 `tao` 命中而误拒），但**不存在误放方向**：归一化后子串匹配
  是 fail-safe——任何包含桌面栈包名的形态（`tauri-runtime-wry`、`webkit2gtk-sys`、大小写/连字符
  /下划线/rename 变体）只会被更多命中，不会被放行。
- 当前 workspace **零第三方依赖**（cargo metadata 实测仅两包），无现实误杀对象；收紧为段级
  精确匹配需要重新证明对 `tauri-*`/`*-sys` 等真实桌面栈包名族的覆盖，在无误杀案例时改动匹配
  语义只会引入"收紧过度漏检"风险而无收益。
- 与 R2 报告一致（"本项不要求修复"）。R02 引入第三方依赖后若出现真实误杀，再改段级匹配并补
  对应自测。
- 附带说明：G2 新增的 manifest 级声明扫描复用同一套归一化子串匹配，误杀方向与 resolve 图检查
  完全一致（fail-safe），未引入新的误放面。

## 5. 修复后全量重跑（仓库内真实文件 + /tmp 隔离注入，全部实际运行）

仓库内（cwd=仓库根，代理环境变量摘除，cargo 全程 --offline，
CARGO_TARGET_DIR=/tmp/r01-t01-repair-r2-target）：

| 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|
| `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py --self-test` | 0 | 正向 O1-O8、O7-drift、D5/D5-reverse、DEP-01/02/03/04/06/07 全 PASS；负向 **N1-N15 全部以匹配规则 ID 拒绝**（N1/O4、N2/O4、N3/O1、N4/O6、N5/O5、N6/D1、N7/D3、N8/D1、N9/D1、N10/D2、N11/O8、N12/O4、**N13/D5、N14/D1、N15/O8**）；既有 N1-N12 原样保留、拒绝规则 ID 与 R1/R2 记录一致 | check-ownership-positive-and-selftest-repair-r2.log |
| `python3 -B docs/rust-tauri/R01/r01_t01_build_ownership.py --check` | 0 | `OWNERSHIP_TARGET_UP_TO_DATE features=736 stores=69`（数据文件零改动，无需重生成） | generator-check-repair-r2.log |
| `cargo build --manifest-path rust/Cargo.toml --workspace --offline` | 0 | 两 crate 编译通过 | cargo-build-repair-r2.log |
| `cargo test --manifest-path rust/Cargo.toml --workspace --offline` | 0 | 12/12（kernel 7 + protocol 5），0 ignored | cargo-test-repair-r2.log |
| `cargo metadata --format-version 1 --offline` | 0 | 解析图仍仅 lingxi-kernel/lingxi-protocol 两包，无 tauri/electron/tao/wry/webkit2gtk/winit/webview | cargo-metadata-repair-r2.json |
| `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | 无 diff | cargo-fmt-check-repair-r2.log |
| `--emit-negative-fixtures` + N1/N2 夹具校验 | 0 / 1 / 1 | 夹具生成成功；双 owner 与"二者同步"绕过均 O4 拒绝 | emit-negative-fixtures-repair-r2.log、check-ownership-negative-{n1-dual-owner,n2-sync-bypass}-repair-r2.log |

/tmp 隔离副本真实注入复证（rust/ + docs/rust-tauri/{R00,R01} 复制到
/tmp/r01-t01-repair-r2，伪造 crate 自带 `[workspace]` 保持外部路径依赖形态）：

| 场景 | 注入方式 | 期望 | 实际退出码/规则 | 证据 |
|---|---|---|---|---|
| G1 未注册 workspace 成员 | members 加 stealth-crate（依赖 tauri），不登记注册表；`cargo build --workspace` 实测真实编译 stealth-crate+tauri | 拒绝 | **1 / D5**（`members missing from module_registry: ['stealth-crate']`） | inject-g1-unregistered-workspace-member-repair-r2.log |
| G2 未激活 optional 禁止依赖 | kernel 声明 `tauri = {…, optional=true}`，无 feature 激活；metadata 实测 resolve 图无 tauri | 拒绝 | **1 / D1 DEP-02**（manifest 声明扫描命中） | inject-g2-optional-inactive-tauri-repair-r2.log |
| G3 写者前缀仿冒 | 副本生成器规范串改 `rust-service-fork (旁路写进程)` 并重生成（退出 0，O7-drift PASS） | 拒绝 | **1 / O8**（exact match, not a prefix） | inject-g3-rust-service-fork-writer-repair-r2.log |
| 干净副本对照 | 无注入，副本跑 `--self-test` | 通过 | **0 / RESULT: OK**（N1-N15 全拒绝） | inject-clean-copy-positive-repair-r2.log |

未执行：全量 npm test（范围外；已知预存审计封印失败与本修复无关）、跨平台编译
（归 R01-T03/R09/R10）。未删除或弱化任何既有负向用例；校验规则只加严未放宽。

## 6. 文件清单与 SHA-256（修复者复算）

修改（1 个，T01 交付物）：

| 文件 | SHA-256 |
|---|---|
| docs/rust-tauri/R01/r01_t01_check_ownership.py | 873094a097610bda0c8b118ad7c426143f4f63e774fe6aa9fe8067dac4116cbf |

新增（本文件 + 证据日志，均带 repair-r2 标记）：

| 文件 | SHA-256 |
|---|---|
| docs/rust-tauri/R01/R01-T01_REPAIR_R2.md | （自引用，以落盘后复算为准） |
| artifacts/rust-tauri/R01/T01/check-ownership-positive-and-selftest-repair-r2.log | 2bb5935f36f22c5892cba666104e44fbec62da73e9e2e451270c0391c98ebf49 |
| artifacts/rust-tauri/R01/T01/generator-check-repair-r2.log | 5b62621f993eb9334fce5cb3cd9f0485ef3ecd2d1a06927d61cfff1e739eff55 |
| artifacts/rust-tauri/R01/T01/cargo-build-repair-r2.log | c5c421d15bf2c2900254f08daae9998996a73b12cca43d06590570af09b9a25a |
| artifacts/rust-tauri/R01/T01/cargo-test-repair-r2.log | 35bd60b4df8378b867b4e3bb36e62af4a039cd183b3d8c6c24c425332af2dc23 |
| artifacts/rust-tauri/R01/T01/cargo-metadata-repair-r2.json | 0d7779c43167e045b9cd91b884eb4281b9ad2e72e5da65acc0b8e0e87dc2396e |
| artifacts/rust-tauri/R01/T01/cargo-metadata-repair-r2.stderr.log | e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 |
| artifacts/rust-tauri/R01/T01/cargo-fmt-check-repair-r2.log | e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 |
| artifacts/rust-tauri/R01/T01/emit-negative-fixtures-repair-r2.log | 991a7cb2acf7744ed2cfa2d9981898ad89b2506a0b9e8fee20d5cfc89551b479 |
| artifacts/rust-tauri/R01/T01/check-ownership-negative-n1-dual-owner-repair-r2.log | 8f73a586ce3587bfb519fa078dac6028a1abab7c412595f792df43c5f2bf8671 |
| artifacts/rust-tauri/R01/T01/check-ownership-negative-n2-sync-bypass-repair-r2.log | 6aebbd9331fc83907933cc5f234e9d99713f757ca75792be9a911b13dff1dec9 |
| artifacts/rust-tauri/R01/T01/inject-g1-unregistered-workspace-member-repair-r2.log | 74967bab71b2f40e679787d5435ff78d909e1b8fead0a8f20be1ccf6c7b897a6 |
| artifacts/rust-tauri/R01/T01/inject-g2-optional-inactive-tauri-repair-r2.log | 62bf08a7aa1129d963cc9a5f21cd9e88d2d018dba390b705103058a30bb66843 |
| artifacts/rust-tauri/R01/T01/inject-g3-rust-service-fork-writer-repair-r2.log | b82ae2d03d5403816fc404b14ad770246c97fd119aac51726a5b3359dfddef0b |
| artifacts/rust-tauri/R01/T01/inject-clean-copy-positive-repair-r2.log | e97c5aa4304ace148e34459cfe01c144d180f7e1bf92d95710a9152e0e6a4b0d |

未改动核验（与 R2 复验报告 §2 哈希逐一相符）：DEPENDENCY_RULES.json
（001d271c…）、OWNERSHIP_TARGET.json（6dfbdd6c…）、r01_t01_build_ownership.py
（176b0435…）、ADR-001-ownership.md（5fd0f4f0…）、R01-T01_REPORT.md（72092940…）、
R01-T01_REVIEW_R1.md（99e8a125…）、R01-T01_REPAIR_R1.md（da816e4e…）、
rust/Cargo.toml（593cf941…）、rust/Cargo.lock（bbbeb538…）、rust/crates/** 全部源文件。
R2 复验报告原文 R01-T01_REVIEW_R2.md 未触碰（修复者复算
9a4575c18823d9d6fd367750a0cf4b9172694a3274f87d716d12036b4ee9b785）。

## 7. 最终 git status --porcelain

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json
?? artifacts/rust-tauri/R01/
?? docs/rust-tauri/R01/
?? rust/
```

（ORCHESTRATOR_PROGRESS.json 为总控账本预存修改，本修复未触碰；
`git diff HEAD --stat -- desktop server core lib shared cli hub plugins skills2set
package.json package-lock.json` 为空，生产零改动。未 commit、未 push。）
