# R06 全量窗口与正式验收只读预检

结论：Root 提议的命令序列符合现有执行器，无需新判定、白名单或放宽断言。先在全新证据根逐项执行全部 `executionBindings`，随后调用现有无范围过滤的叶汇总，再用另一个全新证据根执行原 `verify-stage R06`。用户要求的前置干净全量窗口必须保留；本报告不代替窗口或阶段验收。

## 当前覆盖核对

当前生成器与已生成 inputs/graph 一致，现有 `r06_stage_inputs.py --check` 实际 exit 0。当前 280 个完整叶合同、0 个 partial、592 个 executionBindings。全部叶自身引用去重只有 433 个，另有 159 个已登记绑定，因此仅跑叶引用不是全量。旧 helper 默认 194 叶当前仅选 372 个绑定，会漏 220 个。592 个绑定的 argv 没有完全相同的重复组，不存在可直接删除的同命令副本。

280 中含特殊合同 `R00-T02-LA-960668141E07`，不在 279 项原始 inventory 子集里。不能把 280 ID 塞入 `--proof-leaf-ids`；该参数会按原 inventory 检查并拒绝。无该参数的现有汇总会消费全部 `leafBindings`，正合 Root 方案。初次检查发现的旧锚点和两个 partial，Root 更新后当前均已消除；初次失败证据单独保留，不列作当前阻碍。

## 确认的序列

1. 在冻结前写好任务编排器；冻结后不再修改源码或提交移动 HEAD。选择全新、无符号链接、未含旧证据的外层目录。
2. 遍历冻结 inputs 的全部 592 个 `executionBindings` key，每 key 只调用一次现有执行器：`python3 -B scripts/rust-tauri/r06_stage_inputs.py --binding KEY --evidence ROOT/R06_BINDINGS/KEY`。串行保留实际退出码、标准输出和错误，不因 exit 0 就自签通过。
3. 调用 `python3 -B scripts/rust-tauri/r06_stage_inputs.py --leaf-evidence --binding-evidence ROOT/R06_BINDINGS --evidence ROOT/R06_LEAVES`，不传 `--proof-leaf-ids`。现有汇总核验具名测试、当前候选、日志摘要和合同；其 `NOT_A_STAGE` 仍不能当阶段通过。
4. 窗口通过后另用新空目录执行正式命令：`/Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R06 --evidence /private/tmp/NEW_FORMAL_ROOT`。

正式 R06 当前有 595 个命令：592 绑定、输入检查、R05 回归门和叶汇总。xtask 已按命令 key 去重，不需再对每个 case 调一次包装器，也不需先额外跑一次全 npm/全部 cargo 套件。前置 592 与正式 592 的两轮执行是用户要求与现有新鲜证据约束所需，不能互相借结果省略。

本报告核对的是现有 API 与上述序列；Root 尚未写完的新编排器实现本身未审，写好后可再只读核对其 key 集合、退出码处理和无 scope 参数。

## 可提前排除的环境错误

建议启动父进程明确继承以下设置，避免子命令使用不同工具：

```sh
export PATH="/Users/study_superior/.cargo/bin:$PATH"
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="/Users/study_superior/.cache/lingxi-r06-builds/native-write-clean"
```

本次预检实际 PATH 的 cargo 是 `/opt/homebrew/bin/cargo`，而指定绝对路径 cargo 1.98.1 可用。只给最外层 cargo 绝对路径还不够，R05/R04 子命令内仍有裸 cargo，应统一 PATH。实际 Node 24.16.0、Electron、TypeScript 和保留 Tauri target 均存在，无需安装。业务隔离由执行器的 per-command `LINGXI_HOME`/`TMPDIR`/product 与原 fixture 环境完成；不覆盖真实 HOME。

CARGO_TARGET_DIR 必须在 `/private/tmp` 及允许 TMPDIR 之外，否则沙盒拒写 sentinel 位于本就合法的临时根中，验证失真。旧 helper 的未设置默认值在 tempfile 根下，不适合本轮；应显式指定外部 target。旧 helper 对 rustDesktop 会移除该变量以复用 desktop 默认 target。新编排器如保留这种按种类环境处理，可避免额外重建；不得复制/清理他人缓存。正式执行器本身没有同样 target pop，是否额外构建属于环境成本，不能靠改验收命令解决。

全部冻结窗口应串行，期间不改生产/测试/注册文件、不提交移动 HEAD，不与其他编译或 GUI 验证抢资源。执行器覆盖未忽略未跟踪源码和删除；不是只看已提交文件。artifacts 与当前证据输出按既有规则排除。

## 真实窗口与人工证据条件

两个 rustDesktop 绑定中，`edit_current_host::current_tauri_native_edit_four_commands_and_refusals` 是实际 macOS Tauri 编辑窗口，需要可用图形登录会话、未锁屏和原 e2e feature；`native_resources::real_file_read_write_copy_and_version_boundaries` 是文件边界，不能仅因种类名当 GUI。许多 service 父也启动真实 Electron，不能在无 GUI 会话中预称能通过。

原选择目录/废纸篓两叶四组仍依现有核准人工凭据，不是自动重放。检查既有批准凭据真实存在即可，不把 ignored 手工场景用空匹配当成功。desktop 绑定现有 frontend build prelude 每次都会执行，不应借旧 frontend 构建冒充当轮。部分旧启动器仍可能回落 5173，应避免并行窗口争端口，不能杀占用端口的他人服务；新聊天/会话启动器已使用明确动态非零端口。以上是运行前提/风险，不是已发生的新业务失败。

## 封印适用性

当前旧封印坐标 `ab4f228155dd4246734e6c4c45d7288072361f3c` 是有效提交，并与 matrix 一致；它不是当前 HEAD。直接执行 `.sync-audit/verify-post-verification-diff.mjs` 实际 exit 1，因为之后存在非审计生产改动。直接封印检查、post-verification seal 测试或 full npm 中相关用例不能被本报告称绿，不能修改坐标或白名单解决。

但原 R06 门禁不直接调用 sealguard，间接 R06→R05→R04→R03 的旧回归明确使用 `R02_LEGACY_REGRESSION_MODE=directed-no-seal-family`，只跑 E0–E4.5，在 E5 全 npm/封印之前退出。因此不能把旧坐标笼统写成正式 R06 必败。独立完整 R02 的 E5 是另一条适用路径。upstream matrix 坐标有效性检查也不因 HEAD 较新就必败。以上仅如实区分，不变更任何规则或 `.sync-audit`。

## 实跑与未执行

本轮仅重新生成只读元数据、现有 --check、工具/依赖存在性检查、直接封印 guard；没有重复执行业务大套件、592 全量窗口、正式 R06、GUI 或 full npm。当前业务是否全通过仍以 Root 的当轮原始证据为准。原始结果见同目录 `confirmation.json`、`coverage-and-build.json`、`current-all-scope-differences.json`、`check-after-root-refresh.log`、`environment-and-seal.json`、`seal-direct.log`。没有 Main/门禁/断言/封印修改。

## 后续只读核对：EX05 输出包装与全量编排草稿

Main 新 crash 父相对冻结原文仅增加 child_status 包装，并将两个子进程 status 改为 output。全部原退出码、首写物理字节、恢复字节、日志状态与幂等断言不变；两个子输出的 stdout/stderr 每行均打印有名 EX05_CHILD 前缀，返回同一个真实 ExitStatus。没有过滤失败或删除断言。UTF8 lossy 行输出属于诊断文本保留，不声称二进制原始输出逐字保存。独立 diff 与 SHA 见 ex05-child-output-only.diff / ex05-output-and-driver-review.json。实际修后 registered r2 结果由 Root 窗口出具，本次不重复业务运行。

仓库外草稿 r06-all-current-bindings-window.py（SHA 5bfdda07a08c932426b01ece53535c9ba331669f011b3478c9afbbedab7d9c5e）语法可解析，精确遍历当前全部592项；desktop kind 与当前输入一致，清除headless target；每次原 timeout+180；最终调用既有汇总且无scope限制、明确不产阶段结论。脚本内容尚有一个诚实范围限制：外层 TimeoutExpired/OSError 未捕获会中止后续项且不写summary，因此该情形只能报失败/未完整执行，不能报592全量跑完。若加异常记录继续剩余，是编排健壮性补充，不改任何通过判定。
