# BASELINE_FAILURES — 基线失败分类与登记（P00-T05 / P00-A10）

日期：2026-09-21。原则：基线问题分【本阶段阻塞 / 后续主责阶段 / 范围外建议 / 治理流程】，不塞进 P00 修改。所有失败均可复现（命令ID 见 BASELINE_TESTS.json）。

## F1｜npm test 4 个用例失败（1 个根因）——治理流程（非生产缺陷）

- **现象**：`tests/post-verification-audit-seal.test.ts`×1 + `tests/round2-delivery-evidence.test.ts`×2 + `tests/round3-delivery-evidence.test.ts`×1，命令 `P00-T05-npm-test` exit 1；单独复跑 `P00-T05-round23-isolated` 确认。
- **根因（单一）**：HEAD `92c6646c5` 领先审计封印坐标 `.sync-audit/verified-source-sha.txt = adca95ca3` 两个提交，其中 docs 收尾提交改动了 7 个不在 seal 白名单内的文件：BLOCKED.md、CONTRIBUTING.md、README.md、README_EN.md、RELEASE_VERSIONING.md、docs/README.md、git-ui-relocation-mockup.html（删除）。三个测试文件都调用 `.sync-audit/verify-post-verification-diff.mjs`，报同一组文件。
- **定性**：既有仓库治理状态（进入本轮前已存在），与 P00 证据文件无关（未跟踪文件不进 `git diff VERIFIED..HEAD`）；非生产代码缺陷。
- **分类**：治理流程——推进 VERIFIED_SOURCE_SHA 需按 PROGRESS.md 封印工作流并获用户授权，P00 无权也不应推进。**对 P00 非阻塞**（基线可用：失败确定性可复现、其余 14686 用例绿）。**对 P08 阻塞**（发布门禁不得带红）。
- **最小修复方案**（供授权时参考）：按 PROGRESS.md seal 流程对候选提交跑全量门禁后推进坐标；或该 docs 提交若需拆分由用户决策。不得扩白名单掩盖。

## F2｜npm run lint 3 个生产 error——后续主责 P03（发布前必修）

- **现象**：`P00-T05-lint-r3` exit 1，3 error：`lib/tools/run-code-tool.ts:118,119`（no-control-regex，ANSI 剥除正则）+ `lib/tools/security-scan-tool.ts:191`（no-constant-condition，`if (found.length || true)` 恒真）。
- **来源**：提交 6282cf35b（2026-09-17，工作区批量收口——omp 迁移 14 件+环境依赖系统+按需工具加载+观测子标签+回退检查点；`git log -L` 核实为这三行的唯一历史提交）。CI 每腿都跑 lint（ci.yml:85）→ 当前 HEAD 推上远端 CI 会红。
- **定性**：既有质量门禁破损。ANSI 正则疑似刻意（需精确 `eslint-disable-next-line no-control-regex`）；`|| true` 恒真写法建议改为明确语义（如无条件 push 或注释说明）。
- **分类**：**对 P00 非阻塞**；主责 **P03**（工具阶段顺带修复，属生产文件编辑，需该阶段授权）；P08 前必须清零。
- **附注**：lint 首跑的另 27 error 由 P00 证据工具自身引起，已修复（首次失败保留于 `P00-T05-lint`），最终态我方文件 0 error。由此沉淀一条工程事实：`eslint .` 扫全仓含未跟踪文件，证据/工具类文件必须自守 lint 规范或由 P01 决定是否将 `artifacts/refactor-2026/**` 加入 ignores。

## F3｜四平台 CI 运行证据获取——BLOCKED（环境缺失）

- gh 已安装但 keyring token 失效且本机代理 127.0.0.1:7890 不通（`P00-T05-gh-ci-evidence`）。无法列出 ci.yml 当前运行记录；PROGRESS.md 中的历史 CI 绿只当旧证据。**不阻塞 P00**；P08 全产品验收时必须取回真实四平台证据。

## F4｜范围外建议（不进入本轮任何阶段任务）

- `.ephemeral`、`logs/` 等运行目录的 gitignore 完备性（与重构无关）。
- warning 基线（10688 条）治理——建议独立质量专项，不与架构重构混行。

## F5｜npm test 对跟踪文件的重写副作用——登记（工作区卫生）

- **现象**：终检发现 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch`（跟踪文件）被改写 +18067/-5236 行；取证确认其中吸收了 60 个 P00 新增未跟踪文件（证据 `logs/tracked-patch-side-effect.stat.txt`）。
- **根因**：`tests/round2-delivery-evidence.test.ts` R10-09「增量补丁重放并入库为记录」按设计重生成该补丁，重放基线含当前工作树的未跟踪源文件——**在存在额外未跟踪文件的树上运行 npm test 会改写该跟踪文件**。这是既有测试行为（半设计使然），非本轮引入的缺陷。
- **处置**：已用 `git checkout --` 还原至 HEAD 状态（还原前保存取证）；P00 最终状态零跟踪文件改动。
- **分类**：对 P00 非阻塞（已还原）；**工程卫生登记**——后续阶段每次全量 npm test 后须检查 `git status` 并在需要时还原该文件；是否把该测试改为只读校验或写入独立产物，属测试逻辑变更，主责建议 P08 回归阶段评估（需授权）。

## F6｜外部工作区变动观察——非 P00 造成，如实报告

- 会话开始的系统快照 git status 只有 `Lingxi_Refactor_Taskbooks_2026-09-21/` 一个未跟踪目录；P00 首条命令时多出 `Lingxi_Refactor_Taskbooks_2026-09-21_副本/`；终检时该副本目录已不存在（整个会话期间未有任何 P00 命令引用或删除它——全部 argv 见 command-log.jsonl；npm test 亦无删除仓库根未跟踪目录的测试，git-environment 测试仅用临时仓库）。
- **判定**：无法归因于 P00 工具链；时间线与"用户侧在会话前后对该副本进行了创建/删除操作"一致。原任务书目录完好未动。**提请用户知悉**；若非用户操作请提出，可查 command-log.jsonl 逐条复核。

## 安全阻塞项

**无。** 未发现权限放宽、跨主体污染、数据损坏、重复副作用类阻塞（本轮零生产改动，隔离证明 P00-A03 全绿）。

## 既有非关键问题登记（不阻塞，供后续阶段引用）

1. `cli/local-server.ts:5-15` LINGXI_HOME 解析双实现（主责 P01，OWNERSHIP_MAP 🔴）。
2. taskId 五种自铸格式并存 + 媒体任务双记账（主责 P02，OWNERSHIP_MAP 🔴）。
3. vitest `@hana/*` / tsconfig `@lingxi/plugin-*` 死别名指向已拆除的 packages/（主责 P01）。
4. `security-scan-tool.ts:191` 恒真条件（随 F2 由 P03 处理）。
5. `skills2set/lingxi-plugin-creator/` 仅 pycache 残留（清理授权后处理，非本轮）。
