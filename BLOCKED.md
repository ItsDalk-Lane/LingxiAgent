# RESOLVED — 0.1.24 发布摘要生成

## 已解除

- `release-digest.v1.json` 已由 DeepSeek Responses API 生成 `v0.1.24` 摘要，`release-digest.v2.json` 已把 `0.1.24` 插入史册头部。
- 生成器默认使用 `deepseek-v4-flash`；真实响应暴露的 reasoning/output_text 顺序差异已用 `3655c238` 修复并回归。
- 两份摘要均已通过仓库校验器和人工事实复核，摘要生成阶段不再阻断发布。

## 解除结果

- 最终摘要从最后一个真实 GitHub Release `v0.1.3` 汇总到 `HEAD`，没有手工修改条目；错误声称 Linux 安装损坏的首稿已被人工复核拒绝并由生成器覆盖。
- v2 史册版本顺序为 `0.1.24 > 0.1.23 > 0.1.3 > 0.1.2 > 0.1.0`；失败 tag `v0.1.23` 作为透明历史保留，但没有对应 GitHub Release。

## 已执行步骤

1. 在临时进程读取 `DEEPSEEK_API_KEY` 后运行（默认模型为 `deepseek-v4-flash`）：
   `node scripts/generate-release-digest.mjs --tag v0.1.24 --previous-tag v0.1.3 --ref HEAD --release-notes-file <事实说明>`
2. 运行：
   `node scripts/validate-release-digest.mjs --tag v0.1.24 --file release-digest.v1.json`
3. 再运行：
   `node scripts/validate-release-digest.mjs --tag v0.1.24 --file release-digest.v2.json`
4. 两份校验均通过后，才能推送 main、创建 tag/Release。

---

# 历史记录 — `cce8e86..97595264` 对抗性审计与修复

## 不阻塞审计结论

- 6 节审计已全部跑满，没有因证据不足而跳过的审计项。
- 真实 xAI OAuth、真实远程审批供应商和真实 Ollama 服务未使用用户凭证做端到端测试；相关结论来自源码、官方接口契约、负向脚本和单元测试。

## 工作区外部变化

- 审计开始时主工作树已有 `BLOCKED.md`、`PROGRESS.md` 修改和未跟踪 `AUDIT-REPORT.md`；这些是前两次审计留下的材料，本轮已按独立证据重写。
- 本轮执行期间 `.gitignore` 被其他进程修改，先前可见的重名临时副本也被其他进程移除。本轮没有编辑或回滚 `.gitignore`。因此原任务“最终状态只多三份报告”的字面条件目前不能满足。
- 主工作树的 `node_modules/@types` 曾有 60 个带数字后缀的重名目录，污染类型检查和测试发现。基线改在干净临时工作树完成，避免把环境污染当回归。

## 任务文字与源码不一致

- 任务书称 `export-manifest.json` 有 4 个新增项，实际差异为 8 项；8 项文件都存在，生成边界检查通过，因此不是产物缺陷。
- 任务书要求全仓 `hanaFetch|hanako|Hana` 为零，当前受版本控制文件实测有 1934 处历史命中。范围内生产代码没有新增命中；范围内新增测试残留已作为 P3 报告并进入修复。

## 范围外观察

- `core/server-identity.ts` 和 `desktop/src/react/services/server-connection.ts` 的历史可见品牌默认值不计入六提交缺陷，但用户要求按报告修复所有问题后已一并改为 Lingxi；持久化指纹因此按兼容变更重生成。
- 审批权限的历史持久化键只描述能力种类、不描述具体目标，相关短路逻辑早于本范围；本次没有把它升级为六提交发现，也不会借本次修复扩大权限架构改造。

## 修复验证环境注记

- 主工作树仍有 60 个 `node_modules/@types/* 2` 重名目录，直接 `npm run typecheck` 会报 `TS2688`。未删除这些外部文件；在干净临时工作树应用相同代码差异后，三组类型检查均为 0 error。
- 最终全量为 1 failed / 10985 passed / 7 skipped；唯一失败仍是已确认预存的截图头像 SVG-vs-PNG 断言。
