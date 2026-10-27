# R01-T01｜确定模块和进程所有权 — 独立对抗性验收报告 R1

- 验收者：ZCode:R01-T01-review-r1（全新独立验收代理；未参与执行；只读审查 + /tmp 隔离复跑，未修改任何已提交文件、生产代码或执行者交付物）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `328cc8bb5a807bdaad520b459907fb1fa4e10dca`（开工实测一致）
- **最终判定：FAIL**（失败范围严格限定于依赖方向校验器的执法缺陷 F1；两个 REQUIRED 验收场景 R01-A01/R01-A02 的证据本身经独立复跑成立，OWNERSHIP_TARGET.json 数据经独立复算正确——详见 §3/§4）

## 1. 工作区事实核验（实际运行）

`git status --porcelain` 与 `git diff --stat HEAD`（2026-09-25 复测）：

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json   （总控账本，既存修改，非候选，未触碰）
?? artifacts/rust-tauri/R01/
?? docs/rust-tauri/R01/
?? rust/
```

- `git diff HEAD --stat -- desktop server core lib shared cli hub plugins skills2set package.json package-lock.json`：**空**（生产零改动成立）。
- `grep` `rust/crates|lingxi-kernel|lingxi-protocol` 于 `package.json`/`desktop/main.cjs`/`desktop/preload.cjs`：**无命中**（原型未接入任何生产入口）。
- 与基线事实声明完全一致，无额外改动。

## 2. 候选清单与复算哈希（SHA-256，验收者独立复算）

| 文件 | SHA-256 |
|---|---|
| docs/rust-tauri/R01/ADR-001-ownership.md | 87454fc0678f17d14d9799aa7fc0e28bb218c3b0d718ca0988b23c26dedc3238 |
| docs/rust-tauri/R01/DEPENDENCY_RULES.json | 001d271c31e70837d09d13ae7b18e473714df79b95802e524e3caea1521b27f3 |
| docs/rust-tauri/R01/OWNERSHIP_TARGET.json | 6dfbdd6c7f6c1c705e156a6b043b7c137003f7ce3a6ad1d6ffa1e3c80bd9e381 |
| docs/rust-tauri/R01/r01_t01_build_ownership.py | 176b043582a7a31ff1e92346f4cbf858a9204a05379e67d490883726dd46c173 |
| docs/rust-tauri/R01/r01_t01_check_ownership.py | 54c3c45de1698f7be84bbe4cabf38feb85b1417ccbce2398cceb984852da416b |
| docs/rust-tauri/R01/R01-T01_REPORT.md | 952378cbd73b812cd34d3a7018ba899aa5c7f372f41a58bb30a473ca34e07a3f |
| rust/Cargo.toml | 593cf9414e42417f5f0e66ddc3524e0fdbbe07d0e03d37935c876078054b9a82 |
| rust/Cargo.lock | bbbeb538501e4a20db33c8d6974835fb32814dc642064c233be0a5f3625a436a |
| rust/crates/lingxi-protocol/Cargo.toml | 940c4ac147368d95816e982d7149673daa43dd27af62590ffbdaa5f96fc7ac2e |
| rust/crates/lingxi-protocol/src/lib.rs | 817c0cff8559089c94c06368bebd49e859983bb3b06d8a5dacccdad092e658a7 |
| rust/crates/lingxi-kernel/Cargo.toml | 604e27758d4b9e07d52e6526bc8adc2e0eaaed71fa6128857fb41956a64a33ca |
| rust/crates/lingxi-kernel/src/lib.rs | cd5e89d09894352b9b9ae0a85003a5f23bc949f6a9668a0cdc0b94e0d972213e |
| rust/crates/lingxi-kernel/src/ports.rs | fb71a21263749f2a59baf83e667bdd8b14ecd362b78f53f6db0aad16c637172b |

R00 输入锚定核验：OWNERSHIP_TARGET.json `source_inputs` 记录的两个 sha256 与当前
`docs/rust-tauri/R00/FEATURE_INVENTORY.json`（48141268…）/`STORES.json`（6067bdd3…）
实际文件哈希逐一相符。R00_HANDOFF.json status=READY_FOR_REVIEW，allowed_next_scope 授权 R01
以其清单为输入。

## 3. 验收场景独立复跑（隔离目录 /tmp/r01-t01-review-r1，全部命令真实重跑，未采用执行者日志）

方法：将 `rust/`、`docs/rust-tauri/R00`、`docs/rust-tauri/R01` 复制到
`/tmp/r01-t01-review-r1/repo`，`CARGO_TARGET_DIR=/tmp/r01-t01-review-r1/target`，
全部代理环境变量以 `env -u` 摘除，cargo 全程 `--offline`。

### R01-A01｜核心不依赖桌面 — 独立结论：通过（实际运行）

| 复跑命令 | 退出码 | 结果 |
|---|---|---|
| `cargo build --manifest-path rust/Cargo.toml --workspace --offline` | 0 | 两 crate 真实编译通过（0.20s），非空壳 |
| `cargo test --manifest-path rust/Cargo.toml --workspace --offline` | 0 | 12/12（protocol 5 + kernel 7），无 ignored |
| `cargo metadata --format-version 1 --offline` | 0 | 解析图仅 lingxi-kernel/lingxi-protocol 两包，无 tauri/electron/tao/wry/webkit2gtk/winit/webview |
| `python3 -B r01_t01_check_ownership.py`（正向全量） | 0 | O1-O7、D5、DEP-01/02/03/04/06/07（存在模块部分）PASS |
| `python3 -B r01_t01_build_ownership.py --check`（漂移） | 0 | UP_TO_DATE features=736 stores=69 |

执行者声明的 'tauri'/'electron' 原文 grep 命中为 crate description 英文句，经结构化
metadata 复算确认非依赖项——声明属实。

### R01-A02｜双负责人被检出 — 独立结论：通过（实际运行）

| 复跑命令 | 退出码 | 结果 |
|---|---|---|
| `--emit-negative-fixtures` + N1 夹具校验（双 owner） | 1 | `FAIL [O4] critical fact run_terminal_state has 2 owners` |
| N2 夹具校验（"二者同步"绕过） | 1 | `FAIL [O4] … bypass fields ['reconciliation','secondary_owner'] … not an accepted escape hatch` |
| `--self-test`（执行者负向电池 N1-N7） | 0 | N1-N7 全部以匹配的规则 ID 拒绝；正向全过 |

### 验收者自造反例（非执行者夹具，全部实际运行）

| 编号 | 变体 | 期望 | 实际退出码/规则 | 结论 |
|---|---|---|---|---|
| A1 | authenticated_principal 双 owner（换一条事实） | 拒绝 | 1 / O4 | 符合 |
| A2a | co_owners 同步绕过（model_credential_selection） | 拒绝 | 1 / O4 | 符合 |
| A2b | 全新字段名 also_synced_to 同步绕过 | 拒绝 | 1 / O4（未知字段即拒） | 符合 |
| A3 | worker.doc-parse 拥有 run_terminal_state | 拒绝 | 1 / O5 | 符合 |
| A4 | worker 拥有 store | 拒绝 | 1 / O6 | 符合 |
| A5 | react-ui 拥有 F-ID | 拒绝 | 1 / O6 | 符合 |
| A6 | 同 F-ID 两行不同 owner（feature 双负责人） | 拒绝 | 1 / O1 | 符合 |
| A7 | kernel 真实 path 依赖名为 `tauri` 的 crate（非合成 metadata） | 拒绝 | 1 / D1（DEP-02） | 符合——D1 经真实 cargo metadata 端到端执法，非 mock |
| A8 | RunContext 携带 WebviewWindow 字段（D3 词表外 token） | 拒绝 | 1 / D4（DEP-06） | 符合——D4 有独立执法价值 |
| A9 | 生成器闭表改一条：authoritative store session-jsonl → tauri-host | 应拒绝 | **0 / RESULT: OK** | **不符合（发现 F2）** |
| A10 | 新增 shadow 关键事实（换 fact_id、第二 owner） | 应拒绝 | **0 / RESULT: OK** | **不符合（发现 F3，低）** |
| B1 | kernel 真实依赖带连字符内部 crate `lingxi-adapters`（DEP-03 反向依赖） | 拒绝 | **0 / `PASS rule DEP-03 OK`** | **不符合（发现 F1）** |
| B2 | kernel → lingxi-adapters → tauri（桌面栈经连字符内部 crate 传递到达 kernel） | 拒绝 | **0 / RESULT: OK** | **不符合（发现 F1 最坏形态）** |
| B3 | 伪造 workspace 成员 + kernel 白名单规则（D2） | 拒绝 | **0 / RESULT: OK** | **不符合（发现 F1 同源）** |

## 4. 覆盖面独立核对（验收者自写脚本，直接比对 R00 原始清单；实际运行）

- R00 FEATURE_INVENTORY：表头 feature_count=736，实际 736 行、736 唯一 F-ID。
  OWNERSHIP_TARGET feature_ownership：736 行，**缺失 0、多余/伪造 0、重复 0**。
- R00 STORES：69 唯一 store id。store_ownership：69 行，**缺失 0、多余 0、重复 0**。
- 每行 target_owner 均为单一字符串且存在于 owner_registry（30 项，无重复 id）。
- worker/ui 类 owner 拥有 F-ID 数=0、store 数=0；关键事实 11 条全部单 owner 且均为
  core/service 类——逐条对照任务书 02 §3 关键事实表，11 行全部对应且归属合理
  （认证→service.auth、会话→kernel.session、run 终态→kernel.run-supervisor、
  attempt 栅栏→kernel.run-supervisor、工具可用→kernel.tool-gateway、
  批准→kernel.policy、模型/凭证→kernel.model-gateway、文件授权→kernel.resource、
  投影语义→kernel.session、用量 trace→kernel.model-gateway、调度去重→kernel.scheduler）。
- store 写进程分布：63 项 rust-service、5 项 tauri-host 壳自态（全部
  adjacent_compatible/shell_state，核验属实）、1 项 build-release（signed-artifacts，
  adjacent_compatible）；**authoritative 分类 store 无一旁落非 rust-service 写者**。
- 结论：736/69 覆盖声明真实，无伪造 ID，worker 禁令在数据层面成立。

## 5. 禁止替代核查（源码确证）

- 两个 crate 为真实可编译实现（不透明 ID 宏、Seq 十进制字符串 2^53+7 round-trip、
  八态 RunStatus、RunStateMachine 迁移表、RunContext、ports 四 trait），无占位空壳、
  无永真断言；测试断言具体（非法迁移逐对验证、终态 4×8 全组合拒绝）。
- 校验器/生成器为真实逻辑：闭表映射未知标签即 SystemExit，无静默默认；负向电池要求
  "以匹配的规则 ID 拒绝"否则自判失败——不是跳过断言。
- 生成器确定性成立：`--check` 复跑 UP_TO_DATE；输出无时间戳/随机性。

## 6. 发现的问题

### F1（高，判 FAIL 的唯一依据）依赖方向校验器对连字符内部依赖名失效——DEP-03/DEP-04/D2(DEP-05) 实际未被执法

- **最小重现**（/tmp 隔离副本，已验证）：
  1. 建 crate `lingxi-adapters` 加入 workspace，给 `lingxi-kernel` 的 Cargo.toml 加
     `lingxi-adapters = { path = "../lingxi-adapters" }`，跑
     `r01_t01_check_ownership.py` → **退出 0，输出 `PASS rule DEP-03 OK for module lingxi-kernel`**。
     真实 cargo metadata 中 kernel 节点 deps 为 `['lingxi_adapters','lingxi_protocol']`。
  2. 再让 lingxi-adapters 依赖名为 `tauri` 的 crate → kernel 经传递依赖真实挂上桌面栈，
     校验器仍 **退出 0，RESULT: OK**。
  3. 伪造 workspace 成员 + 给 kernel 配 `allowed_module_deps: ["lingxi-protocol"]` 白名单
     规则 → D2 同样静默通过（退出 0）。
- **根因**：`r01_t01_check_ownership.py` 的 `transitive_deps()` 混用两种命名——cargo
  metadata `resolve.nodes[].deps[].name` 是代码形式（连字符转下划线，如
  `lingxi_adapters`），而 `packages[].name`/`workspace_members` 保留连字符
  （`lingxi-adapters`）。后果三重：(a) `forbidden_dep_patterns` 中的连字符模式
  （DEP-03 的 `lingxi-adapters`、DEP-04 的 `lingxi-kernel/lingxi-adapters/lingxi-service`）
  永远无法匹配真实依赖名；(b) 传递闭包以 `name_of[dep_id] == name` 展开，连字符包名
  查不到，闭包在第一个连字符内部包处断裂，传递检测退化为仅直连一层；(c) D2 的
  `deps & workspace_names` 交集恒为空，白名单规则恒真。无连字符的模式（tauri/electron/
  tao/wry/webkit2gtk）不受影响，故执行者 N6 自测（假包名 `tauri`）与 A01 结论不受影响；
  当前 2-crate workspace 也不存在真实违规——但 DEP-03/DEP-04/DEP-05 恰是 R02 建立
  adapters/service 后才需要执法的规则，缺陷会精确在需要它时失守。执行者报告 §3/§4 称
  "DEP-01..07（存在模块部分）全部 PASS"、"kernel 禁 adapters……校验器机械执法"，
  其中 DEP-03/D2 部分属未被真实执法的声明（规则实际无法触发，等效永真）。
- **影响**：R01-T01 步骤 2"宿主和传输只能依赖内核公开接口/依赖方向"的机器执法链在
  内部模块方向上空转；报告 §9 声明该校验器为"常驻门禁"，带病交接会把空转门禁传给 R02+。
- **涉及场景**：R01-A01（依赖树执法部分；A01 的无桌面依赖事实本身经独立 metadata
  复算仍成立）、任务书 02 §2 依赖方向契约、R01-T01 步骤 2。
- **修复必须覆盖的同根因完整路径**：
  1. `transitive_deps()` 改为以 `deps[].pkg`（包 ID）建邻接、以包 ID 求闭包，最后再映射
     回包名做模式匹配；或对两侧统一做 `-`↔`_` 归一化（推荐包 ID 方案，同时消除同名
     多版本歧义）。
  2. 模式匹配对包名与其下划线变体同时比对。
  3. `--self-test` 增加负向：N8 = kernel 直连连字符内部 crate（lingxi-adapters）的
     合成 metadata（deps name 用真实 cargo 输出形态 `lingxi_adapters`）→ 期望 D1 拒绝；
     N9 = 桌面栈经连字符内部 crate 传递到达 kernel → 期望 D1 拒绝；N10 = D2 白名单
     放行伪造成员 → 期望 D2 拒绝。三者须断言"以匹配规则 ID 拒绝"，不得只断言退出码。
  4. 修复后须用本报告 B1/B2/B3 的真实注入法（非合成 metadata）在 /tmp 复跑证明检出。
  5. 建议一并加 D5 对偶检查：status=planned 模块的 crate 已存在于磁盘/工作区即违例
     （本次 B2 攻击能成立的辅助条件是 planned 模块实际存在却被整体跳过）。

### F2（低）"authoritative store 写者唯一为 rust-service"未被机器执法

- 重现：改生成器闭表将 `session-jsonl`（authoritative）映射给 `tauri-host.desktop-host`，
  重新生成后校验器退出 0。O6 只禁 worker/ui 类 owner 拥有 store，host/build 类不受限；
  `target_writer_process` 字段无任何规则校验。
- 影响：当前交付数据经独立核对全部正确（§4），此为门禁加固缺口而非数据错误；
  生成器闭表改动在 diff  review 中可见。建议：校验器增加规则——classification 为
  authoritative 的 store，owner 不得为 host/build/ui/worker 类且 target_writer_process
  必须为 rust-service。

### F3（低）关键事实词表的语义重复不可机械检出

- 重现：新增 `run_terminal_state_shadow`（不同 fact_id、owner=adapters.storage）→
  校验器退出 0。fact_id 是机器可执法的边界，语义重复只能靠生成器 diff 审查。
- 建议：在 ADR-001 §8 失效信号中明示"新增 critical_facts 条目属治理变更，需关卡审查"，
  或在校验器中锁定 11 条事实的精确集合（当前只查必需子集，不查多余）。

### 非问题记录

- 已知预存失败（全量 npm test 的 4 个审计封印 FAIL、r00 校验器 STALE 分类）与本任务
  无关，未运行、不据此判定，与执行者报告 §7 一致。
- `.gitignore` 第 95 行忽略 `*.log`，执行者证据日志未跟踪属惯例（R00 同样如此）；
  本验收所有关键命令均已独立重跑，不依赖这些日志。
- DEP-05/DEP-07 对 planned 模块暂不生效属执行者明示设计（报告 §6），本身合理；
  其与 F1 的叠加风险已在 F1 修复路径第 5 条覆盖。

## 7. 结论类型标注

- **实际运行**：§1、§3、§4、§6 全部命令与攻击复现（本机 macOS 27.0 arm64 隔离目录，
  真实退出码如上）。
- **源码确证**：§5 禁止替代核查、ADR-001 §5 信任边界（启动者/关闭者/数据写入者，
  TB-01..06 覆盖 rust-service/tauri-host/旧 Electron/worker/外部服务/react-ui）
  与 DEPENDENCY_RULES.json trust_boundaries 逐条一致；02 §3 关键事实表 11 行全对应。
- **受环境限制**：跨平台编译（macOS x64/Windows/Linux）未验证（任务书归 R01-T03/R09/R10，
  不属本场景）；未跑全量 npm test（范围外）。

## 8. 最终判定

**FAIL**。

- R01-A01（核心不依赖桌面）：场景证据独立复跑**通过**——编译/测试/metadata 无桌面栈
  全部真实成立。
- R01-A02（双负责人被检出）：场景证据独立复跑**通过**——双 owner 与"二者同步"绕过
  在验收者自造变体下均被明确拒绝且报错可诊断。
- 但交付物机制存在 F1：DEPENDENCY_RULES.json 的 DEP-03/DEP-04 与 D2/DEP-05 白名单
  被证明无法对真实注入的违规执法（连字符命名归一化缺陷 + 闭包断裂），执行者报告对
  "DEP 全部 PASS/机械执法"的声明在此范围内不成立；该门禁是 R01-T01 步骤 2 依赖方向
  契约的执法载体并将作为常驻门禁传入 R02。按任务书"禁止替代"与本阶段对"规则被校验器
  真实执行"的要求，判定 FAIL，修复范围见 F1 的同根因路径（连同建议的 F2/F3 加固）。
- 修复量预估小（校验器单文件 ~15 行 + 3 条负向自测 + 复跑证据），OWNERSHIP_TARGET/
  ADR/rust crate 数据与实现无需改动。
