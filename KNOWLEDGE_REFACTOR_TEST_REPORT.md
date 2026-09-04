# 知识重构测试报告

固定基线 `3eab85891a1747c64064252804f70c0a3773f021`。当前源码 `61ae60d1afe4f878ae638ff4de96d7e4f30bfefe`；阶段审计 `790b496d88d8af6ad4a621085b93d802cc2553f9` 仅修改六份审计文件。P0 至 P3 各项测试命令、原始失败和提交顺序完整记录在 `KNOWLEDGE_REFACTOR_PROGRESS.md`。本报告没有将旧源码、失败或未执行的结果标为当前通过。

当前状态：本机门禁、第五轮四平台 Build 33864141539 和统一产物门禁全部通过；最终审计结果由独立封印提交记录在 PROGRESS.md。

## 本机最终命令

执行日期为 2026-09-04，Node 24.16.0、macOS arm64。运行时修复提交为 `4ec98d0e`，后续 `aba59a5c` 记录经过兼容审查的代码指纹，`93764185` 修复 Windows 测试退出清理，`61ae60d1` 修复探测初次未就绪的等待。实际产品代码与本机打包源码 `4ec98d0e` 相同（Git 逐目录差异为空）；当前源码在第五轮 CI 中重新完整构建。下表逐命令原始 UTC 时间、退出码保存在对应 JSON；表中耗时单位为秒。

|命令|结果|耗时|通过/失败/跳过|证据|
|---|---|---:|---|---|
|`npm run typecheck`|exit 0，三组类型检查|22.083|三组通过/0/0|`artifacts/knowledge-final-static-verification.json`|
|`npm run lint`|exit 0，0 errors / 9190 warnings|14.594|0 errors，既有警告保留|`artifacts/knowledge-final-static-verification.json`|
|`npm run lint:boundary`|两轮 exit 0|逐轮见 JSON|通过/0/0|`artifacts/knowledge-final-generator-verification.json`|
|`npm test`|exit 0|78.81|13751/0/7 既有；1357 文件通过、1 既有跳过|`artifacts/knowledge-final-vitest-verification.json`|
|`npm run build:server`|完整打包中实际重新执行，exit 0|包含在打包内|不适用|`/tmp/lingxi-knowledge-p307-source4ec-pack.log`|
|`npm run build:server:open`|exit 0|38.748|不适用|`artifacts/knowledge-source4ec-package-verification.json`|
|`npm run build:client`|完整打包中实际执行五入口构建，exit 0|包含在打包内|不适用|`/tmp/lingxi-knowledge-p307-source4ec-pack.log`|
|`npm run test:knowledge-platform-smoke`|本机 exit 0，19 文件|25.86|151/0/0|`/tmp/lingxi-knowledge-p307-platform-1.log`；平台当前源码结果见下|
|`node scripts/smoke-packaged-knowledge.mjs`|真实归档安装、重启、原文检索及原生扩展移除后的回退，exit 0|10.617|不适用|`artifacts/knowledge-source4ec-package-verification.json`|
|`node scripts/scan-persistent-stores.mjs`|两轮 exit 0，66 stores / 779 sites|逐轮见 JSON|不适用|`artifacts/knowledge-final-generator-verification.json`|
|`node scripts/generate-persistence-schema-fingerprint.mjs`|显式兼容审查后，两轮 exit 0|逐轮见 JSON|不适用|同上|
|`node scripts/check-persistence-schema-fingerprint.mjs`|两轮 exit 0；持久化门禁另有 15 PASS|逐轮见 JSON；单测 1.74|15/0/0|同上及 `/tmp/lingxi-knowledge-p307-splash-fingerprint-tests.log`|
|`node scripts/compute-cli-closure.mjs`|两轮 exit 0，10691 文件|逐轮见 JSON|不适用|同上|
|`node scripts/export-open-tree.mjs <临时目标> --force`|两轮 exit 0，885 文件逐字节一致|逐轮见 JSON|不适用|同上|
|`node scripts/test-inventory.mjs`|两轮 exit 0，961 项逐字节一致|逐轮见 JSON|不适用|同上及 `artifacts/knowledge-test-inventory-final.json`|
|`npm run pack`|完整打包 exit 0，本地 ad-hoc，未公证|154.119|不适用|`artifacts/knowledge-source4ec-package-verification.json`|
|`node scripts/verify-seed-kit.mjs`|exit 0|3.747|不适用|同上|
|`node scripts/smoke-packaged-desktop.mjs`|当前清理脚本验证真实首启引导页、知识接口和目录清理通过|8.595（启动阶段）|不适用|`artifacts/knowledge-desktop-startup-darwin-arm64.json`|

正式服务端和客户端也曾独立运行并计时：`c860054b` 对应正式服务端 111.740s、开放服务端 38.797s、客户端 10.473s，均 exit 0，见 `artifacts/knowledge-final-build-verification.json`。后续实际桌面修复已在上述完整打包中重新构建。不能把旧版本的独立计时冒充最后一次命令耗时。

当前生成器共两轮 14 条命令全部通过，完整工作区差异不变。报告当时仍在回填，所以 `initialWorkingDiffEmpty=false`；两轮都没有改变这些报告差异。清单 SHA-256 `43df0f0f503484a22fc2d1e0c532db9d01fe820ea3e8b1ae1839bac40d6a46fd`，指纹 `sha256:5260947940c35b5db4f8dbe69b82fed4345c51ac7a37d62546bbef2fcee9d794`。旧源码的零初始差异记录仍保留在历史执行日志，不与当前记录混用。

## 平台验证

|运行|源码/审计|知识与质量|桌面及下游结果|
|---|---|---|---|
|[首轮 33850016811](https://github.com/ItsDalk-Lane/LingxiAgent/actions/runs/33850016811)|`c860054b` / `e296c017`|四平台各 151 PASS；质量 13728 PASS / 0 FAIL / 17 平台既有 SKIP，601.72s；Linux 性能通过|四平台打包通过、桌面启动失败；下游产物烟测未执行|
|[第二轮 33853266640](https://github.com/ItsDalk-Lane/LingxiAgent/actions/runs/33853266640)|`75dd47d1` / `4984e6fd`|四平台知识专项和质量门禁通过，具体计数见原始摘要 JSON|Linux 全流程成功，真实启动 16.228s；macOS 双架构和 Windows 因启动页重复导航失败；下游产物烟测未执行|
|[第三轮 33855890039](https://github.com/ItsDalk-Lane/LingxiAgent/actions/runs/33855890039)|`aba59a5c` / `4d52730e`|四平台各 151 PASS；质量 13732 PASS / 0 FAIL / 17 平台既有 SKIP，598.63s|macOS 双架构及 Linux 全流程成功；Windows 启动成功但清理临时资料失败，整步失败；下游产物烟测未执行|
|[第四轮 33858404258](https://github.com/ItsDalk-Lane/LingxiAgent/actions/runs/33858404258)|`93764185` / `84247718`，第 1 次尝试|四平台各 151 PASS；质量 13736 PASS / 0 FAIL / 17 平台既有 SKIP，634.18s|Windows、Linux、macOS arm64 全流程通过；Intel DMG 因临时磁盘 Resource busy 失败，启动和下游产物检查未执行；第 2 次尝试 DMG 通过，但初始调试状态探测超时导致启动失败；后续产物检查未执行|
|[第五轮 33864141539](https://github.com/ItsDalk-Lane/LingxiAgent/actions/runs/33864141539)|`61ae60d1` / `790b496d`|四平台各 151 PASS；质量 13741 PASS / 0 FAIL / 17 既有 SKIP，643.87s|四平台构建、包内检索、真实启动与清理全部通过；统一产物门禁 305 PASS / 0 FAIL / 0 SKIP，1.63s|

首轮平台专项：macOS arm64 49.28s、macOS x64 76.75s、Windows x64 104.52s、Linux x64 40.92s，各 19 文件 / 151 PASS / 0 FAIL / 0 SKIP。前三轮逐步骤状态、实际时间、测试摘要和桌面错误分别保留在 `artifacts/knowledge-platform-ci-*-failed.json`、`artifacts/knowledge-platform-ci-*-test-counts.json`、`artifacts/knowledge-desktop-startup-*.json`。P1 旧运行 `33829055797` 的成功只证明当时源码，不替代最终平台验证。

## 失败、复现与修复

|问题|原始结果|修复及复验|
|---|---|---|
|P3-07 首次退役入口测试参数和动作字段不符|191 PASS / 1 FAIL，8.79s|按锁定接口修正测试调用；230 PASS，8.83s；旧回归保留|
|P3-07 首轮全量：旧审计坐标、完整性内部工具缺登记|13734 PASS / 2 FAIL / 7 既有 SKIP，90.72s|用户授权同步审计；按已有隔离工具规则登记并增加真实范围断言，定向 52 PASS；后续全量通过|
|一轮命令错误地将通配符按字面传入|未找到测试，exit 1|纠正 shell 展开；知识全集 1087 PASS，22.69s，不把未执行算通过|
|初次本地完整打包误设公证开关|exit 1，145.034s；`1` 不符合已有开关要求|只纠正临时环境为 `true`，完整重跑 exit 0，86.150s；未改签名流程|
|静态检查与生成器并发，扫到临时构建文件|1092 errors / 9702 warnings|生成器自行清理后串行运行；0 errors / 9190 warnings，未改忽略规则|
|新桌面诊断脚本的全局 WebSocket 名|ESLint 1 error|显式引用全局对象后通过；四平台接线 15 PASS|
|Linux 解包目录沙箱辅助程序权限不完整|首轮真实启动主动拒绝，SIGTRAP|CI 设置正确 root:root / 4755 并核验，保持沙箱启用；第二轮 Linux 完整通过|
|macOS/Windows 启动页面重复导航|第二轮脱敏诊断确认 splash 原页面中止 -3，随后 preparing 页面加载完成|修复生产时序：等待初始页面完成、同次拆箱只切换一次；三项红测 3 FAIL / 11 PASS 后修复，关联 33 PASS；原启动失败断言未过滤或删除|
|下载超限清理早于异步打开完成|全量 13737 PASS / 1 FAIL / 7 既有 SKIP，83.95s；延迟打开的确定性红测失败|等待写入端关闭再删除；原断言保留，整文件 106 PASS，1.09s；全量后续 13739 PASS，再加时序回归当轮 13742 PASS|
|桌面时序修改触发代码指纹变化|生成器 exit 1，后续全量未执行|逐字段确认仅桌面模块代码哈希变化，数据契约不变；显式 compatible review 后生成，持久化 15 PASS，最终全量通过|

原失败日志分别位于 `/tmp/lingxi-knowledge-p307-{retire-tests,full-1,pack,lint-concurrent-generator,desktop-fix-full,splash-order-red}.log`；确定性文件清理红测为 `/tmp/lingxi-knowledge-p307-ota-delayed-red.log`，兼容性差异为 `/tmp/lingxi-knowledge-p307-splash-fingerprint-diff.json`。按名称单独运行红测时，其他用例未执行；随后整个文件及全量都完整运行，没有新增永久跳过。

Windows 第三轮清理失败为 EPERM，原始启动报告已写入就绪成功，但该步骤实际退出码为 1；以完整步骤失败为准。新增清理回归先得到 2 FAIL / 2 PASS（124ms），修复后相关 3 文件 26 PASS（142ms）。Windows 改为在主进程仍存在时按本次 PID 结束整棵测试进程树，再清理独立后台和随机目录；清理结果也写入报告。第四轮 Windows 实机已经验证启动和清理通过（65.529s，cleanupPassed=true）；最终结果见第五轮记录。本机全量 13746 PASS / 0 FAIL / 7 既有 SKIP，83.65s。

第四轮 Intel 环境故障记录见 `BLOCKED.md` 和 `artifacts/knowledge-platform-ci-33858404258-attempt1-environment-failure.json`。磁盘卸载失败发生在 DMG 生成阶段；没有把已生成的 ZIP 当作 DMG 通过，也没有跳过未执行的桌面启动或下游检查。

第四轮第 2 次尝试成功生成 Intel DMG，随后启动探测在 4.279s 因单次调试状态请求超时退出。仅有应用启动事件，尚无页面加载失败或崩溃。新增回归先 1 FAIL / 8 PASS（134ms），修后关联 31 PASS（142ms）；仅探测 TimeoutError 记录计数并留在原有 90s 就绪循环中，非法响应、真实崩溃及全部最终成功条件不变。源码 `61ae60d1` 本机类型、定向 lint、真实桌面启动及清理通过（启动 8.595s）；修后本机全量 13751 PASS / 0 FAIL / 7 既有 SKIP，78.81s；第五轮四平台均通过。

## 证据边界

包内检查使用随机资料目录，验证原始向量保留、原生 HNSW、移走所有原生扩展后在全新进程中精确回退，以及重启后真实本地原文检索。桌面成功条件同时要求打包进程、实际页面完成加载且存在文字/控件、知识接口成功；首启页面是引导页，不冒称已验证主聊天窗口。临时私钥与资料结束后清理，未覆盖用户真实应用或资料。

研究质量使用真实引擎、工具、凭据、台账和最终会话/HTTP/观测链；模型决策及最终供应商响应为本机脚本夹具，不能冒称付费供应商在线语义质量或账单。性能全部保留 CPU、内存、平台、样本和缓存边界，详见性能报告。当前没有发版、创建正式摘要历史或合并 main。

## 最终平台结果

以下均为当前源码对应的第五轮实际结果。每个构建步骤的 UTC 起止时间及原始状态见 `artifacts/knowledge-final-platform-verification.json`；测试原始摘要见 `artifacts/knowledge-platform-ci-33864141539-test-counts.json`。

|平台|知识专项通过/失败/跳过|专项耗时（秒）|实际启动（秒）|探测超时次数|打包/重启检索/启动/清理|
|---|---|---:|---:|---:|---|
|darwin/arm64|151/0/0|43.77|19.630|0|全部通过|
|darwin/x64|151/0/0|74.81|48.796|1|全部通过|
|win32/x64|151/0/0|114.23|77.239|0|全部通过|
|linux/x64|151/0/0|40.88|16.459|0|全部通过|

Intel 实机出现一次早期探测超时，随后在原有 90 秒期限内正常就绪，实际覆盖了此次等待修复。全部平台的原始启动 JSON 都保留了页面和后台成功、清理成功的独立结果。发布、发布链和镜像任务因本次为分支验证而按既有条件跳过；没有发布版本或合并 main。统一产物检查实际执行并通过。

最终源码坐标指最后的实现/修复提交；CI 提交只增加六份阶段审计。最终文档收口和审计封印单独提交，封印坐标见 PROGRESS.md。
