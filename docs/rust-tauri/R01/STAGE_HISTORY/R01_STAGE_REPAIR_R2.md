# R01 阶段修复 R2 — 修复报告

- 修复者：ZCode:R01 阶段修复 R2（ZCode 一次性任务）；未参与 R01 执行、T 验收、R1/R2 阶段验收、R1 修复。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`。
- 结论：**READY_FOR_INDEPENDENT_STAGE_REREVIEW**。F01/F02/F03 全部修复并完成可复现验证；不宣告阶段 PASS、不进入 R02；正式封印（坐标推进+提交推送）留待阶段复验 PASS 后的总控授权动作。
- R2 验收输入：`/tmp/r01-stage-review-r2.md` SHA-256 `727e82f0323fd067b33f49a7c09a44dc494cc58ee5c0697aa5ca490f12f71e82`（与交接钉住一致，本机实算吻合）。

## 0. 开工状态核对（与交接一致）

- 开工时本地/远端（remote-tracking）HEAD = `363999378482dfed42733a3c4e8ad20ac82fd0fc`，本报告全程未变（未 commit/push/tag）。
- R1 候选原样保留：9 个已跟踪修改 + `STAGE_REPAIR_R1/` 16 个新证据文件全部未动；R2 修复在其上叠加。
- R2 验收冻结指纹 `66e84f0bb0cfd9ef4c4eb9a51a61a2e92649b2113b3ec6b89cbf1460cfe5383e` 与交接一致；未覆盖他人改动，未执行 reset --hard/clean -fd。
- 代理不可用：远端只以 remote-tracking ref 核对；未发起真实外发/付费 API；真实推送以本地裸仓预演代替（见 §4.4）。

## 1. F01（BLOCKING）递延风险字段空值绕过 → 已修复

### 根因
`evaluate()` 用 `str(r.get("resolve_by_stage", "")).strip()` 判非空：`None`→`"None"`、`{}`→`"{}"`、`[]`→`"[]"` 均为非空字符串，JSON null/对象/数组冒充"已填"；输入递延状态与登记状态矛盾（如登记 CLOSED 仍挂账 UNVERIFIED）无检查；登记簿完整性（缺 id/重复 id/非列表）无检查。R1 的 N16 只测空字符串形态，漏掉合法 JSON 空值。

### 修复（docs/rust-tauri/R01/r01_t08_gate_check.py，CONTRACT_VERSION 1.1-stage-repair-r1 → 1.2-stage-repair-r2）
1. `_meaningful_str()`：仅接受非空白、非占位（""/null/none/n/a/tbd/todo/unknown/{}/[]/-/--/? 等）的真实字符串；任何非字符串类型（null/对象/数组/数字/布尔）一律拒绝，不做 `str()` 转换。
2. `DEFERRED_RISK_STATUS_MAP`：输入递延状态 → 允许的登记状态（UNVERIFIED/OPEN_FINDING→{OPEN,CARRIED}；REGISTERED_DEFECT→{REGISTERED}；DOCUMENTED→{DOCUMENTED}）；登记 CLOSED* 与递延输入矛盾 → `BLOCKED(risk-status-contradiction)`。
3. 登记完整性：risks 非列表、条目非对象、缺/空白 id、重复 id（同类重复登记）→ `register_violations`，整体 NO-GO；main() 打印 `REGISTER-VIOLATION:` 行使 CLI 输出可判定。
4. R1 的 FROZEN_CONTRACT 闭合（删域/删 user_takeover/删 sha256/删递延/改名/重复/额外域/证据改指等 17 项）原样保留。

### 验证
- 进程内自测：`python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test` → **SELF-TEST OK (29/29)**（新增 N17–N28：null/{}/[]/空白/数字/布尔/占位字段、CLOSED 与 CLOSED_IN_R01 矛盾、REGISTERED_DEFECT 不匹配、重复 id、缺 id、非列表）。证据：`artifacts/rust-tauri/R01/STAGE_REPAIR_R2/f01-gate-selftest-r2.txt`。
- **真实 CLI 负向电池**（按 R2 修复要求以实际命令行入口而非仅内部函数）：新增 `docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh`，mktemp 工作区对 RISK_REGISTER.json 施加 15 种硬编码变异（null-field/object-field/array-field/blank-field/number-field/bool-field/placeholder-field/deleted-field/closed-status/closed-in-r01-status/defect-status-mismatch/documented-status-mismatch/duplicate-id/missing-id/risks-non-list）+ 1 个正向对照，断言 exit=1 且输出含对应 BLOCKED 类别 → **CLI-REGRESSION OK（1 正向 + 15 负向）**。证据：`artifacts/rust-tauri/R01/STAGE_REPAIR_R2/f01-cli-regression-r2.txt`。
- 正向真实输入：`--register RISK_REGISTER.json --out …/gate-report.json` → exit 0、`PASS_WITH_CONDITIONS`、15 递延 TRACKED、`register_violations=[]`；关卡证据（gate-report.json/gate-check.log/gate-selftest.log）已按 1.2 契约重生成（判定不变）。
- R2 评审的 5 个绕过形态（null、{}、[]、CLOSED 矛盾、str 转换族）全部 exit=1（电池含逐一对应项）。

## 2. F02（BLOCKING）全仓 lint 61 errors → 已修复

### 根因
R01-T06 引入的三个 Tauri spike 文件继承了仓内默认 Node 侧 lint 环境，与真实运行环境不符：
- `spike/tauri-shell/app/frontend/probe.js`（25 错）：运行在 Tauri webview（浏览器环境），`window`/`document`/`setTimeout` 等未声明；另有一处 `no-unused-expressions`（`invoke && invoke('finish_e2e')`）。
- `spike/tauri-shell/scripts/evidence_server.mjs`（11 错）与 `probe_webdriver.mjs`（25 错）：Node 侧 runner/证据服务器/WDIO 探针，但使用 Node 18+ 全局 `fetch`/`AbortSignal`，仓内 globals 未声明。

### 修复
- `eslint.config.js`：按真实运行环境精确加两个 scoped block——`spike/tauri-shell/app/frontend/**/*.js` → `globals.browser`；`spike/tauri-shell/scripts/**/*.mjs` → `globals.node + fetch/AbortSignal(readonly)`。不删除任何原型文件、不放松任何仓级规则、不改其他目录的环境。
- `probe.js`：`invoke && invoke('finish_e2e')` → `if (invoke) invoke('finish_e2e')`（语义逐字等价；catch 补注释说明"尽力而为"边界）。

### 验证
- `npm run lint` → exit 0，**0 errors**（61→0）；warnings 10898→10896（净 −2：probe.js 修掉 3 条既有 warning；R2 新增 TS 代码类型化后 0 净增；既有 10896 条为仓内历史存量，不以数量冒充失败）。证据：`artifacts/rust-tauri/R01/STAGE_REPAIR_R2/f02-lint-r2.txt`（完整原始输出）。
- 逐文件对照（HEAD vs 工作树 eslint warning 计数）：patch-seal-fixture.ts 3→3、round2 14→14、round3 10→10、probe.js 3→0、eslint.config.js 0→0。
- T06 原型相关检查：lint 收口即环境声明修复；spike 的 WDIO/e2e 吊具需 Tauri 构建链，本轮未重跑（probe.js 改动为语义等价的语句形态变化，config 改动只影响 lint 环境声明）——如实披露为未执行项。
- 阶段受影响回归：`npm run typecheck`（tsc×3）exit 0；封印预演绿色电池含 lint（见 §4.3）。

## 3. F03（BLOCKING）双补丁超 GitHub 100MiB 硬限 → 已修复（机制+预演）

### 根因
封印流程再生成两份全树二进制 diff（确定性 gzip，mtime=0）：
- round2（BASE 89bc0b64）未压缩 495,953,042 B/5306 项——R01 证据（285.8MiB）入库后补丁自然膨胀；
- round3（BASE 67dee5d2）未压缩 968,940,900 B/5719 项——其 BASE 更旧，把 round2 再生成的 patch.gz 一并打入。
再生产物（本修复预演实算：round2 267,310,615 B、round3 577,588,418 B；R2 评审在 R1 预演副本上量得 267,221,850/577,433,322 B，差值=R1 修复后新增的 16 个证据文件入树）远超 GitHub 普通 Git 100MiB 单文件硬限；当前仓内已提交版仅 29,581,472/89,457,199 B（旧交付），一旦按合同再生成并提交即无法推送。

### 修复（受控分片交付，不损失任何完整性语义）
两个生成器同构修改（`artifacts/f1-f12-repair/round2/create-delivery-patch.py`、`round3-c01-c03/create-round3-patch.py`）：
- 新清单格式 `patch-gzip-shards/v1`：payload 超过 `SHARD_BYTES`（默认 45,000,000 B，低于 GitHub 50MB 警告线；`LINGXI_PATCH_SHARD_BYTES_OVERRIDE` 仅供夹具测试）时，单体 `.patch.gz` 不交付，改交付 `*.part-00001…N` + `*.shards.json` 清单。
- **清单最后落盘为提交点**：分片全部原子写入完成后再写清单；single↔sharded 双向转换互斥回收（转分片删单体、转单体删清单与全部旧分片，含孤儿分片清扫）。
- **MISMATCH 不改写**：重放校验失败时既有交付族逐字节保留（快照/恢复不变式，有专门负向用例）。
- 完整性语义不损失：`patchSha256`/`patchBytes` 仍钉住完整载荷；按序重组 sha256 必须等于 patchSha256、可 gunzip、头部仍是 `diff --git`；重放校验流程本身零改动。
消费者：`tests/helpers/patch-seal-fixture.ts` 新增 `expectDeliveryConsistent()`（路径收容校验拒绝绝对路径与 `..`；single/sharded 双形态全字段复算、逐片哈希、重组哈希、gunzip 头部校验），round2/round3 两个交付测试文件接入（历史记录兼容：缺 patchFormat 按 single）。

### 验证
- 合成 git 夹具（/tmp，`LINGXI_PATCH_*_OVERRIDE` 小阈值）：分片正向 VERIFIED（逐片+重组哈希复算、单体不共存）、分片负向 MISMATCH 保留交付族、单/分片双向互斥回收——**两文件各 3 个、共 6 个新用例全绿**（证据：`f03-shard-fixture-tests-r2.txt`）。
- 主仓既有交付测试：59 passed / 5 failed——5 红全部是坐标仍指 R00 C3 的**预期封印前红**（R10-03/R10-04/R10-09、round3 清单/现场重放），与 R1 候选同因同集，归不变；运行后交付 patch.gz 逐字节不变（MISMATCH 不重写）。
- /tmp 隔离全链预演见 §4。

## 4. /tmp 隔离副本封印全链预演（预演 SHA 不作正式坐标）

预演副本 `/tmp/r01-repair-r2/iso`（克隆主仓 + R1 候选 + R2 修复），候选提交链均为彩排产物：
`R2c' f14d89d30…`（候选源冻结）→ `R2s'' 1eca294f5…`（绿色记录+分片交付）→ `R2a'' facc3e293…`（纯审计坐标推进）。

### 4.1 绿色电池（R2c' 树，全 exit 0）
typecheck（tsc×3）、core-contracts 28 文件、lint:boundary、**npm run lint（0 errors，F02 后重回电池）**、build:renderer。证据：`seal-rehearsal-r2-ceremony.txt`。

### 4.2 分片补丁生成 VERIFIED
- round2：`patchBytes=267,310,615`，`patchSha256=284e1ef024c68d8e1b0699bf71ed25777903a7d7ff11a5170331e8616e3d608f`，**6 片**（5×45,000,000 + 42,310,615），state=source、replayedMatchesCurrent=true、冻结 manifest 8191 项一致。
- round3：`patchBytes=577,588,418`，**13 片**，state=source、replayedMatchesCurrent=true、frozen=true。
- R2s'' 提交后 git status 零漂移（交付族全部入库）。

### 4.3 坐标推进与门禁
坐标六处推进至 R2s''（PROGRESS.md、UPSTREAM_SYNC_AUDIT.md、build-sync-matrix.mjs、verified-source-sha.txt、矩阵再生 133 路径投影）；post-verification diff guard：**仅 6 个审计文件变化**；R2a'' 处四文件门禁 **74/74**（68 既有 + 6 新增分片夹具用例；含 post-verification-audit-seal、round2/round3 交付、upstream-sync-matrix）。

### 4.4 对象尺寸审计与推送预演（审计口径修正后）
- 首跑审计用 `git cat-file --batch-all-objects` 报 5 个 >100MiB blob——**该口径过宽**：它计入本地对象库不可达残留。取证：主仓对象库共 13 个 >100MiB blob（最大 324,997,026 B），**全部不从任何 ref 可达**（`git rev-list --all --objects` 逐一核对），属历史暂存/生成器临时索引残留，git push 永不发送；正确口径=推送引用可达集。
- 修正后审计（`continue-audit-push.sh`）：R2a'' 可达 blob 14,579 个，**0 个 ≥100MiB**，最大 97,003,232 B（92.5MiB，为历史已推送的 round3 未压缩补丁）；50MB 警告线以上 6 个全部为历史已推送对象。
- 本地裸仓推送预演：`git push /tmp/r01-repair-r2/remote.git HEAD:refs/heads/codex/rust-tauri-migration` 成功，REMOTE_SHA == R2a'' `facc3e293…`；裸仓实收对象审计同样 0 个 ≥100MiB（GitHub 视角 ground truth）。证据：`audit-push-rehearsal-r2.txt`。
- 未审计白名单未扩大、未退役任何门禁/测试、未删除应保留证据、未虚报坐标。

### 4.5 全量 npm test：2 红（环境性，已归因，非本次修复引入）
R2a'' 处全量 npm test：**14946 passed / 2 failed / 15 skipped**。2 红为 round2/round3 交付文件的"现场重放"用例，失败原因同为生成器守卫拒绝：`current source manifest does not match HEAD: firstDiff=["build/cli-runtime-closure.json"]`（证据：`npm-test-r2-rerun-failures.txt`）。归因链：
1. `tests/cli-closure-census.test.ts` 的"原位再生成"用例会重写受跟踪的 `build/cli-runtime-closure.json`；本机当前环境再生成结果与已提交版漂移：**nft 追踪新增 `/bin/bash` 一项**（totalFiles 8949→8950，+14/−3 行）。
2. 漂移写入后，同套件并行运行的交付生成器 tree==HEAD 守卫按设计拒绝 → 2 红。四文件门禁单独运行不含 census 测试，故门禁 74/74 不受影响。
3. **三实验证实环境性、与 R1/R2/R01 内容无关**：在 (A) R2a'' 树、(B) 裸 HEAD 363999378、(C) 闭包出生提交 8d55046d5（2026-09-22 19:58）裸树上再生闭包，漂移逐字节一致（证据：`closure-drift-attribution-r2.txt`）。node v24.16.0 自 5 月未变、package-lock 自 09-22 未变。
4. 处置：修复它需改动无关受跟踪文件（提交机器特定闭包）或调整预存测试/守卫行为，超出 R2 授权范围；已登记 `RR-ENV-CLOSURE-DRIFT`（MEDIUM/OPEN）+ HANDOFF 未决项索引，建议总控在正式封印前裁决（再生提交闭包并评估跨机器可移植性，或在无漂移环境执行正式流）。**此红不冒充绿、不记 PASS。**
- 另如实披露：仪式首跑的全量 npm test 同样 2 failed（1 个文件），因日志仅留 tail，首跑失败用例名未留存；重跑定位为上述 2 用例。

## 5. 修改文件清单与逐文件 SHA-256（候选工作树，42 项）

候选 = HEAD 363999378 + 16 个已跟踪修改 + 26 个新文件 = 42 项（R1 候选 9 修改 + 16 新增原样保留；R2 叠加 7 个已跟踪文件修改——两个补丁生成器、eslint.config.js、probe.js、patch-seal-fixture.ts、两个交付测试——与 10 个新文件：9 个 R2 证据 + r01_t08_gate_cli_regression.sh）。
**候选工作树指纹**（与 R2 评审同法：`git diff --binary` 输出串接按路径排序的新文件 SHA-256 清单后取 SHA-256）：

```
beee5117ed3bd0382efa3ac6ac43d353cc37befa56c12ecb6e29e60169808038
```

逐文件 SHA-256（`shasum -a 256`；M=修改，N=新增）：

```
95f04e324d394b0c11643704cfd281274ab30bdb21d5fed70bd131233675d89d  M artifacts/f1-f12-repair/round2/create-delivery-patch.py
d3994dc98d372fa27cd26a7eb9b74714989bae1f5fb0e7842945b5a54ee7ea6d  M artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py
6325d0a7ab61262266a9bd7db1a065221ac113c5edeefce879fae4539b0471bc  M artifacts/rust-tauri/R01/T08/gate-checker/gate-check.log
0d9d31e1c2112b881103d5d40aeff14ba3ca92c45c362265a43f393ff3247bc0  M artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json
69b672cf54721d6b45729b4102e987ed6b8950aaef76cd10ee5cbebca3ad32f3  M artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log
96c79d9dfa637fe228db56aac8178561884235c1c7dda66024868658fb9b542b  M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json
3b3a77cf994b567ec004b26cc0a9ae2092b043eb1ddee06b13d9a7e77b66bb36  M docs/rust-tauri/R01/R01_ACCEPTANCE_LEDGER.json
54a8e9a4485e82aa512fa7a44c35b77b4d606c9d900ed042d1552fa3b1c4bfb1  M docs/rust-tauri/R01/R01_HANDOFF.json
c300f52ea9cd14377bb6e66f8707a535690a1e51e1b1deb042e8d53fcf3302fe  M docs/rust-tauri/R01/R01_REPORT.md
77537ed663002001f4f2668e09ef4b9297a3d0323c3f5423983d2455fed172e1  M docs/rust-tauri/R01/RISK_REGISTER.json
d2bb365c10edd1dc818827e4757d9b6322d56d4564960243d9bb1efd93f5742f  M docs/rust-tauri/R01/r01_t08_gate_check.py
31b986418d50cf5abd837dd7a5e935b11f4ab7db58cdee15af7fd6008d284999  M eslint.config.js
893c851550ee67cfda0624f33ed1dfe3bdb9bc64101c06fe88f8954c40c76d4b  M spike/tauri-shell/app/frontend/probe.js
1bf8003e9104d7161a2a5d1580ba658b4b584023d0b63a1f3a6fbcee2929fedb  M tests/helpers/patch-seal-fixture.ts
1af66eee3ca0318e86b9471353f28eed50de55f0e12b418c0afe0dfebd36da9c  M tests/round2-delivery-evidence.test.ts
5ccc692b59ac37e161c2259b21866e4ce0060058333c145b243d6662cfdc17c4  M tests/round3-delivery-evidence.test.ts
f869e5352c012c1de422e59e73398eebfeeb6456a7249696ab1ec890c911d529  N docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh
--- N artifacts/rust-tauri/R01/STAGE_REPAIR_R2/（9 个证据文件）与 STAGE_REPAIR_R1/（16 个 R1 证据，原样保留）逐文件哈希见 candidate-file-sha256.txt（/tmp/r01-repair-r2/），本表从略以控篇幅；指纹覆盖全部 26 个新文件。
```

（完整 42 行清单：`/tmp/r01-repair-r2/candidate-file-sha256.txt`；指纹输入留存 `/tmp/r01-repair-r2/candidate-fingerprint-input.bin`。）

## 6. 可复现命令

```bash
# F01
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test            # 29/29
bash docs/rust-tauri/R01/r01_t08_gate_cli_regression.sh                     # 16/16（1 正向+15 负向）
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py \
  --out artifacts/rust-tauri/R01/T08/gate-checker/gate-report.json          # exit 0 PASS_WITH_CONDITIONS
# F02
npm run lint                                                                # exit 0，0 errors
npm run typecheck                                                           # exit 0
# F03（夹具，不触主仓交付物）
npx vitest run tests/round2-delivery-evidence.test.ts \
  tests/round3-delivery-evidence.test.ts -t 分片                             # 6 passed
# 阶段门禁其他腿
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --self-test        # 6/6
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py                    # COVERAGE-CLOSED
python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py                   # ISOLATED
# F03 全链预演（仅限 /tmp 隔离副本，勿在主仓运行）
bash /tmp/r01-repair-r2/ceremony-r2.sh           # 仪式全程（含已知 2 红）
bash /tmp/r01-repair-r2/continue-audit-push.sh   # 可达集审计+裸仓推送（全绿）
```

原始日志：仓内 `artifacts/rust-tauri/R01/STAGE_REPAIR_R2/`（9 件，含预演脚本留档）；仓外 `/tmp/r01-repair-r2/logs/`（含全量 npm test 完整输出 npm-test-rerun.log）。

## 7. 正式提交后复验步骤（交接总控/复验代理）

1. 阶段复验 PASS 后，总控提交修复候选（候选内容=本报告 §5 指纹对应工作树），记 R2c。
2. 按 PROGRESS.md seal 工作流：绿色电池（含 lint）→ 再生成双补丁（应自动分片交付，round2 6 片/round3 13 片量级）→ 全量 vitest 证据三件套 → 提交 R2s → 六处坐标推进+矩阵再生 → 纯审计提交 R2a → post-verification diff guard（仅 6 审计文件）。
3. 四文件门禁：`npx vitest run tests/post-verification-audit-seal.test.ts tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts tests/upstream-sync-matrix.test.ts` 应 74/74。
4. 全量 `npm test`：**注意 RR-ENV-CLOSURE-DRIFT**——若执行环境存在同样的 nft `/bin/bash` 漂移，census 原位写会激活 2 红；正式封印前按该条目治理（再生提交闭包或换无漂移环境）。
5. 推送前对象审计（正确口径）：`git rev-list --objects <候选SHA> | awk '{print $1}' | git cat-file --batch-check='%(objecttype) %(objectsize)' | awk '$1=="blob" && $2>=104857600'` 必须为空；推送后 `git ls-remote` 核对远端 SHA，并在远端侧复查接收对象无 ≥100MiB blob。
6. 台账回填：ORCHESTRATOR_PROGRESS/LEDGER/HANDOFF 登记正式提交 SHA 与本报告 SHA-256（本报告 SHA 按惯例由后续账本提交补登）。

## 8. 失败与限制（如实清单）

1. 全量 npm test 2 红：环境性闭包漂移 × 预存测试竞态，三实验证实与 R1/R2/R01 无关；未修复（超范围），已登记 RR-ENV-CLOSURE-DRIFT（OPEN）。
2. 主仓 13 个 >100MiB 不可达 blob（最大 324,997,026 B）为历史残留，不影响推送；`git gc --prune=now` 可清理但属本地卫生动作，本轮未执行。
3. 真实 GitHub 推送未执行（无推送授权且代理不可用）；以本地裸仓推送预演代替，远端 SHA 与实收对象审计一致。
4. T06 spike 的 WDIO/e2e 吊具未重跑（F02 改动为 lint 环境声明 + 一处语义等价改写）。
5. 预演 SHA（f14d89d30/1eca294f5/facc3e293）全部为彩排产物，不作正式坐标；正式封印必须绑定阶段复验 PASS 后的真实候选提交。
6. R01_REPORT.md / 账本 / 风险登记 / 进度 / 交接已按 R2 事实同步；HANDOFF artifact_hashes 18+1 钉住全部实算吻合。
7. 主仓全程未运行会重写交付 gzip 的脚本（主仓仅跑 -t 过滤的夹具用例与 MISMATCH 不重写的既有测试，交付 patch.gz 逐字节未变）。

## 9. 阶段复验矩阵（本修复后状态）

| 项 | 命令/位置 | 结果 | 证据 |
|---|---|---|---|
| F01 进程内负向 29 项 | gate_check --self-test | 29/29 OK | STAGE_REPAIR_R2/f01-gate-selftest-r2.txt |
| F01 真实 CLI 15 负向+1 正向 | r01_t08_gate_cli_regression.sh | 16/16 OK | f01-cli-regression-r2.txt |
| F01 正向真实数据 | gate_check --out（1.2 契约） | exit 0 PASS_WITH_CONDITIONS | T08/gate-checker/gate-report.json |
| R1 既有闭合保持 | 自测 N1–N17 含删域/删项/删哈希/删递延 | 全 NO-GO | 同上 |
| F02 全仓 lint | npm run lint | exit 0，0 errors | f02-lint-r2.txt |
| F03 分片合同夹具 | vitest -t 分片（两文件） | 6/6 passed | f03-shard-fixture-tests-r2.txt |
| F03 隔离全链预演 | ceremony-r2.sh（/tmp） | 分片 VERIFIED×2、门禁 74/74、guard 仅 6 审计文件 | seal-rehearsal-r2-ceremony.txt |
| F03 推送可行性 | continue-audit-push.sh（/tmp） | 可达集/裸仓 0 个 ≥100MiB、推送 SHA 一致 | audit-push-rehearsal-r2.txt |
| A16 覆盖 | coverage_check [--self-test] | COVERAGE-CLOSED，6/6 | T08/coverage/ |
| 原型隔离 | r01_t08_isolation_check.py | ISOLATED | T08/isolation/ |
| typecheck | npm run typecheck | exit 0 | 仪式日志 |
| 全量 npm test | npm test（iso R2a''） | 14946/2 failed（环境性，已归因登记） | npm-test-r2-rerun-failures.txt |

— 报告完。阶段 PASS/放行归全新 Codex 阶段独立复验与总控。
