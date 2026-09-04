# 知识重构测试报告

固定基线 `3eab85891a1747c64064252804f70c0a3773f021`。当前为 P3-07 收口记录，最终源码坐标和四平台结果尚待回填。所有失败原样保留；不把未执行、跳过或旧提交的结果标为当前通过。P0 至 P3 各任务命令、故障和提交见 `KNOWLEDGE_REFACTOR_PROGRESS.md`。

## 本轮已执行

执行日期：2026-09-04（Asia/Shanghai）。Node 24.16.0，本机 macOS arm64。

|命令/范围|耗时|PASS|FAIL|SKIP|退出码|证据日志|
|---|---:|---:|---:|---:|---:|---|
|首轮旧路径与新入口 10 文件|8.79s|191|1|0|1|`/tmp/lingxi-knowledge-p307-retire-tests.log`|
|修正后相关 16 文件|8.83s|230|0|0|0|`/tmp/lingxi-knowledge-p307-retire-tests-2.log`|
|`npx vitest run tests/knowledge-*.test.ts`|22.69s|1087|0|0|0|`/tmp/lingxi-knowledge-p307-all-knowledge-2.log`|
|打包结构、原生移除及守卫 4 文件|2.53s|30|0|0|0|`/tmp/lingxi-knowledge-p307-package-tests-1.log`|
|`npm run test:knowledge-platform-smoke`（19 文件）|25.86s|151|0|0|0|`/tmp/lingxi-knowledge-p307-platform-1.log`|
|`npm test` 首轮（1357 文件）|90.72s|13734|2|7|1|`/tmp/lingxi-knowledge-p307-full-1.log`|
|`npm run typecheck` 第四轮（三组）|未单独计时|全部完成|0|0|0|`/tmp/lingxi-knowledge-p307-types-4.log`|
|`npm run lint`|未单独计时|0 errors / 9190 warnings|0 errors|不适用|0|`/tmp/lingxi-knowledge-p307-lint-full-1.log`|
|`npm run lint:boundary`|未单独计时|通过|0|0|0|`/tmp/lingxi-knowledge-p307-boundary-1.log`|

首轮正式入口测试误传额外工具参数，并读取了不存在的动作字段；修正为锁定接口和真实动作字段后通过。旧回归未删除。首轮全量的两项失败为尚未推进的审计坐标，以及新完整性内部工具未登记；后者按已有隔离工具规则补登记并新增范围断言，审计在最终源提交后同步。一次命令把通配符作为字面参数传入，未找到测试即 exit 1，已改为 shell 展开；不把该次执行算 PASS。

工具登记修复后，文案、完整性隔离入口及权限三文件 52 PASS / 0 FAIL / 0 SKIP，3.86s；最终接线复核 36 PASS，1.21s。日志 `/tmp/lingxi-knowledge-p307-label-fixed.log`、`/tmp/lingxi-knowledge-p307-wiring-reviewed.log`。

## 最终命令清单

以下必须针对最终源码确认后回填，目前不能视为全部完成。

|命令|最终状态|
|---|---|
|`npm run typecheck`|最终本地复验 exit 0，26.404s，2026-09-04 15:20:11–15:20:38|
|`npm run lint`|最终本地复验 exit 0，0 errors / 9190 warnings，17.497s|
|`npm run lint:boundary`|最终本地复验 exit 0，0.897s|
|`npm test`|待修复后全量复验及坐标同步|
|`npm run build:server`|本轮 exit 0，待最终坐标绑定|
|`npm run build:server:open`|本轮 exit 0，待最终坐标绑定|
|`npm run build:client`|本轮 exit 0，五入口构建完成|
|`npm run test:knowledge-platform-smoke`|本机 151 PASS，待四平台当前源码|
|`node scripts/smoke-packaged-knowledge.mjs`|本轮 exit 0，真实归档安装/重启/本地检索/移除原生扩展后检索|
|`node scripts/generate-persistence-schema-fingerprint.mjs`|首轮 exit 0，待第二轮一致性|
|`node scripts/check-persistence-schema-fingerprint.mjs`|待最终执行|
|`node scripts/compute-cli-closure.mjs`|首轮 exit 0，10691 文件|
|`node scripts/export-open-tree.mjs <临时目标> --force`|首轮 exit 0，885 文件|
|`node scripts/test-inventory.mjs`|首轮 exit 0，待逐字节一致性|
|`npm run pack`|本地完整重跑 exit 0，86.150s；ad-hoc 签名，未公证|

## 平台与证据边界

|平台|当前 P3 源码状态|
|---|---|
|macOS arm64|本机源码平台 151 PASS；真实归档构建/验签/重启检索通过；完整应用打包通过（本地 ad-hoc，未公证）|
|macOS x64|待当前源码 CI|
|Windows x64|待当前源码 CI|
|Linux x64|待当前源码 CI 和性能门禁|

P1 的四平台 Build `33829055797` 全部通过，仅证明该次源码；不能替代本轮。包内测试使用临时签名私钥和临时资料目录，结束清理，不修改用户真实数据与正式签名公钥。研究最终合成的本机 HTTP 测试供应商证明传输、观测与引用校验，不宣称付费模型在线质量。

首次完整 `npm run pack` exit 1（145.034s）：临时环境误把已有跳过公证开关值写为 `1`，脚本要求 `true`，因此误入缺少 Apple 密码的公证。原始失败日志 `/tmp/lingxi-knowledge-p307-pack.log` 保留；只纠正临时环境值后重跑完整命令，不改生产签名/打包逻辑。

完整打包纠正后已通过：`/tmp/lingxi-knowledge-p307-pack-reviewed.log`，结果与执行时间见 `artifacts/knowledge-final-package-verification.json`。产物 `dist/mac-arm64/Lingxi.app`；未安装覆盖用户应用。

本机真实打包桌面：首轮主窗口与服务端就绪通过；增强版以本地调试协议只读页面，首启引导页已完成加载、93 字符、7 个交互控件，后台知识接口响应成功，8.691s PASS。证据 `artifacts/knowledge-desktop-startup-darwin-arm64.json`。新的四平台接线测试 15 PASS / 0 FAIL，200ms。脚本首次静态检查因 WebSocket 全局名报 1 error，已改为显式 globalThis 引用后复验。
