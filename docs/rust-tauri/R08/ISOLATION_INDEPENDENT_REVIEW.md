# R08 任务0隔离修复独立验收

结论：**PASS_LOCAL_ISOLATION（仅限下述合成目录、本机开发入口的隔离与退出）**。本结论不接受 R08 全阶段，不解除 R07 独立准入，不证明迁移/回滚或全界面回归已经完成。

- 日期：2026-10-21（Asia/Shanghai）。
- 独立验收者：`/root/r08_leaf_map`。本代理此前仅生成逐叶准备地图，没有实现本次启动器/单实例修复或探针。
- 候选：分支 `codex/rust-tauri-migration`，HEAD `86840d1700397d3dd620a41f2b884df4cddd5cc2` 加未提交隔离修改；以文件摘要绑定，不把基线提交冒充含全部修改。
- 环境：macOS arm64，Node v24.16.0，Electron 42.8.1；真实 `lingxi-service` 调试二进制 SHA-256 `d61c7165e41bfe17ed6af4ae56842389e74e97f430b37bc6371698a7bcba6d9c`。没有访问真实用户数据作为试验输入。

## 审查范围与源码结论

已核对 `scripts/dev-env.js`、`desktop/src/shared/single-instance-lock.cjs`、对应两个测试的完整当前差异，以及 `r08_isolation_probe.mjs` 的实际启动、断言和清理链。

显式 `LINGXI_HOME` 经既有路径解析后保留，并随原 `scripts/launch.js` 传给 Electron 和真实 Rust 子进程；未指定时仍为 `.lingxi-dev`。没有修改全局 `HOME`。默认 `.hanako` / `.lingxi` / `.lingxi-dev` / `.hanako-dev` 的原缓存命名规则保留；显式副本的 Electron 状态进入副本自身 `.electron-user-data`，不再按最后一段同名目录共享缓存和锁。

原候选存在一项真实缺陷：默认目录的符号路径别名与其原路径对应同一真实根，却进入不同缓存/单实例空间。验收者用独立合成目录复现，`sameRealHome=true`、`sameUserData=false`；实施者随后改为先比较真实路径，并增加默认根别名保护。验收者原反例复跑已绿，红/绿回执分别保留于 `isolation-review-01/default-alias-reproduction.json`、`default-alias-after.json`。当前审查范围无未关闭源码缺陷。

## 实际运行及反向证明

| 检查 | 实际结果与证据 |
|---|---|
| 独立针对测试 | `npx vitest run tests/startup-contract.test.ts tests/desktop-single-instance-lock.test.ts`，退出 0，2 文件/20 项通过；`isolation-review-01/unit-after.log`。这些测试含伪造 app 的单元断言，只证明函数和调用顺序，未当原生运行证据 |
| 独立冻结探针 | `node scripts/rust-tauri/r08_isolation_probe.mjs artifacts/rust-tauri/R08/isolation-review-final-live-01`，退出 0，四项实名断言 PASS；不是复制实施者结果 |
| 真实主进程/子进程路径 | 同名 `a/copy`、`b/copy` 两根分别启动，主进程实际 `LINGXI_HOME`、`userData`、`sessionData` 与 Rust `instance.json` 路径及 PID 全部相符；使用真实 Electron Inspector 只读状态，无 Electron 或内核替身 |
| 单实例 | 同一根重复客户端退出 0、原服务 PID 不变；另一个副本仍存活。符号路径别名再次启动也退出 0，不开第二服务 |
| 正常退出与后代 | 经正常 `app.quit()` 退出，两客户端与自有服务退出 0；实际捕获的 10 个 Electron 主进程、GPU/网络/renderer 后代及 Rust 进程全部不存在。`residue=[]`、`forcedCleanup=[]`；验收者另查 PID 无存活 |
| 错误期望根反向探针 | 核 `isolation-wrong-root-command-01/receipt.json` 原始退出码 1，`isolation-wrong-root-01/result.json` 为 FAIL，错误明确“数据根与期望副本不一致”；没有吞退出码或改 PASS。随后实施者 final 及独立冻结版本均恢复绿 |
| Cargo/桌面/类型基线 | 核 `task0-cargo-build-02`、`task0-desktop-build-02`、`task0-typecheck-02` 的命令收据、源码稳定字段和日志摘要；三条退出码均 0。验收者未重复编译，未将日志中的 Finished 文本独自视为通过 |

冻结探针摘要：`ab32571bd6fa9294a51b1042cef9078161d7d310b18da71dac5b243ab0869c35`。独立最终 `result.json` 摘要：`3c5fa448bf8cee8e3c9edfa70ff28de50552fdf3e79493b924f6916724627918`。逐文件候选摘要、命令退出码、日志摘要、收据摘要及验收边界集中在 `artifacts/rust-tauri/R08/isolation-review-01/final-review-receipt.json`。已核该轮输入摘要与当前磁盘一致；相关源码或二进制变化时本结论需按影响复验。

## 保留失败与使用边界

1. `isolation-live-01` 是探针过早连接调试上下文失败；`live-02` 揭示未采用非空目录被 epoch 闸拒启，且早版探针清理留下主进程；`live-03` 是未规范化路径导致 MCP 只读目录拒绝；`live-04` 是探针读取错误实例字段。原失败未改写。后续采用真实路径、真实服务先正式采用空白合成根，并修正探针字段和清理断言。验收者已核早版已知残留 PID 40944/42016 不再存活。
2. 本轮正向探针的两个空白根先由真实 Rust 服务正常启动采用、正常退出，再经 Electron 开发入口启动；没有手工造印章、降印章或删 journal。**未经采用、已被壳日志/缓存写入的空白根会拒启**，不能把本结果描述成任意未采用目录首次点击即能运行。实际预览流程须保留正式采用/校验前置步骤；失败不得自动清场。
3. 实际 Inspector 采样在 `app.isReady()` 后进行，部分窗口 URL 尚未建立；本轮证明原生进程、目录、单实例及退出，不证明 React 完整载入或产品任务执行。真实文件任务、托盘/关窗策略、附着/远端多客户端及浏览器替代宿主仍归 T03/T07。
4. 默认命名规则经源码/单元测试核对，未启动真实默认目录；没有在本审查读取真实用户数据内容。**真实生产目录逐字节前后摘要尚未执行**，不能以路径隔离结论替代 T08 的生产未变证明。正式安装包、其他操作系统同样未验。
5. 本结论仅允许继续在正式采用的合成/获准副本上开展已授权工作；依赖 R07 的业务任务仍服从单独准入结论。没有提交、推送、发布、生产迁移或 R09 放行。
