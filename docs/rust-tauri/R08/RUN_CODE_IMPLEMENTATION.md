# run_code 最小前置修复

状态：LOCAL_SELF_TESTS_PASS_PENDING_INDEPENDENT_REVIEW。工作流边界新候选已完成9项实际自测且列明源码/二进制摘要稳定；此前摘要为历史候选。默认组合根、完整权限、关库重开及独审仍不得代领，尚不构成R07准入通过或R08完成。

## 原要求与实际链路

原叶 `R00-T02-LA-C88F29B5114A` 的原断言保持不变：两次调用共享变量；重启后旧变量消失；取消清理内核；语言无效、无会话、内核死亡或超时显式失败且重启不声称保留旧状态。并沿旧工具保留 python3/node、run/status/restart、30秒等待上限、头尾输出截断和安装引导。`status` 使用读权限，`run/restart` 使用执行权限。

实现使用正式 ToolRegistry/ToolInvocationGateway 注册，Rust 统一授权和归属判定；Python/Node 子进程只执行解释器协议，不裁决任务、授权或恢复。每个主体、助手、会话和语言的内核分别保存；只给解释器必要环境变量。内核由现有 ProcessSupervisor 持有，取消时实际停止进程组，restart 确认旧进程收回后才建立新内核，死亡不静默重建。

## 验证边界

测试只替代外部模型响应；实际 Run、会话、授权网关、资源边界、解释器、进程监督器和 SQLite 存储均为真实实现。每项测试创建独立临时合成目录，不改全局 HOME，不采用真实用户数据。缺语言样本使用独立测试子进程的空 PATH，避免改并行测试进程的环境。

测试补充实际变量/import/函数跨调用、助手/会话隔离、只读 status、环境变量边界、真实输出头尾/UTF-8、异常死亡与显式重建、取消 Python 及真实孙进程、实际30秒等待上限、历史输出读回且文件副作用不重做。真实 UI 卡片消费与最终默认组合根验收由主集成单独负责，不能由本文件代领。

## 本轮命令与证据

工作目录为仓库根，工具链固定为本机已有 rustup 1.98.1。

1. `/Users/study_superior/.cargo/bin/rustup run 1.98.1 rustfmt --edition 2021 rust/crates/lingxi-service/src/coding_run_code.rs rust/crates/lingxi-service/tests/r08_admission_run_code.rs`：退出0。
2. `/Users/study_superior/.cargo/bin/rustup run 1.98.1 cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test r08_admission_run_code --no-run`：退出101。测试把 Arc 传给要求真实存储引用的取消入口，已用 `as_ref()` 修正。失败日志 `compile-01.log` 保留。
3. `/Users/study_superior/.cargo/bin/rustup run 1.98.1 cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test r08_admission_run_code -- --test-threads=1 --nocapture`：首轮退出101，7通过/1失败；Node 原生 REPL 的运行异常从自身 domain 发出、不回调 eval，导致错误被等成30秒超时。现同时接收该错误完成信号，保留真实 REPL 的 const/let 和顶层 await 语义。`test-01.log` 保留。修复后同命令退出0，8通过/0失败/0忽略/0过滤，32.91秒，见 `test-02.log`。
4. `/Users/study_superior/.cargo/bin/rustup run 1.98.1 cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test r08_admission_run_code python_and_node_real_kernels_preserve_variables_functions_imports_and_isolate_agent_sessions -- --exact --nocapture`：增加 Node 顶层 await、动态 import 跨调用检查后退出0，1通过、7项明确过滤，见 `test-03.log`。这是新增断言的定向复验，不冒充全套。

实现者验证后仍须独立审查原断言与实际链路。正式阶段门禁、UI、迁移/回滚/前滚不在此局部结果内。

证据目录：`artifacts/rust-tauri/R08/run-code-implementation-01/`。`receipt.json` 固化每次精确命令、退出码、原叶绑定、最终两份源码摘要、日志摘要和替身边界。首轮失败后的捕获解释器 PID（78667—78671）复查均已退出；成功场景逐一断言真实监督器无活进程。编译出现既有大型二进制 unwind 表链接警告，未吞退出码。

平台限制：本轮只实际执行 macOS。现有非 Unix 进程监督器明确拒绝 UnsupportedPlatform，本轮不声称覆盖 Windows。Node REPL 错误通道使用其内部 domain 形状；启动先检查该形状，缺失时拒启，不能静默换无持久语义的解释器。独审仍需核实默认组合根与允许平台边界。

独审续修：run/restart 若存在真实 workflow_scope 则明确 RUN_CODE_WORKFLOW_UNSUPPORTED；status 保留只读查询。新增实际 workflow agent(writeFolders=窄子目录) 的 Python run/restart 与 status 执行反例，验证兄弟文件未创建、监督器无内核。等待本轮重编实测，不能用此前局部通过覆盖新增差异。

## 2026-10-22 新冻结候选本域执行

主集成统一编译新候选后，直接执行 `rust/target/debug/deps/r08_admission_run_code-465a18f4152e0455 --nocapture --test-threads=1`，实际退出0；9测试通过、0失败、0忽略、0过滤，33.83秒。日志 `test-final-candidate-01.log`；起始源码/二进制摘要见兄弟证据目录 `ptc-implementation-01/final-candidate-01-start.json`，尚待全部本域运行结束后的摘要复核。

新增workflow反例已实际走正式workflow agent(writeFolders=窄子目录)、真实Run和工具网关；run/restart明确拒绝，status只读未启动，兄弟文件没有创建、无解释器进程。早期workflow-test-01/02为测试替身假设错误而红：真实工作流子Run沿父会话，以及同父会话会收到异步steering回灌；改不同父会话逐项核验后绿，原日志和失败根保留。

这是实现者的新候选本域实测，仍不代替默认组合根、完整权限矩阵、独立审查、UI和正式R08。关库重开历史仍未执行，计划单独明确标未执行，没有用同进程读回冒充。

新候选运行结束后的源码/二进制摘要复核全部一致，最终回执 `run-code-implementation-01/final-candidate-01-result.json`，共用完整回执 `ptc-implementation-01/final-candidate-01-result.json`。替身边界补充：外部模型是确定性替身，服务元数据时间使用ManualClock固定NOW；30秒等待、取消和进程均为真实时间/真实实现，不能把逻辑时间夹具说成所有依赖均未替代。
