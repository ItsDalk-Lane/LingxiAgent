# R08 准备地图完整性独立验收

日期：2026-10-21（Asia/Shanghai）。审查者：`r07_admission_review`，未实现本次地图生成器或完整性探针。

## 结论与适用范围

**PASS_PREPARATION_MAP_INTEGRITY_ONLY**：当前准备地图完整镜像既定原账；探针确实调用现有检查器，隔离缺口输入会被拒绝。没有发现阻止本项准备工具使用的问题。

这项通过只证明准备地图的内容完整性与固定生成结果一致，**不证明 R08 业务、迁移、回滚、真实界面、证据语义或正式阶段门禁通过**。R07 准入独审 F01/F02 的 FAIL 不变；T01—T08 依赖业务仍未放行。此审查未修改实现、原书、原账或 R07 证据，未修复 R07。

## 已审方法

已读 `scripts/rust-tauri/r08_map_integrity_probe.py`、`r08_leaf_map.py`、生成地图、主代理 `map-integrity-01` 回执及本阶段进度/阻塞记录。

探针将权威准备地图读入后，对每项缺口使用独立深拷贝，在系统临时目录生成候选 JSON。子进程加载**同一份** `r08_leaf_map.py`，只把 `DEST` 改为候选文件，参数固定为 `--check`，随后实际执行其 `main()`。没有替换 `generate()`、原账来源或内容比较逻辑；原始地图没有被修改。

现有检查器从原书/原账重新生成规范文本，并与候选逐字比较。任一差异都以 `R08_LEAF_MAP_STALE` 非零退出。探针同时核原始退出码与该错误标记，不能把异常启动失败、语法错误或任意非零都当预期拒绝。正常样本还要求输出 `runtimeStatus=NOT_STARTED`。最后重用未修改输入，验证负例没有污染原地图。

**必要边界**：这里验证的是“准备地图必须与原账及生成规则一致”。例如空证据伪 PASS 被拒绝，是因为本阶段准备地图应保持 `NOT_STARTED/UNBOUND`，不是已经建立了可接受真实 PASS 的逐断言证据验收器。正式 R08 的测试名匹配、空集合/跳过拒绝、实际副作用/存储/用户结果及独立判据，仍须在后续 runner 接线中实现并验证。

## 独立实跑与结果

候选提交：`86840d1700397d3dd620a41f2b884df4cddd5cc2`。环境：macOS 27.0.1 arm64，Python 3.14.3。新证据目录：`artifacts/rust-tauri/R08/map-integrity-review-01/`。

实际命令：

```text
python3 -B scripts/rust-tauri/r08_map_integrity_probe.py --evidence artifacts/rust-tauri/R08/map-integrity-review-01
```

探针整体退出 0，表示下表的预期全部命中；它没有吞掉下层错误，逐次原始退出码均保留。

| 案例 | 输入 | 原始退出码 |
|---|---|---:|
| R08-PREP-MAP-01 | 原准备地图 | 0 |
| R08-PREP-MAP-02 | 漏一个基础场景 | 1 |
| R08-PREP-MAP-03 | 漏补充叶，同时伪改数量 | 1 |
| R08-PREP-MAP-04 | 漏原断言 | 1 |
| R08-PREP-MAP-05 | 把原断言弱化为“只需成功一次” | 1 |
| R08-PREP-MAP-06 | 冒用前阶段通过并写 PASS | 1 |
| R08-PREP-MAP-07 | 证据为空却写 BOUND/PASS | 1 |
| R08-PREP-MAP-08 | 漏替代浏览器宿主义务 | 1 |
| R08-PREP-MAP-09 | 漏新增 Rust 存储登记 | 1 |
| R08-PREP-MAP-10 | 恢复原输入后仍为未验收准备态 | 0 |

8 个反例均真实返回 `R08_LEAF_MAP_STALE`，首尾两次正常输入退出 0。每项日志在该目录的 `01.log`—`10.log`；日志摘要已独立重新计算并与回执逐项一致。原准备地图 SHA-256 前后保持 `ff6639b0ad1d15a0a57ff22769d26903cff6758c81bd72725c176935ce434b98`。

主回执 `result.json` SHA-256：`bd3b628c7116da0b0fbf87143024f90383a5a772a8610014d9f2f0998e13ab8e`。独立来源/原断言核对结果另保存在 `review-receipt.json`，SHA-256：`72c14a716c298eb3677ee7ebd8bb37e28c8ba5d10136fc884520f35e9dee4e6c`。

## 独立核对原内容

没有仅从探针 PASS 推断地图正确。本审查另用只读断言直接比较：

- 地图的 126 个叶 ID 与 `FEATURE_STAGE_ACCEPTANCE` 中执行阶段含 R08 的集合相等，也与 `ACCEPTANCE_MAP` 镜像集合相等；逐叶原场景完整相等。
- 全部 281 条 `originalAssertion` 按原顺序逐字相等；未发现遗漏或弱化。
- 8 个任务、16 个基础场景与原书账一致；27 个 R07 laterShare 已包含在 126 叶内，没有重复计数。
- 所有叶仍为 `NOT_STARTED`，`antecedentEvidenceIsR08Pass=false`；所有断言仍 `implementationStatus=NOT_STARTED`、`executionStatus=NOT_STARTED`、`bindingStatus=UNBOUND`、证据为空。没有把准备结果写成产品 PASS。
- 地图登记的 12 个来源摘要均与磁盘一致。旧存储 69 类与兼容矩阵完整相等；新增 Rust 存储登记完整镜像 R02 注册表的一个 `rust-runs-db` 项，包含其 26 条 migration 登记。

新增存储的这一核对**不是运行时存储盘点**：注册表本身只有一个数据库项目，其 tables 字典也不是实际所有目录/文件/表的完整替代。`STORAGE_READINESS.md` 已明确要求后续枚举真实数据库及文件状态，含观测/外发/恢复日志等；本次删除整项登记的负例不能代领这些实际迁移与往返义务。

## 后续责任

正式 R08 尚未接入 xtask；地图的 `gateReadiness.registeredInXtask=false` 保持真实。后续必须继续完成每条断言的真实生产者、具名测试及服务/存储/界面结果，并用原指定新目录入口执行正式验收。不得将本报告、准备地图通过或 8 个预期红例换算为 R08 产品场景通过。

本次未执行真实用户数据迁移、正式发布或 R09 工作；未改变 R07 准入结论，也未把受阻部分标作完成。
