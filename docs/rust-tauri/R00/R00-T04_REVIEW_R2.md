# R00-T04 独立验收报告（R2）

审查者：REVIEWER-R00-T04-R2（全新一次性独立验收任务，非原执行者、非 R1 验收者、非 R1 修复者，不依赖前轮聊天结论，全部证据本轮独立重取）。
审查日期：2026-09-24。本轮只新增本文件；未创建分支/worktree，未 commit/push/PR/tag/release，未接触真实用户数据/外发/付费 API，未修改候选、R1 报告、修复报告与总控账本。

## 0. 结论速览

**R00-A07：PASS（本轮独立复算成立）。R00-A08：PASS（四条链逐跳独立核实成立）。F01（BLOCKING）/F02（MAJOR）/F03（MINOR）均确认已解决。VERDICT: PASS**——R1 三项 finding 的修复经独立验证闭合，无新增阻断/重大 finding；候选 8 文件聚合 SHA-256 独立重算与账本一致且全部可正常纳入 Git。本 PASS 不构成提交/推送授权，候选入库仍由总控按获准流程执行。

## 1. 验收边界与基线核对

- **tested HEAD**：`ffcb85830ffdc75da5fe39b7a8cfddb4617f1aec`（`git rev-parse` 实测；=Task base=账本 `candidate_current_head`）；分支 `codex/rust-tauri-migration`；`origin/codex/rust-tauri-migration` 本地远端引用同 SHA。
- **工作区**：仅总控账本预先存在的未提交修改（`ORCHESTRATOR_PROGRESS.json`，47+/17-，本轮只读）+ T04 候选 8 文件（未跟踪）+ R1 报告/修复报告；`docs/rust-tauri/R00/__pycache__/`、`.mimosa/` 为 gitignore 运行产物，不属候选。本轮结束时 8 文件聚合哈希复算仍为账本值，候选零扰动。
- **候选冻结指纹独立重算**：8 文件（`artifacts/rust-tauri/R00/T04/{deliverable-hashes.txt, final-run-stdout.txt, scan-output.json}` + `docs/rust-tauri/R00/{ENTRYPOINTS.json, OWNERSHIP_CURRENT.md, R00-T04_REPORT.md, STORES.json, r00_t04_scan.py}`）按路径排序、每行 `SHA256␣␣路径` 换行聚合 = **`888df87a31771c7eaea1bbcbeb367e5df070a069004c03a509f90c0cbc087087`**，与总账 `candidate_digest_sha256` 一致；`candidate_report_sha256`（REPORT.md=`8fb07319…f909`）、`review_report_sha256`（REVIEW_R1=`e848a879…0de2e`）、`candidate_repair_summary_sha256`（R1_REPAIR=`447ad03d…5300`）亦逐一复核一致。
- **依赖核对**：账本 R00-T03 `DONE`、`task_commit_sha=ffcb85830`、`push_result=CONFIRMED`；`R00-T03_REVIEW_R3.md` 为 PASS。T04 `depends_on:[R00-T03]` 满足。
- **任务规格**：独立完整读取 00_README、01–06 共同约束、R00 阶段书、90 源码依据、91 交付与独立验收模板、stage-index.json（R00 条目）、task-catalog.json（R00-T04 四 Steps/三 Deliverables/depends_on）与 acceptance-catalog.json（R00-A07/A08，均 REQUIRED）。
- **环境**：macOS darwin 27.0 arm64；Node v24.16.0、Python 3.14.3；`package-lock.json` SHA-256 前 16 位 `e54a16fe14f15b47`（本轮未安装/变更依赖）。
- **本轮对候选的唯一写操作（如实披露）**：F02 负例 E2E 测试曾临时将 `ENTRYPOINTS.json` 的 `plugins_mcp.active` 3→4（单字符），`--validate` 失败取证后立即改回；恢复后 `ENTRYPOINTS.json` SHA-256 复算为冻结值 `f69e658f…4d669`，8 文件聚合复算仍为 `888df8…87087`，逐字节恢复有证。全程另有 tar 备份 `/tmp/reviewer-r2-backup.tar`（SHA-256 `0a70c8c33f5eecc1ceabf2601fa92e53063c579fcf9296acde290c8a3794efaa`），最终未需动用。

## 2. 实际命令与结果（本轮独立执行）

| 命令 | 退出码 | 结果 |
|---|---|---|
| 8 文件聚合 SHA-256 重算（shasum 路径排序拼接） | 0 | `888df8…87087`，与账本一致（负例恢复后复算两次均同） |
| `python3 -B docs/rust-tauri/R00/r00_t04_scan.py --validate` | 0 | A=101 锚点 0 错、A2 计数 0 错、B=392（470 注册位/50 文件）、C=69↔69、D=0 未分类/0 过期/146 token 文件、E=8/8、负向 4/4、fatal 0；重写的 `scan-output.json` 与冻结候选**逐字节一致**（哈希 `3fc44133…cfdf1`） |
| 同上（`plugins_mcp.active` 篡改为 4 的 E2E 负例） | **1** | `FATAL: A2: entrypoint_category_counts[plugins_mcp] mismatch: declared={'active': 4, 'total': 5} recomputed={'active': 3, 'total': 5}`；恢复后退出码 0 且哈希复原 |
| 内存负例 ×10（importlib 载入 `validate_entrypoint_counts`，不写文件） | 全部 fail-closed | 见 §4-F02：未知状态/None 状态/状态计数篡改/缺失/条目状态翻转/类别删除/未申报新类别/类内多余键/重复条目/清空条目——10/10 触发报错，未篡改基线 CLEAN |
| `python3 -B docs/rust-tauri/R00/r00_t04_scan.py`（默认=生成+校验） | 0 | stdout：69 stores、classification 14/13/42、A=101、A2=0、B=392、C=69、D=0/0/146、E=8、负向 4/4、`R00_T04_SCAN_OK`；重生成 `STORES.json`（`6067bdd…7b493`）与 `scan-output.json`（`3fc44133…`）均与冻结候选逐字节一致 |
| `python3 -B …r00_t04_scan.py --generate-stores`（二次重生成） | 0 | `STORES.json` 哈希仍 `6067bdd…7b493`，字节稳定 |
| `npx vitest run tests/persistence-store-registry.test.ts tests/persistence-schema-tripwire.test.ts tests/http-route-security.test.ts tests/server-auth.test.ts tests/ws-scope.test.ts tests/device-registry.test.ts` | 0 | 6 文件 74/74 通过（registry 含 788 位点恰一归属、deterministic receipts match committed inventory、new write site 负例） |
| `node scripts/check-persistence-schema-fingerprint.mjs` | 0 | `170 watched sources; OK` |
| `node --input-type=module -e "import {PERSISTENT_STORES,…}"`（注册表独立 dump） | 0 | 69 store + 38 豁免；epochPolicy 分布 42/13/14 |
| `git check-ignore -v`（8 候选逐一） | 1×8 | 全部未被忽略（addable）；未使用 `git add`/`-f` |
| `git status --porcelain --ignored artifacts/` + `find artifacts/rust-tauri -name "*.log"` | 0 | 无 `!!` 项；`.log` 文件数 0 |
| 审查者独立宽口径 token 抽查（自选 13 token，区别于脚本与 R1 集合） | 0 | 见 §5：15 个未覆盖命中逐文件甄别全部为只读/导入性引用，无新增真实写点 |

测试均使用仓库既有隔离（临时目录）；本轮全部操作只读源码与 /tmp。

## 3. R1-F01（BLOCKING）复核：**已解决**

- **字节迁移佐证**：现 `final-run-stdout.txt` SHA-256 = `c4deafda678c898801527a43dfc4a90f05d08db482884fa7c77df8b288eca842`；R1 报告 §5-F01 记录的旧 `.log` 登记哈希与修复报告「前后均为 c4deafda…」三方一致——原字节重命名成立。
- **gitignore 闭合**：`.gitignore:95` 仍为 `*.log`（未改动、未扩大例外）；`git check-ignore -v` 对 8 候选逐一退出码 1（未被忽略）；`git status --porcelain --ignored artifacts/` 无 `!!`；`find artifacts/rust-tauri -name "*.log"` 为 0——旧 `.log` 无残留。未用 `git add -f`。
- **清单闭合**：`deliverable-hashes.txt` 6 条哈希逐一与实际文件复算一致（ENTRYPOINTS/STORES/OWNERSHIP/scan 脚本/scan-output/final-run-stdout.txt）；被引用文件全部存在且 addable。清单不含 REPORT.md 与自身，与 R1 审查时结构一致（先例沿用，非本轮差异）。
- **最终状态可证明性（任务指定质疑点）**：冻结的 `scan-output.json` 本身即**修复后**状态（含 `A2_entrypoint_counts` 与 4 项负向自检，哈希入账），加之本轮 `--validate`/默认模式重跑均逐字节复现——最终状态由「冻结 scan-output + 独立确定性复现」充分证明。`final-run-stdout.txt` 内容为修复前那次默认运行（负向仅 3 项、无 A2），但其内容自明且修复报告「剩余限制 2」已如实披露；本轮另在 /tmp 捕获了修复后默认模式完整 stdout（含 A2 与负向 4/4）作为审查证据。此为已披露的命名性瑕疵，不影响证据闭合（见 §7 NOTE-1）。

## 4. R1-F02（MAJOR）复核：**已解决**

- **计数独立机械重算**（审查者自写统计，不用候选校验器）：entries 37 条 = active 35 + dormant 1（EP-DEV-01）+ residual 1（EP-DEV-02）；逐类 active/total 与声明块完全一致（desktop_shell 7/7、renderer_clients 2/2、http_ws_server 7/7、cli 3/3、bridge 3/3、schedulers 3/3、channels_dm 4/4、subagents 1/1、plugins_mcp **3/5**、tool_boundary 2/2）；状态词表无越界、37 个 ID 唯一。`entrypoint_status_counts`（35/1/1/37）与 `entrypoint_category_counts` 双口径声明全部准确。
- **叙述一致性**：`R00-T04_REPORT.md`（交付表「35 个现役入口 + 1 休眠 + 1 残留（合计 37 条）」、Step 1 分类列表 plugins_mcp 5（现役 3+休眠 1+残留 1）、负向 4 项、命令表 A2 行）与 `OWNERSHIP_CURRENT.md` §2（「37 条登记 = 35 现役 + 休眠/残留各 1」、plugins_mcp（5：现役 3 + 休眠 1 + 残留 1））全部同步，无残留旧「36」叙述。
- **fail-closed 独立验证**：E2E 篡改 `plugins_mcp.active` 3→4 → `--validate` 退出码 1，FATAL 精确指出 declared/recomputed 差异；恢复后候选哈希不变。另以 importlib 在内存中对 `validate_entrypoint_counts` 做 10 个独立负例（未知状态 `'semi-active'`、`status=None`、状态计数篡改/整块删除、条目状态翻转不改计数、声明类别删除、entries 出现未申报新类别、类内多余键 `dormant:0`、追加重复条目、清空 entries）——**10/10 全部触发报错**，未篡改基线 CLEAN。脚本负向自检 `tampered_counts_detected` 亦在每次运行中内建验证。
- **实现盲区复查**（`r00_t04_scan.py:334-366` + 602-607）：状态词表硬约束、`total=len(entries)` 与声明 dict 整体相等、类别集合双向比对（多报/漏报类别均失败）、类内声明必须恰为 `{active,total}`；负例仅改内存浅拷贝不污染真实数据。未发现可静默放行的计数漂移路径。

## 5. R1-F03（MINOR）复核与 R00-A07 独立重验：**已解决 / PASS**

F03：
- `STORES.json` `server-runtime-info.processes` = `writers:["server","desktop(unlink-only)"]` + note；note 明确「desktop 仅做删除性 unlink，不创建/写入内容……内容唯一写者为 server」——**不误称 desktop 写入 token 内容**，方向正确。
- 源码逐行核实：`desktop/main.cjs:1364/1368`（stale/死内核探测清理，两处 `fs.unlinkSync(serverInfoPath)`）、`:1915`（spawn 前删旧 server-info）、`:6635`（shutdownServer 的 removeServerInfo 分支）——四行号与语义全部命中；committed inventory（`build/persistence-store-inventory.json`）该 store 7 位点中 desktop 侧恰为 4 个 `remove-path`，唯一 `write-file` 在 `server/index.ts`——「仅删除性」有 committed inventory 直接支持。
- 生成器源数据一致性与确定性：注记落在 `r00_t04_scan.py` `STORE_OVERLAY`（121-125 行）内；本轮默认模式与单独 `--generate-stores` 重生成 `STORES.json` 均逐字节复现冻结哈希 `6067bdd…7b493`——修复后重生成路径稳定，且 diff 范围仅 processes 块（由重生成复现间接佐证）。无其他不期望改动（8 文件聚合不变）。
- 进程统计复核：server 出现在 writers 的 store = 62（与 OWNERSHIP「server 写 62 项」一致）；desktop 内容写者（排除 unlink-only）= 壳态 5 项 + `signed-artifacts`（OTA 列车），与叙述一致。

A07（独立重走，非引用 R1 结论）：
- 注册表独立 dump：69 store + 38 豁免；epochPolicy 42 epoch-managed / 13 regenerable / 14 compatible，与 STORES.json counts 一一对应。inventory 788 位点：0 个既无 storeId 又无 exemptionId、0 个双归属；168 个不同文件与脚本 D 检查 `inventory_covered_files` 一致。
- 「无漏写」双机械门 + 审查者第三口径：① AST 常谱测试 `owns every production persistence site exactly once` 本轮实跑通过（74/74 内）；② 脚本 D 文件级 oracle（146 token 文件 → 141 常谱覆盖 + 5 分类，0 未分类 0 过期）；③ 审查者自选 13 个不同 token（`DatabaseSync`/`node:sqlite`/`writeJSON`/`outputFile`/`ensureDirSync`/`openSync(` 等）独立扫生产根：36 命中中 15 个未被常谱覆盖的文件逐一手查——`approval-review-context.ts:49`、`model-observability.ts:324` 为 `fs.openSync(path,"r")` 只读；`qq-local-upload.ts:204`、`file-import-security.ts:94` 为 `fs.promises.open(path,"r")` 只读；`session-compactor.ts` 仅 import `readFile`、`grep-pager.ts` 仅 `stat`、`studio-workspaces.ts` 仅 `stat`；其余为 import/注释命中——**未发现任何未登记的真实持久化写点**。
- 5 个 oracle 差集分类逐一开源码核实：`session-inline-media-prune.ts:47`（`sessionManager._rewriteFile()` 子串）、`mac-self-install.cjs:290`（注入 `fsImpl.writeFileSync`，目标 `mkdtempSync(tmpdir,"lingxi-self-install-")` 一次性脚本）、`server-readiness.cjs:22`（模块名字符串）、`session-search-tokenizer.ts:22`（词元样例字符串）、`lib/sandbox/index.ts:211`（转发 shim→resourceOps）——分类与理由全部属实。
- 缓存重建依据可核实：13 个 rebuildable store 的 `rebuild_or_loss_semantics` 与注册表 `checkpointPolicy` **13/13 逐字一致**；全部 69 store 的 `epoch_restore_policy` 与注册表 `restorePolicy` **69/69 逐字一致**（含 knowledge-indexes ANN 从保留向量 BLOB 重建、session-checkpoints 丢失仅失回退锚点、workspace-snapshots 影子 git 重捕）。
- epoch/拒写/版本链：`shared/contract-versions.json` DATA_EPOCH=1；`git log -p --follow` 全历史该键仅一次 `+ "DATA_EPOCH": 1`（恒 1）；`server/index.ts:270-273` 冷启动竞态自认注释（R-T04-01）、`:298` `LINGXI_ALLOW_DATA_DOWNGRADE=1` 唯一逃生门、`:324-347` hasHigherStamp/hasHigherTransition 阻断 + `LINGXI_DATA_EPOCH_BLOCKED` 机读标记 + `process.exit(1)` 属实；package.json 0.1.43 = 最新 tag v0.1.43。指纹 tripwire 170 源实测通过。
- desktop 单写者：`main.cjs:177-212` 四个 preferences 读取器全部 `safeReadJSON` 只读；写者唯一 `server/routes/config.ts:289 route.put("/config")`。

**R00-A07 判定：PASS**。无未登记的生产权威存储（常谱测试 + 脚本 oracle + 审查者独立第三口径三层一致）；缓存可重建理由逐字引自注册表可核实声明。

## 6. R00-A08 独立重验：**PASS**

四条链逐跳开源码核实（本轮亲自读取，非引用 R1）：
- **CLI**：`cli/entry.ts:89` startChat → `cli/chat.ts:104` `client.createWebSocket()` → `server/index.ts:626` resolveHttpRequestPrincipal → `core/server-auth.ts:63-71`（环回 token 非 local 即 `loopback_token_requires_local_transport` 拒绝；local 则 createLocalPrincipal）→ `server/routes/chat.ts:2894` `hub.send(promptText,…)` → `hub/index.ts:305-315` desktop owner 路由（sessionPath ? submitDesktopSessionMessage : engine.prompt）。
- **Bridge**：`telegram-adapter.ts:112` `polling:true` → `bridge-manager.ts:1127` `_isOwner` → `owner-policy.ts:8-17`（配置 userId 比对；wechat DM 全员 owner 视角；qq 额外 aliases）→ `bridge-manager.ts:1995-2009` `hub.send({role:bridgeRole})` → `hub/index.ts:326/330` guest→GuestHandler / owner→executeExternalMessage。
- **cron**：`cron-scheduler.ts:42` 60s tick → `cron-store.ts:746` markRun（CAS expectedConfigRevision）→ `hub/scheduler.ts:389-402`（`permissionMode=auto||getAutomationPermissionMode`、`approvalPolicy="deny_on_prompt"`、`allowHumanApproval=false`、`permissionContext.surface="automation"` 含 actorAgentId/configRevision/executionScopeKey）→ `session-coordinator.ts:8048` executeIsolated。
- **开发调用**：`server/http/cors-policy.ts:1` `^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?$` 白名单 + `desktop/preload.cjs:34-35` getServerPort/getServerToken IPC → 同 HTTP 认证 → hub.send role=owner → 同桌面链。
- **工具边界**：`hub/index.ts:299-348` 四路路由表逐行核实（ephemeral 路由硬编码 `permissionMode:"auto"+approvalPolicy:"deny_on_prompt"+allowHumanApproval:false`）；`core/engine.ts:4119-4124` 网关 authorize 回调原文 `throw new Error("model-facing plugin calls must be authorized by the session permission wrapper")`；`session-permission-wrapper.ts:656` wrapWithSessionPermission。101 个锚点经我的 `--validate` 运行机械校验 0 错，另亲读约 28 个关键锚点语义全部吻合。
- **未证实路径列风险而非安全（核实为真）**：R-T04-04 MCP app-tools 直调——`core/mcp/manager.ts:2133` `callAppTool` 直接走 connector `client.callTool`，不经 ToolInvocationGateway（授权仅靠路由 STUDIO_OWNER 分类），属实；R-T04-05 Bridge owner 信任=平台账号体系外部边界，属实。EP-DEV-01 休眠判定：`prepareAndInvokeForLocalDeveloper` 生产消费者为 0（仅 `core/tool-invocation-gateway.ts:506` 定义 + 2 个测试文件引用），`/api/plugins/dev` 路由在 server/routes 下 0 命中（仅 route-security.ts:351-352 分类残留）——dormant/residual 分类准确；`createLocalDeveloperPrincipal`（gateway.ts:65-78）硬校验 local_user+local+loopback_token 属实。负空间：desktop 全源码 `setAsDefaultProtocolClient` 0 命中（无深链）。

**R00-A08 判定：PASS**。四条链身份/授权来源逐跳明确；休眠/残留区分准确；未证实项均以风险登记，无一处写成安全结论。

## 7. 其他观察（均不阻断）

- **NOTE-1（已披露的命名性瑕疵）**：`final-run-stdout.txt` 内容对应修复前最终运行（负向 3 项、无 A2）。修复报告「剩余限制 2」已明示并保留原字节（F01 修复要求）。最终状态由冻结的修复后 `scan-output.json`（哈希入账）+ 本轮逐字节确定性复现充分证明，证据闭合不受影响；若总控要求 final-run 证据与修复后状态逐字节对应，可另行授权重捕，非本验收阻断项。
- **NOTE-2**：`deliverable-hashes.txt` 沿用 6 文件结构（不含 REPORT.md 与清单自身），与 R1 审查时一致；总账另以 8 文件聚合覆盖全候选，无缺口。
- **NOTE-3**：`docs/rust-tauri/R00/__pycache__/`（含 t02/t03/t04 的 .pyc）为 gitignore 运行产物，不属候选、不影响 addability；本轮一律 `python3 -B` 未新增。

## 8. 未验证范围（如实区分源码推断 / 实际运行 / 受环境限制）

- 源码+既有测试支撑、未 LIVE 运行：真实模型会话、真实五平台账号收发、真实 CLI 交互会话、GUI dev 窗口、睡眠唤醒补触发、四平台安装包——与执行者/R1 声明一致；任务书允许真实账号验证延后 R10，属允许的未验证而非虚报。
- 「历史版本 epoch 恒 1」以 `git log -p --follow` 全历史单次引入佐证（强证据），未逐一签出历史 tag 重跑。
- 392 条字面路由为描述性口径，未逐路由比对；路由授权面由 `http-route-security.test.ts`（22/22，本轮实跑）覆盖 fail-closed 语义。
- 101 锚点为机械校验（0 错）+ 审查者约 28 个语义亲读抽样，未对全部锚点逐条人工语义复核。
- 审查者宽口径 token 抽查（13 token、15 文件甄别）与 R1 的宽口径（24 文件）为两组独立抽样，非穷举证明；「无漏写」的机械底座是 AST 常谱测试（788 位点恰一归属，本轮实跑通过）。

## 9. 逐项判定汇总

| 项 | 判定 | 依据 |
|---|---|---|
| R00-A07（REQUIRED） | **PASS** | 本轮独立重走：69/38 注册表、788 位点 0 漏归属、13/13+69/69 逐字语义一致、三层口径零未登记写点、epoch/指纹/版本链属实、6 测试 74/74、指纹 170 OK |
| R00-A08（REQUIRED） | **PASS** | 四链逐跳源码核实（含 hub 四路表与 ephemeral 固定 deny_on_prompt、模型面 authorize 抛错原文）；两风险主张与休眠/残留分类属实 |
| R1-F01（BLOCKING） | **已解决** | §3：8×addable、无 .log 残留、清单 6 哈希闭合、c4deafda… 三方一致、最终状态可证 |
| R1-F02（MAJOR） | **已解决** | §4：计数独立重算一致、叙述三处同步、E2E+10 内存负例 fail-closed、恢复后聚合不变 |
| R1-F03（MINOR） | **已解决** | §5：四 unlink 行号/inventory 支持、注记不误称写入、重生成逐字节稳定、无其他改动 |
| 候选完整性 | 聚合 `888df8…87087` 与账本一致；负例测试后复算不变 | §1/§2 |
| 无用户数据接触 / 无越权改动 | 是 | 仅源码只读 + /tmp + 仓库既有隔离测试；账本与 R1/修复报告未触碰 |

## 10. VERDICT

**VERDICT: PASS**

理由：两项 REQUIRED（R00-A07/A08）均有本轮独立重走并复现的真实证据；R1 的三项 finding（F01 BLOCKING/F02 MAJOR/F03 MINOR）修复经独立验证全部闭合，未发现修复引入的新问题；候选 8 文件聚合 SHA-256 与总账登记一致、全部可正常纳入 Git、证据链可从仓库历史复现；仅余 NOTE-1 命名性瑕疵（已披露、不影响闭合）。本判定为候选验收结论，不自行修复、不批准提交——入库、账本 `candidate_digest_sha256` 之外的后续状态流转与推送仍由总控/用户按获准流程执行。

（本独立验收任务到此结束。）
