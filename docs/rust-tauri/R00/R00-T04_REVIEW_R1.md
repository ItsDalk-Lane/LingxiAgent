# R00-T04 独立验收报告（R1）

审查者：REVIEWER-R00-T04-R1（全新一次性独立验收任务，非执行者，未参与实现与自测）。
审查日期：2026-09-24。本报告只新增本文件；未改产品源码、测试、T04 候选、旧任务交付或总控账本。

## 0. 结论速览

**R00-A07：PASS（证据充分，本轮独立复算成立）。R00-A08：PASS（四条链逐跳独立核实成立）。VERDICT: FAIL**——不是因 A07/A08 证据不实，而是存在 1 项 BLOCKING 交付打包缺陷（F01：候选证据链无法按冻结指纹进入版本库）与 1 项 MAJOR 清单元数据缺陷（F02）。按本轮规则「无 BLOCKING finding 才可 PASS」，判 FAIL，交修复任务处理后重验。

## 1. 验收边界与基线核对

- **tested HEAD**：`ffcb85830ffdc75da5fe39b7a8cfddb4617f1aec`（=Task base=当前 HEAD，`git rev-parse` 实测）；分支 `codex/rust-tauri-migration`；`origin/codex/rust-tauri-migration` 同 SHA（T03 已推送核实在案）。
- **工作区**：仅 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（总控账本，预先存在的未提交修改）+ 8 份 T04 新增候选；与任务交代的进入状态一致，无越权改动。本轮结束时再次核对，候选字节未变。
- **依赖核对**：账本载 R00-T03 `DONE`、`task_commit_sha=ffcb85830`、`push_result=CONFIRMED`、A05/A06=PASS；`R00-T03_REVIEW_R3.md` 结论为 `R00-A05: PASS；R00-A06: PASS；VERDICT: PASS`。T04 `depends_on: [R00-T03]` 满足。
- **任务规格**：独立完整读取 00_README、01–06 共同约束、R00 阶段书、90 源码依据、91 交付与独立验收模板、92（R00 段）、stage-index.json、task-catalog.json（R00-T04 Steps/Deliverables/depends_on）与 acceptance-catalog.json（R00-A07/A08 的 Given/When/Then/Evidence/Environment/Activation Condition，均 REQUIRED）。
- **环境**：macOS 27.0 arm64；Node v24.16.0、Python 3.14.3；`package-lock.json` SHA-256 前 16 位 `e54a16fe14f15b47`（本轮未安装/变更任何依赖）。
- **candidate hash 独立重算**：8 份候选（5 docs + artifacts/T04 三份）按路径排序、每行 `SHA256␣␣路径` 换行聚合，得 `61d0a8efead570c451c2b26c90dbfafd69c4f68f84e244b5d32c430dcca3300d`，与总账 `candidate_digest_sha256` 一致；`R00-T04_REPORT.md` 单文件 SHA `34de7c56…b5ad2` 与账本 `candidate_report_sha256` 一致。

## 2. 实际命令与结果（本轮独立执行）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t04_scan.py --validate` | 0 | A=101 锚点 0 错、B=392/470/50、C=69↔69、D=0 未分类/0 过期/146 token 文件、E=8/8、负向 3/3；重生成的 `scan-output.json` 与冻结候选**逐字节一致**（cmp 通过，哈希 `3d6d1664…d124e`） |
| 重跑生成器（`--generate-stores`，经备份包装执行） | 0 | `STORES.json` 重生成**逐字节一致**（`e3b84942…dc46`），确定性主张成立 |
| `npx vitest run tests/persistence-store-registry.test.ts tests/persistence-schema-tripwire.test.ts tests/http-route-security.test.ts tests/server-auth.test.ts tests/ws-scope.test.ts tests/device-registry.test.ts` | 0 | 6 文件 74/74 通过，与报告一致；registry 测试确含运行时重扫与 committed inventory 比对、新增写点负例 |
| `node scripts/check-persistence-schema-fingerprint.mjs` | 0 | `170 watched; OK` |
| `python3 -m py_compile docs/rust-tauri/R00/r00_t04_scan.py` | 0 | — |
| 审查者独立更宽 token oracle（只读扫描，见 §4） | 0 | 24 个额外命中文件逐条甄别，无新增真实未登记写点 |
| `git check-ignore -v artifacts/rust-tauri/R00/T04/final-run-stdout.log` | 0 | 命中 `.gitignore:95 *.log`（F01 证据） |

测试均使用仓库既有隔离（临时目录），未触真实用户数据；本轮全部操作只读源码与 /tmp。

## 3. Step 1/2（入口与七要素）独立核实

- **规格覆盖**：任务书 Step 1 十类入口（desktop/CLI/HTTP-WS/Mobile-LAN/五平台 Bridge/定时心跳/频道 DM/子代理/插件开发）在 ENTRYPOINTS.json 均有登记；101 个锚点经脚本机械校验 + 本轮约 35 个逐跳语义复核（server-auth 环回 token 仅 local、owner-policy 配置比对、scheduler automation 选项、gateway authorize 抛错、mcp app-tools STUDIO_OWNER 直调、epoch 闸、train 序列、restore 活内核探测、subagent 衰减等）全部与源码一致。
- **负空间独立反例扫描（审查者自制）**：全仓 `listen/createServer` 仅 server 主监听与端口探测；WS 面确为 2（`/ws` Hono wsRoute + `/internal/browser` 原生 `WebSocketServer`，`server/cli.ts:124` 为出站客户端）；desktop 无 `setAsDefaultProtocolClient`/`open-url`；无入站 webhook 路由（bridge 配置中 webhook 字段为平台出站参数）；`/api/plugins/dev` 路由在 server/routes/composition 已不存在；`prepareAndInvokeForLocalDeveloper` 仅定义+测试消费，dormant 判定属实；`createLocalDeveloperPrincipal` 硬校验 local_user+local+loopback_token（gateway.ts:65-78）。
- **A08 四链逐跳核实**：CLI（entry.ts:89→chat.ts:104→index.ts 中间件→routes/chat.ts:2894 hub.send→hub/index.ts:305 owner 路→包装器）、Bridge（telegram-adapter 轮询→bridge-manager.ts:1127 owner 判定→:1995 hub.send→executeExternalMessage→bridge 会话）、cron（tick→cron-store.ts:746 markRun→scheduler.ts:389-402 automation 选项→session-coordinator.ts:8048 executeIsolated）、开发调用（vite dev 渲染端：cors-policy.ts:1 localhost 白名单实测 + preload.cjs:34-35 token）各跳锚点与语义均成立；hub/index.ts:299-348 四路路由表（ephemeral 固定 `deny_on_prompt`+`allowHumanApproval:false`）核实。
- **裁定一（无 finding）：「开发调用=Vite 渲染端」满足规格**。Step 1 要求枚举"实际存在项"：插件开发面生产路由已随 04f90d2b2 删除，唯一能"追踪到实际工具执行"的现役开发链就是 vite dev 渲染端（EP-DEV-03）；休眠网关面（EP-DEV-01，含 principal 派生与 authorize DI 分析）与路由分类残留（EP-DEV-02）均已单独登记并标注 R04 裁决，符合 A08「未证实路径列风险而非安全」与区分休眠/残留的要求。EP-DEV-03 notes 明示选择理由，无隐瞒。
- **裁定二（无 finding）**：Bridge owner 信任假设（R-T04-05）与 MCP app-tools 绕网关直调（R-T04-04）均以源码核实为真（`manager.ts` callAppTool 直接 `client.callTool`，不经 ToolInvocationGateway/包装器；授权仅靠路由 STUDIO_OWNER 分类），候选将其列为风险而非安全，定级准确。

## 4. Step 3/4 与 R00-A07 独立复算

- 注册表独立重取：69 store + 38 豁免；epochPolicy 分布 epoch-managed 42 / regenerable 13 / compatible 14，与 STORES.json 分类一一对应；inventory 788 位点全部有归属（无 neither-owner 位点）。
- 5 个 oracle 差集分类逐条对照源码核实：`session-inline-media-prune.ts:47`（`_rewriteFile()` 命中子串，真写在已注册 session-jsonl）、`mac-self-install.cjs:290`（注入 `fsImpl.writeFileSync`，目标 `os.tmpdir()/lingxi-self-install-*` 一次性脚本，AST 常谱 `callKind` 只认直接 fs 绑定——盲区主张与 `scripts/scan-persistent-stores.mjs:226-245` 实现相符）、`server-readiness.cjs:22` 与 `session-search-tokenizer.ts:22`（模块名/词元样例字符串）、`lib/sandbox/index.ts:211`（转发 shim→LocalFsProvider，后者为 `external-resource-io` 豁免，registry:1766-1772，inventory 13 位点）。分类全部成立。
- **审查者独立更宽 token oracle**（在脚本 token 集上追加 `openSync/writeSync/DatabaseSync/node:sqlite/promises.rm|unlink|rename|copyFile/outputFile|writeJSON/graceful-fs` 等，同一生产根/排除规则）：额外命中 24 个三方（常谱/豁免/分类）均未覆盖的文件，逐条甄别：类型字符串与位点 kind 枚举（resource-io/types、registry、plugin-context 等）、chokidar 事件名（file-watch-registry）、只读打开 `"r"`（approval-review-context.ts:49、model-observability.ts:324）、`realpathSync`、OS temp/用户自选路径（desktop IPC 文件桥归 `desktop-caller-selected-output` 等豁免）、`writeSync(1,…)` stdout 心跳（server/bootstrap.ts:50）。**未发现任何未登记的真实持久化写点**——「无未登记的生产权威存储」在比执行者更宽的口径下成立。
- 38 项豁免逐条过目：全部为用户自选路径/OS temp/原子写原语/内存态/外挂挂载类，无权威 LINGXI_HOME 数据漏登记。
- 缓存可重建理由抽验（knowledge-indexes/session-checkpoints/workspace-snapshots/managed-runtime-caches）：`rebuild_or_loss_semantics` 与注册表 `checkpointPolicy` 逐字一致，且 restorePolicy（ANN 从保留向量 BLOB 重建、checkpoints 仅丢回退锚点、shadow git 可重捕）与架构描述互证，可核实性成立。
- epoch/拒写/版本链独立核实：`contract-versions.json` DATA_EPOCH=1（git 全历史仅出现 `+ "DATA_EPOCH": 1`，恒 1 主张成立）；启动闸顺序（同宅互斥探测→epoch 事务闸，index.ts:273-349）、`epoch-downgrade-blocked`→exit 1 + `LINGXI_DATA_EPOCH_BLOCKED` 机读标记、唯一逃生门 `LINGXI_ALLOW_DATA_DOWNGRADE=1`（index.ts:298；cli/server-runner.ts:141-153 透传）、epoch=1 基线损坏元数据降级但更高印章仍阻断（index.ts:324-347）、restore 前活内核探测（data-epoch-restore.ts:149-164）全部属实；指纹 tripwire 170 源实测通过；package.json 0.1.43=最新 tag v0.1.43=release-digest.v2 首条（24 条目）。
- desktop 单写者分工核实：`desktop/main.cjs` 25 个位点归 7 个 store/豁免（diagnostics/window-version/caller-selected-output/skill-preview-temp/screenshot-html-temp/observability-export/server-runtime-info）；`user/preferences.json` 在 desktop 仅 `safeReadJSON` 读（main.cjs:176-212），写者唯一 server（`routes/config.ts:289 PUT /config`）；OTA 列车固定步骤表（train-update-apply.cjs）与 preload 契约闸（ota-core.cjs:1051-1065）属实。

**R00-A07 判定：PASS**。无未登记权威存储（含审查者宽口径复算）；缓存可重建理由可核实；差集分类逐条成立且固化为护栏（新差集即失败）。

**R00-A08 判定：PASS**。四条链身份/授权来源逐跳核实；未证实身份与权限均以风险登记（R-T04-04/05、EP-DEV-01），未写成安全结论。

## 5. Findings（按根因合并）

### F01｜BLOCKING｜候选证据链无法按冻结指纹闭合进入版本库（.log 被 gitignore）
- **位置**：`artifacts/rust-tauri/R00/T04/final-run-stdout.log`；`artifacts/rust-tauri/R00/T04/deliverable-hashes.txt:6`（引用该文件）；总账 `candidate_digest_sha256=61d0a8ef…`（8 文件含该 log）。
- **证据**：`git check-ignore -v` 命中 `.gitignore:95 *.log`；`git status --ignored` 显示该文件为 `!!`。`deliverable-hashes.txt` 登记其哈希 `c4deafda…`，冻结聚合包含它。
- **后果**：按 R00 既有流程（T01/T02/T03 候选均提交入库），T04 提交时 `git add` 会静默跳过该 log——入库的 7 文件集既无法复算总账登记的 8 文件聚合哈希，`deliverable-hashes.txt` 也引用了仓库中不存在的文件，证据链从仓库历史不可复现；若强行 `git add -f` 则对抗仓库 `*.log` 忽略策略。**先例**：T01 全部 stdout/stderr 证据均以 `.txt` 提交跟踪（`artifacts/rust-tauri/R00/T01/server-stdout*.txt` 等），T03 提交不含任何 `.log`——T04 是首个引入被忽略 `.log` 证据的任务。
- **同类路径**：未来各任务 evidence 目录里任何 `*.log` 命名。
- **修复要求**（修复任务执行，本轮不动候选）：将 `final-run-stdout.log` 重命名为 `final-run-stdout.txt`（内容不变，逐字节保留）；重新生成 `deliverable-hashes.txt` 并重算 8 文件聚合；更新总账 `candidate_digest_sha256` 与 `candidate_note`；`--validate` 重跑确认 scan-output 不受影响（脚本不读该文件）。禁止用 `git add -f` 绕过忽略策略，禁止改 `.gitignore` 扩大例外。
- **需重跑**：`python3 -B docs/rust-tauri/R00/r00_t04_scan.py --validate`（应仍字节一致）、聚合哈希重算。

### F02｜MAJOR｜ENTRYPOINTS.json 计数块与"36 现役"叙述自相矛盾（实际 35 现役/37 条）
- **位置**：`docs/rust-tauri/R00/ENTRYPOINTS.json:711-722`（`entrypoint_category_counts` 声明 plugins_mcp=4、合计 36）；`docs/rust-tauri/R00/R00-T04_REPORT.md:11` 与 `OWNERSHIP_CURRENT.md:23,27`（"36 个现役入口/登记入口 + 1 休眠 + 1 残留"）。
- **证据（本轮机械复算）**：entries 实际 37 条 = active 35 + dormant 1（EP-DEV-01）+ residual 1（EP-DEV-02）；plugins_mcp 实际 5 条（3 active）。声明合计 36 既不等于 active 35 也不等于总数 37；任一读法（36+2=38 / 36 含休眠残留）均不成立。
- **根因**：追加 EP-DEV-03（vite dev 开发链）后未刷新计数块与叙述；`r00_t04_scan.py` 校验锚点但不校验 counts 与 entries 一致，缺陷无法被护栏发现。
- **后果**：R07/R10 等下游按 category_counts 消费入口面会少数 1 条开发链或误信"36 现役"；基线账本自身数字不可信。
- **同类路径**：STORES.json counts 块由生成器派生（无此风险）；ENTRYPOINTS 手工计数是唯一同类。
- **修复要求**：修正计数块（建议同时给出 `active` 与 `total` 两种口径，plugins_mcp active=3/total=5）与三处叙述（35 现役 + 1 休眠 + 1 残留 = 37）；在 `--validate` 增加计数一致性检查（counts vs entries 机械比对）。
- **需重跑**：`--validate`（新增检查过）；聚合哈希随候选变更重算并更新总账。
- **注**：逐条 entry 与锚点本身完整正确，本 finding 不推翻盘点结论，仅元数据/叙述准确性。

### F03｜MINOR｜STORES.json `server-runtime-info.processes.writers` 漏记 desktop 的 remove-path 角色
- **位置**：`docs/rust-tauri/R00/STORES.json` server-runtime-info 条目（`writers:["server"]`）。
- **证据**：committed inventory 中 `desktop/main.cjs` 有 4 个 `remove-path` 位点（`fs.unlinkSync(serverInfoPath)`）归属该 store；EP-DESK-01 自身也记载"shutdownServer 决定 server-info 去留（main.cjs:6558-6640）"。`site_rule_files` 已含 `desktop/main.cjs`，事实可复原，仅 curated `processes` 字段欠精确。
- **后果**：R02 单写者设计若只按 writers 字段取输入，会漏掉 desktop 在关停/接管路径对 server-info.json 的删除性变更（adjacent_compatible 面，风险低）。
- **修复要求**：writers 改为 `["server","desktop(shutdown unlink)"]` 或加注；随 F02 同轮修复。
- **需重跑**：`--generate-stores` 重生成 + `--validate`。

## 6. 未验证范围（本轮如实区分）

- 源码推断+静态追踪成立、但未 LIVE 运行：真实模型会话、真实五平台账号收发、真实 CLI 交互、GUI dev 窗口、睡眠唤醒补触发、四平台安装包——与执行者声明一致（任务书允许真实账号验证延后 R10）。
- 未逐一签出历史 tag 重跑（"历史版本 epoch 恒 1"以 `git log -p --follow shared/contract-versions.json` 全历史只见 `DATA_EPOCH: 1` 佐证，属强证据非逐 tag 实测）。
- 392 条字面路由为描述性口径：动态/变量路径不在其内，但 route-security 对未列举 `/api/*` fail-closed 落 STUDIO_OWNER，授权面不依赖该计数，本轮未逐路由比对。

## 7. 逐项判定汇总

| 项 | 判定 | 依据 |
|---|---|---|
| R00-A07（REQUIRED） | **PASS** | 双向差集独立复算闭合（含宽口径反例扫描零新增）；69/38、788 位点、5 分类、缓存语义全部核实；validator/生成器字节级复现 |
| R00-A08（REQUIRED） | **PASS** | 四链逐跳锚点+语义核实；身份/授权来源明确；休眠/残留区分；未证实项列风险（R-T04-04/05） |
| 三份必交物 | 内容合格，元数据缺陷见 F02/F03 | ENTRYPOINTS/STORES/OWNERSHIP 与源码一致 |
| 候选完整性 | 聚合哈希一致；但 F01 使其无法原样入库 | §5-F01 |
| 无用户数据接触 | 是 | 本轮仅源码只读 + /tmp + 仓库既有隔离测试 |

## 8. VERDICT

**VERDICT: FAIL**

理由：R00-A07 与 R00-A08 两项 REQUIRED 均有本轮独立重走并复现的真实 PASS 证据、候选主张与源码一致、无虚报；但 F01（BLOCKING：被 gitignore 的 `final-run-stdout.log` 进入冻结证据集，候选无法按登记指纹提交入库，破坏提交态证据闭合）触发本轮「无 BLOCKING finding 才可 PASS」规则。F02（MAJOR 计数矛盾）与 F03（MINOR writers 精度）随同轮修复。三项均为打包/元数据修复，不动摇 A07/A08 的实质结论；修复后重算候选聚合并重验即可，无需重做盘点。

（修复交由下一个全新 ZCode 任务执行；本审查任务到此结束。）
