# RR3 F53（L包）：terminal_family_share_cases 满载时序 flake 加固

你是全新空历史修复实施者，未参与此前任何轮。全文读 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、最新 RR3_ISSUE_MATRIX.json（F53 行）/PROGRESS/HANDOFF、FINAL-03/STAGE_REVIEW.md §四失败项1（flake 证据链：同候选6跑5绿1红、红仅嵌套R03层满载~50分钟后、定向--exact重跑绿）。

## 根因面（你先亲核再修）

`rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs:103` 的 `terminal_family_share_cases` 中 case `terminal-snapshot-current-transcript`：`pinned expectation 1 did not hold (observed 0)`——PTY 第二标记"只交付新输出"的快照断言依赖 `send_and_expect` 有界轮询与真实 PTY 回显时序，高负载下偶发观察 0。这是测试时序确定性缺陷，不是产品确定性缺陷（FINAL-03 已定性）。

## 任务（唯一范围：该测试文件的时序加固）

1. 读通该测试与所依赖的测试基建（PTY helper 等），定位竞态机制（回显未达/轮询窗口/快照触发时机），写成简短根因说明。
2. 最小加固：保持断言语义不变（快照必须只含新输出、旧输出不得重现——不得放宽为"包含即可"）。允许的方式例：在触发快照前等待可观察的就绪屏障（如先等到第一标记回显完成再触发）、把有界轮询改为 deadline 驱动的确定性等待、消除对调度时序的隐式假设。禁止：加大常量 sleep 掩盖、跳过断言、把断言改为存在性弱化、忽略失败重试循环。
3. 自检：(a) 正常单跑绿；(b) 定向连跑 ≥20 次全绿（记录每次 exit）；(c) 负载下复跑（如与并行编译/压力进程共存）≥5 次全绿；(d) 隔离副本中做目标性变异红：人为破坏"只交付新输出"契约（如让快照回放全量输出）该断言必须红、还原绿——证明加固未削弱检测力；(e) 完整 r04_t08_tool_matrix 套件跑一次全绿；(f) fmt/clippy 绿（绝对 /Users/study_superior/.cargo/bin/cargo，--manifest-path rust/Cargo.toml）。
4. 红线：只改 rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs（及其测试内私有 helper，若 helper 在共享基建文件则先报告并等总控确认所有权）；不改生产源码、stage map、pins、其他测试；无 Git 写；不派子代理；长命令若超10分钟用 start_new_session 脱离宿主会话防误杀（FINAL-03 教训）。
5. 产物：artifacts/rust-tauri/R05/RR3/L-01/REPORT.md（根因、diff 摘要、全部自检命令/exit/UTC）、红绿对照记录。完成停写，交总控另派全新独立审查（L-REVIEW-01）。
