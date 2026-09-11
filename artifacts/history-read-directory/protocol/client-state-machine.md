# 客户端条件校验状态机（E03，与 protocol-contract.md 冻结合同一致）

- 实现：`desktop/src/react/stores/history-protocol-client.ts`（记录/键/条件包装器）
- 接入点：`desktop/src/react/stores/session-actions.ts`
  - `loadMessages`（重校验语义时；见下表 S1–S4）
  - `reconcileCurrentSessionMessages` 同 revision 分支（既有真实触发链：sessions_refresh/session_switch/chat-find-locate/mobile_foreground_refresh）
- 明确无条件（不经本模块）：loadMoreMessages（新页首次加载）、`fetchSessionHistoryPage`（reconciliation=1 严格对账）、强制恢复/流式修复。

## 请求键（E03.1）

```
requestKey = epoch | sessionId | sessionPath | mode("normal") | before(latest|n) | limit | lang | proj
epoch      = connectionId | authState | tokenFingerprint   // 凭证只存指纹，不存原文
```
任一维度变化 → 不同键 → 旧标签不可复用。认证变化/切换 server 通过 epoch 变化自然失效。

## 状态转换

| 当前状态 | 事件 | 动作 | 次态 |
|---|---|---|---|
| S0 无记录 | 重校验触发 | 无条件 GET（错误语义=lingxiFetch 默认） | S1 |
| S1 已加载（有记录） | 重校验触发 | 预检（记录存在+epoch 未禁用+目标未切换+非流式+版本一致）→ 发 If-None-Match（cache:'no-store'，throwOnHttpError:false） | S2 |
| S1 | 无记录/预检失败 | 无条件 GET | S1（重新建立记录） |
| S2 条件在途 | 304 | **新鲜状态重查**（非发请求前快照）→ G 判定 | S3(有效)/S4(stale)/S5(superseded) |
| S2 | 200 | 正常完整归并/结局裁决链（既有 loadMessages 应用），完成后原子保存新记录 | S1 |
| S2 | 401/403/4xx/5xx | 抛错走既有错误流程；epoch **不**标记 | S1 |
| S2 | 网络错误 | 同目标一次无条件回退；epoch 不标记（一般网络错误≠不支持） | S1 |
| S2 | 400（条件头被拒） | 同目标一次无条件回退 + 标记该 epoch 暂不条件 | S1（epoch 禁用） |
| S3 304 有效 | — | 不解析 JSON、不 initSession、不 prepend、不推 nextBefore/hasMore；保留 messages/blocks/todos/sessionFiles/Run 状态与已加载更早页；只更新校验时间 | S1 |
| S4 304 stale | — | 至多一次无条件补取（无 If-None-Match，禁止 304→补取→304 循环）；补取 200 → 完整应用并保存新记录；补取失败 → 既有错误流程 | S1 |
| S5 superseded | — | 直接退出：目标已切换/已取消/会话淘汰/更新 load 在途——不向新目标补发旧请求 | S0/S1 |

### S2→G 新鲜状态重查清单（用响应到达后的最新 store 状态）

| 检查 | 失败归类 |
|---|---|
| chatSessions 中该会话仍存在（未淘汰/清空） | superseded |
| pendingNewSession / pendingSessionSwitchPath | superseded |
| currentSessionPath 仍是本会话 | superseded |
| streamingSessions 不含本会话 | superseded |
| `_loadMessagesVersion[path]` 仍等于发起时请求版本 | superseded |
| messageLiveVersion 仍等于记录 appliedLiveVersion | stale（WS 消息/块更新） |
| todosLiveVersion 仍等于记录 appliedTodosVersion | stale（todo 状态更新） |
| 缓存 revision / nextBefore / hasMore 与记录覆盖一致 | stale（表示漂移） |

## 失效清单（E03.4）与落点

| 失效源 | 落点 | 机制 |
|---|---|---|
| WS 消息更新 | `bumpMessageLiveVersion`（chat-slice 各 live 应用点） | 记录 appliedLiveVersion 失配 → 304 判 stale → 补取 |
| WS 块更新/interlude | chat-slice invalidateSessionCache（:484/:517） | 记录立即丢弃 |
| 文件 registry 更新（upsert/整表） | chat-slice setSessionRegistryFiles/upsert（:614/:634/:677）→ invalidateSessionCache | 记录立即丢弃 |
| todo 状态更新 | todosLiveVersionBySession | appliedTodosVersion 失配 → stale |
| 会话淘汰（LRU >8） | chat-slice:160 invalidateSessionCache | 记录立即丢弃 |
| 登出/切换 workspace | invalidateSessionCache()（无参） | 全部记录丢弃 |
| retry / fork 后切换 / archive/restore | 服务器文件变更 → revision/表示变化 → 标签失配 200；客户端重试动作经 invalidateSessionCache | 标签失配 + 记录丢弃 |
| 认证变化 / 切换 server | epoch 变化（connectionId/authState/token 指纹） | 旧记录键不可达，自然淘汰 |
| 语言/投影规则变化 | 请求键 proj/lang 维度 | 键变化 → 自然失效 |
| 流式进行中 | reconcile/预检守卫 | 保守不发 ETag（superseded/跳过） |

## 限额（E03.2）

- 每会话 ≤32 条校验记录（超限淘汰最旧记录，不淘汰消息）
- 全局校验元数据估算 ≤256KiB（key/etag/epoch/覆盖字段字节合计）
- 不持久化、不新增 raw-page 缓存；记录只存 requestKey/etag/epoch/版本/原始覆盖边界/能力与推荐值

## 回退（E03.5）

- 响应无 `Lingxi-History-Protocol` 头或非 "1" → 不保存记录（清除不可信能力），按普通 200 处理
- 条件请求 400（头被代理拒绝）→ 一次无条件回退 + 该 epoch 标记暂不条件
- 一般网络错误 → 一次无条件回退，不标记；401/403 → 既有错误流程，不标记
- 新服务端正常路径证据：`tests/history-protocol-conditional.test.ts`（服务端 304 真实产生）+ 本文件客户端测试（304 被正确消费）
