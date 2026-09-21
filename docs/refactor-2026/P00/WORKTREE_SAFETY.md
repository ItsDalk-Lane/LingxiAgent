# WORKTREE_SAFETY — P00 工作树保护声明

执行时间：2026-09-21 晚（本地）。执行分支：`docs/knowledge-closeout-2026-09-21`（未切换）。起点 HEAD：`92c6646c581883436e026c53898e8bfc34ce4d27`。

## 1. 进入时的既有状态（受保护，零改动）

| 项 | 进入时状态 | 退出时状态 |
|---|---|---|
| 已跟踪文件修改 | 0 个 | 0 个（本轮零 commit、零生产文件改动） |
| 未跟踪 `Lingxi_Refactor_Taskbooks_2026-09-21/` | 任务书原件 | 原样保留，未修改（MANIFEST 完整性由其自带 sha256 负责，本轮未重算） |
| 未跟踪 `Lingxi_Refactor_Taskbooks_2026-09-21_副本/` | 任务书副本 | 原样保留，未读取、未修改 |
| `~/.lingxi`（生产数据） | 存在 | 只读哨兵，P00-A03 快照证明零写入 |
| `~/.lingxi-dev`（开发数据） | 存在 | 只读哨兵，P00-A03 快照证明零写入 |
| git reflog / 分支拓扑 | — | 未 reset / 未 clean / 未回滚 / 未切换分支 |

## 2. 本轮新增文件（全部未跟踪，批准路径）

- `docs/refactor-2026/P00/**` — 文档与结构化结果（BASELINE/SCOPE/矩阵/报告）。
- `artifacts/refactor-2026/P00/**` — 证据：命令日志 JSONL 与原始 out/err、隔离 HOME（home-A）、fixture、性能样本、工具脚本（run-logged.mjs、isolated-server-proof.mjs、后续基准脚本）。

回退方式：整体删除上述两个目录即可，无 git 操作、无数据迁移。

## 3. 关键安全事实

1. **启动器覆盖**：`scripts/launch.js` 经 `applyDevEnvironment` 无条件覆盖 `LINGXI_HOME` → `~/.lingxi-dev`（命令 `P00-T01-launcher-override-proof`，`overridden=true`）。任何需要隔离 HOME 的测试必须绕过 launch.js 直连入口，或另行改造（属后续阶段决策）。
2. **隔离证明**（命令 `P00-T01-A03-isolation-proof-r2`，首次尝试 `P00-T01-A03-isolation-proof` 因脚本 find 占位符 bug FAIL，已保留）：直连 `server/main-full.ts` + 显式 env，合成写入 `PUT /api/input-drafts` 仅落隔离目录；真实哨兵 `find+stat` 快照（mtime+size+清单）前后零变化；server SIGTERM 正常退出。
3. **同宅互斥闸**：server 启动会检查同一 LINGXI_HOME 的既有内核（server/index.ts 同宅互斥段），隔离目录为全新空目录，不受影响。
4. **零真实模型调用**：本轮全部 API 调用为 input-drafts（无模型调用路径）；未配置任何真实凭证；未消耗任何账户预算。

## 4. 明确不做（本轮授权边界）

不改 UI、不升级依赖、不修改真实用户数据、不使用真实账户发消息或消耗预算、不 push/PR/tag/发布、不推进审计封印坐标（VERIFIED_SOURCE_SHA 保持 `adca95ca3`）、不修改历史日志与旧 PASS 证明。

## 5. 已知风险提示

- HEAD 领先审计封印坐标 2 个提交且含非白名单文件改动，`tests/post-verification-audit-seal.test.ts` 预计既有红（详见 BASELINE_TESTS / BASELINE_FAILURES）。该状态是进入本轮之前就存在的仓库治理状态，P00 只登记不修复。

## 6. 执行期间的两项工作区事件（如实登记，详见 BASELINE_FAILURES F5/F6）

1. **npm test 重写跟踪补丁（F5）**：round2 R10-09 测试按设计重生成 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` 并吸收未跟踪文件；已取证（`logs/tracked-patch-side-effect.stat.txt`）后 `git checkout --` 还原至 HEAD。最终状态零跟踪文件改动。后续阶段全量 npm test 后必须复查 git status。
2. **副本任务书目录消失（F6）**：`Lingxi_Refactor_Taskbooks_2026-09-21_副本/` 在会话开始快照中不存在、P00 首条命令时出现、终检时不存在；P00 无任何命令触碰它（command-log.jsonl 全量 argv 可复核），判定为用户侧外部操作，原任务书目录完好。
