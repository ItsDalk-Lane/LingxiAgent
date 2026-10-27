# R07 叶绑定剩余账（机器生成，只读）

- 真实测试池（门禁套件日志）：1055 条通过测试
- 叶：408；已逐断言绑定 408（1050 断言）；未绑定 0（0 断言）

## 未绑定叶缺口类型


## 按域

- BRIDGE_ADAPTER：BOUND 5 / UNBOUND 0
- BUILTIN_PLUGIN_ADAPTER：BOUND 2 / UNBOUND 0
- BUILTIN_PLUGIN_TOOL：BOUND 13 / UNBOUND 0
- CLI：BOUND 11 / UNBOUND 0
- DESKTOP_BEHAVIOR：BOUND 10 / UNBOUND 0
- PLUGIN_TOGGLE：BOUND 2 / UNBOUND 0
- PROVIDER：BOUND 39 / UNBOUND 0
- ROUTE_BEHAVIOR：BOUND 23 / UNBOUND 0
- SCHEDULER：BOUND 3 / UNBOUND 0
- SEMANTIC_EFFECT：BOUND 256 / UNBOUND 0
- SLASH_COMMAND：BOUND 6 / UNBOUND 0
- TOOL：BOUND 20 / UNBOUND 0
- UI：BOUND 16 / UNBOUND 0
- UI_BEHAVIOR：BOUND 1 / UNBOUND 0
- WS_IN：BOUND 1 / UNBOUND 0

## 缺口类型含义与补齐路径

- `candidates_exist_needs_semantic_review`：域内已有 ≥ 断言数的真实通过测试候选，需逐断言语义审核后写入 R07_LEAF_BINDINGS.json（测试名须与断言条款直接对应，叶内不复用）。
- `insufficient_distinct_tests_needs_negative_or_positive_probe`：候选少于断言数，通常缺「拒绝/无效输入」负向探针或「成功效果」正向探针，需补具名测试。
- `no_real_test_candidate_needs_new_probe`：无任何域内候选，需为该路由/操作新增正反向探针测试。
- `client_behavior_needs_ui_or_cli_suite_in_gate`：断言属客户端（UI/CLI/桌面）行为，Rust 服务套件结构性不覆盖；需把 vitest/CLI 套件接入正式门禁作为生产者，或按已有依据划定阶段份额。

逐叶明细见同名 .json（candidates 为关键词候选，仅供审核起点，不是绑定依据）。
