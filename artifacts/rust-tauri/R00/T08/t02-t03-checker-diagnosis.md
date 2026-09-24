# T02/T03 检查器在当前 HEAD 的诊断（R00-T08 取证）

时间：2026-09-24｜HEAD/Task base：`e0b7be6108c4d5bc873061dee279b7163ca78a41`｜本目录同侪日志：`t02-negative-checks-at-head.log`、`t03-validate-at-head.log`、`t04-validate-at-head.log`

## 结论

1. **T02 `r00_t02_inventory.py --negative-checks` 退出 1（STALE）——纯 `tested_sha` 戳差异，盘点内容零漂移。**
   取证方法：在本机以 `__main__` 之外命名空间执行 `build()`（只读），将三份再生成输出与冻结交付逐行 unified diff。结果（会话原始输出转录）：

   ```text
   FEATURE_INVENTORY.json DIFF lines: 2
      -  "tested_sha": "16aeb380d58d68ff1a38bb46f5cc5d18f985f084",
      +  "tested_sha": "e0b7be6108c4d5bc873061dee279b7163ca78a41",
   FEATURE_STAGE_ACCEPTANCE.json DIFF lines: 2
      -  "tested_sha": "16aeb380d58d68ff1a38bb46f5cc5d18f985f084",
      +  "tested_sha": "e0b7be6108c4d5bc873061dee279b7163ca78a41",
   ENTRYPOINT_COVERAGE.json DIFF lines: 2
      -  "tested_sha": "16aeb380d58d68ff1a38bb46f5cc5d18f985f084",
      +  "tested_sha": "e0b7be6108c4d5bc873061dee279b7163ca78a41",
   ```

   即 736 生产功能、验收映射、入口覆盖的全部内容在当前 HEAD 逐字节一致；STALE 是该工具「树移动即需重绑戳」的设计语义（`--write` 为再绑路径），非内容性回归。该 STALE 在本任务会话开始前的 HEAD 即存在（戳差异与 T08 改动无关），分类：预存（相对 T08）。独立复核方式：在任意工作副本执行 `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks`（得同样 STALE 行），再按上法或 `--write` 后 diff 验证唯一差异为 tested_sha。

2. **T03 `r00_t03_validate.py` 退出 1——冻结导入图（1664 文件）与当前受控源集（1677 文件）计数不等。**
   根因：T03 冻结后 T05/T06/T07/T08 陆续新增 TS/JS 源文件（tests/migration 五件+守卫负向测试、scripts/rust-tauri 十四件等）未入冻结图。校验器的 scope 一致性断言按设计失败；T03 的 PASS 证据绑定其验证树（`60dbe0384e…`/`ffcb85830…`），不因后续新增文件失效。分类：预存（相对 T08，自 T05 提交起即如此）。处置：需要新鲜导入图时按 T03 报告的重生成路径执行（属后续迁移任务输入），R00 封存期不重写冻结图证据。

3. **T04 `r00_t04_scan.py --validate` 退出 0（R00_T04_SCAN_OK）**——STORES/ENTRYPOINTS 与当前源码一致性在 HEAD 仍绿。

## 对 R00 放行的影响

三项均不阻塞：T02 内容零漂移（证明如上）、T03 为冻结证据与树增长的设计性错位（重生成属后续任务）、T04 通过。R00-A15 登记的预存失败（审计封印家族）另行见 `R00_REPORT.md` §7。
