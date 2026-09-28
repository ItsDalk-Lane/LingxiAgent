# R17 CLI / Rust 服务接线子组（截至 2026-09-28）

状态：**局部修复已做，R02 补充叶未整体 PASS**。本文件只记录本子组事实；R00 原始验收文件未改。

## 原合同与所有入口

| 原 R00 叶 | 原行为 / 负面要求 | 本子组入口与边界 | 当前结论 |
|---|---|---|---|
| LA-B8A1AD32A8E1 | help 退出 0；未知参数错误、帮助、退出 1、不启服务 | `cli/args.ts`、`cli/entry.ts`；纯客户端生产者由 34 叶子组负责 | 本子组静态接线；客户端生产者须按最终摘要独立重跑 |
| LA-1B09760C2B1C | serve 前台、channel、数据降级约束；启动和版本失败明确诊断、不切数据根 | Node 原入口保留；显式 `--runtime rust` 走 `lingxi-service` 二进制、数据根、renderer 指针、旧 Node 同宅预判、子进程退出码和信号 | Rust 真服务 CLI 起停尚未在最终 Rust 源运行；Rust 没有显式数据降级协议，CLI 明确拒绝该选项；A01 服务侧证据另计，原客户端全叶未闭合 |
| LA-200D4E5D52C9 | 认证列表 ≤20、空态 No sessions yet；断线/未授权报错，不读别的主体 | 显式 Rust：本机 `instance.json` + 同实例 `local-token.json`、`/lingxi/v1/sessions`；Node 原入口 | 定向单元/模拟响应通过；真实 Rust 线级 CLI 尚未执行 |
| LA-EEC1A2A5CD04 | status 六字段；不可达诊断；身份失败只用健康信息、不伪造认证来源 | Rust `/health` 和 `/me`，Node 原入口 | 不可达/身份失败分支已定向验证；Rust 没有 Agent、Model、Studio 的真实有效来源，原叶未闭合 |
| LA-D3710D637C19 | 选/建会话，WS 消息、真实回复与工具流；断线/身份失败非零、错会话帧不显示、Ctrl+C 仅取消当前流 | Node 原 WS 入口修断线、错会话、早按 Ctrl+C；Rust 显式选路查真实已有会话后因能力缺失返回 1 | Rust 缺会话创建、模型执行、回复/工具事件与按 stream 取消；原完整叶未闭合 |

## 根因与改动

1. 旧 CLI 只接 Node `/api/*` 和 `/ws`。新增明确 `--runtime node|rust`，默认保留 Node，防止 R02-A16 旧入口被未完成 Rust 聊天替换。Rust 本机连接只接受同一实例 ID、原数据根、loopback 地址和本用户私有的 token 文件；读取会话前还要由 `/me` 真正确认 `local_user` + `loopback_token`，不能把其他主体列表当本机所有者。HTTP 用 Bearer，拒绝跨源重定向及超过 8 MiB 的响应。`instance.json` 只是定位，不作为存活证明，后续 HTTP 真实检查决定结果。
2. 参数解析原先允许一些已识别却毫无作用的选项。现在按命令拒绝无效选项及重复选项，`--token` 必须伴随 `--url`；`--runtime rust serve` 在二进制缺失、数据根缺失、不支持数据降级、beta 无已激活 frontend、旧 Node 所有权记录不可判明时明确失败。Rust 子进程退出码沿前台 CLI 返回。
3. Node status 原先在 `/identity` 失败后把本地连接来源误显示为认证身份；现显示身份不可用。Node 聊天原先意外 WS close 返回 0、无身份流帧可被显示；现分别非零退出、丢弃无归属流帧。后续同根因自查发现本地只知道 `sessionPath`、来帧却只带另一个 `sessionId` 时，旧匹配仍会放行；现任何会话流帧都必须至少有一个与本地已知身份完全一致的标识，带身份的错误帧也不能借缺失的另一标识通过。Ctrl+C 在活动 stream ID 尚未返回时排队，ID 一到仅发送绑定该 stream 的 abort；重复请求不跨流重发。
4. Rust 当前只有认证 `GET /sessions/{id}` 和合成 execute 记录，没有会话创建、模型回复、工具进度与取消协议；`chat/continue` 严格返回 1，绝不把 `/execute` 的 run accepted 伪装为聊天完成。此为原合同范围内、R02 提前纳入 R07 客户端后尚未补齐的产品缺口，不是测试失败的替代描述。
5. Rust `/health` 仅提供服务版本、协议和 epoch；`/me` 本地主体的 `studioId` 为 null，服务没有当前 Agent/Model 配置来源。Rust status 显示缺失项为 unavailable，不能因命令退出 0 就把原六字段叶判 PASS。最小完整依赖是服务端持久的当前 Studio/Agent/Model 权威配置与经认证状态读取；聊天还需真实会话创建、模型与工具运行、带会话/stream 身份的 WS 事件及取消闭环。这些能力与 R03 运行状态机和后续模型/工具实施相连，不能生成假事件解锁 R02。
6. 凭证边界复核：显式 Rust token 进入 HTTP header 前拒绝控制字符和超长值；服务错误若回显当前 token 则遮盖，终端所显远端字段去控制字符并限长，防止错误响应或会话标题操纵终端。这是本轮 R17 新 Rust CLI 适配代码引入的输入/显示风险，经后续同根因自查修正；未改变 Rust 服务端认证权威。正常和恶意输入的定向检查在下表。

## 定向验证及原始记录

拟提交给 34 叶门禁的真服务案例由 `r02_cli_rust_matrix.py` 产生；已执行两轮但**均失败，后续端口复跑受平台限制**。案例按原叶分组：serve 为 `cli-rust-serve-ready/sigterm-clean/startup-error/beta-no-silent-stable/downgrade-refused/newer-epoch-no-root-switch`，sessions 为 `cli-rust-sessions-owner/limit20/empty/unauthorized/disconnected`，status 为 `cli-rust-status-real-health-identity/no-fake-auth`，continue 为 `cli-rust-continue-missing-no-create/existing-no-fake-reply`，chat 为 `cli-rust-chat-no-fake-run`。另有 `cli-rust-current-binary-build`、`cli-rust-candidate-unchanged-during-probe` 两项证据新鲜度检查。现有正常路径只覆盖服务/认证/列表，不能把 continue 的显式拒绝或 chat 的无假 run 当作原正向回复/工具流通过；status 的 Agent/Model/Studio 实源也仍缺。

| 检查 | 结果 | 原始证据 |
|---|---|---|
| CLI Rust/参数/聊天 helper/旧 runner 定向测试，早期候选 | PASS，49/49（历史观察） | `run-08/command.txt`、`start-utc.txt`、`end-utc.txt`、`exit-code.txt`、`stdout.log`、`stderr.log`、`candidate-sha256.txt` |
| Node 和 test TypeScript 检查 | PASS，双 exit 0 | `typecheck-06/` 内命令、时间、退出、原日志、摘要；`typecheck-05/` 测试 mock 类型错误的首次失败亦保留 |
| 真实 Node WS 子进程：错会话帧隐藏，正确帧显示，意外断线 exit 1 | PASS，1/1；获平台批准的本机端口 | `run-05/` 原始命令、时间、退出、日志、摘要；`cli/chat.ts` 在其后未变 |
| 首次 WS 测试默认沙箱 | FAIL（环境：listen EPERM），未产生产品结论；首失败保留 | `run-04/` 原始记录。之后修测试遇 listen 错误的超时处理，并按许可重跑，不覆盖 |
| 首次 Rust adapter 测试 | FAIL（测试合成 home 的 `/var` 与 canonical `/private/var` 不同），未产生产品缺陷结论；原始记录保留 | `run-01/`；修测试 fixture 后 `run-02/` PASS |
| 首次 TypeScript 检查 | FAIL（判别联合类型缩窄错误）；原始记录保留 | `typecheck-01/`；源修后 `typecheck-02/`、最终 `typecheck-04/` PASS |
| CLI 对真实 Rust 服务的 E2E：serve/status/sessions/continue、启动/epoch 错误 | FAIL（probe-1 环境拒绝监听；probe-2 启动后 11/18 PASS、7/18 FAIL）；修复后的重新验证 BLOCKED | `r17-leaf-gate/cli-rust-probe-1/` 与 `cli-rust-probe-2/` 各自保存当前源码构建、候选摘要、UTC、退出和原日志；具体根因、恢复条件见下节。不得复用旧二进制或局部通过 |
| 错会话仅带另一种身份标识的帧、带身份错误帧 | PASS，11/11 定向单测；Node/test 类型检查均 exit 0 | `cli-session-identity-4aln5abj/` 内三个命令的 UTC 起止、退出码、原始日志和新源码摘要；这是 Node CLI 负面边界检查，不是 Rust 原聊天叶 PASS |
| 新 CLI→Rust 证据生产者拒绝旧目录 | PASS（按预期 exit 1） | `cli-rust-matrix-guard-a4c73_ub/` 的命令、UTC、退出、原 stderr、旧日志前后 SHA；这只是防覆盖小检查，不能代替该生产者的真服务场景 |
| Rust CLI 凭证与终端显示安全边界 | PASS，16/16 定向单测；Node/test 类型检查均 exit 0 | `cli-token-validation-ob890ell/` 内原始命令、UTC、退出、stdout/stderr 和候选摘要；改动后仍需真 Rust 服务 E2E |
| 当前 CLI 四文件集成定向测试 | 首次 FAIL：默认沙箱 `listen EPERM`，1/36 受本机监听限制；按平台批准重跑后 PASS，36/36 | 失败保留 `cli-integrated-current-3umppmop/`；获准新目录 `cli-integrated-port-approved-9miew24y/`。两次各有命令、UTC、退出、原始 stdout/stderr 与源码摘要，后者只证明 CLI/Node WS 定向测试，不替代真 Rust E2E |

本子组初版候选摘要在 `typecheck-06/candidate-sha256.txt`；随后 `cli/chat.ts` 的同类身份匹配补查见 `cli-session-identity-4aln5abj/result.json`，凭证与终端边界后续补查见 `cli-token-validation-ob890ell/result.json`。最新已定向核对摘要：`cli/chat.ts` 为 `ea981a843a94495e63e4b2dfa3e337284329a41b5c3943b7ec80bef4fae34c1f`，`cli/rust-service.ts` 为 `ed7d22f49c7810435f8bad33ecf1eda65eb3f1a9dd44ddbce9c6d449d5e0a3a5`，`cli/entry.ts` 为 `616e2885eeab9a7c05a1593489e0954fbdacef64938d13faf6060979fcbac866`，`cli/server-runner.ts` 仍为 `357b9b2542eca630c2776525f3457fe5bb7596ca2be620c2f4734c37072d87b3`。早前 `run-08` 的 49/49 只绑定改动前的 CLI 源码，不能冒充本次新候选；首跑失败与所有复跑均保留原文件，不互相覆盖。

## 后续同根因修复（本轮续接）

- 本机实例记录新增 `transport` 后，旧 CLI 曾一律拼 `http://`，HTTPS 服务会误连 HTTP。`tests/cli-rust-service.test.ts` 先加明文、TLS、旧记录及未知值检查；首次红灯 `transport-red-20260928/result.json` 的原始 `vitest.log` 证明 TLS 被误判。`cli/rust-service.ts` 现按记录连接，旧缺字段按旧 HTTP 协议识别，未知值拒绝；使用 Node 默认 HTTPS 验证路径，不关闭证书检查。新目录 `transport-green-20260928/result.json`：17/17 定向测试、Node/test 两项类型检查均 PASS。当前 `cli/rust-service.ts` 摘要为 `3fb3d0097a30e0eaca8a5601dadab9c395aa7faf4b5c1bf583678337f29a5cc8`；上段旧摘要只作历史坐标。
- `sessions` 同入口回查：旧 Rust 数据库列表按最早创建时间升序，CLI 取前 20 项会漏掉最新会话。`storage/run_store.rs` 已改按时间及 ID 降序；新真服务生产者在合成库播入 25 项比原项更新的会话，要求 API 前 20 个 ID 精确为 `sess_cli_24` 到 `sess_cli_05`，并逐项核对 CLI 行序。冻结 R00 原句要求“已认证服务最多 20 个会话/空态”，并未逐字写“最近”；此排序修复是同入口既有 CLI 行为一致性，不改写 R00 合同。真服务尚未运行，不能记通过。
- 真服务生产者现在记录 81 个相关源码、锁文件和 CLI 测试文件的摘要，检查运行前后不漂移；启动失败还对照合成数据目录前后内容，退出清理只操作本轮已确认的子进程。AST 与 `git diff --check` 静态检查通过；已执行两次但均失败，不能记为叶 PASS。

## 真服务首跑和停止点

源码稳定信号后已经执行两次独立首验，**均未 PASS**，原目录和日志未覆盖：

| 本轮 | 实际命令与候选绑定 | 真实结果 |
|---|---|---|
| `r17-leaf-gate/cli-rust-probe-1` | `python3 scripts/rust-tauri/r02_cli_rust_matrix.py artifacts/rust-tauri/R02/r17-leaf-gate/cli-rust-probe-1`；`cli-rust-cases.json` 记录 81 文件摘要、Fresh Cargo target、二进制 SHA、每命令 UTC/退出/原日志；外层 exit 1 | Rust `--locked --offline` fresh build exit 0，但默认运行环境 `serve.stderr.log` 报本机监听 `Operation not permitted (os error 1)`，服务未 READY；不能算产品通过。所有原始文件保留。 |
| `r17-leaf-gate/cli-rust-probe-2` | 同一命令结构、全新目录；外层 exit 1；独立 fresh Cargo target 和本轮源码/二进制 SHA 均在 `cli-rust-cases.json` | 真服务 READY，18 项案例 **11 PASS、7 FAIL**；失败集中在 `status/sessions/continue/chat` 本机实例读取链，stderr 原文 `instance identity, home, or local token does not match`。清理、负面版本/启动/降级、候选未漂移等各自通过，但不得合并为整叶 PASS。 |

`probe-2` 的直接产品根因已静态定位：`main.rs` 的实例锁/记录生成一个实例 ID，`lib.rs::ServiceState::bootstrap_with_deps` 再生成另一实例 ID 给 `AuthService::bootstrap` 写本机令牌；CLI 逐项比对时正确拒绝。应由服务端把同一 `InstanceGuard` 身份交给认证初始化，不能放宽 CLI 对实例、令牌和数据根的一致性校验。服务端小组已收到缺陷；本组不并发改其文件。

后续服务端已改二进制启动入口：`main.rs` 调用 `ServiceState::bootstrap_with_locked_instance`，传入同一独占锁 guard；`lib.rs` 在数据根相等检查后把 guard 的身份交给认证初始化。服务端小组的新无端口真文件对账已报告 PASS。此项按**原范围产品缺陷：源码与无端口检查已修、真 CLI E2E 复验 BLOCKED**记录；`probe-2` 的 7 个旧 FAIL 仍是原始历史，不改成通过。

在 `probe-2` 已发起后，父任务通知当前平台端口动作不得再次等效重试。现停止所有真服务端口复跑；修复后只能作无端口的静态/单元核查，真 CLI→Rust E2E 标 **BLOCKED**，恢复条件为平台真实允许本机监听并允许按正式流程新目录运行。`probe-2` 即使某些局部案例为 PASS，也属于失败的中间候选，不可供 Gate 升级整叶。
