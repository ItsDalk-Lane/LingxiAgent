# BLOCKED — 0.1.23 发布摘要生成

## 阻塞项

- `release-digest.v1.json` 与 `release-digest.v2.json` 当前头部仍是 `0.1.3`，不能通过 `v0.1.23` 的发布校验。
- 项目规则要求摘要由 `scripts/generate-release-digest.mjs` 维护、不得手工改条目；生成器已切换到 DeepSeek Responses API，但当前环境没有 `DEEPSEEK_API_KEY`。
- 本任务源码与门禁已提交为 `cf0be5bc`；DeepSeek 迁移在校验通过并提交后，摘要来源必须重新采集完整的 `v0.1.3..HEAD`。

## 影响

- 不影响源码修复、升级 smoke、类型检查、代码检查或本地构建。
- 会按设计阻断 `v0.1.23` Release；在摘要生成并校验前，仓库是“代码与门禁就绪”，不是“可立即打 tag”。

## 解除方式

1. 在当前进程可读取 `DEEPSEEK_API_KEY` 后运行（默认模型为 `deepseek-v4-flash`）：
   `node scripts/generate-release-digest.mjs --tag v0.1.23 --previous-tag v0.1.3 --ref HEAD`
2. 运行：
   `node scripts/validate-release-digest.mjs --tag v0.1.23 --file release-digest.v1.json`
3. 再运行：
   `node scripts/validate-release-digest.mjs --tag v0.1.23 --file release-digest.v2.json`
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
