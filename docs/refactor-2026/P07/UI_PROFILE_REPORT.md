# UI_PROFILE_REPORT — P07-T04 流式 UI 与大结果展示

日期：2026-09-22｜结论：**UNCHANGED_VERIFIED（渲染链零改动）+ wall-clock 画像测试新增（2 例全绿）**。

## 1. 每 delta 的状态/组件更新定位（沿现有 React profiler 面）

渲染侧流式缓冲单一权威：`desktop/src/react/hooks/use-stream-buffer.ts`（StreamBufferManager 单例，app-ws-shim 直接 `handle(msg)`，不依赖组件生命周期）：

| 机制 | 实现 | 计量证据 |
|---|---|---|
| 普通增量画面合并 | `scheduleFlush`：`STREAM_FLUSH_FPS=30`（FLUSH_INTERVAL≈33ms）上限 + `requestFlushFrame`；缓冲纯 JS 对象写入，不触发 React | wall-clock 测试：50,000 canonical delta 同步突发 → **stream_flush=2**（首帧+末帧）、`structural_message_update=1` |
| 语义边界立即发布 | `publishBoundary` 强制发布并把未发布增量并入同一次更新 | A06 形状测试：合并窗口内 content_block（文件）→ 边界发布，文件后正文继续累积（TAIL_AFTER_FILE 保留） |
| 流式不预解析 Markdown | 正文存 `source`，渲染时按需解析 | 50k delta 期间 **markdown_parse=0**（既有 fake-timer 基线 10k/50k/100k 三档同断言） |
| canonical 单一真相 | `assistant_segment_*` 锁定后 legacy text/thinking 不再产生第二 block；resume 增量按 seq 幂等去重 | 既有 use-stream-buffer-dedupe / steer-split 测试族 |
| live-turn 投影 | `publishLiveAssistantMessage` 段级投影（segmentsById/segmentOrder），非整树重挂 | 代码路径核对（P05 锁定面，零触碰） |

## 2. wall-clock 画像（真实计时器，本阶段新增）

测试：`desktop/src/react/__tests__/chat-performance/p07-stream-buffer-wallclock.test.ts`（命令 `P07-T04-ui-wallclock`，exit 0）。

| 指标 | 实测（50,000 delta） |
|---|---|
| 同步突发缓冲墙钟 | **15.6ms** |
| handle 吞吐 | **3,196,837 handle/s** |
| 发布次数（stream_flush） | 2（30fps 合并生效） |
| 主线程响应度（10ms 探针最大漂移） | **8.7ms**（<1 帧） |
| markdown_parse | 0 |

结论：**正在输入的用户不受后台输出拖慢**——单帧预算内完成整轮突发缓冲；服务器侧 W3 Layer 1 吞吐 90k–141k ev/s 亦远超模型 delta 实际到达速率（真实流受网络节拍约束，W3 Layer 2 实测本地替身满速 66k–100k deltas/s）。

## 3. 大结果与终态契约（不回归清单）

- **末帧不丢**：50k delta 全量进入最终 text source（断言长度相等）；finishRun/assistant_run_end 收口前 flush 全部挂起增量。
- **错误/取消/文件到达**：`content_block` 走 publishBoundary 立即发布（A06 测试）；`assistant_run_end` exactly-once 终结（runKey 不变量，既有 use-stream-buffer 测试族）。
- **选中/输入法/滚动锚定/Markdown/代码块/公式**：本阶段零渲染改动，契约由既有 UI 回归族覆盖（desktop/src/react/__tests__/ 全量 npm test 内）。
- **preview/full 与资源引用**：大结果按现有 content_block/deferred_result 投影，折叠展开状态一致性由 chat-slice / conversation 系列测试覆盖；本阶段未改任何相关文件。
- 不做全量虚拟化、不换 Markdown renderer、不按任意字符切割：零改动即零回归（任务书 §3 明确不做项，全部保持）。

## 4. 改动与回退

生产代码零改动。新增 1 个测试文件（独立可回退：删除即恢复原状，不影响生产路径）。命令：`P07-T04-ui-wallclock`（首次含断言校准迭代，最终绿；日志见 command-log）。
