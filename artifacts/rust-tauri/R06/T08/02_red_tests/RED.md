# R06-T08 RED 记录（真实失败先于 GREEN）

任务为行为评测（无产品功能新增）；RED 分两层：

## 1. 产品缺陷 RED（T08-FIX-01，subagents.rs agent 身份/lane 混淆）

- 首轮全评测套件运行（`red_first_full_suite_run.txt`，真实退出码 101）：
  10 任务中 6 失败。其中 **BE-08 失败即产品缺陷的真实暴露**：
  - 现象一（首轮）：`child run never settled`（child run 从未创建——delegation
    请求 target 为 bare name，在网关 prepare 层被拒，评测侧随后按 R04 词汇
    修正为注册表 target id）。
  - 现象二（修正 target 后，BE-08 单跑）：child run 创建但
    `settled as failed (failed.provider_error)`——真实缺陷暴露：
    subagents.rs 把配额 lane 字符串 `subagent:{agent}` 传给 drive_run 的
    agent_id 形参，T01 编译器以其解析代理材料 → `agents/subagent:{id}/`
    不存在 → 响亮编译失败。生产 wired 链上**所有**子代理 run 必然失败
    （现役：子代理获得父代理裁剪后的上下文）。
  - 每次失败的 panic 原文与退出码见本目录 `red_first_full_suite_run.txt`
    与会话执行日志（`grep -A3 panicked`）。
- 修复（RED→GREEN）：`subagents.rs` agent 身份与并发 lane 分离（见
  BEHAVIOR_EVAL.json `defect_found_and_fixed[0]` 与 07_diff_ledger D-T08-1）。
- GREEN：BE-08 过 + R03-T06 五套件 30 用例全绿（05_targeted_tests/
  subagent_regression.txt，exit 0）+ workspace 全量。

## 2. 评分器鉴别力 RED（A15 存在证明）

- BE-10 在套件内**恒久**内嵌一个「参数合法但语义指向错误」的替身行为
  （任务要求改 a.md、替身写 b.md），断言评分器输出
  {toolSelection=true, schemaValid=true, semanticParamsCorrect=false,
  taskComplete=false, overall=fail}——若评分器混算（schema 过即算成功），
  本断言即红。这是常驻的反例鉴别力自证，等价于变异测试的常驻化。
- 评测侧断言修正记录（按现役金线改锚，非放松）：
  - BE-02 首轮误以 wire `description`（= display_name「write」本地名）
    断言「描述不进提示词」→ 改锚为 schema 独特字面量
    （oldText/newText/additionalProperties）不进系统提示词（T01 契约）。
  - BE-05 首轮误锚 `resource_outside_write_scope` → 实际生产链词汇为
    网关资源域派生拒绝 `resource_scope_refused/resource_access_denied`
    + 授权闸原文「not inside any authorized root」→ 按真实链改锚。
  - BE-03 首轮路由替身在拒绝后循环重放同调用 → turn_budget_exceeded →
    替身改为拒绝后如实解释（镜像可解释受限行为）。
  - BE-06 首轮对 gemini 2.5 族带 size → 族规则响亮拒绝（现役规则）→
    腿内按族条件修正（prompt/n 两族恒等；size 为族条件参数）。
  - BE-07 首轮等待 ingestion `done` → 无嵌入模型配置的终态是
    `pending_embedding`（T07 语义，FTS 已可查）→ 两终态皆接受。
