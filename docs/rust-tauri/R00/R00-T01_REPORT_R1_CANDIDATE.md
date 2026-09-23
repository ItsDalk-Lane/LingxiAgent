# R00-T01 固定基线与隔离工作区

**实施状态：READY_FOR_REVIEW**。本报告只覆盖 R00-T01；没有独立审阅结论，也不代表 R00 阶段放行。

## 修改内容与原因

- 新增 `BASELINE.json`：固定研究/实施 SHA、分支、工具链、依赖锁摘要、相关源码摘要、原有工作树状态和本次探针结果，供后续报告复核。
- 新增 `BASELINE_DELTA.md`：研究 SHA 与实施 HEAD 相同；明确区分启动前已有的旧任务书删除、新任务书未跟踪文件、本任务成果和总控账本。
- 在 `artifacts/rust-tauri/R00/T01/` 留下可重跑的启动/工作树探针、进程/目录摘要、沙盒拒写负向结果与测试输出。未修改生产运行代码、依赖、用户配置或真实数据。

## 真实入口与选择

现有 `npm run server` → `scripts/launch.js` → `scripts/dev-env.js` 会覆盖传入的 `LINGXI_HOME` 为 `~/.lingxi-dev`。本次调用真实服务链 `server/bootstrap.ts` → `server/main-full.ts` → `server/index.ts:startServer` → `ensureFirstRun` / `LingxiEngine.init`，与桌面开发态服务使用同一组合入口。服务进程参数、启动日志和隔离目录内生成的 `server-info.json` 相互印证；经随机 loopback 端口请求 `/api/health` 得到 200 和 `status=ok`，SIGTERM 后退出码为 0，`server-info.json` 被清除。

隔离目录：`/tmp/lingxi-r00-t01-bdmbvepw/isolated`，实际 `LINGXI_HOME`：`/tmp/lingxi-r00-t01-bdmbvepw/isolated/lingxi-home`。临时 `HOME` 与 `TMPDIR` 同属该目录树；启动后该 `LINGXI_HOME` 有 56 个文件、46 个目录，包含 `logs` 和启动生成的数据；默认 Desktop 工作区也位于临时 `HOME`。外部模型/账号未配置，启动日志出现“无可用模型”的预期诊断，因此本探针不证明模型请求。服务只进行了本机 HTTP 健康检查，没有真实供应商调用。

## 逐项验收

| 场景 | 预期和实测 | 命令、结果、证据 |
|---|---|---|
| R00-A01 隔离目录可证明 | **PASS（本机）**。受控生产样例目录和只读哨兵启动前后摘要一致；真实 `~/.lingxi` 与 `~/.lingxi-dev` 的递归元数据清单摘要前后一致，真实生产目录现有文件的内容哈希一致。服务写入真实用户 HOME 的负向探针得 `EPERM`；实际数据/日志在临时目录。服务正常启动、健康检查 200、正常退出。 | `python3 artifacts/rust-tauri/R00/T01/startup_probe.py`，退出码 0；`sandbox-exec -f <probe.sb> node -e <写入拒绝探针>`，拒写断言退出码 0；`startup-probe.json`、`sandbox-denial.txt`、`isolated-tree-summary.json`、`server-stdout.txt`、`server-stderr.txt`。 |
| R00-A02 脏工作区不丢失 | **PASS（本机）**。旧任务书 17 个已跟踪删除保持删除；新任务书 33 个未跟踪文件的逐文件 SHA-256 一致；本任务没有引入其他已跟踪文件改动，也未写总控账本。 | `python3 artifacts/rust-tauri/R00/T01/worktree_snapshot.py before` 与 `... after`，均退出码 0；`worktree-before.json`、`worktree-after.json`、`worktree-preservation.json`。 |

本次验收候选为 `7d1a0c6bc28062ff455adcf8f68c3a80b117e90d` 加工作区原有任务书改动及本任务文档/证据；macOS 27.0 arm64、Node v24.16.0、npm 11.13.0。`package-lock.json` SHA-256 为 `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`。两项结果只绑定此候选与环境。

## 回归与失败记录

`HOME=<独立目录> TMPDIR=<独立目录> ./node_modules/.bin/vitest run tests/startup-contract.test.ts tests/hana-runtime-paths.test.ts tests/server-composition-boundary.test.ts`：退出码 0，3 个文件、26 个测试通过。它们保护启动脚本覆盖规则、路径解析和真实 full composition 路由。完整原始输出在 `targeted-tests.txt`。

最终 `git status --short --untracked-files=all` 分类：原有旧任务书删除 17 项、原有新任务书未跟踪 33 项、本任务文档 3 项、本任务证据 25 项、总控账本 1 项；没有其他已跟踪源码差异。原始输出在 `git-status-final.txt`。证据文件的引用、SHA-256 和实际进程退出检查均通过，见 `artifact-integrity.txt`。

另试验一次全局拒写再放行临时目录的 macOS 沙盒配置：临时目录的 `mkdir` 仍被拒绝，服务退出码 1。其根因是试验的沙盒规则挡住了预期写入；失败及输出留在 `startup-probe-attempt2-deny-all.json` 和对应 `.txt`，最终使用只拒写真实用户 HOME 的配置重跑通过。没有修改产品来掩盖该试验失败。

## 未验证与风险

- 真实 Lingxi.app 与生产服务持续运行。真实目录两次只读清单相同，沙盒也拒写整个真实用户 HOME，足以证明本次受控服务没有写入那里；但并发应用在扫描间写入又恢复无法由目录摘要排除。真实目录没有按场景字面新增哨兵，以遵守“不触碰真实用户数据”；只读哨兵位于受控生产样例目录。目录摘要基于路径、类型、权限、大小和修改时间，并非读取全部真实用户文件内容。
- 未启动 Electron 桌面界面或正式安装包；未验证 Windows、Linux、macOS x64、真实模型/Bridge/外部协议。没有执行全量 typecheck/lint/build/npm test，因为本任务只增加文档和验收探针，且已对实际启动链和相关测试做针对验证。
- 没有提交、推送、创建 PR/tag 或发布；没有迁移真实用户数据。独立审阅尚未执行，实施者只标 READY_FOR_REVIEW。

## 独立验收重点

审阅者可从 `startup_probe.py` 核实沙盒范围和实际服务入口，重跑健康检查并检查 `server-info.json` 在关闭后消失；再比对 `worktree-before.json` / `worktree-after.json` 的原有两组任务书哈希，确认总控账本和生产代码未被本任务编辑。必要时检查两次真实目录清单摘要与受控哨兵哈希，以及失败的全局拒写试验未被误记为产品缺陷。
