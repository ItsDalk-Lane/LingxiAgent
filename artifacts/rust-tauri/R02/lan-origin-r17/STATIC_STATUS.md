# R02 局域网浏览器来源：静态修复与未执行端到端验收

- 原范围内旧缺陷：真服务只接受 loopback/file/null 来源，手机在服务自己公布的局域网地址访问时，登录、会话、退出、WS 握手会先得到 403。原管理生产者只发 loopback Host 且没有 LAN Origin，导致网页登录退出叶可能假绿；归属为 R00 原始 34 叶中的网页登录/会话/退出/WS 与 LAN 地址入口。
- 本次改动：来源只在网络模式为 LAN 且请求协议、实际端口、Host IP 与正在运行的本机网卡一致时放行；管理员保存的公开域名必须是单一 http(s) Origin，且请求 Origin、Host、真实 TLS 状态一致。拒绝异源、错误协议/端口/路径，不给拒绝请求附带凭据 CORS 响应。允许来源的预检和业务响应都返回精确 CORS 来源。
- 同类入口：`transport_guard` 对所有路由统一判定；`browser_cors_response` 使用同一个判定；`management` 验证并读取保存的公开域名；管理生产者新增真实 LAN 网卡登录、会话、退出与异源拒绝案例，并把它钉进退出叶门禁；TLS Web 测试新增保存公开域名后的同源正例和异源负例。
- 静态/无端口结果：来源策略 6/6 单测 PASS（`unit-strict-origin.log`）；公开域名格式 1/1 PASS（`public-url-validation.log`）；管理及 TLS 真连接测试仅 `--no-run` 编译 PASS（`management-no-run.log`、`tls-web-no-run.log`）；JSON/Python 解析、`git diff --check` PASS。这些只证明代码可编译及纯策略，不能证明真实网页链已通过。
- 真实状态：**BLOCKED，未执行**。当前候选的管理生产者 60 案例和 TLS Web 真实连接案例都需要本地端口；先前平台返回 `Operation not permitted`。不得把旧 58/58 或 59 案例、旧 TLS 测试结果移给当前候选。
- 剩余风险：真实局域网网卡可能不存在、发生动态变更，公开域名/TLS 部署也需在目标平台实测；若网卡枚举失败，代码明确拒绝 LAN Origin。移动端完整 UI 与其他 34 叶的剩余行为另列总表。
