# F02 / F04 交叉复核

2026-09-22，只读审查当前实际 diff、生产调用和本轮测试。审阅者拥有 F01 实现，因此本页针对 F02 / F04 的独立检查，不将自审 F01 当独立验收。未重跑全套，定向执行的跳过项仅由 -t 过滤，不是 expected failure 或必需验收的豁免。

## 结果

发现并实际复现两处 F04 门禁自测缺口，均已交回主执行者修复并复验。当前未发现 C09—C13 输入边界的新阻断缺陷；F04 当前严格检查是实际实现检查，仍须结合最终严格范围账本和完整工程结果，不等于全仓 strict。

1. C21 “空值”负例误写不存在的 registry.get，实际报 TS2339；测试原来只看行号，未证明 query 的可空结果。修正为真实 registry.query，并明确要求 TS2531/TS18047 空值诊断。
2. C23 配置 include 中的精确源码入口不存在时 TypeScript 会静默忽略；用“真实 identity-brands + 缺失入口”临时配置实际取得 diagnostics=[]。主执行者新增精确 include 文件存在检查和专门测试。通配 include 不受此精确文件规则约束；当前生产清单使用精确文件。

原始复现：artifacts/refactor-2026/independent-fix/f01/cross-review-diagnostics.log。修复后运行 `npm test -- tests/core-contracts-strict.test.ts -t 'C21|C23'`：7 PASS、6 未选中，exit 0，见 cross-review-gate-green.log 与 commands.jsonl。覆盖真实实现错误 attempt、可空 query、入口种类、跨域身份、生产 STATUS_KIND 删分类、无文件全局诊断、配置损坏/缺失、精确入口缺失及真实导入缺失。

## F02 边界核查

两个实际 route 都先保持原 authorizeSessionRoute，再 strictJson + parseSessionCreateInput，之后才工作区选择、createSession/createSessionForAgent/createDetachedSession、persistSessionMeta、工作区历史、项目和谱系写入。错误 cwd/mount/folders/元素不再落到默认创建；detached 不再吞 JSON 解析错误。

逐个核对消费字段：共有 cwd、workspaceMountId、workspaceFolders、memoryEnabled、agentId、thinkingLevel、projectId；focused 另消费 currentAgentId，detached 另消费 permissionMode、forkedFromSessionId、recordWorkspaceHistory。两入口没有消费 model，focused 没有消费 permissionMode，保持未知字段忽略与现有范围一致。字段 null、空 body、空目录元素过滤是既有语义；整个 body null/数组/标量拒绝。思考档位/权限字符串值域仍由原引擎 normalizer 处理，未虚称新的枚举拒绝。

新真实 HTTP 测试在路由拒绝时检查创建、模型替身、元数据、配置、项目、谱系零调用，以及临时目录与预先初始化授权注册表不变；成功/只读主体对照同时存在。注意这是实际 HTTP/授权/路由接线加引擎副作用替身，不能写成真实供应商或完整引擎行为。没有发现通过身份注入绕开原授权的生产改动。

## F04 实际闭包与边界

TaskRegistry/总线、hono helper/实际输入解析器、实际 stream admission、工具 schema 验证、权限分类、资源清理及窄 Pi shutdown 实现进入真实 strict 编译闭包。未看到仅测试消费者的平行实现；Pi index 重新导出同一 shutdown 方法，真实 teardown 直接调用该实现。

检查器保留所有 getPreEmitDiagnostics，包括无 file 全局错误；报告分别列 roots、实际项目闭包和外部依赖。sourceOverrides 只作用于门禁测试的编译器读取，不覆盖生产源码、不削弱正常编译。负例真实调用实现，不再靠不存在的方法凑失败。

shared/safe-fs 的新增第三方边界采用 createRequire 取 unknown，运行时确认模块对象及 load 为函数后调用；没有新增假 .d.ts、双重断言或 ts-ignore。锁定 js-yaml 暴露 CJS load，服务器构建是 Node ESM；该边界合理。safeReadYAML/safeReadYAMLSync 原有泛型宽松返回没有被误称为已全收敛；TaskRegistry 实際使用其 atomicWriteSync。权限对象从 unknown 经对象检查收窄；TaskStatus 与 STATUS_KIND 有真实生产消费者及删分支负例。

剩余遗留 any（如 TaskRegistry schedule 动态数据、safe-fs 旧通用返回）需在最终范围账本保留，不能把本轮新增 task options 归回遗留。本轮 options/patch/result/输入解析没有发现这种逃避。全产品 GUI、平台与产物证据不由本交叉审阅授予 PASS。

补充独立 HTTP 复验：`npm test -- tests/session-create-input-contract.test.ts -t '拒绝错误 cwd'`，两种真实 HTTP 入口 cwd 错类型均通过拒绝与零副作用断言，2 PASS、42 未选中，exit 0；原始输出 cross-review-http-green.log。未用此两例替代 F02 全范围执行记录。
