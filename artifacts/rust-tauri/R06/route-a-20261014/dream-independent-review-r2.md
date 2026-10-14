# Dream 独立 R2 生命周期复核

结论：PASS_SCOPED_LIFECYCLE_ONLY。R1 的“删除失败后存活助手核心永久停用”已修复。此次只确认停止/取消/结算/重新运行生命周期，不给完整删除流程、整叶或阶段总体 PASS。

## 候选绑定

原 R1 独立树 `/private/tmp/lingxi-r06-dream-review` 应用增量 `29fac9faca2b09b85f98bc7d9d475845bb5a5996bcb304274afdd7291057eae6`；三个候选文件 SHA 全与作者清单一致。运行前后已保存的全部 Rust 源 SHA 不变；清单 `/tmp/r06-dream-independent-r2-source-sha.json`。原 R1 三个独立测试文件及断言原样保留，并在同一候选重新执行，不改失败预期为成功预期。

cargo 1.98.1、--locked，真实已安装工具缓存、合成 HOME/TMPDIR/LINGXI_HOME、独立 fixture product，target 仍在仓库外。只替代外部供应商 HTTP；存储、核心、权限、仲裁、请求消费者和退出接线实际执行。主树、门禁、白名单、原义务文字未修改，未提交或推送。

## 实际复验

- 新候选 `r06_dream_completion` 17/17 PASS，49.89 秒；原 `r06_t06_memory` 7/7 PASS，0.61 秒，完整运行 exit0。日志 `/tmp/r06-dream-independent-r2-original.log`。
- 原独立 R1 三个探针原断言 3/3 PASS，7.61 秒，exit0。故障 DELETE 仍真实500，而随后 status200/coreAvailable=true、run202，核心可以启动；最后助手拒删保全正在等付费答复的原运行；只改 today 的来源竞态拒绝发布、真实重试成功。日志 `/tmp/r06-dream-independent-r2-experiments.log`，原探针源 `/tmp/r06-dream-independent-experiments.rs`。
- 新强付费案例独立复跑通过：全树真实 uchg 拒删，A 原调用待答期间 DELETE500；A 状态仍200/旧runId，重复开始409；B 真正五步完成。解锁/答复后 A 原调用 Reported usage 和 transport_attempts=1 保留、旧任务 failed/dream_cancelled，原 memory lease 释放之后，新 runId 真五步成功。旧 A 一调用 + B 五调用 + 新 A 五调用 = 11，取消旧任务没有复活。
- 真成功删除仍不重新建 A 目录；真实退出等待已付费调用结算、保留取消终态；这两项原回归也独立通过。

源码核对：`delete_agent_files` 在 shared active 门内先取消旧任务，物理删除成功才加入永久 stopped；失败保留既有运行的取消，不伪恢复旧任务，也不解除原来已停止/全局退出状态。最终状态的 roots 检查及 persist 也在同一门内，修复“检查后删除、随后状态 mkdir 重新建目录”的竞态。

## 仍存在的独立真缺口

生命周期恢复不能代替数据完整性。已在同一 R2 候选独立复跑作者完整性探针：仅顶层目录 uchg，真实 DELETE500，agentDir/config 仍在，但 memory/facts.md 与 longterm.md 均已被 remove_dir_all 删除。日志 `/tmp/r06-dream-independent-r2-partial.log`。该探针 exit0/1 passed 只表示缺口确实复现，绝非产品已修复；其显式断言要求 factsExists=false。因此顶部拒删案例随后 run202 只证明核心接受新任务，不证明丢掉的正文保全或成功整理。全树 uchg 的强付费案例也仅用来隔离生命周期，不代替顶部只读故障的保全证明。

R3 可恢复/逻辑删除及数据保全仍 OPEN；整体删除成功/失败语义不得签总体 PASS。0B483D7F6A43 的自动 ticker 宿主与实际暂停恢复仍未实现，保持 PARTIAL。未运行全阶段门禁、其他平台、正式打包或实际付费供应商账单。

机器结论 `/tmp/r06-dream-independent-r2-review.json`。
