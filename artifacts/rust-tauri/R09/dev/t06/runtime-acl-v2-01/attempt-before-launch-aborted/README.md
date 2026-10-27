# v2 runtime ACL hidden 批次 — ABORTED BEFORE APP LAUNCH（未消耗 hidden 启动）

- cleared 批次：v2 G1 evidence exe sha `f69a8100b5bd35aa0d91cac33ff7cce0624bdd5f841aed8bca868dee388c48cb`
  （features e2e-test,r09-host,r09-test-hooks），`LINGXI_R09_T06_RUNTIME_ACL_CLEARED=1` + hidden-only + automation。
- 失败点：harness `main()` 把 `--wdio-port` 的 Path 直接 `int()` → `TypeError: int() argument ... not 'PosixPath'`；
  错误发生在 `run_runtime()`/`AppProcess` 之前，**没有任何 app/window/service 启动**（ps=0、端口未监听）。
- 处置：按 Lead “harness 再遇缺陷失败即停并报告（不重试）”规则，本批次停止，不再启动 app。
- 修复（离线，未启动 GUI）：`--wdio-port` 解析改为 `int(str(arg_value(...)))`；无 GUI 自检
  `dev/t06/runtime-acl-selfcheck-04/` 验证 exit 2 `RUNTIME_ACL_NOT_CLEARED`（22 cases，不 spawn）。
- 重跑条件：Lead 重新 cleared 后一次执行（对象/参数同上；hidden-only 策略不变）。
