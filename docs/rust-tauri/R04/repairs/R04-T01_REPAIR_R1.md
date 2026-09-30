# R04-T01 修复报告（第 1 轮审查 findings）

- 修复代理：REPAIR-R04-T01-F01（只负责 R1 的 F01/F02/O04 及同根因路径，未接管阶段）。
- 日期：2026-09-30。
- 输入：`docs/rust-tauri/R04/reviews/R04-T01_REVIEW_R1.md`（VERDICT: FAIL，必修项 F01）。
- 候选：基线 `bf6450bcd` 之上的未提交工作树（分支 `codex/rust-tauri-migration`，HEAD 未移动，无
  commit/push 操作）。
- 工具链：`/Users/study_superior/.cargo/bin/cargo`（rustup 1.98.1）；Node v24.16.0。

## 0. 逐项结果总览

| Finding | 级别 | 处置 | 结果 |
|---|---|---|---|
| R04-T01-R1-F01 minItems/maxItems 被 items 门静默跳过 | MEDIUM 必修 | 修复方案 a（长度检查移出 items 门）＋永久负例单测 | 已修复，复现→修复→回归全链验证 |
| R04-T01-R1-F02 TS 双实现头注释保留已证伪等价论断 | LOW 文档 | 注释改为 UTF-16 码元序口径（PROTOCOL_SPEC §9 同源） | 已修复，行为零变化，node 用例复跑通过 |
| R04-T01-R1-O04 R04_TEST_MAP.json 通过数过期（524≠实际） | LOW 台账 | 回填真实数字并注明来源链 | 已修复（527，含来源链说明） |
| R04-T01-R1-O03 ORCHESTRATOR_PROGRESS.json 形状 | 观察 | 审查裁定归总控台账事项，修复轮无义务，未触碰 | 不适用（按派单） |

---

## 1. R04-T01-R1-F01（MEDIUM）——数组长度约束在无 `items` 子 schema 时被静默跳过

### 1.1 复现（首次失败保留）

修复前先在 `rust/crates/lingxi-kernel/src/toolcatalog.rs` tests 模块加入永久负例单测
`array_length_bounds_are_enforced_without_items_subschema`（见 §1.3），对未修复代码运行：

```
$ cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-kernel array_length_bounds
test toolcatalog::tests::array_length_bounds_are_enforced_without_items_subschema ... FAILED
thread '...' panicked at crates/lingxi-kernel/src/toolcatalog.rs:2956:14:
maxItems must bind without an items sub-schema: PreparedToolCall {
  target_id: ToolTargetId("tool:first-party:tagged"), ...,
  effective: EffectiveArguments { canonical: Object {"tags": Array [1, 2, 3, 4, 5]}, ... },
  args_digest: ContentDigest { ... hex: "19468c98990b350638b860486c161beff06524da1d7654f62229fac5ad1bf03e" },
  args_summary: "{tags:arr[5]}", ... }
test result: FAILED. 0 passed; 1 failed
```

失败形态＝审查探针 P1 的等价复现：schema `{"type":"array","minItems":1,"maxItems":2}`
（无 items）注册成功，且 `{"tags":[1,2,3,4,5]}` **通过规范化并产出 PreparedToolCall**
（违反 maxItems=2 却获得有效摘要与摘要哈希）。finding 属实，无反证。

### 1.2 根因与同族路径审查

根因：`normalize_value()`（修改前 L1318-1346）把 `minItems/maxItems` 检查嵌在
`if let (Some(items_schema), Some(items)) = (schema_object.get("items"), value.as_array())`
双 Some 分支内——数组 schema 不带 `items` 时长度检查整体不执行，落入
`return Ok(value.clone())` 静默放行。

同族路径逐项核查（`normalize_value` 全部约束，按值类型归位；修改后行号）：

| 约束 | 执行条件 | 是否被兄弟字段门控 | 判定 |
|---|---|---|---|
| `type` | schema 有 `type` 即查（L1253+） | 否 | 无问题 |
| `enum` / `const` | 任意值，无条件（L1271/1276） | 否 | 无问题（审查 P4 已探针证实 const 无 items 正确拒绝） |
| `minimum/maximum/exclusiveMin/Max` | 值为整数（`value.as_i64()`，L1281） | 否。浮点不查数值界，但 type:"integer" 先报类型违例；type:"number" 的浮点在 `EffectiveArguments::from_value` 被 `ArgumentsNotSafeInteger` 硬拒——不存在浮点宽松成功路径 | 无问题 |
| `minLength/maxLength` | 值为字符串（L1308） | 否（不要求 schema 有 `type:"string"`） | 无问题 |
| **`minItems/maxItems`** | **修改前要求 items 与数组值同时存在** | **是——本 finding** | **已修** |
| `minProperties/maxProperties` | 值为对象（L1365+），不要求 `properties` 存在 | 否（审查 P5 已探针证实无 properties 正确拒绝） | 无问题；本次修复正是向该对称形态对齐 |
| `required` / `additionalProperties:false` | 值为对象，无条件于 `properties` | 否 | 无问题 |
| `items` 递归本身 | items 子 schema 与数组值同时存在才递归 | 语义正确：无 items＝无逐元素 schema 可查；数组元素仍受 `EffectiveArguments::from_value` 的安全整数/深度/预算全量遍历约束 | 无问题 |
| `pattern`/`uniqueItems`/`contains` 等 | 注册期 `REJECTED_VALIDATION_KEYWORDS` 拒绝（L686-708） | 不存在执行面 | 无问题 |

结论：被兄弟字段门控的约束**仅 minItems/maxItems 一处**，与审查 §2 F01 同类路径结论一致；
一次修根因即闭环，无其他同类缺口。

### 1.3 修改（方案 a：长度检查移出 items 门）

按模块设计一致性选**方案 a**：对象侧 `minProperties/maxProperties` 本就无 `properties` 门
（值是对象即查），数组侧对称地改为「值是数组即查」，与 `validate_schema_node`
已把 minItems/maxItems 声明为受支持关键词的注册面自洽（注册放行 ⇒ 执行面精确生效）。
未选方案 b（注册期拒绝无 items 的长度约束）：会把第三方/MCP 常见合法形状
`{"type":"array","maxItems":N}` 错误判为不兼容，缩小兼容面且与对象侧不对称。

修改文件与位置：

- `rust/crates/lingxi-kernel/src/toolcatalog.rs` L1318-1359（`normalize_value` 非对象分支）：
  长度检查（maxItems/minItems）移至 `if let Some(items) = value.as_array()` 层，
  `items` 子 schema 仅门控**逐元素递归**；无 items 的数组仍被长度界约束，
  元素仍受 `EffectiveArguments` 安全整数/预算遍历约束（注释内写明该依据与 F01 出处）。
- `rust/crates/lingxi-kernel/src/toolcatalog.rs` L2937-3058：新增永久负例单测
  `array_length_bounds_are_enforced_without_items_subschema`——
  (1) 无 items 的 maxItems 违例（5 元素 > 2）必须 `ArgumentsInvalid` 且违例文案含 `maxItems`；
  (2) 无 items 的 minItems 违例（空数组 < 1）必须 `ArgumentsInvalid` 且文案含 `minItems`；
  (3) 界内负载通过且逐字保留；
  (4) 带 items 对照组仍执行同一长度界（守护被移动代码本身，而非只守护缺口）。
- 修复后 `cargo test -p lingxi-kernel toolcatalog`：**19/19**（原 18 + 本回归 1）。

行为影响面（如实登记）：无 items 数组 schema 的超长/过短参数从「静默通过」变为「显式拒绝」
（fail-closed 方向）；随之，`default` 填充路径中违反自身 minItems/maxItems 的数组默认值
也会以违例暴露——与模块既有声明一致（"a default that violates its own schema surfaces as
a violation here, never silently accepted"，toolcatalog.rs defaults 注释）。

### 1.4 F01 验证命令（全部实跑，仓库根）

| # | 命令 | 退出码 | 结果 |
|---|---|---|---|
| 1 | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-kernel toolcatalog` | 0 | 19/19（含新回归） |
| 2 | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r04_t01_tool_catalog` | 0 | 6/6 |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 101 | **41 套件 ok / 527 passed / 0 断言失败 / 0 ignored**；唯一失败 `r00_management_leaves`（见 §4，环境项） |
| 4 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | |
| 5 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | |
| 6 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | 0 | 626 条目生成物零漂移 |
| 7 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | 0 | D1-D5＋负向电池 |
| 8 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh /tmp/r04t01-repair/g07-fresh` | 0 | 十套件钉数精确全绿（R03 面） |

---

## 2. R04-T01-R1-F02（LOW 文档）——TS 双实现头注释的已证伪论断

- 修改：`tests/migration/r01-t02/canonical-json.mjs` L5-10。原
  "object keys sorted by code point (UTF-16 unit order equals code point order, so
  Array.prototype.sort default is correct here)" 改为：键按 **UTF-16 码元序列**排序——
  `Array.prototype.sort` 默认比较走 UTF-16 码元，与码点序在「星形键（≥U+10000，代理对
  D800-DFFF）与 U+E000..U+FFFF 键混排」时分歧（RR-T02-F1，R04-T01 关闭），指向
  PROTOCOL_SPEC §9 与 canon.rs（写入器按 UTF-16 码元排序，双端所有键形字节一致）。
  与 `rust/crates/lingxi-protocol/src/canon.rs` 头注释及 `docs/rust-tauri/R01/PROTOCOL_SPEC.md`
  §9 现行裁定同口径，双端权威说明不再互相矛盾。
- 纯注释改动，无行为变化。验证：`node --test tests/migration/r01-t02/roundtrip.mjs` →
  exit 0（12 golden 样本双向逐字节相等）；`node artifacts/rust-tauri/R04/T01-E01/cross-lang-canon-nonbmp.mjs`
  → exit 0（PARITY OK，分叉类字节相等）——即注释修改后 TS 实现与 Rust golden 仍一致。

## 3. R04-T01-R1-O04（LOW 台账）——R04_TEST_MAP.json 通过数

- 修改：`docs/rust-tauri/R04/R04_TEST_MAP.json` L49（T01 `gate_results`）。
  "524 passed" → "527 passed / 0 failed / 0 ignored, 41 suites ok, except
  r00_management_leaves (environment…)"，并注明来源链：原记载 524 失准；候选项实际 526
  （R04-T01_REPORT.md §5 #3；审查 R1 §3 #6 独立实测 526）；修复轮新增 1 条 F01 永久回归
  → 527，本次修复实测核证（本报告 §1.4 #3）。JSON 语法校验通过（`python3 json.load` OK）。
- 数字与前向一致性：任何人现在重跑 `cargo test --workspace --locked` 应得 527 passed
  （除环境项）——台账不再滞后于树。

## 4. r00_management_leaves 环境项隔离核证（沿审查 §4 归因，不当作回归）

- 全量运行中唯一失败套件；隔离重跑
  `cargo test --locked -p lingxi-service --test r00_management_leaves` 确定性同形态失败：
  panic 于 `crates/lingxi-service/tests/r00_management_leaves.rs:49`，
  "request to 192.168.3.5:<port> stalled during write/read exchange
  (POST /lingxi/v1/web-auth/login, 0 bytes read so far) — … macOS application firewall /
  proxy TUN may be blocking inbound connections to this unsigned test binary"。
- 与审查 R1 §3 #5 / §4 的独立机理探针结论一致（防火墙拦截未签名测试二进制在非回环地址的
  入站）；本修复改动（toolcatalog 长度界、两条注释、一处台账数字）与该测试面零交集，
  41 个其余套件全绿。**未把它计为通过，也未掩盖任何断言**。

## 5. 应失效/被取代的旧证据（T01-E01 内）

| T01-E01 条目 | 状态 | 说明 |
|---|---|---|
| `workspace-test-cargo-test-workspace-locked.log` | **被取代** | 记录的是修复前树（526 passed）；当前候选的权威全量数字为本修复轮 527（§1.4 #3） |
| `gates/`（fmt/clippy/contracts/boundaries 日志） | **被取代（作为当前候选的门禁证据）** | 本修复轮已全部重跑全绿（§1.4 #4-#7）；旧日志仅作修复前候选的历史证据保留 |
| `r03-regression/`、`r03-repair-suites-regression/` | **被取代** | 修复轮以全量 workspace＋十套件钉数脚本复证（§1.4 #3/#8） |
| `cross-lang-canon-nonbmp.mjs` | **继续有效** | F02 仅注释；修复轮复跑 PARITY OK，仍是 RR-T02-F1 关闭主张的证据 |
| `run_t01_validation.sh` | 继续有效（复跑脚手架） | 其输出将以修复后的树为准 |

R04-T01_REPORT.md / R04_TEST_MAP.json 的 T01 状态与数字以本报告与其回填为准；
报告正文的 526 记载属修复前候选事实，不追溯改写。

## 6. 两层自查

### 6.1 普通自查

- [x] F01 先复现后修复：首次失败输出全文保留（§1.1）；未删测试、未放宽权限、未取消
      REQUIRED、未伪造通过——新单测是**收紧**（要求拒绝），修复前其必然红、修复后绿。
- [x] 同族路径逐项审阅并留表（§1.2），仅一处根因，无遗漏同类门控。
- [x] 修改面最小：1 处代码分支重构＋1 条单测＋2 处注释/台账文字；未触碰授权、journal、
      网关、runs.rs、canon.rs 行为面；无 commit/push；未操作用户数据、无外发。
- [x] 指定重跑集合全部执行且全绿（除已核证环境项）；fmt/clippy/check-contracts/
      check-boundaries exit 0。
- [x] F02 注释与 PROTOCOL_SPEC §9、canon.rs 头注释三处口径一致；node 侧既有用例与
      跨语言证据脚本复跑通过。
- [x] O04 数字可复核（来源链写入台账条目）。

### 6.2 对抗性自查（以「攻击本修复」的立场复核）

- **「修复只对顶层生效？」**——负例单测走的是属性级路径（`properties.tags`），即
  `normalize_value` 对象分支对子 schema 的递归调用点，与审查探针（注册+prepare 全链）
  同路径；顶层裸数组 schema 经 `normalize_arguments` 入口本就要求 raw 为对象
  （`ArgumentsNotObject`），不存在其他入口。
- **「带 items 的原有行为被改坏？」**——对照组 (4) 钉住带 items 的越界拒绝与界内通过；
  违例文案与检查顺序（maxItems 先于 minItems）与修复前逐字相同；41 套件＋十套件钉数
  全绿证明无既有消费者依赖旧的宽松路径。
- **「非数组值误伤？」**——长度检查仍在 `value.as_array()` 之内；`minItems` 遇字符串/
  对象值按 JSON Schema 语义不适用，行为不变。items 递归仍要求双方同时存在，非数组值
  不进入逐元素校验（语义正确）。
- **「预算/节点计数被改动？」**——`nodes` 扣减仅发生在 schema 节点访问与（有 items 时的）
  逐元素递归，重构未动计数结构；深度检查仍在函数顶部。
- **「default 填充出现新静默面？」**——无 items 数组 default 违反长度界现在以
  `ArgumentsInvalid` 暴露（fail-closed），与模块对 default 的既有声明一致；不存在
  default 绕过长度界的路径（default 与实参走同一 `normalize_value`）。
- **「F02 改注释偷改行为？」**——diff 仅头注释 6 行；roundtrip（12 样本字节相等）与
  cross-lang-canon-nonbmp（PARITY OK）在改动后复跑通过，行为零变化可证。
- **「O04 数字伪造前向一致？」**——527=526+1（新回归单测），任何第三方重跑可复算；
  来源链写入条目本身，数字不再是无出处孤值。
- **「环境失败被用来洗绿？」**——r00_management_leaves 独立隔离复跑并引用其自身 panic
  文案如实登记为环境项；41 套件 ok 的计数不含它，0 ignored。

## 7. 修改文件清单（本次修复轮）

| 文件 | 类型 | 内容 |
|---|---|---|
| `rust/crates/lingxi-kernel/src/toolcatalog.rs` | 代码＋测试 | L1318-1359 长度界移出 items 门；L2937-3058 永久负例单测 |
| `tests/migration/r01-t02/canonical-json.mjs` | 注释 | L5-10 UTF-16 码元序口径 |
| `docs/rust-tauri/R04/R04_TEST_MAP.json` | 台账 | L49 通过数回填＋来源链 |
| `docs/rust-tauri/R04/repairs/R04-T01_REPAIR_R1.md` | 报告 | 本文件 |

## 8. 结论

F01（必修）、F02、O04 全部处置完毕，同族路径核查无遗漏，指定重跑集合与门禁全绿
（除已独立核证的 macOS 防火墙环境项）。候选维持 R1 审查对其余交付面的判定不变，
仅本修复轮改动面需重审。

READY_FOR_REVIEW

— REPAIR-R04-T01-F01，2026-09-30
