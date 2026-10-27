# runtime ACL batch attempt 3 — ABORTED（harness wdio 端口参数 bug；10 分钟超时）

- 根因（已定位）：harness main() 只读 `TAURI_WEBDRIVER_PORT` env，**忽略 `--wdio-port 19481`**，
  默认 fallback 19465 → wdio 服务实际监听 19465（app env 实证），随后 wdio session/handles 路径
  在 19465 上不返回（renderer 已就绪、service 正常响应），harness 卡在 wdio 调用直到工具侧
  600s 超时转后台；未有可见窗口（hidden-only + risky commands 尚未执行到矩阵阶段；仅主窗 hidden）。
- 已处置：TERM app（graceful service stop）→ 断言并清理 harness/service；ps=0、端口释放。
- 修复（v2 重跑前）：`--wdio-port` 真正生效（CLI > env > 19465），并对 wdio session/handles
  增加短超时+重试上限，避免再次长时间挂起；批次需 Lead 重新 cleared。
