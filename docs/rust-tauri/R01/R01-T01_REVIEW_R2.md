# R01-T01｜确定模块和进程所有权 — 独立复验报告 R2（修复后候选）

- 复验者：ZCode:R01-T01-review-r2（全新独立复验代理；未参与 T01 执行、R1 验收、R1 修复；
  只读审查 + /tmp 隔离复跑，未修改任何已提交文件、生产代码、执行/修复交付物；本报告是
  唯一新建仓库内文件）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `328cc8bb5a807bdaad520b459907fb1fa4e10dca`
  （开工 `git rev-parse HEAD` 实测一致）
- **最终判定：FAIL**（R1 三发现 F1/F2/F3 经独立注入复证**全部真实关闭**，两个 REQUIRED
  场景与全部正向复跑通过；但本轮新对抗面发现 G1：DEP-07 声明的"全 headless workspace
  禁桌面依赖"对**未注册 workspace 成员**不执法——真实注入桌面栈后校验器仍 RESULT: OK，
  与 R1 判 FAIL 的"声明执法 ≠ 实际执法"同类。修复范围小且同根因，见 §6）

## 1. 工作区事实核验（实际运行）

`git status --porcelain` / `git diff --stat HEAD`（2026-09-25 复测）：

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json   （总控账本，既存修改，非候选，未触碰）
?? artifacts/rust-tauri/R01/
?? docs/rust-tauri/R01/
?? rust/
```

- `git diff HEAD --stat -- desktop server core lib shared cli hub plugins skills2set
  package.json package-lock.json`：**空**（生产零改动成立，本任务树外无 diff）。
- 与基线事实声明完全一致，无额外改动。

## 2. 候选清单与复算哈希（SHA-256，复验者独立复算）

| 文件 | SHA-256 | 与修复报告 §5 对照 |
|---|---|---|
| docs/rust-tauri/R01/ADR-001-ownership.md | 5fd0f4f03736b682c03d584f48c47195f47b01a27378c88f391a59c8c239ef83 | 一致（修复改动件） |
| docs/rust-tauri/R01/DEPENDENCY_RULES.json | 001d271c31e70837d09d13ae7b18e473714df79b95802e524e3caea1521b27f3 | 一致（=R1 值，未改） |
| docs/rust-tauri/R01/OWNERSHIP_TARGET.json | 6dfbdd6c7f6c1c705e156a6b043b7c137003f7ce3a6ad1d6ffa1e3c80bd9e381 | 一致（=R1 值，未改） |
| docs/rust-tauri/R01/r01_t01_build_ownership.py | 176b043582a7a31ff1e92346f4cbf858a9204a05379e67d490883726dd46c173 | 一致（=R1 值，未改） |
| docs/rust-tauri/R01/r01_t01_check_ownership.py | 3c73de7bdab9ea9cfb8bc9682deb37cfc8a1fb392d371e659832b055bcd36728 | 一致（修复改动件） |
| docs/rust-tauri/R01/R01-T01_REPORT.md | 7209294051d49f4aeb36385781770969f64b0daa3c1e684836e4d6dfab35f6b8 | 一致（修复改动件） |
| docs/rust-tauri/R01/R01-T01_REVIEW_R1.md | 99e8a125dffae35e2f10ffd35bd1172cf0b2faada1e9f11e934da6aa706865a4 | 一致（原文未触碰） |
| docs/rust-tauri/R01/R01-T01_REPAIR_R1.md | da816e4e03797cb21b497e00960c802945f41be66728e9fe47c415d50a816e29 | 落盘值 |
| rust/Cargo.toml | 593cf9414e42417f5f0e66ddc3524e0fdbbe07d0e03d37935c876078054b9a82 | 一致（=R1 值） |
| rust/Cargo.lock | bbbeb538501e4a20db33c8d6974835fb32814dc642064c233be0a5f3625a436a | 一致（=R1 值） |
| rust/crates/lingxi-protocol/Cargo.toml | 940c4ac147368d95816e982d7149673daa43dd27af62590ffbdaa5f96fc7ac2e | 一致（=R1 值） |
| rust/crates/lingxi-protocol/src/lib.rs | 817c0cff8559089c94c06368bebd49e859983bb3b06d8a5dacccdad092e658a7 | 一致（=R1 值） |
| rust/crates/lingxi-kernel/Cargo.toml | 604e27758d4b9e07d52e6526bc8adc2e0eaaed71fa6128857fb41956a64a33ca | 一致（=R1 值） |
| rust/crates/lingxi-kernel/src/lib.rs | cd5e89d09894352b9b9ae0a85003a5f23bc949f6a9668a0cdc0b94e0d972213e | 一致（=R1 值） |
| rust/crates/lingxi-kernel/src/ports.rs | fb71a21263749f2a59baf83e667bdd8b14ecd362b78f53f6db0aad16c637172b | 一致（=R1 值） |

R00 输入锚定：OWNERSHIP_TARGET.json `source_inputs` 记录的 sha256 与当前
`docs/rust-tauri/R00/FEATURE_INVENTORY.json`（48141268…）/ `STORES.json`
（6067bdd3…）实测哈希逐一相符。修复报告 §5 列示的证据日志抽查 3 个
（check-ownership-positive-and-selftest-repair-r1.log、
inject-p2-kernel-adapters-tauri-transitive-repair-r1.log、
generator-check-repair-r1.log）哈希与其声明一致。

## 3. 正向全量独立复跑（隔离目录 /tmp/r01-t01-review-r2，全部真实重跑，未采用修复者日志）

方法：`rust/`、`docs/rust-tauri/R00`、`docs/rust-tauri/R01` 复制到
`/tmp/r01-t01-review-r2/repo`，`CARGO_TARGET_DIR=/tmp/r01-t01-review-r2/target`，
全部代理环境变量以 `env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY
-u https_proxy -u HTTPS_PROXY` 摘除，cargo 全程 `--offline`。

| 复跑命令 | 退出码 | 结果 |
|---|---|---|
| `cargo build --manifest-path rust/Cargo.toml --workspace --offline` | 0 | 两 crate 真实编译通过 |
| `cargo test --manifest-path rust/Cargo.toml --workspace --offline` | 0 | 12/12（kernel 7 + protocol 5），0 ignored |
| `cargo metadata --format-version 1 --offline` | 0 | 解析图仅 lingxi-kernel/lingxi-protocol 两包；无 tauri/electron/tao/wry/webkit2gtk/winit/webview 包名 |
| `python3 -B r01_t01_check_ownership.py --self-test` | 0 | 正向 O1-O8、O7-drift、D5、DEP-01/02/03/04/06/07（存在模块部分）全 PASS；负向 **N1-N12 全部以匹配规则 ID 拒绝**（N1/O4、N2/O4、N3/O1、N4/O6、N5/O5、N6/D1、N7/D3、N8/D1·DEP-03、N9/D1·DEP-02、N10/D2、N11/O8、N12/O4） |
| `python3 -B r01_t01_build_ownership.py --check` | 0 | `OWNERSHIP_TARGET_UP_TO_DATE features=736 stores=69` |
| `--emit-negative-fixtures` + N1/N2 夹具校验 | 0 / 1 / 1 | 双 owner → O4 拒绝；"二者同步"绕过 → O4 拒绝 |
| `cargo fmt --all -- --check` | 0 | 无 diff |

R01-A01（核心不依赖桌面）与 R01-A02（双负责人被检出）两个 REQUIRED 场景证据
独立复跑**通过**。既有 N1-N7 未被弱化（仍全部以原匹配规则 ID 拒绝）。

## 4. R1 三发现逐项关闭判定（/tmp 隔离副本真实注入，全部实际运行）

每个场景独立复制干净副本后注入；伪造 crate（lingxi-adapters/tauri/forged-internal）
带自有 `[workspace]` 表保持外部路径依赖形态。

| 场景（对应 R1 反例） | 注入方式 | 期望 | 实际退出码/规则 | 判定 |
|---|---|---|---|---|
| (a1)（B1 外部路径形态） | kernel Cargo.toml 加 `lingxi-adapters = { path = <外部绝对路径> }` | 拒绝 | **1 / D1 DEP-03**（`forbidden ['lingxi-adapters']`） | 关闭 |
| (a2)（B1 工作区成员形态） | crate 建于 `rust/crates/lingxi-adapters` + 加入 members + kernel path 依赖 | 拒绝 | **1 / D5**（planned 模块实体化即违例，先于 D1；仍为明确拒绝） | 关闭 |
| (b)（B2 传递形态） | kernel→lingxi-adapters（外部）→tauri（外部） | 拒绝 | **1 / D1 DEP-02**（传递闭包穿透连字符包检出 tauri） | 关闭 |
| (c)（B3 白名单绕过） | 伪造成员 forged-internal + kernel 依赖 + 副本规则加 `allowed_module_deps:["lingxi-protocol"]` | 拒绝 | **1 / D2**（`disallowed workspace modules ['forged-internal']`） | 关闭 |
| (d)（A9） | 生成器闭表改 `session-jsonl → tauri-host.desktop-host + TAURI_HOST`，重生成（退出 0）再校验 | 拒绝 | **1 / O8** | 关闭 |
| (e)（A10） | OWNERSHIP_TARGET 注入 `run_terminal_state_shadow`（owner=adapters.storage） | 拒绝 | **1 / O4**（`outside the locked 02 §3 set`） | 关闭 |

根因复核：场景 (b) 的真实 cargo metadata 实测 `deps[].name =
['lingxi_adapters','lingxi_protocol']` 而 `packages[].name` 保留连字符——F1 的
命名归一根因属实；修复版以 `deps[].pkg` 包 ID 建邻接求闭包、模式匹配双侧
`norm_dep_name()` 归一，实测对上述全部形态执法。

**F1（高）关闭、F2（低）关闭、F3（低）关闭。** 修复报告对修复内容与复证结果的
声明与本轮独立复跑一致，无夸大。

## 5. 新对抗面复验（修复是否引入/遗留新漏洞；全部实际运行）

| 探针 | 注入方式 | 实际退出码/规则 | 结论 |
|---|---|---|---|
| F-rename：rename 绕过 | kernel 加 `innocent-util = { package = "tauri", path = … }` | **1 / D1 DEP-02**（包 ID 建图经 `name_of[pkg]` 还原真实包名，rename 不可绕过） | 安全 |
| F-target：target 特定依赖 | kernel 加 `[target.'cfg(windows)'.dependencies] tauri = …` | **1 / D1 DEP-02** | 安全 |
| F-devdep：dev 依赖 | kernel 加 `[dev-dependencies] tauri = …` | **1 / D1 DEP-02** | 安全 |
| F-optional-on：已激活 optional | kernel 加 `tauri = {…, optional=true}` + `default=["tauri"]` | **1 / D1 DEP-02** | 安全 |
| G2：未激活 optional（见 §6） | kernel 加 `tauri = {…, optional=true}`，无任何 feature 激活 | **0 / RESULT: OK** | **漏洞（低）** |
| G1：未注册 workspace 成员（见 §6） | workspace members 加 `stealth-crate`（不在 module_registry）且其依赖 tauri；无任何已注册模块依赖它 | **0 / RESULT: OK**；`cargo build --workspace` 真实编译 stealth-crate | **漏洞（中）** |
| G1 对照：可达未注册成员 | 同上但 kernel 依赖 stealth-crate | **1 / D1 DEP-02**（闭包经包 ID 覆盖未注册节点） | 安全 |
| G3：O8 写者前缀（见 §6） | authoritative store owner 改 service.auth（authority 类）+ writer `rust-service-fork (旁路写进程)` | **0 / RESULT: OK** | **漏洞（低）** |
| G4：归一过度匹配 | kernel 加良性 crate `taos`（含子串 "tao"） | 1 / D1 DEP-02（`forbidden ['taos'] (pattern 'tao')`） | 误杀方向（fail-safe），未见误放 |
| D5/D1 优先级 | 场景 (a2)：planned 模块实体化 + kernel 依赖 | 1 / D5 先行拒绝；无放行路径 | 安全 |

## 6. 新发现问题

### G1（中，判 FAIL 依据）DEP-07 的"全 headless workspace 禁桌面依赖"对未注册 workspace 成员不执法

- **最小重现**（/tmp 隔离副本，已验证）：在 `rust/Cargo.toml` members 加入
  `crates/stealth-crate`，新建该 crate（`Cargo.toml` 声明
  `tauri = { path = … }`），不登记进 DEPENDENCY_RULES.json `module_registry`，
  也不让任何已注册模块依赖它。跑 `r01_t01_check_ownership.py` → **退出 0，
  RESULT: OK**；同时 `cargo build --workspace --offline` 真实编译
  stealth-crate（桌面栈进入 headless workspace 的构建）。
- **根因**：`check_dependencies()` 的全部 D 规则以
  `rule["applies_to_modules"]` ∩ 注册表 `status=exists` 模块为执法锚点；cargo
  `workspace_members` 与 `module_registry` 的差集从不被检查。修复版的 D5 对偶
  检查只覆盖"已注册但 status=planned 的模块被实体化"一个方向，未覆盖"完全未
  注册的成员"。DEP-07 的规则名即
  `no-desktop-dependency-anywhere-in-headless-workspace`、rationale 声明
  "headless workspace 全程无桌面依赖"，其 declared coverage 是整个 workspace，
  实际执法只有已注册模块——与 R1 F1 同类（声明执法 ≠ 实际执法），且该门禁将
  作为常驻门禁传入 R02（R02 恰好开始新增 crate，缺口会在需要它时生效）。
- **与 F1 的差异**（如实标注）：不削弱对已注册模块的执法（可达未注册成员会被
  传递闭包捕获，见 §5 G1 对照）；攻击需新增 crate + 改 rust/Cargo.toml，diff
  可见；属修复前即存在的历史缺口，非本次修复引入的回归。严重度定为中。
- **同根因完整修复范围**：
  1. 校验器新增检查（扩展 D5 或新增 D6）：cargo metadata `workspace_members`
     的每个包名必须存在于 `module_registry`；未注册成员即违例（反向真子集
     检查，与 D5 双向闭合"注册表 ≡ workspace 现实"）。
  2. 负向自测新增 N13：合成 metadata 注入未注册成员 → 期望该检查拒绝，
     断言匹配规则 ID。
  3. 用本节真实注入法（非合成 metadata）在 /tmp 复证退出 1。

### G2（低）声明但未激活的 optional 禁止依赖逃逸

- 重现：kernel 声明 `tauri = { path = …, optional = true }` 且无任何 feature
  激活 → 退出 0。根因：cargo metadata resolve 图只含已激活可选依赖，包 ID
  闭包天然看不到未激活边。一旦被任何下游 feature 激活即进入 resolve 图并被
  D1 捕获（§5 F-optional-on 实测），故风险窗口限于"潜伏声明"阶段；但 DEP-02
  的契约措辞是"不能依赖"，manifest 级声明即违约。
- 修复（与 G1 同根因：执法只覆盖 cargo 现实的子集）：对已注册 exists 模块
  增加 manifest 级声明扫描——`packages[].dependencies[]`（其 `name` 为真实
  包名、rename 不可伪装，且涵盖 optional/target-specific 声明）比对
  `forbidden_dep_patterns`（双侧归一）；新增 N14 负向自测。

### G3（低）O8 写者校验为前缀匹配，`rust-service-fork` 可过

- 重现：authoritative store 保持 authority 类 owner（O8 第一半通过），writer
  填 `rust-service-fork (旁路写进程)` → 退出 0。根因：
  `writer.startswith("rust-service")`。writer 字段本身是描述性文本，O8 的
  实际牙齿是 owner-kind 检查，故定低。
- 修复：writer 改为锁定词表精确匹配（取当前数据真实使用的规范串
  `rust-service (lingxi-service 组合根进程，唯一业务数据写者)` 或其规范化
  形式），新增 N15 负向自测。

### G4（提示，非阻塞）归一化子串匹配只存在误杀方向

- 实测：良性 crate `taos` 被模式 `tao` 子串命中而误拒（fail-safe）；未发现
  反向误放（大小写/连字符/下划线/rename 形态均被覆盖）。当前 workspace 零
  第三方依赖，无现实误杀；R02 引入第三方依赖后若遇误杀可再改段级精确匹配。
  本项不要求修复。

## 7. 一致性与数据核对（实际运行 + 源码确证）

- **736+69 覆盖独立复算**（自写脚本直比 R00 原始清单）：FEATURE_INVENTORY 736
  唯一 F-ID，feature_ownership 736 行，缺失 0 / 多余 0 / 重复 0；STORES 69
  唯一 store id，store_ownership 69 行，缺失 0 / 多余 0 / 重复 0；worker/ui
  类 owner 拥有 F-ID 数=0、store 数=0（worker 禁令数据面成立）；authoritative
  store 42 项全部 core/service/adapters 类 owner 且 writer 均 rust-service 前缀
  （当前真实数据无 G3 形态）；writer 分布 63 rust-service / 5 tauri-host /
  1 build-release，与 ADR §5 声明一致。
- **ADR-001 ↔ DEPENDENCY_RULES.json ↔ 校验器三者一致**：ADR §5 记载 O8 执法
  约定与校验器 O8 实现一致；ADR §8 记载关键事实 11 条锁定精确集合（O4 拒绝
  词表外条目、治理变更须修订 ADR）与校验器 `LOCKED_CRITICAL_FACTS` 一致；
  ADR §8 记载包 ID 建闭包 + 双侧归一 + D5 对偶检查，与校验器实现一致。
  DEPENDENCY_RULES.json 与 R1 验收时逐字节相同（哈希 001d271c… 未变）。
- **T01 报告 §7 更正段与事实一致**：其"DEP-03/D2 当时未被真实执法、修复后改
  包 ID 建图/双侧归一/D5 对偶/O8/锁定集合/N8-N12"的陈述与本轮独立复跑结果
  全部相符。
- 修复者未触碰验收报告原文（R1 报告哈希 99e8a125… 与修复报告引用值一致）。
- 已知预存失败（4 个审计封印 FAIL、r00 校验器 STALE 分类）与本任务无关，
  未运行、不据此判定。

## 8. 结论类型标注

- **实际运行**：§1、§3、§4、§5、§6、§7 数据复算全部命令与注入（本机 macOS
  27.0 arm64，/tmp/r01-t01-review-r2 隔离目录，真实退出码如上）。
- **源码确证**：校验器/生成器逻辑审查（闭表未知标签即 SystemExit、负向电池
  要求匹配规则 ID、无永真断言）；ADR 与规则文件一致性比对。
- **受环境限制**：跨平台编译（macOS x64/Windows/Linux）未验证（任务书归
  R01-T03/R09/R10）；未跑全量 npm test（范围外）。

## 9. 最终判定

**FAIL**（窄口径）。

- R1 三发现 F1/F2/F3：经独立真实注入复证**全部关闭**；修复报告声明属实。
- R01-A01 / R01-A02 及全部正向（构建 0、测试 12/12、metadata 2 包无桌面栈、
  O1-O8/D1-D5、N1-N12、生成器 --check、fmt）：**通过**。
- 但本轮新发现 G1（中）：DEP-07 声明的 workspace 级桌面依赖禁令对未注册
  workspace 成员不执法——真实注入后门禁仍 RESULT: OK，与 R1 判 FAIL 的
  "声明执法 ≠ 实际执法"同类，且该门禁将作为常驻门禁传入正要开始新增 crate
  的 R02。按与 R1 一致的判定标准，候选不能带病交接。
- 修复范围（同根因：执法只覆盖 cargo 现实的子集——resolve 图激活边 ∩ 已注册
  模块）：仅校验器单文件——(1) workspace_members ⊆ module_registry 反向检查
  + N13；(2) 已注册模块 manifest 级声明依赖扫描（`packages[].dependencies`）
  + N14；(3) O8 writer 改锁定词表精确匹配 + N15；(4) 三项真实注入 /tmp 复证。
  OWNERSHIP_TARGET/ADR/rust crate 数据与实现无需改动（G4 可选记录，不阻塞）。
- 修复量预估小（校验器 ~25 行 + 3 条负向自测 + 复跑证据）。
