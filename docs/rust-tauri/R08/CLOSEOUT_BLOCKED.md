# R08 收尾阻塞与未决（CLOSEOUT_BLOCKED.md）

## 环境边界（非阻塞、如实披露）

| 编号 | 类别 | 事实、影响与解除条件 |
|---|---|---|
| C-HOST | 环境边界 | 本机 Homebrew cargo 1.93.0 不读 rust-toolchain.toml；全部 cargo 调用经 rustup（PATH 前置 ~/.cargo/bin，锁 1.98.1）。不构成阻塞。 |
| C-LEGACY-STACK | 架构事实 | 叶验收=旧 Node 栈（lib/）与 Rust 栈双内核对拍；旧栈只在本机夹具根上运行，不触真实 ~/.lingxi（A15 V1–V5 硬校验）。未提交工作树是本轮成果唯一副本，提交/推送待业主另行指令。 |
| C-CONCURRENCY | 执行环境 | A05 宿主演练的进程树计数断言要求**独占运行**：并发跑其他 lingxi-service/Electron 演习会使其 `rust-kernel count` 失真（2026-10-26 19:20 实测预检 4!==1；同一环境串行执行即通过）。正式门执行期间必须无其他候选写入与演练进程。 |
| C-LEGACY-RETENTION | 数据语义 | 旧栈工具结果有内核侧输出保留窗口（大输出按引用取回的正文不等于完整字节流），Rust 溢写完整输出。延迟内容叶按「取回真实大正文、非 240 字预览」断言，两内核各自真实，不要求同字节集。 |
| C-SLASH-TAKEOVER | 证据边界（独立审查 F1，非阻塞） | slash 族「接管态/无历史」子句：/new 485B2D0A8A02 接管态与无历史分支无直接测试；/reset、/fresh-compact 接管态由既有 rust r06_slash_completion 单测覆盖但不在 R08 门内命令；/stop 仅非接管态 other-session 案例。R09 补 bridge-takeover A/B 场景（记录在 CLOSEOUT_PROGRESS/HANDOFF）。 |
| C-UI-SUBCLAUSE | 证据边界 | UI 5 叶按「每条原断言 ≥1 专属 case」绑定；断言内个别子句（如 MCP 批量 HTTP200 混合部分成功、Web 端 window.open 路径、ConfirmStore 不可用活体开关）无活体构造面或属既有内核单测，已在 `artifacts/rust-tauri/R08/closeout-proofs-02/ui-pages/R08_UI_COVERAGE_MATRIX.md` 与进度账逐条披露，不冒充已测。 |

## 历史阻塞（已解除，留档）

| 编号 | 影响 | 解除事实 |
|---|---|---|
| ENV-A05-FOCUS（2026-10-26 回退） | closeout-03/04 门 a05 row1-hide 红（System Events 焦点回退）。 | closeout-05 自愈通过；closeout-06 与第六轮复核（本会话）均绿，未做宿主权限修改。 |
| C-SUBAGENT（旧会话） | 早期并行子代理因 model 并发限制失败。 | 本轮改用共享任务板 + 写入范围互斥的并行子代理，终端/交互/UI 三域均交付，未再出现并发失败。 |

## 需产品裁决（已全部裁定）

五项裁定见 **CLOSEOUT_ADJUDICATIONS.md**（ADJ-1 改说明书 / ADJ-2 接受等效 / ADJ-3 保留严格 / ADJ-4、ADJ-5 以新版为准关闭），已全部执行落实。R08 收尾第六轮未新增需裁决项：
- D-16 已翻案（线束错 id，非产品缺口）并撤 recorded；
- D13 终端已按「真实 PTY + 真实 WS 三帧正路径」实现并双内核取证，不存在「显式不可用=完成」的降级。

## 真实阻塞

无。
