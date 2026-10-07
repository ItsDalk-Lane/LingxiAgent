# RR3 I-01 / F49 精准实现报告

状态：**SELF_CHECKED**。执行者 rr3_i_impl_01；独立审查 PENDING，等待全新 I-REVIEW-01。仅 R05；不自签独立 PASS，不关闭 F49，不改变原放行公式，R06_READY=false。本包源码已停止修改。

## 先登记的最小范围

- 唯一既有生产文件：scripts/rust-tauri/r05_t08_negative_gate.sh。根因：快照与恢复重复列举导致 kernel 漏恢复；复制/比较异常没有逐项拒绝；最后 N16 未恢复。改为同一变异文件清单、一次 pristine 快照、复制后逐字节比较、最终恢复。保留候选 dirty 字节，不读 HEAD 覆盖文件。
- 必要邻接仍在同一文件：N06 初始绑定同步。当前 main.rs 先 Scope::new 创建输出根，再 snapshot 和 runner 检查，随后 verify.rs 输出 authority 命令启动行，run_command 才创建命令目录/日志与启动子命令。根存在不能证明 before 已完成。使用当前 R02 场景首命令 authority 推导并精确匹配现有启动输出，同时检查本门禁仍存活；缺前提明确失败，无注入。原 G 20 checkpoint 不稳有效结果保留。
- 同文件既有 Git 身份查询从嵌入 note 改为显式检查返回值；Git 异常不能作有效输入。
- 新永久回归文件：scripts/rust-tauri/r05_t08_restore_selfcheck.py，所有者 rr3_i_impl_01。只调用提取的真实 shell 快照/恢复/同步函数，覆盖所有变异目标、N06/N16、幂等、dirty、cp/cmp/query 与前提失败；不改其他 B 文件。


## 最终改动与根因

生产改动仅 scripts/rust-tauri/r05_t08_negative_gate.sh；新增永久回归仅 scripts/rust-tauri/r05_t08_restore_selfcheck.py。原 B 权威计数变异器、自检、pins、Rust、绑定器及 H/E 文件未改。按本包开工原文件比较的完整差异在 implementation.diff，避免把先前 B 改动算入 I。

1. 快照与恢复共用 MUTATED_FILES（12 个文件，实际源码/权威表变异目标 11 个，binary-wiring 文件为原有安全超集）。含 kernel。快照拒绝重拍；mkdir/cp/cmp 的相关异常明确失败；每次 reset_copy 按 pristine 字节恢复并逐项 cmp。最后 N16 后增加 reset_copy，才汇总。没有删注释、回退 HEAD 或接受漂移。
2. N06 从“根存在 + sleep 3”改为：从当前 R02 authority 的首场景首命令读取 argv，精确匹配 verify.rs 原命令启动输出，再确认同一门禁进程存活后追加原 kernel 变异。只按信号轮询，未增盲等待。PS/Git/日志/authority 查询失败、未捕获启动、已退出/僵尸均拒绝；缺前提不注入、不计有效负测。同步失败保存 sync exit 并等待实际进程结束后明确失败。PS 状态检查是“仍在运行”必要邻接，防止 kill -0 把未回收的退出进程当活跃；未增加生产接口。
3. 原准备阶段的 source HEAD / copy branch / copy HEAD / status 查询改为逐项检查退出码，避免 note 中的查询错误被吞掉。只读主 Git，不更改 HEAD/index。

## 原 N01–N16 的变异文件完备性

逐项清单和直接脚本文字目标扫描见 mutation-completeness.json；永久回归独立保留预期目标集合，并校验实际目标集合与注册清单。N01/N07/N14→R05 map；N02/N03→pins；N04→service credentials + suites；N06/N16→kernel；N08→service lib；N09/N11→tool_render；N10→openai_completions；N12→runs；N13→R04 map；N15→adapter credentials。N05 只变更本例新证据文件，不变更候选源。恢复清单包含全部，重复项为零。

## 原逻辑旧红与修后绿

旧原脚本已先保存 original-negative-gate.sh。old-red-command.sh 在自己的新 copy 复制真实 12 文件，用原 snapshot_pristine/reset_copy；追加原 N06 kernel 注释后调用原 reset_copy，再实际 cmp：**exit 1，目标红**。stderr 明确 EOF，pristine hash=8f6595e9be39865f74a91817089ab1d6b016beb84f31a6ed08ca58b96229d3e5，原 reset 后 hash=0a5f0c3aee15f437cae03cca5eb182680f6e57aa64e3dcd95fbb5561a243720f。不是编译失败/手工理想还原。原命令/cwd/UTC 在 old-red-receipt.json，旧红未覆盖。

最终永久回归：`python3 scripts/rust-tauri/r05_t08_restore_selfcheck.py --evidence artifacts/rust-tauri/R05/RR3/I-01/selfcheck-final --production-sync`，exit=0，**56 项通过 / 0 failed / 0 ignored / 0 filtered**（这里是检查命令计数，非业务测试数量）。UTC 2026-10-07T02:33:57.724677+00:00—2026-10-07T02:34:11.448561+00:00；每个实际命令、cwd、UTC、exit、日志 hash 在 selfcheck-final/commands.json 和 result.json。

覆盖：原 N06/N16 两条实际 append 语句→原 reset 精确还原；全部 12 文件任意字节变异→原 reset 精确还原；重复两次恢复幂等；真实 Git 的 tracked dirty kernel 与 untracked 非文本文件保留，status 前后相同；禁止重拍快照；cp 失败（含精确 kernel 目标）、cmp exit2、cp 假成功、缺 pristine、snapshot cp/cmp 故障均明确拒绝并恢复绿；四个原 Git 查询故障拒绝；authority 非法、根已出现但无启动、日志查询失败、进程查询失败、已退出（含已有真实启动输出后退出）均拒绝。

正常同步永久对照使用**自身新源码副本、新构建 production xtask**，main/verify/Scope/Git/真实 OS FD 发现器保持原逻辑；只在隔离 R02 map 安放 RX 控制场景，以释放文件控制一条命令结束。输出来自生产 verify.rs，不自造信号，不替换 entry，不冒称完整 R02 业务 gate。三次均 runnerSourceBinding=PASS、实际子命令 exit0/PASS、checkpoint=1：

| 对照 | 实际退出 | 来源 stable | overall |
|---|---:|---|---|
| 正常同步、释放命令 | 0 | true | PASS |
| 原生产同步观察启动并确认存活→原 N06 append→释放 | 1 | false | FAIL |
| 原 reset 精确还原→再次同步 | 0 | true | PASS |

sync-order-audit.json 校验真实 before 时间早于捕获信号、append 晚于 wait 成功。目标红唯一变化路径 kernel，子命令全绿，证明 stable=false 独立强制 FAIL；没有用业务/环境失败充绑定红。完整原 R02/R05 多 checkpoint/业务 gate 本包 NOT RUN。

## B 相关复验与汇总防缩水

最终当前脚本：原 B 15 项自检 exit0；原 B 独立 41 项对抗检查只复制驱动到 I 新目录并改证据输出路径，exit0（15 注入输入 + 26 汇总夹具）。包含旧 7→6、未来计数、零/重复/非法锚点拒绝；默认 ALL、完整原16唯一身份；空/缺项/重复/错误项拒绝；N03 仅 1，其他15 NOT RUN。夹具汇总不冒称16实跑。真实命令/UTC/exit/hash 在 final-local-commands.json。

另以原生产 run_n03/xtask_test/write_results 和新原 shell 恢复函数在自己新副本实际运行：正常镜像 **8 passed /0 failed/0 ignored/113 filtered，exit0** → 唯一权威目标 `svc:r05_t01_model_plane` **24→23，matches=1，mutations=1** → 目标具名单测 **0 passed/1 failed/0 ignored/120 filtered，exit101** → 字节还原镜像 **1 passed/0 failed/0 ignored/120 filtered，exit0**。实际 summary scope=N03/cases=1，其余15未执行；细节 b-production-n03/command.json、mutation.json、日志和 b-production-counts.json。B 原权威表/辅助文件 hash 与开工相同。

## 失败记录、边界与来源

- selfcheck-01 是初始 32 命令自检成功截点，尚无生产正常同步，不能代最终。
- selfcheck-02 实际 exit1：RX 夹具漏拷必需 R00 两台账，生产提前拒绝，未达到启动；不算有效目标红。旧日志保留。
- selfcheck-03 实际 exit1：正常同步实际 PASS 后，自检读取错 JSON 字段名而失败；修为现行 checkpointAfterEveryCommand。另一次写测试时缩进错误 exit1，未执行测试；随后 selfcheck-04 56 项成功。以上均保留，不择绿伪报；最终 selfcheck-final 使用同生产候选、最终永久回归源码重新成功。
- H 并行修改 smoke/redaction/lib/logging；每次副本只按复制时实际字节作为边界，不声称全树被冻结。本包 source-after 列明 I/B/xtask/lock/schema 的输入 hash 和局部摘要；不是整个工作区 digest。主 .git HEAD/index 字节相等；未对主 Git 写入或提交/推送。真实 Git init/add/commit 仅构造本包新小夹具，不复制大 .git。
- 不写/构建 G 的 negcopy.hTXzzN 或共享 NEG_TARGET，不改变 G 的旧输入/历史证据。G 原 n06-binding-audit.json 的20 checkpoint 不稳/唯一 kernel 变化为有效旧输入结果，保持原结论；G 手动恢复不能代主树修复。
- 未装依赖、未修改系统、未外发/派 agent。新自己的 xtask target 只构建 xtask，无全服务或大 target 复制。
- 工具链：rustc/cargo 1.98.1、Node v24.16.0/npm11.13.0、Darwin arm64；所有 Rust 命令 --locked 且 offline，原 rust/Cargo.lock 和 rust-toolchain.toml hash 不变；toolchain.json 保存实际命令输出。

## 交接与受影响项

候选主脚本 SHA256 `a69110b81298705a9161652d28402a5e5a585f945fdfd214ccf071132ecb67dd`；永久回归 SHA256 `0cd24624bf7d6079e3cf7b4e6cf693222026db8261ffc60691bdc77fde0f57a4`。主 HEAD=b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b。source-before/after、read-inputs、锁/生成 schema hash、runner-binary、所有日志与产物在 manifest.json。

本轮影响**全默认16的共用 pristine/reset/final恢复与身份查询**；N06 另影响初始绑定同步。N03 注入和唯一完整汇总公式原样保留。这里只 SELF_CHECKED，交全新 I-REVIEW-01 亲跑永久对照（建议以上最终命令，证据改为全新目录）并核原逻辑旧红/恢复；不要将实现者的受控 PASS 当独立验收。之后总控等 H/I 全新独立通过、源码冻结，再派**新 G 完整默认 N01–N16**；旧 G 当前副本不能套用到新候选。最终正式原 §5.3 集成/原放行公式仍由新阶段审查者执行。本包不再写脚本，不负责耗时全16重跑。
