# RESOURCE_HISTORY_CONTRACT — 历史资源身份与安全预览（P05-T05）

日期：2026-09-22｜基线 HEAD：`1f0537b08`。生产实现 **UNCHANGED_VERIFIED**（1 处新增测试），
PDF/媒体生成链未触碰（任务书 T05-5：仅登记引用与展示接线，现状即正确）。

## 1. 三类内容引用的身份模型（T05-1）

| 类别 | 身份 | 不可变内容来源 | 读取入口 |
|---|---|---|---|
| 工具结果正文/补丁/搜索结构/技能正文 | HistoryDeferredContent locator（v1=sourceIndex+entryId+kind+ordinal；v2 实时=sessionPath+toolCallId） | **会话 JSONL 持久记录**（"解析读的是已经保存下来的那一条记录，不重跑工具、不读当前磁盘文件"，history-deferred-content.ts:16-19） | GET /api/sessions/content/:id |
| 工作区当前文件（交付引用） | 稳定 fileId `sf_*` → 资源 `res_sf_*`（resource-envelope.ts）；块级版本证据 `{mtimeMs,size[,sha256]}` | 快照三通道：FileHistoryService（watcher/event/sweep 逐版本）、workspace-snapshots（每轮快照 + write/edit 改前备份兜底）、会话注册表 sidecar（.jsonl.files.json） | GET /api/resources/:id[/content]；版本浏览 /api/file-history/* |
| 媒体生成/PDF/截图/artifact | taskId / artifactId / fileId + 替换占位（replacesTaskId） | 生成产物入会话注册表；大图/大 artifact 走 deferred（screenshot/artifact locator） | 同上 + blocks 渠道 |

**实时交付与历史重建共用同一份文件证据**（session-file-block.ts 开头注释 = 代码事实）：
同一 sessionFileToContentBlock 投影，块身份（fileId）与版本证据在实时与历史两侧一致。

## 2. 预览 vs 全文（A10：预览不是全文唯一副本）

- 首包阈值 `HISTORY_INLINE_CONTENT_LIMIT=8KiB`；超限只带 `preview`（240 字符）+
  deferred 引用（体积恒定，不携带正文）。
- 全文永存于持久记录，展开时按 locator 解析 → **预览丢失不影响全文可得**；
  测试：sessions-route "defers large command output and legacy screenshot/artifact payloads"
  （三种 kind 首包不含尾部标记、逐个展开恢复全文）、tool-presentation-history
  "大输入、补丁和写入内容首包仅预览，详情按保存记录完整恢复"、"大搜索只传计数预览，
  展开恢复所有文本块而不包含图片数据"。
- 展开不重新执行工具：locator 只定位既有记录（v1 需 entryId 校验通过；v2 按 toolCallId
  找已保存 toolResult），无任何重执行路径。

## 3. 每请求重新授权（T05-2，A12）

- 会话内容展开：`/api/sessions/content/:id` 每次请求经 `authorizeSessionRoute(
  requestContext, "sessions.read", …)` + 路径合法性校验；locator 明确**不是授权凭证**
  （引用写的会话 ≠ 本次授权会话 → 解析不出内容，sessions.ts:1257-1290）。
- 资源元数据/内容：ResourceAccessService 分别对 `resources.read` / `resources.content|read`
  逐请求裁决并审计（resource-access-service.ts:18-52）；内容下载用短时单资源 ticket
  （resource-ticket-service），过期/错资源显式报错。
- 远程主体路径脱敏：removePathFields **递归数组与嵌套对象**（逐块），媒体事件逐事件
  分类（classifyMediaEventPayload）——不存在"只看第一块"的判断点；
  测试：resource-access-service（deny/metadata 兼容/remote base64 拦截）、
  http-route-security（scoped device 只开 resources 读）。
- 合成 secret：tool_input 展开时 apiKey 以 `********` 遮盖后返回
  （tool-presentation-history 用 credential-sentinel 合成数据锁定）。

## 4. 文件状态语义（T05-4：不可访问必须显式）

| 状态 | 行为 | 证据 |
|---|---|---|
| 同名文件被修改（A11） | 历史工具交付读持久记录（本阶段新增真实磁盘对照测试：当前内容标记不进首包、展开不读新文件、删除后仍完整可读）；资源信封按当前文件刷新 mtimeMs/etag（如实反映"这是当前位置"，块上版本证据保留，不冒充历史版本） | tool-presentation-history `A11：历史交付不受工作区同名文件当前内容影响` |
| 文件已删除/移动 | registry 状态 `missing` + `missingAt`（UI 渲染禁用卡片）；内容路由 404 `resource_content_missing`（不假装空文件） | session-file-expired.test.tsx、resource-service resolveContent |
| 过期 | 410 `resource_expired`；信封不产出 content 链接 | resource-envelope/ResourceService:60-65 |
| 旧别名 | 侧车 legacyFileIds 经会话归属优先 + alias 兜底解析（_findSessionRefForFileId） | resource-service.ts:171-217 |
| 越权 | 403 `resource_forbidden` / `resource_content_forbidden`，审计记录 | resource-access-service |

## 5. 本阶段执行命令

1. `npx vitest run tests/tool-presentation-history.test.ts`（含新增 A11 例）→ 全绿
   （P05-T05-a11-tool-history）。
2. `npx vitest run tests/resource-access-service.test.ts tests/resource-envelope.test.ts
   tests/resource-ticket-service.test.ts tests/http-route-security.test.ts
   tests/file-history-route.test.ts tests/file-history-store.test.ts
   tests/file-history-service.test.ts tests/resource-io-route.test.ts
   tests/resource-io-session-file-resolver.test.ts` → 全绿（P05-T05-resource-suite）。

## 6. 结论

资源历史身份、快照登记、逐块脱敏与显式不可用语义在当前实现已正确且测试充分；
本阶段增量 = A11 端到端真实文件对照测试（此前"持久记录不读当前文件"只有设计注释与
间接用例，现在有直接证明）+ 本契约文档。无生产改动。
