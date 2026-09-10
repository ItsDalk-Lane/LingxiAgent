# 协议真实链路 Smoke 报告（E08）

- 生成：2026-09-10（批次 E-b4/E08）
- 脚本：`scripts/smoke-history-protocol.mjs`（真实 HTTP loopback：@hono/node-server 挂 D01 同一 Hono 路由 + server/index.ts 同一 CORS 头契约函数 applyCorsResponseHeaders）
- 运行：`node scripts/smoke-history-protocol.mjs --output artifacts/history-read-directory/protocol` → **exit 0，0 失败**（smoke-results.json 逐条断言）

## 环境限制（如实标注）

- 本 smoke 验证 **Node HTTP loopback 层**：真实 Hono 路由 + 真实生产 CORS 头契约函数 + 真实协议头/304/私有缓存头。
- **真实浏览器/Electron 环境未验证**（凭据自动附带、预检缓存、渲染进程头可见性、HTTP cache 交互）——留待 F 阶段指定环境；本报告不把 Node loopback 结果冒充浏览器网络结果。

## 断言结果（1k 与 10k 各执行一轮，全部 PASS）

| 断言 | 结果 |
|---|---|
| 消息 200 + `Lingxi-History-Protocol: 1` + `Lingxi-History-Page-Limit: 100` | PASS |
| `Cache-Control: private, no-store`（真实响应头） | PASS |
| `ETag: W/"hrp1-<64hex>"` | PASS |
| CORS Allow-Origin 回显白名单来源 | PASS |
| Expose-Headers 含 ETag/Lingxi-History-Protocol/Lingxi-History-Page-Limit | PASS |
| 条件请求 If-None-Match → 304，无正文、无 Content-Length、ETag/私有策略在 | PASS |
| 概览 200 available:true + recommendedLimit 与消息头同源（=100）+ 私有缓存头 | PASS |
| OPTIONS 预检 Allow-Headers 含 If-None-Match；Allow-Credentials=true 且非通配 | PASS |

## 与 E07 对照

- 私有头（private, no-store）经真实 HTTP 链路核实 ✓；未设置 public/s-maxage；无 Service Worker 预缓存。
- CORS：来源白名单（loopback/file:// 配置白名单）回显、非通配 + credentials；Allow-Headers 增补 If-None-Match（不影响 Authorization）；Expose-Headers 暴露协议三头；预检往返已计入性能证据（D07/E06 各格含跨源同构请求路径，未见新增必需预检——同源 Electron 渲染不触发；跨源 Web 场景每会话首次预检一次，实测 <1ms loopback）。
- 未知 schemaVersion / 结构不兼容 / 404/405 能力缺席 / 401/403 分类 / 服务器重启失配：见 history-protocol-client / history-overview-client / history-protocol-conditional 测试（E07 故障场景清单）。
