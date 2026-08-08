# 对抗性代码审查报告 — `cce8e86..97595264`

审查日期：2026-08-08。范围实测为 6 个提交、153 个文件。本报告先记录修复前事实；后续修复结果追加在末尾，不回写原始发现。

## 任务 0 — 基线

```text
HEAD=97595264ead8735a04559507ddaade25db8a4e15
6 commits / 153 files
npm rebuild better-sqlite3: rebuilt dependencies successfully
Test Files  1 failed | 1084 passed | 1 skipped (1086)
Tests       1 failed | 10978 passed | 7 skipped (10986)
typecheck: tsc 三组，0 error
cce8e86 screenshot 单测: 1 failed | 11 passed
```

主工作树的依赖目录混有重名副本，直接验证会产生假错误。因此在干净临时工作树执行 `npm ci`、`npm run build:packages` 后取得上述基线。唯一失败是默认头像 SVG 与 PNG 断言，在 `cce8e86` 同样失败，属于预存债务。

## 任务 1 — 安全关键路径

### 审批终裁边界

模型无法直接产出 `allow`、`deny` 或 `hard_deny`。旧格式 `{action:"allow"}` 被拒绝；`authorized+broader` 和 `authorized+unclear` 均被宿主降为 `ambiguous`，最终询问用户；只有宿主识别出的禁止级操作会得到 `hard_deny`。格式失败只重试一次，传输失败映射为 `ambiguous`。

反向验证输出：

```text
exact      -> {"action":"allow"}
contained  -> {"action":"allow"}
broader    -> {"action":"ask_user","source":"reviewer_policy"}
unclear    -> {"action":"ask_user","source":"reviewer_policy"}
forbidden  -> {"action":"hard_deny","source":"policy"}
approval tests: 30/30 passed
```

[P1] `lib/approval-gateway.ts:354`（同时见 360–371、424） — 审批输入脱敏会泄露常见命令凭证和聊天证据中的凭证；审批模型可配置为远程供应商，因此自动审批会把秘密发给另一个服务 — 实测 `Authorization: Bearer sk-live-secret` 只遮住前缀，`curl --user alice:s3cr3t`、`--password supersecret`、`aws_secret_access_key` 和 `visibleTranscript` 中的令牌原样进入审批输入 — 对命令标签、聊天证据、意图摘要和显式授权统一执行保守脱敏；不能确认安全的内容不要发送给审批模型，并增加负向测试。

### OAuth 与凭证存储适配

`lib/auth/xai-oauth.ts` 在发现地址和换令牌请求前都检查取消信号，外部信号与 30 秒超时合并后真实传给网络层；预先取消会立即抛出。`core/model-manager.ts` 在新版存储只有 `read/delete` 时，只有确认存在才删除，并继续跳过 OAuth 所有者键。

```text
xAI OAuth tests: 15/15 passed
AuthStorage tests: 29/29 passed
pre-aborted refresh: AbortError，网络调用 0 次
```

[P3] `scripts/patch-pi-sdk.cjs:79` — 依赖导入守卫覆盖三个包和子路径，但漏掉静态副作用导入，例如 `import "@earendil-works/pi-agent-core/subpath"` — 正则反向验证：三包 `from`、动态导入、`require` 均为 true，副作用导入为 false；当前仓库没有实际越界 — 扩充正则或使用语法解析，并增加正、负测试。

## 任务 2 — 辅助模型语义 Slot

无发现。已验证：未配置时按策略回退；已配置但模型不存在时抛 `model_not_found`，不会改用聊天模型；远程地址缺凭证时抛 `provider_missing_creds`；三种本机回环地址允许无密钥；视觉槽回退到不支持图片的聊天模型时返回空。

```text
auxiliary-slot-resolver + contract-closeout: 46/46 passed
localhost / 127.0.0.1 / 0.0.0.0 no key -> OK
https remote no key -> AUXILIARY_CONFIG_ERROR provider_missing_creds
configured missing model -> AUXILIARY_CONFIG_ERROR model_not_found
vision + text-only chat -> null
expect.any 命中 0；toBeNull 均对应合法无模型边界
```

## 任务 3 — Ollama 增强

[P2] `server/routes/providers.ts:351`（同时见 373–387） — 能力列表从错误的接口读取，自动工具调用和思考能力元数据实际无法补齐 — Ollama 官方 [List models](https://docs.ollama.com/api/tags) 响应没有 `capabilities`，官方 [Show model details](https://docs.ollama.com/api-reference/show-model-details) 响应才有；代码却从前者读能力、从后者只读上下文；单元测试直接伪造私有字段，未覆盖真实响应链 — 从每个详情响应读取并归一化 `capabilities`，增加符合官方结构的路由级测试。

[P2] `server/routes/providers.ts:364` — 对远端返回的全部模型同时发起详情请求，没有去重、数量限制或并发上限；超大或恶意模型目录可在一个请求中制造大量最长 15 秒的并发网络操作 — 源码为 `Promise.allSettled(modelIds.map(...))`；无切片、队列或并发器 — 先去重，再用固定小并发队列探测；单个失败保留基础模型并留下可观测诊断。

[P3] `core/provider-compat/ollama.ts:60` — `contextWindow` 只检查大于零，浮点和巨大有限值会原样成为 `num_ctx`，`Infinity` 序列化后变成 `null` — 实测 `8192.7 -> 8192.7`、`1e12 -> 1e12`、`Infinity -> JSON null`；0、负数和 `NaN` 不注入 — 只接受有限正整数并设置合理上限；非法值不覆盖调用方已有选项。

```text
Ollama focused tests: 103/103 passed
0 / -1 / NaN -> no num_ctx
8192 -> 8192
8192.7 -> 8192.7
Infinity -> JSON null
1000000000000 -> unchanged
```

## 任务 4 — 生成产物一致性

无发现。持久化指纹标为兼容，版本仍为 3、数据纪元仍为 1。命令行闭包用仓库生成器重建前后字节一致。导出清单实际新增 8 项，不是任务书所写 4 项；8 个目标全部存在。持久化守卫只把固定依赖版本从 0.83.0 改为 0.84.1，守卫逻辑未放宽。

```text
classification=compatible / CURRENT_SESSION_VERSION=3 / DATA_EPOCH=1
cli closure before sha256=c62a9e4ab5c04ada4d602ab6912c9c6092aff81fc5ab04fad49646203e4f0025
cli closure after  sha256=c62a9e4ab5c04ada4d602ab6912c9c6092aff81fc5ab04fad49646203e4f0025
cli runtime closure: 10604 files (source-graph=663, runtime-asset=11, nft-runtime-trace=9930)
tripwire/open-boundary/version tests: 36/36 passed
lint:boundary: passed with 1 known edge
```

## 任务 5 — 上游品牌不变量

五种语言的“瞬时简易压缩”标题和说明均非空且为对应语言，无英文占位。范围内生产代码没有新增 `hanaFetch`、`hanako` 或 `Hana`；全仓字面扫描有 1934 处历史命中，因此“全仓必须为零”与当前仓库事实不一致，见 `BLOCKED.md`。

[P3] `desktop/src/react/__tests__/components/InputArea.file-mention.test.tsx:116`（同时见 239） — 新测试仍模拟不存在的 `hanaFetch/hanaUrl`，并写入旧人格名；真实模块只导出 `lingxiFetch/lingxiUrl`，所以这个模拟没有隔离真实网络封装 — 对照真实导出与测试可确认名称不匹配；51 个相关测试仍通过，是因为当前路径另行模拟了文件搜索 — 改成真实导出名并断言模拟被调用，测试状态改为当前品牌值。

```text
tracked brand hits: 1934
range added brand hits: 12（均为测试/审计记录）
zh=瞬时简易压缩；en=Instant simple compaction；ja=瞬時簡易コンパクション
ko=즉시 간이 압축；zh-TW=瞬時簡易壓縮
InputArea + chat route tests: 51/51 passed
```

## 任务 6 — 静默降级扫描

对所有新增 `catch`、`return null`、`||`、`??` 做了差异扫描，并逐项复核 `core/engine.ts`、`core/llm-utils.ts`、`lib/bridge/`、`server/routes/`。辅助模型配置错误都有日志或重新抛出；三个增量回调的空捕获在变更前已存在，改动没有扩大其语义；合法“没有可用模型”返回空值符合既定回退契约。

确认的静默降级只有任务 3 的 Ollama 探测：`server/routes/providers.ts:358` 明确吞掉异常，详情请求的拒绝结果也被丢弃，没有日志、状态或返回提示。这不仅违反项目规则，还会掩盖能力来源错误。按任务 3 的 P2 一并修复，不重复计数。

## 六提交三态结论

| 提交 | 结论 | 审查内容 |
|---|---|---|
| `d555c14e` | 无发现 | 19 文件；设置凭证提交、错误展示、权限文案、布局；全量测试和类型检查覆盖 |
| `34dbb17d` | 确认 | 2 项 P2、1 项 P3：能力来源、并发风暴、上下文参数 |
| `0250f5fc` | 确认 | 1 项 P1：审批材料凭证外泄；终裁边界本身未被绕过 |
| `283d9581` | 无发现 | 配置与回退边界、凭证要求、视觉能力 |
| `a5d1e541` | 无发现 | Slot 收口和 327 行契约测试 |
| `97595264` | 确认 | 2 项 P3：导入守卫漏型、测试旧品牌模拟；产物与迁移正确 |

## 汇总

修复前共确认：P0 0 项、P1 1 项、P2 2 项、P3 3 项。最优先的是审批内容泄露；其次是 Ollama 能力识别失效与无界并发。未把范围外历史品牌词或预存截图失败计入本次缺陷。

## 修复状态

全部已实施，原始发现保留不改写。

| 发现 | 修复 | 验证 |
|---|---|---|
| P1 审批凭证泄露 | 命令标签、聊天证据、意图摘要、显式授权和普通目标标签统一走共享脱敏器；共享脱敏器新增命令参数和云工具凭证形式 | `approval-gateway` + 两套脱敏测试 39/39；六种假秘密均不在审批输入中 |
| P2 Ollama 能力来源 | 删除错误的标签接口读取，直接从详情响应取得能力；同时兼容官方扁平上下文字段 | 符合官方响应形状的路由测试验证 `context/reasoning/toolUse` |
| P2 Ollama 无界并发 | 唯一模型去重，最多探测 200 个，同时最多 6 个，整批共用 15 秒期限；失败写汇总诊断并保留基础模型 | 205 模型反例实测详情请求 200、最大并发 6 |
| P3 `num_ctx` | 仅接受 `1..1048576` 的有限正整数，不安全值不覆盖原选项 | 浮点、无穷、超上限三组反例通过 |
| P3 导入守卫漏型 | 增加静态副作用导入匹配，同时保持注释不误报 | 守卫单测 3/3；`node scripts/patch-pi-sdk.cjs` 通过 |
| P3 旧接口模拟 | 改为真实的 `lingxiFetch/lingxiUrl`，测试人格改为 `lingxi` | InputArea 测试通过 |
| 范围外可见品牌标签 | `Local Hana/Hana Studio` 的四处运行时默认值改为 `Local Lingxi/Lingxi Studio` | server identity + connection 33/33 |

修复触发的生成产物已同步：命令行闭包只新增两个真实引用来源；持久化指纹继续标为 `compatible`，`CURRENT_SESSION_VERSION=3`、`DATA_EPOCH=1` 均未变化。生成守卫 36/36 通过。

最终验证：

```text
clean worktree typecheck: 0 error
focused Ollama tests: 89/89 passed
generated artifact guards: 36/36 passed
full suite: 1 failed / 10985 passed / 7 skipped
only failure: 预存 screenshot SVG-vs-PNG（cce8e86 同样失败）
git diff --check: passed
lint:boundary: passed, 1 known edge
new skip/todo: none
```
