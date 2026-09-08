applicableSource: `89bc0b64bf0a9b84ef3532efaa66c23213affb70 worktree`

replacedBy: `round2`

## S11 最终交付状态

R01–R10 本地实现/联合验证关闭；受测源码候选提交后的审计封印状态以根目录 `PROGRESS.md` 和 Git guard 为准。整体发布验收仍受 notarization 凭证、真机/供应商/跨平台验收阻塞。

最终源码摘要：`f0a527e5637c9c3e286ae30106ad6e23beb0e46c8e284a4ec810ca8a756fcac7`。

| 验证 | 真实结果 | 日志（round2/logs） |
|---|---|---|
| X2 联合冻结 | 25 文件 / 321 passed | s11-joint-freeze.log |
| typecheck | PASS | s11-typecheck-final.log |
| lint | exit 0，9203 warnings | s11-lint-final.log |
| 原样 npm test | 13383 passed / 1 failed / 7 skipped，exit 1 | s11-full-test-final.log |
| 干净 89bc0b64 同环境 audit seal | 2 passed / 1 failed | s11-audit-seal-baseline.log |
| 排除基线 seal 的补充全量 | 13381 passed / 7 skipped，exit 0；不替代原样门禁 | s11-full-test-excluding-baseline-seal.log |
| client 构建 | PASS | s11-build-client.log |
| server 构建 | 初次缺 key 失败；临时合成 key 重跑 PASS，key 已删除 | s11-build-server.log / s11-build-server-synthetic-signing.log |
| speech helper / permissions 构建 | PASS | s11-build-speech-helper.log / s11-build-speech-permissions.log |
| pack | 从无 dist-speech 开始，ad-hoc 签名及验证成功；最终 exit 1 | s11-pack-synthetic-signing.log |

原样全量唯一失败为 post-verification audit seal。verified SHA `04f90d2b` 到基线 HEAD `89bc0b64` 已有 113 个已提交差异；干净基线复现同一失败，不能修改白名单或宣称完整门禁通过。pack 最终因缺 `APPLE_APP_SPECIFIC_PASSWORD` / `APPLE_ID_PASSWORD` 失败，notarization 为 BLOCKED，不能把签名阶段成功写成 pack 全流程通过。

真实 macOS 权限/转写、真实供应商、Windows/Linux/其他架构均 BLOCKED；本地构建与合成数据不能替代这些验收。聊天工具栏上没有增加语音输入的组件是用户要求的。后端 system-speech 模型解析、上传转写、错误码、原生音频附件、当前快捷键、ASR/TTS/朗读与宿主授权桥保留，旧前端听写不恢复。真实用户恢复仍受 BLOCKED_HOME_OWNERSHIP 及单独数据授权限制。

补丁为 `patches/89bc0b64-to-r01-r10-source.patch`（相对 round2）；文档收尾后还会重生成，大小与 SHA-256 以主代理最终 readback 为准。此前 S10 数值仅为阶段历史，不作为最终补丁身份。无提交、推送、合并、发布或真实用户数据迁移。

---

以下为恢复说明或历史交付背景；当前结果以上方 S11 和 round2 记录为准。

> applicableSource: `89bc0b64bf0a9b84ef3532efaa66c23213affb70 worktree`
> replacedBy: `round2`
> 状态：旧交付记录已被 round2 取代；下方历史内容不构成当前验收。

聊天工具栏上没有增加语音输入的组件是用户要求的。旧 F7 前端听写、D01–D12、X04 状态为 **SUPERSEDED_BY_PRODUCT_CHANGE**，不称当前通过，不恢复旧前端听写流程。

明确保留：后端 system-speech 模型解析、上传转写、错误码、原生音频附件、当前快捷键、TTS/朗读、宿主授权桥。当前快捷键按现行产品行为保留，不以旧听写快捷键合同复活废弃流程。

现行入口：[round2 修复报告](round2/R01_R10_REPAIR_REPORT.md)、[逐项测试矩阵](round2/R01_R10_TEST_MATRIX.json)、[真实执行记录](round2/COMMAND_RESULTS.json)、[阶段断点](round2/PROGRESS.md)。不在旧交付文件推定最终 S11 结果、补丁 hash 或当前 manifest。

旧日志 `logs/full-vitest-final.txt`、`logs/swift-core-tests.txt`、`logs/electron-node-bridge-load.txt`、`logs/helper-failclosed-smoke.txt` 当前均 **UNAVAILABLE**。旧测试数字、旧 PASS、旧本机工件或签名声明仅为历史文档原述，不能冒充 round2 当前通过。真实系统权限/识别、付费供应商凭证、目标跨平台验收仍按适用条件 **BLOCKED**；本机合成自动验证不等于真实权限或识别验收。真实用户恢复 apply 仍受 `BLOCKED_HOME_OWNERSHIP` 与单独数据授权约束。

## 当前恢复说明（R01/R02）

以下恢复说明沿用已更新的 round2 合同；其中操作示例不是执行授权，实际验收状态仍以现行矩阵为准。

# 置顶与原则迁移及本地恢复说明

本轮适用：`89bc0b64` 起的 R01/R02 未提交工作树。精确受测身份见 `round2/SOURCE_MANIFEST.json`；执行记录见 `round2/COMMAND_RESULTS.json`。当前仍处于实施验收，不能据此宣称真实用户数据已经迁好。

## 只读预览

```sh
node scripts/pinned-tenets-recovery.mjs --home <合成或经授权的LINGXI_HOME>
```

预览包含归档摘要、条目身份、当前状态和 `approvalTemplate`。模板默认全部 `skip`，只将明确要恢复的条目改为 `restore`；保留 schemaVersion、operationId、agentId、sources 完整 SHA-256、observedTargetHash 和每项 source/sourceEntryKey/contentHash。相同内容在不同归档中各有自己的来源身份。缺失或重复旧 ID 使用源摘要和条目序号。

批准后来源或目标变化会拒绝为 `stale_approval`，必须重新预览并获得新批准，不能偷偷刷新原单。相同 operationId 的批准内容不可改变。相同批准单重试返回原结果；完成后用户删除的内容不会被原单复活。

## apply 当前边界

```sh
node scripts/pinned-tenets-recovery.mjs --home <LINGXI_HOME> --apply --approval <approval.json>
```

**目前此命令明确返回 `BLOCKED_HOME_OWNERSHIP`。** 源码没有可供 Node CLI 复用且由所有 home 写入方共同持有的排他锁。关闭同 home 的应用是未来执行的必要条件，但不能仅靠人工确认或现有 server probe 冒充原子所有权；没有 `--force` 绕过。内部事务目前只在合成独占目录中验收。取得经过验证的所有权支持及另行数据操作授权后，才能解锁真实 apply。

## 收据与备份

首次迁移保留 `memory/pinned-tenets-migration.receipt.json` 语义；本轮使用 v3，读取兼容合法 v2。批准恢复单独保存在 `memory/pinned-recovery-operations/<operationId>.json`，其完成范围仅限该次批准。归档文件不会被恢复工具移动或删除。

备份位于 `memory/pinned-migration-backup/<operationId>/`，以完整摘要命名。目标存在时保留原字节；不存在由收据明确记录 absent。来源、目标及已有相关收据备份成功并验证后才写 prepared。备份包含完整记忆，只留本地受控位置，不进入补丁或普通日志。

迁移的归档目的地在复制前记录为 planned，使用独占复制、不覆盖目的地，回读完整摘要后才删除原名并记 done。中断后沿同一目标继续；冲突不覆盖、不自动重导。

旧 v2 completed 只表示历史记录，不补造 v3 验证，预览诊断为 `LEGACY_COMPLETION_UNVERIFIED`。旧 kind=recovery 全局收据不会封住其他预览候选，也不会授权自动迁移。未完成 v2 只有来源或唯一归档的完整摘要和逐项计划均能证实时继续；证据不足为 conflict。

## 恢复与回退

代码回退仅逆向应用本轮精确补丁，不覆盖其他修改。数据回退需分别核对目标备份、收据、归档映射；只有确认此后没有任何用户写入并获明确批准后，才能整体还原原 tenets。若已有后续写入，只能做经批准的增量恢复，不覆盖整库，不删除 completed 收据来强迫重跑。
