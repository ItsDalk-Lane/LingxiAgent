# 生产工具声明整合修复

结论：本分工已完成，实际36项检查全部通过。原F24六项完全未改，原四个失败均恢复真实通过；F27所需已注册worker声明也已在生产同一链路补齐。F27替身历史边界与完整资源负载复验由另一作者负责，本报告不替其签通过。不代表整轮正式验收通过。

## 改动范围

仅3文件：kernel/toolcatalog.rs 新增单个已注册target的 additive resident API；service/lib.rs 在按需目录注册完成后只加入真实 write_stdin 目标，在受信 worker 配置原校验、真实注册及Available设置均成功后只加入该实际目标；r06_ar_f07_catalog.rs 新增2项边界回归。没有修改原F24/F27、closed-loop、background-steering、session-file、阶段输入、文档或任何门禁。没有提交/推送，也没有Co-authored-by/Copilot署名。

原 toolcatalog_bridge.rs（含RESIDENT_NAMES七项列表）与HEAD逐字相同；通用目录仍原七项默认，只有生产宿主明确完成注册的终端配对/配置worker具有附加资格。没有开放全部Available/Deferred，也没有将模型提供的名称或参数作为激活来源。

API在同一个registry锁内先核实际目标存在，仅在已配置resident集合实际新增ID时提高目录代次。重复调用不提高代次；未注册ID拒绝且不改代次；未配置集合None保持原全声明语义与原代次。Available/Deferred/Disabled判断、真实schema/target身份、未知wire拒绝、执行权限与资源审批均仍走原路径。增加声明资格不会改availability或赋予执行权限；已发送快照保留冻结。

## 实际验证

所有Cargo使用绝对 `/Users/study_superior/.cargo/bin/cargo`（1.98.1）、`--locked`、`CARGO_INCREMENTAL=0`、`RUST_TEST_THREADS=1`与显式`--test-threads=1`；复用仓库外 `/Users/study_superior/.cache/lingxi-r06-builds/native-write-clean`。独立合成 `LINGXI_HOME`/`TMPDIR`位于本目录runtime-r1，`LINGXI_PRODUCT_DIR`指Main/lib，没有覆盖真实HOME或读取真实业务资料。本分工无Node步骤。

- 原F24 6/6 PASS，11.11秒：五工具真实wire；真实exec；越界workdir拒绝；global permit1下真实main→worker→aux→main和费用JOIN；子进程无凭证；callback停泊时SIGTERM物理回收。
- F07 原8+新2共10/10 PASS：默认七项逐项原文；未注册/重复/None代次；已注册准确target/schema；Deferred/Disabled不提升；其它目标不出现；冻结旧快照；原真实describe→call、本Run说明/代次隔离及五协议历史原断言全部保留。
- 原R04 catalog 6/6、gateway 13/13 PASS：真实目录与执行同源、代次、伪造digest/参数零dispatch、跨入口权限/批准、子代理/委派约束。
- adapter原 unknown_wire_names_are_protocol_violations_never_guesses 1/1 PASS，未知工具仍拒绝。
- 三文件diff --check通过；只对拥有文件运行rustfmt。按Root协调，不重复跑全workspace strict/fmt/登记，留各修复整合后统一执行。

命令主体（共享上述固定环境）：

```
cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_production_tools --test r06_ar_f07_catalog -- --test-threads=1
cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r04_t01_tool_catalog --test r04_t02_tool_gateway -- --test-threads=1
cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --lib unknown_wire_names_are_protocol_violations_never_guesses -- --test-threads=1
```

日志分别 original-f24-and-catalog-r1.log、original-r04-boundaries-r1.log、unknown-wire-r1.log；原F24字节相等、F07仅新增2函数且原全文字节保留、七项策略源文件字节未改，均由source-sha.json机器断言确认。真实合成Run状态、费用JOIN及发出的8/9工具声明也收录该JSON（不含token）。

新增需Root统一登记的完整case名：

- r06_ar_f07_catalog::trusted_resident_addition_preserves_default_policy_and_real_target_boundaries
- r06_ar_f07_catalog::trusted_resident_addition_keeps_unconfigured_registry_unrestricted

## 交付与限制

net.patch仅本3文件；base/与candidate/保存精确源码；source-sha.json含base/candidate/patch/日志SHA和实际运行摘要。生产源码已在Main，其他作者同期文件未触碰，不应整树回滚或应用重复补丁。

已验证当前macOS原生Rust链；没有重跑全599/正式stage，没有宣称Windows/Linux动态验证、真实外部供应商或完整发布通过。整合后的全workspace质量检查、F27完整资源回归及后续正式窗口由Root统一推进。后续任何新增resident来源仍须来自可信配置的实际注册结果，不得从模型输入调用该宿主API。
