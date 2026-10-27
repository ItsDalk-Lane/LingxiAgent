# runtime ACL batch attempt 1 — ABORTED（非结论性证据）

- 现象：hidden app 启动后 Rust service 退出 2（`unstamped-home-with-data`：harness 把 viewer fixture 误放进了
  隔离 LINGXI_HOME，被数据安全闸拒绝）→ 宿主 automation 路径写 sink（`modalShown:false`，无弹窗）并 exit(2)。
- 随后 app 崩溃：直接运行未打包 debug 可执行文件时 `UNUserNotificationCenter currentNotificationCenter`
  抛 `bundleProxyForCurrentProcess is nil`（macOS 需要 .app bundle）——见 native-app.log.tail。
- harness bug：teardown 的兜底清理匹配到 harness 自身命令行（相对 `--app` 路径），SIGTERM 自杀，未写 cases/result。
- 处置：ps 断言 0；保留 sink/failure/log 片段；修复 harness（fixture 移出 home、最小 debug .app 包装、
  清理排除自身 pid、隐藏证据解析）后**唯一一次重试**（hidden）。
