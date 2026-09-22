# P03-T02｜工具参数 Schema 契约（目录 / describe / validator 同源）

日期：2026-09-22｜状态：既有实现 UNCHANGED_VERIFIED + 本阶段补一致性测试。本文描述当前真实行为，均有测试坐标。

## 1. 单一 schema 与版本来源

**注册即快照。** 每个目标注册时经 `createToolSchemaValidator(schema, identity)`（`lib/tools/invocation/schema-validator.ts`）：

1. `snapshotToolInvocationInput`（`lib/permission/tool-invocation-permission.ts:237`）对来源 schema 做深拷贝并冻结——纯 JSON 数据、无访问器/原型污染/循环引用、拒绝 `__proto__`/`prototype`/`constructor` 键。
2. `TOOL_SCHEMA_META_SCHEMA` 检查 schema 自身形状（支持的数据校验子集：type/properties/required/items/anyOf/oneOf/allOf/not/additionalProperties/enum/数值与长度边界/pattern）。
3. 六个探针值（`{}`/`[]`/`null`/字符串/数字/布尔）实际消费该 schema，注册一个"表面是对象、调用才炸"的契约直接抛 `TOOL_SCHEMA_INVALID`。

`validator.schema` 就是这份冻结快照。engine 三处注册（插件 `engine.ts:4147`、MCP `:4198`、第一方 `:4410`）都令 `target.parameters === validator.schema`（同一对象）。

**版本可追踪：**
- 内置/插件目录行：`builtinCatalogEntryFromTarget` 的 `schemaRef: () => target.parameters`——与 validator 同一冻结对象（对象同一性）。
- MCP 目录行：`schemaRef: () => publishedTool.parameters`（`core/mcp/manager.ts:2560`）——指向来源原件，可追踪来源版本；执行校验恒用注册期冻结快照。两份内容一致（深拷贝关系，见测试），装配后来源漂移只能让 describe 变、不能让执行契约变（漂移的参数会被冻结快照拒绝，fail-closed）。

## 2. 三处消费同源（测试锁定）

| 消费处 | 读取对象 | 一致性证据 |
|---|---|---|
| 目录 describe（`mcp_describe_tool`） | catalog entry `schema`/`schemaRef` | `tests/engine-tool-defer.test.ts` "renders deferred describe from the same schema content the validator snapshot was built from" |
| 直挂面 schema（模型请求里的 parameters） | facade `.parameters` = target.parameters | 同文件 "keeps direct facade parameters, registry parameters and validator schema one frozen object"（`toBe` 对象同一性，含 MCP 与插件两源） |
| 运行时校验 | `target.validator.validate`（网关 resolvePermission 与 invoke 各一次） | 同文件 "cannot widen the execution contract by mutating the source schema after registration" + `tests/tool-invocation-gateway.test.ts` mismatch 族 |

同版本同输入，三处解释一致；schema 没有第二份手工拷贝。

## 3. 规范化规则（先于审批）

- **规范化发生在权限解析与摘要绑定之前**：`ToolInvocationGateway.resolvePermission` 先 `validator.validate` 再 `digestToolArguments` 入 prepared；`invoke` 执行前重校验并比对 `argumentsDigest`——审批后悄悄改参数会 `PREPARED_INVOCATION_MISMATCH`（gateway 测试"参数替换"）。键序不影响 digest（同文件"参数键顺序变化"用例）。
- **不注入、不转换、不删字段**：validate 通过即原样返回（`tests/tool-schema-validator.test.ts` "接受满足 union 与范围约束的对象并原样返回"）；默认值仅在 describe 渲染中展示（`默认 …`），validator 不回填；无参数别名机制。
- **输入边界**：深度 ≤16、节点数 ≤4096、字符串 ≤2MiB、循环拒绝（`MAX_INPUT_DEPTH/MAX_INPUT_ITEMS/MAX_INPUT_STRING_LENGTH`；超限用例见 `tests/tool-schema-validator.test.ts` "拒绝超过深度/数量上限的参数"）。

## 4. 错误反馈

- 错误码稳定：`TOOL_SCHEMA_INVALID`（注册期）/`ARGUMENTS_NOT_OBJECT`（非对象或超限）/`ARGUMENT_SCHEMA_INVALID`（内容不匹配）。
- `details.issues = [{path, message}]` 按 path 排序，`issuePaths` 去重——安全字段路径（JSON Pointer），不回显参数值；网关诊断日志只含 route/origin/targetId/sourceId/generation/code（`tests/tool-invocation-gateway.test.ts` "错误日志只记录安全归因字段"断言不泄 `/Users/alice` 与密钥样例）。

## 5. 供应商 schema 关键字：能力限制（无转换层）

当前**不存在** schema 关键字翻译/裁剪层：Pi SDK 请求携带的就是上述同一 schema 对象，后端校验子集即完整契约。因此：

- 没有"只去掉 required"式的静默裁剪；供应商忽略某关键字只会影响模型产出质量，违规参数在触及执行器前被 validator 拒绝（fail-closed）。
- 运行时校验只执行第 1 节元 schema列出的关键字子集；未知关键字（如 `format`）按 JSON Schema 惯例容忍但不参与校验——这是登记的能力限制，不是校验绕过（描述层可见、执行层不放宽）。
- 若未来引入按供应商投影，必须以本契约第 1 节冻结快照为唯一后端真值，投影只做显示层，并补"可转换子集"双向测试。

## 6. 变更记录

- 2026-09-22（P03-T02）：补三处同源一致性测试（engine-tool-defer 3 例）与输入上限用例（tool-schema-validator 1 例）；生产代码零改动（UNCHANGED_VERIFIED）。
