# F54-CLOSEOUT-01 针对性负例自检记录（7 项）

日期：2026-10-08。载体：隔离副本 `/tmp/f54_negcheck/iso`（rsync 主树，剔除
.git/rust/target/artifacts/node_modules/.claude/dist*）；主树在负例期间零写入。
每项负例=把一类"虚假完整"欺诈重新注入隔离副本，验证收口后的机器层将其拒绝。

## N7（先跑的正对照）

命令：`cargo test --manifest-path rust/Cargo.toml --locked -p xtask r04_production_map`（cwd=隔离副本）
结果：`test result: ok. 6 passed; 0 failed`——未注入的隔离副本全绿。

## N1 不可调用 AST 工具标 full 必须被拒

注入：把 ast_edit 叶（R00-T02-LA-96DD1FF9E9D5，已修订为 R04+R07、share 分类）改回
full_original_behavior，只钉"可发现不可调用"案例（M-01 欺诈原样：工具可发现≠可调用）。
三个变体、三层闸门全部命中：

1. 变体1（1 组案例对 3 条断言）：
   `the registered R04 stage map must parse with this runner: "stage map invalid: supplemental leaf \"R00-T02-LA-96DD1FF9E9D5\" originalAssertionCases must cover all 3 original R00 assertions in order (got 1)"` → FAILED。
2. 变体2（3 组各钉一个真实但语义无关案例）：
   `stage map invalid: supplemental leaf "R00-T02-LA-96DD1FF9E9D5" original assertion #1 references unpinned case "matrix-lifecycle-disable-holes"` → FAILED。
3. 变体3（契约与分组完全自洽，仅分类漂移）：
   `assertion left == right failed: the full/share/deferred split drifted from the registered decision — left: (7, 48, 69), right: (6, 49, 69)` → FAILED。

## N2 普通终端回显证变量共享必须被拒

注入：run_code 叶（C88F29B5114A，REPL 变量跨调用存活）以 full 分类、仅钉
terminal-tail-cursor-continuation（普通终端回显/续读案例，与 REPL 变量共享无关）。
命令：`python3 scripts/rust-tauri/r04_t08_generate_stage_map.py`（隔离副本）。
结果：`AssertionError: R00-T02-LA-C88F29B5114A`（F54 closeout 不变量：FULL 叶必须
R04 独占；该叶已修订为 R04+R07），generator exit=1，不写图。

## N3 连接器握手证资源读取必须被拒

注入：资源读取叶（04A6A2BD1547，按 uri 返回资源内容）以 full 分类、仅钉
mcp-connector-register-handshake / mcp-connector-catalog-sync（握手与清单同步，
均不证明指定 URI 内容读取）。
结果：`AssertionError: R00-T02-LA-04A6A2BD1547`（同一 F54 不变量：该叶已修订为
R04+R07+R08，不得 full），generator exit=1。
（语义说明：在 M-01 世界——台账仍登记 R04 独占时——该欺诈能通过全部机器闸，
正是本轮逐断言语义审计纠正的对象；收口后该回退路径被生成器不变量+计数镜像封闭。）

## N4 删原始断言保留测试数必须被检测

注入：隔离副本 FSA 中删除 8A3C87812B4F 的一条原始断言（4→3），案例组数不变（4）。
结果：`AssertionError: R00-T02-LA-8A3C87812B4F: 4 case groups for 3 original assertions`，
generator exit=1。（图层另有 12 镜像字段核对：r00Assertions 台账↔图逐字段比对。）

## N5 给未完成项随意加未来阶段必须被检测

注入：两本 R00 台账给 2D194C1684BC 追加不存在的阶段 "R99"。
命令：`python3 docs/rust-tauri/R00/r00_t07_validate_ledger.py`（隔离副本）。
结果：`LEDGER-ERROR STAGE-PREFIX scenario=R00-T02-LA-2D194C1684BC unknown execution stage 'R99'`，
`LEDGER_INVALID errors=59 checks=14976`，exit=1。
（机器层拒绝臆造阶段名；真实阶段的延期权威性=每叶任务书条款引用+阶段审查，
本轮 40 个 D 叶均逐叶登记了条款依据。同轮输出的 STALE-SOURCE 为负例编辑的
预期连带，非收口对象。）

## N6 缺实际承接责任的延期必须被拒

注入：图内 2D194C1684BC 保留 share 分类但 r00ExecutionStageIds 仅 ["R04"]（无人承接）。
命令：`cargo test --manifest-path rust/Cargo.toml --locked -p xtask r04_production_map_keeps_the_124_leaf_split`（隔离副本）。
结果：`assertion left == right failed: leaf R00-T02-LA-2D194C1684BC is EXCLUSIVE to R04 — a stage_share_satisfied classification would leave an unowned remainder (F54) — left: "stage_share_satisfied", right: "full_original_behavior"` → FAILED。
（与 xtask verify.rs:919-944 的 F25 规则同构；正式门禁内在 verify 层再次强制。）

## 结论

7 项全部按预期拒绝/通过：N1/N2/N3/N4/N5/N6 六类欺诈分别被解析层断言、生成器
F54 独占不变量、生成器组数核对、台账校验器、镜像测试 F54 断言拒绝；N7 正对照
6/6 绿。机器闸封闭"分类漂移/虚假分组/臆造阶段/无承接延期"路径；"案例语义是否
真证明断言"由本轮逐断言语义审计（F54_SEMANTIC_AUDIT.json）人判并留痕。

## 观察项（非阻断）

生成器 HEAD 既有的覆盖断言诊断消息存在 `^` 优先级问题（`A | B | C ^ D` 实际按
`A | B | (C ^ D)` 解析，失败时打印混淆列表）；断言本身语义正确、集合相等时恒通过。
本轮不改（非 F54 范围），留档供后续维护参考。
