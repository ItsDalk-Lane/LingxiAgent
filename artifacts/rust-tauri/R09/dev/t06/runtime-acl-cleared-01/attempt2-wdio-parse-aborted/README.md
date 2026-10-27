# runtime ACL batch attempt 2 — ABORTED（wdio 响应解析 bug）

- hidden app 正常启动（isolated home 空、service READY 由后续流程证明），app 在 finally 中
  SIGTERM 清理：exitCode 0、residual 0；ps=0、wdio 端口已释放；sink 未生成失败记录（见同目录）。
- harness 在 `GET /session/{id}/window/handles` 解析处崩溃：W3C 响应 `{"value":[...]}` 的 value
  是数组，旧代码按 dict 访问 → AttributeError。
- 处置：请求层改为“value 为 dict 且含 error 才判错”；补异常路径写回执；**唯一一次最终 hidden 重试**。
