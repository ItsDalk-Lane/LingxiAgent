# R04-T02 修复报告（第 1 轮）

- 修复代理：REPAIR-R04-T02-F01（一次性修复代理；只处理派单 FINDING_IDS：
  R04-T02-R1-F01、R04-T02-R1-F02、R04-T02-R1-O04 及同根因路径，不接管阶段）。
- 日期：2026-09-30。基线 `fd5fc2c2c583c6f4273ad3fa2122530edd8b152c`；候选＝基线之上的
  未提交工作树（T02 执行者成果 + 本修复轮改动）。
- 审查输入：`docs/rust-tauri/R04/reviews/R04-T02_REVIEW_R1.md`（VERDICT: FAIL；
  F01 MEDIUM + F02/O03/O04/O05 LOW）。
- 结论：**READY_FOR_REVIEW**（F01 按审查推荐方案 1 修复并永久回归；F02/O04 修复；
  O03 措辞属执行者报告历史文本不改写、O05 属 T03 范围未动，见 §6）。

---

## 1. R04-T02-R1-F01｜MEDIUM｜网关接线 read-only 子代理对全部注册目标（含 Read 类）被拒

### 1.1 根因（与审查 §5 一致，码级复核）

- 判定点：`rust/crates/lingxi-service/src/runs.rs`（修复前 L1233-1248）
  `RunGrant::Subagent{tier}` 分支先 `authorize_child_tool(tier, &request.target)`
  （request.target＝网关接线下的注册表 id，如 `tool:first-party:read`），仅当其返回
  Allowed 才用 prepared `local_name` 复验。
- kernel（`rust/crates/lingxi-kernel/src/subagent.rs` 修复前 L266-291）的 read-only
  判定是裸名词表：`SUBAGENT_READ_ONLY_TARGETS`（`read`/`grep`/… ）不含任何
  `tool:…` 命名空间 id → id 先判对 ReadOnly tier 恒 `Denied`（
  `ACTION_BLOCKED_BY_READ_ONLY`），local_name 复验不可达。审查探针 P1（真实链）：
  网关接线拒 / 传统接线同一 tier 放行。
- 语义角色错位：R03 裸名词授权词表与 R04 注册表命名空间 id 的词表错配被放进
  "先 id 后 local_name" 的短路次序。id 判定的语义角色应仅是反递归名单的纵深
  防御；read-only 允许表的权威判定应落在 local_name（R03 词表）。

### 1.2 首次失败证据（先红，永久回归测试在仓库测试框架内）

新增 4 条集成回归（`rust/crates/lingxi-service/tests/r04_t02_tool_gateway.rs`）
+ 5 条 kernel 单测（见 §1.3）。修复前实跑
（`cargo test -p lingxi-service --locked --test r04_t02_tool_gateway`，退出码 101，
9 passed / 4 failed，首次失败输出逐字保留如下）：

```text
---- f01_read_only_subagent_registered_read_tool_dispatches_on_both_wirings stdout ----
assertion `left == right` failed: F01: the read-only tier must keep the registered Read-class target
  callable on the gateway wiring (explicit-read attenuation keeps the research surface open)
  left: 0
 right: 1

---- f01_read_only_attenuation_still_denies_write_and_blocklist_targets stdout ----
the blocklist code keeps its precedence over the read-only layer: not dispatched: authorization
  denied (ACTION_BLOCKED_BY_READ_ONLY [subagent_access]): tool:first-party:subagent is blocked:
  this subagent runs in read-only mode. …
（修复前 ReadOnly × 委派 family 被误报为 read-only 层拒绝——blocklist 层级失位，同根因第 2 表现）

---- f01_access_read_delegation_child_dispatches_registered_read_tools stdout ----
assertion `left == right` failed: the access:read child run dispatched its registered Read-class call
  left: 0
 right: 1

---- f01_delegation_family_routing_is_first_party_only stdout ----
a plugin-origin local-name collision must never spawn a child run
（审查点名的潜伏同类路径：插件来源 local_name "subagent" 委派载荷被路由进真实 launcher）
```

四条红与审查 F01 的三条路径一一对应：网关接线 ReadOnly×注册 Read 类（核心格）、
同格在委派 `access:"read"` 派生子运行中的表现、blocklist 层级误标、委派 family
未限定 first-party origin。

### 1.3 修改（按审查推荐方案 1：local_name 为权威，注册表 id 仅作反递归纵深拒绝）

1. **kernel 新判定函数**（`rust/crates/lingxi-kernel/src/subagent.rs`）：
   - `denied_by_blocklist`/`denied_by_read_only`（L261/L275）：从
     `authorize_child_tool` 提取的两个拒绝构造器（拒绝码/层/文案逐字不变；
     `authorize_child_tool` 本体行为零变化——既有 7 条 kernel 单测原样通过）。
   - **`authorize_child_tool_with_registry_id(tier, local_name, registry_id)`**（L331）：
     ①反递归名单先判 local_name（拒绝码恒 `ACTION_BLOCKED_IN_SUBAGENT`，层级
     `subagent_blocklist`，tier 无关）；②注册表 id 再对反递归名单精确匹配（纵深
     防御：任一词表命中即拒，命名空间化 id 不是绕过）；③read-only 允许表**只按
     local_name 判定**（已知只读名放行、未知/写类名拒绝——fail-closed 与 R03 完全
     一致）；④注册表 id 永不参与允许判定 → id 不可能放宽（它不是允许表成员），
     也不可能像修复前那样把整个 Read 面 blanket-deny。
2. **驱动接线次序修正**（`rust/crates/lingxi-service/src/runs.rs` L1246-1258）：
   `RunGrant::Subagent{tier}` 有 prepared 时调用新函数（local_name 权威 + id 纵深
   拒绝）；无网关路径（prepared=None）保持 `authorize_child_tool(tier, request.target)`
   裸名词判定**逐字不变**（R03 十套件钉数复跑证明，§5 #11-13）。授权判定点位置
   （journal authorized 步）与唯一 journal 写入者（网关零存储端口）未动。
3. **委派 family 限定 first-party origin**（审查 F01 同类路径第 3 条，T07 前闭合）：
   - `rust/crates/lingxi-kernel/src/toolcatalog.rs`：`PreparedToolCall` 新增 `origin`
     字段（L1549，prepare_invocation 从 manifest 填充；唯一构造点，无其他构造器）。
   - `rust/crates/lingxi-service/src/toolgateway.rs`：`PreparedInvocation` 新增
     `origin`（L506，prepare 填充；服务端事实，非模型可填）。
   - `rust/crates/lingxi-service/src/runs.rs` L1622-1689：`delegation_first_party`
     由 prepared.origin 判定；非 first-party 目标携带委派载荷 → 与非 family 目标
     同形的响亮 `InvalidTarget` 拒绝（invalid_message: "does not accept a delegation
     payload"），零子运行、零派发。无网关路径（无 origin 事实）保持裸名 family
     不变。`subagent`/`subagent_reply`/`subagent_close` 三臂全部受限。
4. **永久回归测试**（全部真实链：ServiceState::bootstrap_with_deps 组合根 + 真实
   RunSupervisor/journal/registry/网关；替身仅外部答复者）：
   - `f01_read_only_subagent_registered_read_tool_dispatches_on_both_wirings`
     （tests L1487）：注册 Read 类目标 `tool:first-party:read`（local_name `read`，
     与审查 P1 探针及 T04 将来的真实注册同形）× 网关接线 × ReadOnly 子运行 →
     恰 1 次执行、canonical 载荷逐字节断言、journal Succeeded+dispatched=true；
     **对照腿**＝传统接线（无网关、tool_executor 端口）同一 tier 调裸名 `read`
     → 恰 1 次执行。两种接线同 tier×Read 类结论一致（A03 通过条件）。
   - `f01_read_only_attenuation_still_denies_write_and_blocklist_targets`
     （tests L1554）：保护面三腿——(a) policy Allowed 下写类仍
     `ACTION_BLOCKED_BY_READ_ONLY` 零派发（隔离出 kernel 层为拒绝层）；
     (b) **接 ApproveAll 批准面仍拒**（衰减先于批准轮——子代理不能经批准面升权，
     R03-A11 语义）；(c) ReadOnly × `tool:first-party:subagent` 委派 →
     `ACTION_BLOCKED_IN_SUBAGENT`（blocklist 层级优先，且断言**不含**
     ACTION_BLOCKED_BY_READ_ONLY——修复前该格误标）+ 零子运行。
   - `f01_access_read_delegation_child_dispatches_registered_read_tools`
     （tests L1672）：真实 launcher 全链——用户 run 委派 `access:"read"` → 真实
     子运行（ReadOnly tier）→ 子运行调 `tool:first-party:read` → 恰 1 次执行 +
     子 journal Succeeded+dispatched=true + 载荷断言（marker 分队脚本替身保证
     父/子各取各的确定性脚本，R03-A11 房式）。
   - `f01_delegation_family_routing_is_first_party_only`（tests L1778）：插件来源
     `tool:plugin:p1:subagent`（local_name 与委派 family 碰撞）携带委派载荷 →
     InvalidTarget 响亮拒绝、零子运行、零派发。
   - kernel 单测 5 条（subagent.rs L692-811）：注册 Read 类放行、写/未知名拒绝、
     blocklist tier 无关且优先、id 仅纵深防御（含 id 也不能用允许表名"救"非允许
     local_name 的负向）、**两词表全名单 × 两 tier 的裸名判定等价扫描**（网关
     接线不得改变任何既有结论）。

### 1.4 保护语义双成立（审查派单第 6 条 / 总控 §4.2 / R03_HANDOFF SUP-05）

- **显式 read 衰减的 Read 面保持开放**：`f01_…both_wirings` + 委派子运行测试证明
  （这正是 R03 冻结语义"research/review keeps working"，未为统一而删衰减）。
- **子代理不能升级权限（R03-A11）**：写类经任何路径不放行——kernel 层
  （`f01_…still_denies` (a)）、批准面存在时（(b) ApproveAll 也拒）、委派 family
  反递归（(c) + 既有 Operate 档对抗测试）、插件来源 family 碰撞
  （`f01_…first_party_only`）。read-only 允许表仍只认 kernel 冻结名单（fail-closed
  对未知名），且注册表 id 在任何路径都不构成放行依据。

### 1.5 A03 矩阵逐格复核（直接/按需/子代理/后台 × 允许/拒绝/需批准）

| 格 | 依据（测试） | 状态 |
|---|---|---|
| 允许×直接（Read, policy Allowed） | `r04_a03_allow_group…` 腿 1 | 既有 PASS |
| 允许×按需（Deferred） | 同上腿 2 | 既有 PASS |
| 允许×子代理 Operate | 同上腿 3 | 既有 PASS |
| **允许×子代理 ReadOnly×注册 Read 类** | `f01_…both_wirings` 腿 A + 传统接线对照腿 B | **本轮修复格** |
| 允许×委派 access:"read" 子运行×注册 Read 类 | `f01_access_read_delegation_child…` | 本轮新增 |
| 允许×后台（用户 run Full） | 同上腿 4 | 既有 PASS |
| 拒绝×FailClosed 无批准面（直接/子代理 Operate/后台） | `r04_a03_deny_group…` | 既有 PASS |
| 拒绝×子代理 ReadOnly×写类（kernel 层隔离） | `f01_…still_denies` (a) | 本轮加强 |
| 拒绝×子代理 ReadOnly×写类+批准面 ApproveAll | `f01_…still_denies` (b) | 本轮新增 |
| 拒绝×子代理 ReadOnly×委派 family（blocklist 码） | `f01_…still_denies` (c) | 本轮修复格（修复前层级误标） |
| 拒绝×子代理 Operate×委派 family | 既有 `adversarial_subagent_blocklist…` | 既有 PASS（修复后仍绿） |
| 拒绝×插件来源 family 碰撞 | `f01_delegation_family_routing…` | 本轮新增 |
| 需批准×直接/子代理 Operate（ApproveAll）+ RejectAll + policy Allowed 不重复弹 | `r04_a03_needs_approval_group…` | 既有 PASS |
| 需批准×ReadOnly×写类（衰减先于批准） | `f01_…still_denies` (b) | 本轮新增（登记：kernel 衰减层先于策略/批准层） |
| 拒绝×未注册/陈旧 pin/禁用/代次变化 | T01+网关既有测试（A02/A04 族） | 既有 PASS |

后台×子代理不是独立工具路径（background 驱动经同一 RunSupervisor 链，工具路径
按该 run 自身的 grant 走——审查 §3 已接受该口径）；"网关 policy Allowed 抑制已
接线 R03 批准面"（O05）保持登记内设计（总控 §4.2 不重复弹审批），收口归 T03。

### 1.6 同根因普查结果

- `runs.rs` 全文：kernel 词表判定唯一点＝授权块（已修复）；裸名 family 匹配
  唯一点＝委派分支（已限 first-party）；其余 `request.target` 使用全部是
  journal/event/审计写入（grep 逐点核对，非判定）。
- `subagents.rs`/`sessions.rs`/`background.rs`：无 kernel 词表判定、无注册表 id
  判定、无 local_name 匹配（`resolve_subagent_access` 衰减在 dispatch 时未触碰）。
- kernel `authorize_child_tool` 本体行为零变化（等价扫描单测 + 既有 7 条单测）。

## 2. R04-T02-R1-F02｜LOW（文档）｜TEST_MAP workspace 通过数 532 ≠ 实际

- 修复：`docs/rust-tauri/R04/R04_TEST_MAP.json` T02 `gate_results[
  "cargo test --workspace --locked"]` 重写为修复轮实测：**exit 0，771 passed /
  0 failed / 0 ignored**，并记录数字来源：
  - 审查测得的 533 passed / 1 failed 是**fail-fast 部分计数**——`cargo test` 不带
    `--no-fail-fast` 时在首个失败二进制（r00_management_leaves）后停止，其后约
    237 个测试（r00_static_web、r03_t08、r04_t01/t02、recovery、request、
    resource、run_lifecycle、service、session、shutdown、subagent、tls、
    tool_receipt、spike、xtask 等）从未运行；
  - 原记录 532 为转写笔误（审查 F02）；
  - 本修复轮 r00_management_leaves 环境失败**未复现**（70.4s 通过，防火墙未拦截
    本次未签名测试二进制），全工作区二进制全部执行：771 = 全量计数（其中含本
    轮新增 10 条永久测试：4 集成 f01_* + 5 kernel registry_id_wiring_* + 1 网关
    熵错误码）。
- 复算命令（审查同款）：
  `grep -oE "test result: (ok|FAILED)\. [0-9]+ passed; [0-9]+ failed" <log> | awk …`
  → `TOTAL passed: 771 failed: 0`；逐二进制明细见 §5 #3。

## 3. R04-T02-R1-O04｜观察（LOW）｜CSPRNG 失败误标 `gateway_prepared_registry_full`

- 修复（按审查 §5 建议：专用 refusal 码）：`toolgateway.rs`
  - `GatewayRefusal::HandleMintFailed { source }`（L311），码
    `gateway_prepared_handle_mint_failed`（L344），`to_tool_error` 映射
    UpstreamUnavailable + 文案点名 entropy source 失败与"绝不降级为可猜测句柄"
    （L405-415）；
  - 铸造失败分支改用该码（L738），`tracing::error!` 保留；
  - 钉数单测 `entropy_failure_refusal_carries_its_own_code`（L1185）：码 ≠
    registry_full、文案含 entropy——容量分诊不再被误导。
- 行为面不变：仍是响亮拒绝、绝不降级（审查结论"响亮且安全"保持）。

## 4. 未处理项（如实说明）

- **O03**（"逐字保留"措辞强于事实）：属执行者报告 `R04-T02_REPORT.md` 的历史
  文本；本修复轮不改写执行者报告的历史叙述（其行为主张本身成立，等价性由
  回归证明），如需措辞修订留待报告所有者/复审裁定。
- **O05**（policy Allowed 抑制已接线 R03 批准面）：审查明示"登记内设计，T03 须
  收口"，不属本修复轮 FINDING_IDS 的行为修复范围；矩阵登记见 §1.5。

## 5. 重跑验证（全部修复后最终字节实跑；仓库根，rustup 1.98.1 经
`/Users/study_superior/.cargo/bin/cargo`）

| # | 命令 | 退出码 | 结果 |
|---|---|---|---|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | （本修复轮代码先按 fmt 重排后复检通过） |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked`（全量日志 `/tmp/r04t02-repair/workspace-test-full.log`） | 0 | **771 passed / 0 failed / 0 ignored**；r00_management_leaves 本轮通过（70.4s，环境失败未复现）；含 r04_t02 13/13、service lib 249、kernel lib 77、T01 6/6 |
| 4 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | 0 | 生成契约零漂移 |
| 5 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | 0 | O1-O8+D1-D5+B1 |
| 6 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py --self-test` | 0 | N1-N17 全绿（含 N16/N17） |
| 7 | `cargo test -p lingxi-service --locked --test r04_t02_tool_gateway` | 0 | **13/13**（原 9 + 新 4；原 9 条断言零改动） |
| 8 | `cargo test -p lingxi-service --locked --lib toolgateway` | 0 | 7/7（含新熵码钉数） |
| 9 | `cargo test -p lingxi-service --locked --lib` | 0 | 249 |
| 10 | `cargo test -p lingxi-kernel --locked --lib` | 0 | 77（含新 5 条 registry_id_wiring_*） |
| 11 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh /tmp/r04t02-repair/final/repair-suites` | 0 | 十套件钉数精确全绿（7/8/13/6/5/5/5/2/8/7） |
| 12 | `bash scripts/rust-tauri/r03_t08_matrix.sh /tmp/r04t02-repair/final/t08-a15` | 0 | A15 28 叶+11 组合 ALL GREEN |
| 13 | `bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh /tmp/r04t02-repair/final/t08-a16` | 0 | A16 四相 ALL GREEN |

注：#11-13 首轮曾在 fmt 重排前跑过一次同样全绿（`/tmp/r04t02-repair/r03-regression*`）；
fmt 只重排版不改语义，为严谨在最终字节上复跑一遍，两轮结论一致。r00 环境失败
本轮未复现属环境层波动（防火墙对未签名测试二进制的拦截非确定）；该环境项按
审查建议仍如实跟踪，T08 Gate 前需环境层处置。

## 6. 两层自查

### 普通逐项

- F01 核心格：先红（§1.2 首次失败输出）→ 修复（kernel 权威函数 + 驱动次序）→
  绿（#7 13/13，含两接线对照腿、委派子运行全链腿）。前提/动作/预期/实际与
  测试断言一一对应（§1.3 第 4 条）。
- 写类不放行：三重独立证据（kernel 层隔离 (a)、批准面 ApproveAll (b)、
  blocklist (c)）+ 既有 deny 组/对抗测试修复后仍绿。
- 委派 family：三臂（subagent/subagent_reply/subagent_close）经
  `delegation_first_party` 全部受限；插件来源碰撞负向 + first-party 正向
  （既有委派入口测试修复后仍绿）双证。
- F02：实测数字与复算命令登记（§2）；O04：专用码 + 钉数单测（§3）。
- 未删任何测试、未放宽任何断言：r04_t02 原 9 条、kernel 既有单测、R03 十套件
  钉数全部原样通过；新增均为增量。无 commit/push。

### 对抗性自查（推翻自己的尝试与结果）

1. **"修复是否放行了本不该放行的东西？"**——逐路径反证：read-only 允许表仍只认
   kernel 冻结名单；注册表 id 在新函数中只出现在 blocklist 精确匹配（只能加拒），
   无法凭 id 进入允许表（kernel 单测 `registry_id_wiring_uses_the_id_as_…
   depth_defense_only` 的负向：id 为允许表成员名 "read" 也不能救非允许 local_name
   "write"）。等价扫描单测保证两词表全名单 × 两 tier 的结论与 R03 裸名判定逐格
   相同——网关接线不产生任何新放行格。
2. **"local_name 可被谁控制？"**——local_name 来自 manifest（注册面，服务端事
   实），prepare 时由 registry 填充进 PreparedInvocation；模型 arguments 无法触
   及。允许表命中但 permission kind 为 Execute 的注册目标（假想恶意注册名 "read"
   的执行类工具）：kernel 层与 R03 裸名接线结论一致（名字是 R03 冻结词表的判定
   依据——这是既有语义不是本轮放宽），网关策略层（FailClosed 对 Execute 类
   NeedsApproval）只会更严——分层防御交集不变。
3. **"委派 origin 判定能否被绕过？"**——origin 由 registry.prepare_invocation
   从 manifest 服务端填充；执行期 execute_prepared 复验 target_id/代次（origin
   变化即不同 target id → TargetChanged 拒绝）。无网关路径无 origin 事实，保持
   R03 裸名 family（十套件复跑证明未回归）。
4. **"测试是否替身化了被测面？"**——四条新回归的拒绝/放行全部由真实 kernel
   判定函数 + 真实驱动授权块产生；CountingExecutor 只记录到达载荷（外部系统
   替身）；VerdictPolicy/ApproveAllGate 是外部答复者（与既有 A03 套件同一边界）；
   委派腿经真实 launcher 派发真实子运行。
5. **"workspace 771 是否掩盖失败？"**——逐二进制明细核对（§5 #3 注），75 个
   `test result: ok` 行合计 771/0，无 FAILED 行，exit 0；此前 533 计数的 fail-fast
   机制已在 TEST_MAP 中说明，防止后续轮次再把部分计数当全量。
6. **"R03 语义是否被修复波及？"**——kernel `authorize_child_tool` 行为零变化
   （拒绝构造器逐字提取）；无网关路径驱动分支逐字不变；十套件钉数 + A15 + A16
   在最终字节复跑全绿（#11-13）。

## 7. 被取代的旧证据

- 审查探针 P1-P4（`/tmp/r04t02-review/probe`，矩阵外、临时）：P1 的复现形态已由
  仓库内永久回归 `f01_read_only_subagent_registered_read_tool_dispatches_on_both_
  wirings`（含传统接线对照腿）取代；P2 负向形态由 `f01_delegation_family_
  routing_is_first_party_only` 扩展（插件来源碰撞）取代。
- 执行者报告 §5 表"533 通过/1 环境失败"与 TEST_MAP 旧"532 passed"：由本轮全量
  实测 771/0/exit 0 取代（fail-fast 部分计数说明见 §2）；`r00_management_leaves`
  环境失败项仍按环境波动如实跟踪，不因本轮未复现而销项。
- 执行者报告 §2.2 "L1236 仅增加 local_name 复验" 的描述：该次序即 F01 根因，
  已由本修复（local_name 权威 + id 纵深）取代；执行者报告作为历史文档不改写。

## 8. 修改文件清单（本修复轮）

- `rust/crates/lingxi-kernel/src/subagent.rs`：拒绝构造器提取 + 
  `authorize_child_tool_with_registry_id` + 5 条单测。
- `rust/crates/lingxi-kernel/src/toolcatalog.rs`：`PreparedToolCall.origin`。
- `rust/crates/lingxi-service/src/toolgateway.rs`：`PreparedInvocation.origin`；
  `GatewayRefusal::HandleMintFailed`（O04）+ 单测。
- `rust/crates/lingxi-service/src/runs.rs`：授权块次序修正（F01）+ 委派 family
  first-party 限定（F01 同类路径）。
- `rust/crates/lingxi-service/tests/r04_t02_tool_gateway.rs`：4 条 f01_* 永久回归
  + legacy/markered 两个测试装置 + `read` 注册目标与 executor 绑定。
- `docs/rust-tauri/R04/R04_TEST_MAP.json`：T02 条目（F02 + 新测试登记 + 修复轮
  说明）。
- 本报告。
