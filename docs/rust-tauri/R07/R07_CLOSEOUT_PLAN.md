# R07 收口计划（任务 0 记录，2026-10-19）

1. 目标：关闭审查确认的 A（观测链）/B（Bridge /loop）/C（Loop 媒体授权）/D（A23/A24 证明力）缺陷，408 叶（1050 断言）逐断言绑定后门禁 overall 真实转绿，取得 R08 准入。
2. 基线：HEAD bfe32d107 与审查基线一致，工作区干净；verify-r07-closeout-baseline 复跑确认现状（24 基础 PASS、408 叶 BLOCKED、overall FAIL）。
3. 剩余账：R07_CLOSEOUT_LEDGER.json（408 叶：CONVERT_REGISTRY 181 / BIND_EVIDENCE 180 / SHARE_DOC_AND_BIND 47）。
4. 顺序：D 门禁机器先行（扫描器/合同/聚合生产者/反例自检）→ A/B/C 实现修复 → 逐域绑定 authoring → 正式门禁 → 反向验证红→绿 → 独立审查 → 交付物。
5. 风险：整轮门禁约 48 分钟，迭代轮次受绑定准确率制约；A/B/C 修复可能暴露下游缺口；52 跨阶段叶的 R07 份额拆分需逐叶语义判断，禁止整叶移走或提前拉回。
6. 门禁命令（仓库根，证据目录逐轮全新）：cargo run --manifest-path rust/Cargo.toml -p xtask -- verify-stage R07 --evidence artifacts/rust-tauri/R07/T12/verify-r07-closeout-<NN>
7. 不弱化原条款；历史失败证据保留；不提交不推送（无新增授权）；真实账号/跨平台/正式包继续保留。
