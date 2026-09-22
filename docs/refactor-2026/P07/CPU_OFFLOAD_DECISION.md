# CPU_OFFLOAD_DECISION — P07-T05 CPU 任务隔离决策

日期：2026-09-22｜结论：**无需新增卸载（NOT_APPLICABLE 有证据）+ 现有 worker 基础设施 UNCHANGED_VERIFIED**。

## 1. 决策依据（全部来自本阶段实测，非"感觉 Node 慢"）

| 测量面 | 结果 | 对主事件循环的含义 |
|---|---|---|
| 启动 CPU 剖析（--cpu-prof） | node:internal 模块编译 37.5%、wasm 11.9%、idle 10.2%、I/O syscall ~16%、应用代码 <3% | 启动期无业务 CPU 热点；模块编译是一次性成本且 bundle 形态已消大半（-18%） |
| W3 Layer 1（server 流路径） | 90k–141k ev/s；事件环 p99 <0.001ms（同步突发内无采样即无跨帧阻塞） | 单事件 ~7–11µs，远低于 16ms 帧预算 |
| W3 Layer 2（模型流解析） | 66k–100k deltas/s（真实 HTTP SSE） | 等待主要受网络/供应商节拍约束（A09：不承诺网络耗时降低，不造 worker 改动） |
| UI wall-clock（50k delta） | 突发 15.6ms、探针最大漂移 8.7ms | 渲染进程主线程单帧内完成 |
| W4 取消 | abort→收口 1.2–5.2ms | 取消路径无阻塞段 |
| 历史热页 | 0.7–0.9ms/页 | 无阻塞段 |

**没有任何一段 CPU 工作实测阻塞主事件循环超过一帧预算**。按任务书 T05.1：没有热点则验证后 N/A，不强造 worker 改动。

## 2. 现有 worker/子进程基础设施（保留复用，未改动）

| 设施 | 位置 | 有界性/回收 |
|---|---|---|
| server bootstrap keepalive worker | `dist-server/<plat>/bootstrap.js`（`server/bootstrap.ts` 源） | 独立线程 fs.writeSync 心跳，防 native 加载阻塞误判（#719/#736）；unref 定时器 |
| PTC 运行时隔离 worker | `lib/ptc/ptc-runtime.ts:284` | **每次执行独立 worker**（沙盒隔离语义，非池）：`resourceLimits.maxOldGenerationSizeMb` 内存上限、wall timer 超时、abort 监听、finish 即 `terminate()`（无孤儿）；env 空（`env: {}`，不继承宿主环境） |
| usearch 向量构建 worker | `lib/knowledge/usearch-vector-backend.ts:263` | 构建期 worker（BUILD_WORKER），完成即回收 |
| 工具子进程 | `lib/sandbox/exec-helper.ts` spawnAndStream | 进程组管理 + signal 取消（W4 S3 实测 5.2ms 收口、进程树整组退出、哨兵存活） |

以上均为**按任务所有者归属的一次性/受限 worker**，不存在共享可变 session 或凭证跨线程传递；本阶段零改动（UNCHANGED_VERIFIED）。

## 3. 若未来出现真实 CPU 热点（预案，不实施）

优先复用上述基础设施；有限池 + 队列上限 + 取消/超时/崩溃回收；跨线程消息保留最小安全身份（taskId/generation），不传类实例/闭包/AbortSignal/AsyncLocalStorage 引用；回传内容设大小上限或改资源引用；不可信第三方执行仍走 PTC 的 OS 级隔离（worker 不是沙盒）。仅使用 Node 24 锁定支持 API。

## 4. 安全不因线程简化

本阶段零改动即无权限面变化；A11（安全不能提速牺牲）由 bench-compare --selftest 假优化拒绝（INVALID）+ 全部基准的工作量硬断言佐证（见 P07_PERFORMANCE_COMPARISON §A11）。

## 5. 回退

零生产代码改动，无回退项。
