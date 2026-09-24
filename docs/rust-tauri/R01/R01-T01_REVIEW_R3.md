# R01-T01｜确定模块和进程所有权 — 独立复验报告 R3（R2 修复后候选）

- 复验者：ZCode:R01-T01-review-r3（全新独立复验代理；未参与 T01 执行、R1 验收/修复、R2 复验/修复；
  只读审查 + /tmp 隔离复跑，未修改任何已提交文件、生产代码、执行/修复交付物；本报告是唯一新建仓库内文件）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `328cc8bb5a807bdaad520b459907fb1fa4e10dca`（开工实测一致）
- 环境：macOS 27.0 arm64；Python 3.14.3；cargo 1.93.0；全程 `env -u all_proxy -u ALL_PROXY
  -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY` 摘除死代理，cargo 一律 `--offline`，
  `CARGO_TARGET_DIR=/tmp/r01-t01-review-r3/target*`
- **最终判定：PASS**（R2 三发现 G1/G2/G3 经 /tmp 真实注入独立复证全部关闭；G4 不改理由成立；
  N1–N12 无弱化；正向全量通过；未发现新的放行路径）

## 1. 工作区事实核验（实际运行）

`git status --porcelain`（2026-09-25 复测）：

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json   （总控账本，既存修改，非候选，未触碰）
?? artifacts/rust-tauri/R01/
?? docs/rust-tauri/R01/
?? rust/
```

`git diff HEAD --stat -- desktop server core lib shared cli hub plugins skills2set
package.json package-lock.json`：**空**（生产零改动成立）。与基线声明一致，无额外改动。

## 2. 候选清单与复算哈希（SHA-256，本代理独立复算）

| 文件 | SHA-256（复算） | 对照 |
|---|---|---|
| docs/rust-tauri/R01/r01_t01_check_ownership.py | 873094a097610bda0c8b118ad7c426143f4f63e774fe6aa9fe8067dac4116cbf | =REPAIR_R2 §6 声明（唯一修复改动件） |
| docs/rust-tauri/R01/DEPENDENCY_RULES.json | 001d271c31e70837d09d13ae7b18e473714df79b95802e524e3caea1521b27f3 | =R1/R2 值，未改 |
| docs/rust-tauri/R01/OWNERSHIP_TARGET.json | 6dfbdd6c7f6c1c705e156a6b043b7c137003f7ce3a6ad1d6ffa1e3c80bd9e381 | =R1/R2 值，未改 |
| docs/rust-tauri/R01/r01_t01_build_ownership.py | 176b043582a7a31ff1e92346f4cbf858a9204a05379e67d490883726dd46c173 | =R1/R2 值，未改 |
| docs/rust-tauri/R01/ADR-001-ownership.md | 5fd0f4f03736b682c03d584f48c47195f47b01a27378c88f391a59c8c239ef83 | =R2 值，未改 |
| docs/rust-tauri/R01/R01-T01_REPORT.md | 7209294051d49f4aeb36385781770969f64b0daa3c1e684836e4d6dfab35f6b8 | =R2 值，未改 |
| docs/rust-tauri/R01/R01-T01_REVIEW_R1.md | 99e8a125dffae35e2f10ffd35bd1172cf0b2faada1e9f11e934da6aa706865a4 | 原文未触碰 |
| docs/rust-tauri/R01/R01-T01_REPAIR_R1.md | da816e4e03797cb21b497e00960c802945f41be66728e9fe47c415d50a816e29 | 原文未触碰 |
| docs/rust-tauri/R01/R01-T01_REVIEW_R2.md | 9a4575c18823d9d6fd367750a0cf4b9172694a3274f87d716d12036b4ee9b785 | =REPAIR_R2 §6 声明，原文未触碰 |
| rust/Cargo.toml | 593cf9414e42417f5f0e66ddc3524e0fdbbe07d0e03d37935c876078054b9a82 | =R1/R2 值 |
| rust/Cargo.lock | bbbeb538501e4a20db33c8d6974835fb32814dc642064c233be0a5f3625a436a | =R1/R2 值 |
| rust/crates/lingxi-protocol/{Cargo.toml,src/lib.rs} | 940c4ac1… / 817c0cff… | =R1/R2 值 |
| rust/crates/lingxi-kernel/{Cargo.toml,src/lib.rs,src/ports.rs} | 604e2775… / cd5e89d0… / fb71a212… | =R1/R2 值 |

REPAIR_R2 证据日志抽查 4 个（inject-g1/g2/g3-repair-r2.log、
check-ownership-positive-and-selftest-repair-r2.log）复算哈希与其 §6 声明逐一相符
（74967bab… / 62bf08a7… / b82ae2d0… / 2bb5935f…）。R00 输入锚定：
OWNERSHIP_TARGET.json `source_inputs` 记录值与当前 FEATURE_INVENTORY.json
（48141268…）/ STORES.json（6067bdd3…）实测一致。

## 3. 正向全量独立复跑（/tmp/r01-t01-review-r3/repo 隔离副本，全部真实重跑）

方法：`rust/` + `docs/rust-tauri/{R00,R01}` 复制到隔离目录，未采用修复者日志。

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo build --manifest-path rust/Cargo.toml --workspace --offline` | 0 | 两 crate 真实编译（Compiling lingxi-protocol/lingxi-kernel） |
| `cargo test --workspace --offline` | 0 | 12/12（kernel 7 + protocol 5），0 ignored |
| `cargo metadata --format-version 1 --offline` | 0 | workspace 成员与全图包均仅 lingxi-kernel/lingxi-protocol；无 tauri/electron/tao/wry/webkit2gtk/winit/webview |
| `cargo fmt --all -- --check` | 0 | 无 diff |
| `python3 -B r01_t01_check_ownership.py --self-test` | 0 | 正向 O1-O8、O7-drift、D5、DEP-01/02/03/04/06/07、D5-reverse 全 PASS；负向 **N1-N15 全部以匹配规则 ID 拒绝**（N1/O4、N2/O4、N3/O1、N4/O6、N5/O5、N6/D1、N7/D3、N8/D1、N9/D1、N10/D2、N11/O8、N12/O4、**N13/D5、N14/D1、N15/O8**），RESULT: OK |
| `python3 -B r01_t01_build_ownership.py --check` | 0 | `OWNERSHIP_TARGET_UP_TO_DATE features=736 stores=69` |
| `--emit-negative-fixtures` + N1/N2 夹具 | 0 / 1 / 1 | 双 owner、二者同步绕过均被拒 |

R01-A01（无桌面依赖编译核心）与 R01-A02（双 owner 被检出）两个 REQUIRED 场景独立复跑**通过**。

## 4. G1/G2/G3 关闭复证（/tmp 真实注入，不信修复报告；全部实际运行）

| 场景 | 注入方式（独立副本） | 期望 | 实际退出码/规则 | 判定 |
|---|---|---|---|---|
| G1 未注册 workspace 成员 | members 加 `crates/stealth-crate`（外部伪造 `tauri` 路径依赖，自带 `[workspace]`），不登记注册表 | 拒绝 | 先证注入成立：`cargo build --workspace --offline` 日志真实出现 `Compiling tauri` + `Compiling stealth-crate`（桌面栈进入 headless 构建）；校验器 **exit 1 / D5**（`members missing from module_registry: ['stealth-crate']`） | **关闭** |
| G1 变体（无 tauri 的未登记成员） | 同上但 stealth-crate 无任何依赖 | 仍拒绝（登记是义务，与依赖内容无关） | **exit 1 / D5**（同一违例消息） | 关闭（方向正确） |
| G2 未激活 optional 禁止依赖 | kernel Cargo.toml 追加 `tauri = { path=…, optional=true }`，无 feature 激活 | 拒绝 | 先证盲态成立：`cargo metadata` 实测 resolve 节点仍仅 2 个、packages[] 无 tauri，而 kernel manifest 声明含 `('tauri', True)`；校验器 **exit 1 / D1 DEP-02**（manifest 声明扫描命中） | **关闭** |
| G2 变体 a（非禁用 optional 未激活） | kernel 加 `leftpad-util = { path=…, optional=true }` | 不误杀 | **exit 0 / RESULT: OK** | 无误杀 |
| G2 变体 b（非禁用 optional 已激活） | 同上 + `default=["leftpad-util"]`，真实 build 后校验 | 不误杀 | build 0；校验器 **exit 0 / RESULT: OK** | 无误杀 |
| G3 写者前缀仿冒 | 副本生成器 `RUST_SERVICE` 改 `rust-service-fork (旁路写进程)` 并重生成（生成器 exit 0、owner 类不变） | 拒绝 | **exit 1 / O8**（`exact match, not a prefix`；O7-drift 因同副本生成器而通过，证明 O8 是唯一牙齿且有效） | **关闭** |
| G3 变体（近真串尾随空格） | authoritative store writer 改为规范串 + 尾随空格 | 拒绝 | **exit 1 / O8** | 关闭（精确匹配无近似通道） |

**G1（中）关闭、G2（低）关闭、G3（低）关闭。** 修复报告对修复内容与复证结果的声明
与本轮独立复跑一致，无夸大。

## 5. G4 不改的理由核验

- 误杀方向独立复证成立：kernel 加良性 crate `taos` → exit 1 / D1（子串模式 `tao` 命中，
  fail-safe）。未见误放方向（rename/大小写/连字符形态在 R1/R2 及本轮源码审查中均被
  包 ID 建图 + 双侧归一覆盖）。
- 当前 workspace 零第三方依赖（cargo metadata 实测全图仅 2 包），无现实误杀对象；
  收紧为段级匹配需重新证明对 `tauri-*`/`*-sys` 包族的覆盖，无误杀案例时改动匹配语义
  只有引入漏检的风险。**"不改"理由成立，同意不阻塞。**

## 6. 修复回归面（实际运行 + 源码确证）

- **N1–N12 未弱化**：--self-test 实测 12 条既有负向全部以 R1/R2 记录的原匹配规则 ID
  拒绝（§3 表逐条列出），无一条被删除、改判或静默通过。
- **D5 反向闭合与 D2 触发顺序**：源码确证 D5-reverse 位于规则循环**之后**；N10 场景
  （伪造成员 + kernel 白名单规则）实测仍以 **D2** 拒绝——D2 先触发，反向闭合不遮蔽
  具体规则，也不产生放行路径（规则循环命中即抛，反向闭合只在循环无违例时追加拒绝）。
- **新探针：同名别名绕过**（本轮新增，非历史轮次场景）：未注册成员取名已注册模块名
  `lingxi-kernel`（version 9.9.9 + tauri 依赖）试图令 D5-reverse 的按名差集失效——
  `cargo metadata` 自身 exit 101（`two packages named 'lingxi-kernel' in this workspace`），
  校验器对 metadata 失败转 D1 违例（fail-safe），**无逃逸**。
- **G2 manifest 扫描误杀面**：非禁用 optional（未激活/已激活两种形态）实测不误杀（§4）。
- 校验规则只加严未放宽：DEPENDENCY_RULES.json 逐字节未变（001d271c…），
  校验器 diff 仅新增 D5-reverse、manifest 声明扫描、O8 精确匹配、N13–N15 四处。

## 7. 既有结论抽查（实际运行）

- **736+69 覆盖独立复算**（自写脚本直比 R00 原始清单）：FEATURE_INVENTORY 736 唯一
  F-ID ↔ feature_ownership 736 行，缺失 0 / 多余 0 / 重复 0；STORES 69 唯一 id ↔
  store_ownership 69 行，缺失 0 / 多余 0 / 重复 0。
- **worker 禁令数据面**：worker/ui 类 owner 拥有 F-ID 数=0、store 数=0；42 项
  authoritative store 全部 core/service/adapters 类 owner 且 writer 均为规范串
  `rust-service (lingxi-service 组合根进程，唯一业务数据写者)`；11 条关键事实全部单一
  owner 且 core/service 类；writer 分布 63 rust-service / 5 tauri-host / 1 build-release，
  与 ADR §5 一致。
- **三者一致**：生成器 `RUST_SERVICE` 与校验器 `RUST_SERVICE_WRITER` 两常量逐字符相等
  （实测比对 True）；ADR §5「target_writer_process 必须为 rust-service，由校验器 O8
  机械执法」与精确匹配实现方向一致（精确匹配是该契约的严格化）；ADR §8 包 ID 闭包 +
  双侧归一 + planned 对偶记载与实现一致；LOCKED_CRITICAL_FACTS 11 条与 ADR §8 锁定
  集合一致。
- 生产零改动（§1）；R1/R2 报告与修复报告原文均未触碰（§2 哈希链）。

## 8. 观察（非阻塞，不构成新发现）

- ADR §4 验证记录段与 §9 的负向电池枚举仍写 N1–N7 / N1–N12（现为 N1–N15）、正向枚举
  写 O1-O7（现为 O1-O8+D5-reverse+manifest 扫描）。属描述性台账滞后，无契约性错误
  （ADR 无任何与实现矛盾的陈述，校验器只比 ADR 措辞更严）。建议下次触碰 ADR 时刷新，
  本轮不要求。
- 残余理论面（与本门禁设计一致、非漏洞）：攻击者可通过**显式编辑** DEPENDENCY_RULES.json
  注册新模块而不挂 DEP-07——这是契约文件的可见治理变更（ADR §8 退出条件明示「须先改
  DEPENDENCY_RULES/OWNERSHIP_TARGET 并通过校验器」），diff 可见，不属机械门禁应拦截的
  隐蔽通道。
- 已知预存失败（4 个审计封印 FAIL、r00 校验器 STALE）与本任务无关，未运行、不据此判定。

## 9. 结论类型标注

- **实际运行**：§1、§2（复算）、§3、§4、§5、§6（N 系与探针）、§7 全部命令与注入
  （本机 macOS 27.0 arm64，/tmp/r01-t01-review-r3 隔离目录，真实退出码如上）。
- **源码确证**：校验器三处修复落码审查（D5-reverse 位置与语义、manifest 扫描取
  `dependencies[].name` 真实包名不受 rename 伪装、O8 精确等值）；D5/D2 顺序分析；
  ADR↔规则↔校验器一致性比对。
- **受环境限制**：跨平台编译（macOS x64/Windows/Linux）未验证（归 R01-T03/R09/R10）；
  全量 npm test 未跑（范围外）。

## 10. 最终判定

**PASS。**

- R2 三发现：G1（D5 反向闭合 + N13）、G2（manifest 级声明扫描 + N14）、G3（O8 锁定词表
  精确匹配 + N15）全部经 /tmp 真实注入独立复证关闭，且各自变体（无 tauri 未登记成员仍拒、
  非禁用 optional 两种激活形态不误杀、近真串尾随空格仍拒）方向正确。
- G4 不改的理由经独立复证成立（fail-safe 单向、零第三方依赖无现实误杀对象）。
- 回归面干净：N1–N12 原规则 ID 全保留；D5-reverse 后置不遮蔽 D2、不产生放行路径；
  同名别名绕过被 cargo 自身拒绝；校验规则只加严。
- 正向全量（build 0 / test 12/12 / metadata 2 包无桌面栈 / fmt 0 / O1-O8+D 系 PASS /
  N1-N15 全拒 / 生成器 --check 0）与 R01-A01、R01-A02 两个 REQUIRED 场景全部通过。
- 修复者声明（单文件改动、哈希清单、证据日志、未触碰既往报告与生产代码）经独立复算
  全部属实。
