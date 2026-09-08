# 置顶信条（Pinned Tenets）迁移与恢复指南

适用：F3/F9（P1 阶段）迁移链路的**合成数据验证已完成**；真实用户数据迁移在本任务中
被明确禁止执行。本指南说明工具的适用条件、dry-run、批准清单、备份、冲突与回退边界。

## 1. 何时需要本指南

- 旧版（父基线 3b40fbe6 语义）agent 的 `pinned-memory.md` 生效内容需要进入新版
  `pinned-memory.json`（首次迁移由服务端 `core/pinned-tenets-migration.ts` 自动执行）。
- 自动迁移曾失败/中断（崩溃注入点：receipt:prepared、archive:before、commit:before、
  completed:before），或用户误删新版条目但旧 markdown 仍在，需要**选择性**恢复。

## 2. 一次正常迁移发生什么（自动，无需手工）

1. 读取 agent 目录旧 `pinned-memory.md` 权威条目（来源分类：user_direct 用户手写 /
   model_proposed 模型提议）。
2. 逐条转换进入 `pinned-memory.json`；active 上限 user_direct=200、model_proposed=20，
  **两个来源分别计数、互不占用**（F9）。
3. 旧文件归档为 `.migrated`（不删除）；写迁移收据（receipt）标记 completed。
4. 任一步失败：迁移不标完成、归档与收据保持一致状态，重启后按收据幂等续跑
  （不重复导入、不复活迁移后已删除的条目——M10 三处崩溃注入验证）。

## 3. 选择性恢复工具（pinned-tenets-recovery）

```bash
# dry-run（默认，只读）：扫描所有 agent 的可恢复候选并打印 JSON 报告
node scripts/pinned-tenets-recovery.mjs --home <LingxiHome>

# 应用恢复：必须提供人工批准清单
node scripts/pinned-tenets-recovery.mjs --home <LingxiHome> --apply --approval <approval.json>
```

- `--home`：目标 LingxiHome（默认 `~/.lingxi`）。
- dry-run 报告按 agent 列出候选条目：来源、内容预览、`contentHash`（批准清单按哈希
  精确到条目，不按位置/时间猜测）。
- 批准清单 `approval.json` 形状：`{ agentId, source, decisions: { [contentHash]: true } }`
  ——只恢复你逐条勾选的哈希；未勾选的一律不动。
- 恢复写入同样走 20/200 配额与重复检测（与内容哈希相同的条目不会二次导入）。

## 4. 备份与安全边界

- 迁移/恢复**从不删除**旧 markdown：原文件只会被归档为 `.migrated`。
- 应用恢复前建议自行复制一份 `<LingxiHome>/agents/<agent>/` 目录快照（工具自身不
  另行建备份，保持写入路径单一）。
- 工具不会触碰收据为 completed 的历史迁移（除非旧文件被移回非 .migrated 位置形成
  新候选——此时按新候选处理，仍需逐条批准）。

## 5. 冲突处理

- 旧条目与新版现有条目内容哈希相同：跳过（不重复）。
- 同旧文件内重复条目：按迁移规则去重后计一次。
- 配额满：user_direct 超出 200 / model_proposed 超出 20 的部分进入候选报告但不自动
  写入，由批准清单显式取舍。

## 6. 回退边界（P10.3 对齐）

- **代码回退**：以本次修复补丁/授权提交为单位，不覆盖他人改动。
- **数据回退**：不能只还原代码——tenets 新增/删除、收据、`.migrated` 归档要分别处理。
  仅当确认快照之后该 agent 没有新的用户写入时才可整体还原数据快照；有后续写入时先做
  增量比对恢复，禁止旧备份直接覆盖。已 completed 的迁移收据不得删除来强迫重跑
  （收据是幂等与“不复活已删除内容”的依据）。
- 授权/权限与 tenets 无关；本工具不触碰系统权限。
