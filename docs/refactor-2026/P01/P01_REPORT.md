# P01 阶段执行报告｜模块边界、Pi适配与严格类型入口

日期：2026-09-21｜执行分支：`docs/knowledge-closeout-2026-09-21`（未切换，P00 交接允许）

## 结论

**PASS（验收修复轮后）**。首轮独立验收判 FAIL（5 项发现：A07 字段类型维度未闭合、command-log 结构退化、artifacts 工具 lint error 未登记、检查器拼接绕过面未登记、若干小项）；本修复轮全部闭合并复验（见 §验收修复轮）。生产改动 20 个跟踪文件（详见 §改动清单），全部为本阶段任务书与验收场景必需的最小实现；未 push/未开 PR/未 tag/未发布（提交推送按用户后续明确授权执行）。全量 `npm test` 结果见 §验证（预期红 = F1 治理 4 例，与 P00 基线一致，未扩大）。

## 实际输入

- START_SHA = END_SHA（工作区）= `92c6646c581883436e026c53898e8bfc34ce4d27`（零生产 commit；全部改动为工作区未提交状态，与 P00 工作模式一致，等待用户授权提交）
- 研究基线 `8037fae7a` 差异 = docs-only（与 P00 记录相同，无新漂移）
- Node v24.16.0｜npm 11.13.0｜darwin 27.0 arm64｜lockfile sha256 `a9735825cea1d018…`（与 P00 相同）
- 前置：P00 PASS（P00_RESULT.json）；必读输入（OWNERSHIP_MAP/CALLSITE_MATRIX/ENTRYPOINT_MATRIX/BASELINE_FAILURES）全部读取并逐项复核

## 任务逐项结果

| 任务ID | 实现动作 | 生产入口与源码位置 | 测试与日志 | 旧路径去向 | 状态 |
|---|---|---|---|---|---|
| P01-T01 边界固化 | 纯文档（零代码） | docs/refactor-2026/P01/BOUNDARIES.md：六类职责映射、7 个既有公共接口清单（含消费者）、Pi↔产品状态映射方向（事件 Pi→产品/控制 产品→Pi）、5 条禁止反向依赖 | 依赖 P00 证据 + 本阶段 grep/AST 复核（BOUNDARIES §4 现状列全部实测 0 处） | 无旧路径（任务为固化规则） | PASS |
| P01-T02 Pi 适配清点 | UNCHANGED_VERIFIED（适配层零修改）+ 1 处测试盲区关闭 | lib/pi-sdk/index.ts（唯一入口）；深路径例外 2 处（适配层内）+ 构建脚本 2 个 + 注释 1 处；锁定版本实测 0.86.0 | P01-T02-pi-sdk-regression.out（9 文件 41 用例绿，含真实 SDK+witness e2e） | tests/pi-sdk-import-boundary.test.ts 扫描面 .js/.mjs/.cjs→+.ts/.tsx、根目录 +cli/shared/plugins/desktop（旧盲区由 postinstall 兜住，现双层） | PASS |
| P01-T03 宿主可替换 | 纯文档 + 集成测试（核心零改动） | docs/refactor-2026/P01/HOST_CAPABILITY_MAP.md；core 对 electron 依赖实测 0 处 | tests/server-composition-boundary.test.ts Part 4（9/9 绿） | 无桌下能力语义实测登记（通知=通道广播；ComputerUse=显式不可用） | PASS |
| P01-T04 strict 核心入口 | MODIFIED（新增工程/脚本/品牌模块 + 4 文件类型化） | tsconfig.core-contracts.json + scripts/check-core-contracts-strict.mjs + npm run typecheck:core-contracts；范围 6 文件（identity-brands/hana-runtime-paths(.d.cts)/model-ref/errors/model-call-identity/local-server）；strict+noUncheckedIndexedAccess+exactOptionalPropertyTypes | tests/core-contracts-strict.test.ts 4/4（正例+A05+A06+fixtures 隔离） | 宽松区（tsconfig.node strict=false）保持原样=显式剩余账本，未扩大 | PASS |
| P01-T05 运行时契约 | MODIFIED（1 处生产缺口修复 + 测试扩展） | server/hono-helpers.ts strictJson（新增）→ /sessions/new 切换；A08 网关用例×2；A09/A11 CLI 用例×5 | vertical-slice（A07 修复前红 r5→修复后绿 r9）；gateway-a08 17/17；cli 8/8 | safeJson 其余调用点不动（宽松读取面维持既有行为；写入口按需逐个切换，属后续阶段） | PASS |
| P01-T06 依赖纪律检查 | MODIFIED（新检查器 + CI 两步） | scripts/check-dependency-boundaries.mjs（5 规则：sdk-direct-import/sdk-deep-path/host-into-core/adapter-reverse-dep/sdk-dynamic-unproven）+ npm run check:dependency-boundaries + ci.yml 两步（未删任何原门禁） | tests/dependency-boundary-check.test.ts 6/6（A03/A04/A12）；生产 2025 文件零违例 | check-tool-invocation-boundaries 保持原职责（2243 文件绿），不重复建设 | PASS |
| P01-T07 纵向链+清退 | MODIFIED（LINGXI_HOME 统一 + 死别名清退） | cli/local-server.ts 统一到 shared/hana-runtime-paths；tsconfig.json/tsconfig.test.json @lingxi/plugin-* 与 vitest.config.js @hana/plugin-* 死别名清退（先核实零引用）；tsconfig.json packages/ include 移除 | cli-local-server 8/8（A11 语义逐例保持）；vertical-slice 9/9 | resolveCliLingxiHome 保留为 thin wrapper（env 取值+trim 后调 shared 权威）——双实现收口，非删除 API | PASS |

## 场景逐项结果

见 docs/refactor-2026/P01/ACCEPTANCE_MAP.json（12/12 PASS，每条含 test_path/test_name/fixture/production_entry/expected/actual/command_ids/evidence）。

## 接线和旧路径

- 新链消费者：typecheck:core-contracts 与 check:dependency-boundaries 已进 CI（lint 步之前）；品牌类型的铸造厂消费者=lib/llm/model-call-identity.ts；strictJson 消费者=/sessions/new；无桌面纵向链测试复用 composition 测试文件的真实 spawn 工具。
- 旧路径去向：LINGXI_HOME 双实现→收口（CLI wrapper→shared 权威）；死别名→删除（零引用核实）；safeJson 在 /sessions/new→被 strictJson 替换（其余读取面保留）；tests/pi-sdk-import-boundary 旧扫描面→扩面（非删除）。
- 无双写：LINGXI_HOME 单一权威；无新增第二 Agent 循环/网关/凭证体系（Pi 适配层零修改）。
- 无"仅测试使用的新模块"：identity-brands 由生产铸造厂消费；strictJson 由生产路由消费。

## 验证（命令与 exit code）

| 命令 | exit | 日志 |
|---|---|---|
| npm run typecheck（3 腿） | 0 | P01-T07-typecheck-r4.out（r1-r3 失败迭代留档） |
| npm run typecheck:core-contracts | 0 | P01-T04-core-contracts-pass.out |
| npm run check:dependency-boundaries | 0 | （终端直跑，2025 文件） |
| npm run check:tool-invocation-boundaries | 0 | P01-T06-tool-invocation.out（2243 文件） |
| npx eslint（本轮全部改动文件） | 0 error / 32 warning（既有 any 警告） | P01-T06-lint-changed.out |
| vitest pi-sdk 族 9 文件 | 0（41 用例） | P01-T02-pi-sdk-regression.out |
| vitest server-composition-boundary | 0（9 用例，含纵向链） | P01-T03-vertical-slice-r9.out（r1-r8 失败迭代留档） |
| vitest core-contracts-strict | 0（4 用例） | P01-T04-strict-gate-test-r2.out |
| vitest dependency-boundary-check | 0（6 用例） | P01-T06-boundary-selftest.out |
| vitest tool-invocation-gateway | 0（17 用例，含 A08×2） | P01-T05-gateway-a08-r5.out |
| vitest cli-local-server | 0（8 用例，含 A09×4/A11） | P01-T05-cli-a09-a11.out |
| vitest 契约三件套（quality-gates/cli-closure/open-boundary） | 0（45 用例） | P01-T08-contract-tests-r3.out（r1 随全量红：见下"契约基线再生成"） |
| npm test（全量第一轮） | 1（9 失败 = F1 基线 4 + 契约基线 5，全部定位修复） | P01-T08-full-npm-test.out |
| npm test（全量第二轮，契约修复后） | 见日志（预期仅 F1 4 例） | P01-T08-full-npm-test-r2.out |
| npm run build:server:open / smoke:server:open | **BLOCKED（环境）**：依赖安装需 npm registry，本机代理 127.0.0.1:7890 不通且直连被拒（curl 000，F3 同源）。等价覆盖=composition 测试 Part 3/4 在开发树 spawn 全量组合完成真实 HTTP/WS；未覆盖仅打包 open 树变体（归 P08） | P01-T07-build-server-open.out（终止于依赖安装） |

**契约基线再生成记录**（全量第一轮 5 个新失败的处置，全部为"生成物基线需按仓库脚本更新"类）：
1. tests/quality-gates-contract：tsconfig.json 的 packages/*/src include glob 被契约钉住（目录已不存在但 glob 空匹配无害）——恢复 glob；退役该契约属治理变更（P08/用户决策）。
2. tests/cli-closure-census ×2：cli/local-server.ts 新导入 shared/hana-runtime-paths.ts 使 CLI 闭包变化——按设计流程重跑 `node scripts/compute-cli-closure.mjs` 再生成 build/cli-runtime-closure.json + build/open-boundary-baseline.json。
3. tests/open-boundary-lint ×2：新文件 shared/identity-brands.ts 未登记 open 面——加入 export-manifest.json 白名单（shared/ 字母序）。
4. 修复后定向复跑 45/45 绿；F1 四例单独复跑确认失败形态与 P00 基线逐字一致（P01-T08-f1-isolated-r2.out）。

首次失败全部留档（vertical-slice r1-r8、typecheck r1-r3、gateway r1-r4、strict-gate r1）。

## 数据、权限与平台

- 全部测试使用 mkdtemp 隔离目录与本地 witness 协议 server；零真实供应商调用、零真实用户数据写入（纵向链测试预置有效 agent config 避免 first-run 触碰真实 ~/Desktop）。
- 权限面：A08 证明模型 args 不能覆盖宿主身份（prepared 绑定）；/sessions/new 畸形载荷现被显式拒绝（400 invalid_json）。
- 平台：仅 darwin arm64 实测；四平台 CI 证据仍 BLOCKED（F3 环境不变）。

## 差异与限制

1. **超出 P00 建议修改清单的生产改动**（任务书 T05/A07 必需，已在报告显式说明）：server/hono-helpers.ts（+strictJson）、server/routes/sessions.ts（/sessions/new 切换）。行为变化仅"非空但不可解析的 body 由静默成功变为 400"；空 body/合法 JSON 行为不变。
2. **strict 化暴露并修复的既有死路径**：lib/env-deps/detect.ts managedBinPath 零参调用（6282cf35b 引入，运行时必 throw→静默 null，违反"禁止静默降级"红线）。修复=显式传入 resolveLingxiHome()；行为恢复设计意图（托管二进制目录重新参与 env-deps 检测，仅 fs X_OK 探测，风险面小）。
3. **登记移交**：桌面通知通道 status:"sent"=已广播而非已确认送达（用户采纳行为，P01 不改，语义澄清移交 P05）；typebox/value 是否经适配面 re-export 移交 P04/P08；Math.random 铸造（非密码学，Mimosa 建议关注项）登记于 STRICT_SCOPE 例外账本。
4. **未完成/BLOCKED**：四平台 CI 运行证据（F3：gh 认证+代理不可达，移交 P08）；Windows/Linux 实机（本地 darwin only）；真实供应商（零授权零消耗，P04）。
5. F1（seal 坐标滞后 4 用例红）与 F2（lint 3 生产 error）维持 P00 定级（治理流程/P03 主责），P01 未触碰。

## 回退与下一阶段

- 回退：全部改动为工作区未提交状态，`git checkout -- <file>` 可逐文件回退（生产文件 12 个见清单）；新增文件（scripts/tests/docs/config）直接删除即可；无数据格式变更、无不可逆操作。回退接口重构时保留新增回归测试（任务书 §9）。
- 交接：docs/refactor-2026/P01/NEXT_STAGE_HANDOFF.md。

## 改动/保留/删除清单

**生产代码（MODIFIED/NEW）**：server/hono-helpers.ts（+strictJson）、server/routes/sessions.ts（/sessions/new 切换）、cli/local-server.ts（LINGXI_HOME 统一+strict 化）、shared/model-ref.ts（显式类型+AvailableModelLike）、shared/errors.ts（exactOptional 修复）、shared/identity-brands.ts（NEW）、shared/hana-runtime-paths.d.cts（NEW）、lib/llm/model-call-identity.ts（品牌接入）、lib/env-deps/detect.ts（死路径修复）、tsconfig.core-contracts.json（NEW）、scripts/check-core-contracts-strict.mjs（NEW）、scripts/check-dependency-boundaries.mjs（NEW）、tsconfig.json（@lingxi 死别名清退；packages include glob 恢复）、tsconfig.test.json（死别名清退+fixtures 排除）、vitest.config.js（@hana 死别名清退）、package.json（2 个 npm script）、.github/workflows/ci.yml（2 步）、export-manifest.json（+identity-brands）、build/cli-runtime-closure.json + build/open-boundary-baseline.json（脚本再生成）。
**测试（NEW/EXTENDED）**：tests/core-contracts-strict.test.ts（NEW）、tests/dependency-boundary-check.test.ts（NEW）、tests/fixtures/core-contracts-negative/{a05,a06}.ts（NEW）、tests/server-composition-boundary.test.ts（+Part 4 纵向链）、tests/pi-sdk-import-boundary.test.ts（扫描面扩展+typebox 例外）、tests/tool-invocation-gateway.test.ts（+A08×2）、tests/cli-local-server.test.ts（+A09×4/A11）、tests/model-observability-{durable-matrix,trace-projection}.test.ts（mock 铸造经守卫）。
**保留**：lib/pi-sdk/** 全部（UNCHANGED_VERIFIED）；safeJson 及其余调用点；@/ 与 @/* 有效别名。
**删除**：@lingxi/plugin-*（tsconfig.json+tsconfig.test.json）、@hana/plugin-*（vitest.config.js）、tsconfig.json packages/ include glob。

## 验收修复轮（2026-09-21 深夜，独立验收 FAIL 后）

独立验收（只审查不改代码）判 FAIL，5 项发现。修复与复验逐项如下：

| # | 验收发现 | 修复 | 复验 |
|---|---|---|---|
| 1 | **A07"错误字段类型→运行时拒绝"只闭合了整体不可解析一半**：实测 `memoryEnabled:"yes"` / `thinkingLevel:{}` 被 200 静默建会话、`agentId:12345` 以 500 崩溃；`null`/`123` 等非对象 JSON 透传（null 解构 500、其余按默认静默执行） | `server/hono-helpers.ts` 新增 `ensureJsonObjectBody`（非 plain object → 400 invalid_body）与 `rejectWrongFieldTypes`（字段类型不符 → 400 invalid_field_type，null/缺省视为未提供）；`/sessions/new` 接入（memoryEnabled:boolean、thinkingLevel:string、agentId:string、currentAgentId:string）。错误经 `classifySessionCreationError` 既有通道扁平透传 code+status | 纵向链测试新增 7 组真实入口断言（3 字段类型 + 4 非对象 body，均断言 400 + code + 会话清单不变）→ 9/9 绿（P01-ACC-FIX-vertical-slice） |
| 2 | **command-log.jsonl 退化为三字段文件索引**，违反 92 模板 §3（缺 command_id/argv/exit_code/时间戳/status/retry_of） | 新建 `tools/rebuild-command-log.mjs` 从日志产物+mtime 重建 42 条命令级条目，全部标注 `rebuilt:true`+`timestamps_reconstructed:true`；原索引保留为 `log-file-index.jsonl`；重验轮命令改用 `tools/run-logged.mjs`（与 P00 同构）实时记录 | 重建条目 exit/status 与已知事实逐条核对（r5=1、r9=0、lint-changed=0、"0 errors 32 warnings"、build-server-open=BLOCKED/null、全量=F1 红）；重验轮 12 条 P01-ACC-FIX-* 为实时记录 |
| 3 | **P00 artifacts 工具引入 3 个 lint error 未登记**（bench-startup 'performance'×2、prompt-baseline 'Buffer'），全仓 6 error 而 P01 报告写"F2 维持 3 error" | 两个 .mjs 补显式 node import（`node:perf_hooks`/`node:buffer`）；报告如实更正 | 全仓 lint 复验 exit 1、**3 errors**（=F2 基线逐字一致：run-code-tool 118/119 + security-scan-tool 191；P01-ACC-FIX-lint-full-r2） |
| 4 | **依赖检查器拼接绕过面未登记**：`"elect"+"ron"`、`"@earendil-works"+"/pi-ai"` 拼接后计算式动态 import 全部 0 违例（保守规则只认完整字面量；host-into-core 无计算式兜底） | `check-dependency-boundaries.mjs` 加固：常量拼接折叠（StringLiteral 二元 + 求值）、敏感片段提及（SDK scope/包名片段 + electron）、新规则 `host-dynamic-unproven`（core-like 根内计算式动态 import + 提及 electron）；运行时拼接（env/函数返回值）登记为固有静态边界 | 新增拼接反例用例（sdk-concat→sdk-dynamic-unproven、electron-concat→host-dynamic-unproven、runtime-loader 维持放行）→ 7/7 绿；生产 2025 文件复扫零误报（P01-ACC-FIX-boundary-selftest / -dependency-boundaries） |
| 5 | 小项：extraFiles 指向不存在文件被静默忽略（假绿风险）；export-manifest identity-brands 非字母序插入（报告却称字母序）；gateway 测试一处两行意外合并 | `check-core-contracts-strict.mjs` 对不存在 extraFile 显式 throw（顺带修正错误路径上未定义的 `TSCONFIG_PATH` 笔误）；manifest 条目移至正确字母序位置；格式恢复 | strict 自测新增 missing-file 用例 → 5/5 绿（P01-ACC-FIX-strict-gate）；manifest/格式为无行为差异修正 |

**修复轮验证汇总**（run-logged 实时记录，command-log.jsonl 追加）：typecheck 三腿 0｜typecheck:core-contracts 0｜check:dependency-boundaries 0（2025 文件）｜strict 自测 5/5｜边界自测 7/7｜纵向链 9/9（含新 A07 断言）｜gateway 17/17 + cli 8/8｜pi-sdk 族抽样 6 文件 14/14｜契约三件套 45/45｜全仓 lint = F2 基线 3 error（预期红，主责 P03）｜全量 npm test = F1 基线 4 红其余全绿（P01-ACC-FIX-full-npm-test-r2；14705 通过含新增用例）。

**重验轮新登记（发现 6，不阻塞、移交治理域）**：全量首轮（P01-ACC-FIX-full-npm-test）出现 R10-09 偶发红——`create-delivery-patch.py` 的校验流程对工作树做两次 `git add -A` 快照，全量运行期间任何对扫描范围内文件的并发写入（本轮为 run-logged 实时追加的 stdout 日志文件）会使两次快照撕裂导致 manifest 不一致；输出重定向到工作区外复跑即绿、单独复跑亦绿（4 failed 基线逐字恢复）。属 round2 交付证据契约的运行期脆弱性（F1/治理域，P01 无权修改），登记移交；本仓后续全量验证建议 stdout 落 /tmp 或等待测试静默期。

**验收结论状态变化**：首轮 FAIL → 修复轮后 PASS。修复未触碰 F1/F2/F3 定级、未修改任何审计封印文件；A07 现覆盖场景规格全部三个维度（畸形 JSON / 非对象 JSON / 字段类型错误）。
