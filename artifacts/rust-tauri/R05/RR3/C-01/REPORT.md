# RR3 C-01 实施与补证报告

资源观测补齐并完成自检，但资源整项 **FAIL：F46 OPEN**。原160轮完整测量的旧断言显示2/2通过；完整文件清单随后证明重启后4个日志文件超过原上限3，不能据2/2宣布I10或R05通过。本包状态为 **SELF_CHECKED，未独立签收/关闭**。按总控要求保留真实红，交给全新F46实施者与随后全新C/F46联合验收。本报告没有stage/R06结论。

## 改动和事前约束

仅修改 `rust/crates/lingxi-service/tests/r05_t08_resources.rs`，新增 `tests/support/r05_resource_sampler.rs`；完整差异见 [source.diff](source.diff)。保留原两测试身份、测试数2与原160轮负载：双会话，60预算取消+60错误+15正常+10×512KiB长响应+15真实worker。没有新增生产接口、资源管理系统或更换生产provider/supervisor/storage；没有修改logging生产文件、stage-map、pins或producer，没有提交/推送。

[preregistered.json](preregistered.json) 在2026-10-07T00:20:00.153592Z、首次资源测量前登记原负载与原界限：RSS 409600KiB、RSS增量153600KiB、FD400、FD增量64、日志3；进程树同RSS/FD上限，存活worker1/释放0，存活TCP≥2/释放0；owner峰值任务2、model/tool permit各1、model排队1、清理后0，3次连续100ms稳态窗口。阈值没有放宽。初始准备错误读取不存在的rust/rust-toolchain.toml，在任何测量前改为根rust-toolchain.toml，见environment记录；第一次构建E0382在任何测量前修正stub计数读取顺序，exit101与未记录时间如实留在[build-01-compiler-error.json](build-01-compiler-error.json)。不伪造原始stderr或UTC。

资料基线包括RR3_BRIEF、RR1/RR2完整主提示词、RR2/G-R2/I-MAPPING、FINAL-01资源序列与审查、原R05任务书与05验收性能、10-02中适用资源要求、PERFORMANCE_THRESHOLDS与R05性能结果。新[I-MAPPING.md](I-MAPPING.md)只修正本包I10证据引用；历史材料未覆盖。

## 环境、候选与摘要绑定

[environment.json](environment.json)记录macOS27.0.1 arm64、rustc/cargo1.98.1（根工具链锁）、Node24.16.0、分支codex/rust-tauri-migration与HEAD b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b。所有Cargo测试/clippy使用绝对Cargo路径与--locked（fmt不解析依赖）。这是带其他工作包并行改动的工作区证据，不把HEAD当作本轮源码。受影响源码/配置324文件的原始清单及摘要见environment；该清单包括被发现的本地隐藏状态，故其摘要只说明实际采集范围，不能当全仓库标准冻结清单。

原160测量的主测试SHA256为104af86dc594f4dc990f43bfc420d194be647667a30819e5645bd3d82bd65d7d；最终加入post-restart/owner稳态日志断言后的主测试为23b03f56bef900fc0563b3348596e61437995f7c05cc25a5173455f11a23e33f。helper为1f6601122e5d7841f3975ec68ea8c60adb653fc939ca4e65cc3da7565c269274。每条命令before/after记录自身输入一致性；build02自身before/after相同，后续主测试增加内容使用新摘要。最终[FINAL_SOURCE_BINDING.json](FINAL_SOURCE_BINDING.json)注明当前绑定、与measurement的区别及未重跑完整负载。其他工作包后续变动不由本包重新签字。

[measurement-01/runtime-binding.json](measurement-01/runtime-binding.json)记录实际服务PID、带--home/--config/--bind/日志阈值/1500ms预算的完整启动命令、严格磁盘配置fixture（合成凭据内容已脱敏）、二进制/fixture/测试装备摘要。服务debug二进制SHA256 9628ba09d14e66ab9ba7824b4f3afeb1844e568ae1d66b88f95a57a8a02d119b；worker fixture 231b3fc53d8150d617fa8a63ab7b9e1a85ea2644daf8f020b3232e55477ae06f；实际测试装备ca5759489586aebfc83c541d74665dac9e2708d690902b950db0ef3f861eb7e2。原始系列SHA256 **9311950697691ffee529e27654c8cd71ec249d473d31d660d3c43f0d85b078f2**。最终完整文件摘要见[DIGESTS.json](DIGESTS.json)。

## 实际资源观察与清理

[measurement-01/f27-resource-series.json](measurement-01/f27-resource-series.json)保留原单文件`lingxi.r05-f27-resource-series.v1`并增加serviceTree/files/cycleResults/ownerResourceSeries；原producer仍从原打印路径复制同一文件。54个二进制采样点：baseline1、cancel-live6、cancel6、error6、ok3、worker-live15、worker-released15、worker-final1、post-restart1。cycleResults逐轮160条，分布60/60/15/10/15。服务RSS27024–37920KiB，完整父子树RSS27024–40496KiB、FD15–22。15轮真实aux HTTP屏障时每树service+worker恰2，放行且实际worker结束后每树恰1；各PID、RSS、FD、ESTABLISHED连接均在原始序列。存活连接每轮2、释放每轮0；仅排除本次ps采样器自己的精确PID，未忽略所有ps或不认识的后代。服务/worker与测试装备进程分别记录。

预算drop后原事实为cancelSettled0、cancelDanglingActive60、errorRuns60；保留重启恢复的真实边界，不把running行说成普通取消已终结。Stub总命中368包括原二进制负载323和新增owner补证45；drop60。不能把368全部解释为原160轮。

ownerResourceSeries用真实严格配置、CredentialService、ConfigModelGateway和生产组合根在测试进程内补证，15轮真实worker与排队兄弟，61点（baseline1、峰值15、稳态45）。峰值每轮activeSessionRuns/liveRunIds=2、modelPermit1、toolPermit1、modelWaiter1、toolWaiter0；取消排队兄弟并join，放行worker并join后，连续3个100ms窗口任务/permit/等待0，liveRunIds和backgroundIds空，worker树回1/TCP0。读取的是现有资源对象；这份进程内实际owner证据与独立正式服务二进制进程树分列，不能宣称已经从二进制内部直接观测全部任务或所有Tokio后台线程，也不能替代二进制边界。

home/workspace每点递归完整路径+bytes，遇缺目录/读取失败拒绝填空。负载稳态没有.tmp残留；数据库、WAL/SHM、凭据/身份、lock、日志、两件workspace输入按实际清单保留，没称它们泄漏。文件数与完整日志清单同时暴露F46，详见下一节。真实service exit/reap、owner close、stub结束、隔离home/workspace/config成功删除后才写cleanup true；最终服务PID55703、exit0。真实产品取消中媒体产物释放另复用具名既有证据，见I-MAPPING；本聊天/worker负载没有生成媒体，不用删除测试目录代替产品媒体回收。

## F46：必须保留的真实失败

旧测量完整raw中post-restart及owner released-steady持续存在service-000002.log、service-000003.log、service-000004.log、service-000005.log（不是全时段并集，不是已清除的暂态），原界限3；cleanup前清单分别65355、65358、33642、0 bytes。旧测试只在worker-final检查数量3，故measurement01 exit0只代表旧断言，通过文案不能升级整项结论。最后测试已在post-restart及每个owner released-steady新增同一≤3断言；最终源编译与格式/clippy通过，但故障未修，**最终源160完整运行未执行**。

[f46-directed-red-01/result.json](f46-directed-red-01/result.json)在同一全新home连续4次真实生产debug服务启动、READY、SIGTERM与reap，参数--log-max-files3/--log-max-bytes65536，无注入logging/provider状态；PID80590/80595/80597/80598，实际清单1→2→3→4，第四次service-000001至000004.log均0 bytes，准确断言FAIL，脚本exit1。4个服务本身都exit0，全部reap且home删除；不能把脚本红说成服务崩溃。完整四条二进制命令/UTC/文件清单在result，输出在f46-directed-command01。

只读定位：`rust/crates/lingxi-service/src/logging.rs::RotatingLogFile::open`目前119行先prune、120行open_current，已有prune合同明确总数包含active文件；先保留已有3再新增1符合观察，rotate路径顺序不同。这是疑似生产根因供新实施者确认，未修改该文件。不宣称无限泄漏或性能回归；这是原3上限的有限启动偏差且属于必需项。

## 测量器已知保留/释放与负控

原controls测试identity不变；完整原始controls JSON在measurement01/stdout与restored-controls01/stdout。已知额外3FD精确3→6，alivehelper非零RSS/≥3numericFD，kill+reap与假PID拒绝；真实TCP双端保留2→释放0；已知3个17B.tmp文件→空、缺目录拒绝；进程树alive/dead与TCP空/错PID拒绝。所有ps/lsof验证退出、PID身份与所需字段，失败保持UNKNOWN/报错。

两个独立负控只在其子进程PATH前置证据目录lsof包装器，未改系统工具/生产源码：negative-fd-zero删除numericFD记录，实际exit101，报“invalid live-process sample”/0FD不可信；negative-tcp-zero仅删除ESTABLISHED记录、FD透传/usr/sbin/lsof，实际exit101，已知保留双端检查left0/right2明确红。各自calls.jsonl记录参数/PID，cleanup.json核对全部辅助进程结束，未误杀未知进程；随后无包装器restored-controls01 1passed/0ignored/1filtered、exit0。负控的FAILED是预期侦测证据，不能充作产品测试通过。

## 普通取消既有有效证据

RR2/D-R2/workspace-green-window-REVIEW-r1.log中subagent_closeout上述精确测试1922行ok，late_result_fence上述精确测试1207行ok；本轮又分别重跑，1passed/0ignored/7filtered、exit0，和1passed/0ignored/4filtered、exit0。普通取消的实现/同实例恢复/晚到结果隔离已有，RR3 I-MAPPING引用本轮实际记录；不重报缺实现，也不以预算drop+重启替代普通取消。

## 全部实际命令与结果

以下UTC、退出码和cargo汇总直接来自各command.json；完整vector、before/after输入与stdout摘要见链接。run_command.py是证据本地捕获器，不属于生产producer。fmt/clippy不是资源负载验收；测量336.81s是测试执行时间，外层UTC还包括编译/等待锁。各次filtered是精确选测产生，没有ignored。

| 记录 | 开始UTC | 结束UTC | exit | 原始汇总 |
|---|---|---|---:|---|
| build-02 | 2026-10-07T00:20:20.794039+00:00 | 2026-10-07T00:20:26.197084+00:00 | 0 | 无 test result；见原始输出 |
| clippy-01 | 2026-10-07T00:23:24.225133+00:00 | 2026-10-07T00:25:19.978536+00:00 | 0 | 无 test result；见原始输出 |
| clippy-final-01 | 2026-10-07T00:32:55.619297+00:00 | 2026-10-07T00:32:57.892768+00:00 | 0 | 无 test result；见原始输出 |
| f46-directed-command-01 | 2026-10-07T00:32:44.207070+00:00 | 2026-10-07T00:32:45.454326+00:00 | 1 | 无 test result；见原始输出 |
| fmt-01 | 2026-10-07T00:23:08.577199+00:00 | 2026-10-07T00:23:09.683523+00:00 | 0 | 无 test result；见原始输出 |
| fmt-final-01 | 2026-10-07T00:32:54.588546+00:00 | 2026-10-07T00:32:55.564953+00:00 | 0 | 无 test result；见原始输出 |
| measurement-01 | 2026-10-07T00:21:05.622143+00:00 | 2026-10-07T00:27:23.651377+00:00 | 0 | test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 336.81s |
| negative-fd-zero-run | 2026-10-07T00:23:52.725889+00:00 | 2026-10-07T00:25:20.562018+00:00 | 101 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.49s |
| negative-tcp-zero-run | 2026-10-07T00:25:40.479981+00:00 | 2026-10-07T00:25:41.788764+00:00 | 101 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out; finished in 1.16s |
| ordinary-cancel-late-result-01 | 2026-10-07T00:28:22.947480+00:00 | 2026-10-07T00:28:41.905944+00:00 | 0 | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.06s |
| ordinary-cancel-subagent-01 | 2026-10-07T00:27:38.259646+00:00 | 2026-10-07T00:27:53.326679+00:00 | 0 | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 0.10s |
| restored-controls-01 | 2026-10-07T00:26:07.485898+00:00 | 2026-10-07T00:26:08.647945+00:00 | 0 | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 1.00s |

### build-02

```sh
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources --no-run
```

[逐条输入/退出码/摘要](build-02/command.json)；[原始输出](build-02/stdout.log)。

### clippy-01

```sh
/Users/study_superior/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources -- -D warnings
```

[逐条输入/退出码/摘要](clippy-01/command.json)；[原始输出](clippy-01/stdout.log)。

### clippy-final-01

```sh
/Users/study_superior/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources -- -D warnings
```

[逐条输入/退出码/摘要](clippy-final-01/command.json)；[原始输出](clippy-final-01/stdout.log)。

### f46-directed-command-01

```sh
python3 artifacts/rust-tauri/R05/RR3/C-01/f46_reopen_probe.py
```

[逐条输入/退出码/摘要](f46-directed-command-01/command.json)；[原始输出](f46-directed-command-01/stdout.log)。

### fmt-01

```sh
/Users/study_superior/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check
```

[逐条输入/退出码/摘要](fmt-01/command.json)；[原始输出](fmt-01/stdout.log)。

### fmt-final-01

```sh
/Users/study_superior/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check
```

[逐条输入/退出码/摘要](fmt-final-01/command.json)；[原始输出](fmt-final-01/stdout.log)。

### measurement-01

```sh
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources -- --nocapture --test-threads=1
```

[逐条输入/退出码/摘要](measurement-01/command.json)；[原始输出](measurement-01/stdout.log)。

### negative-fd-zero-run

```sh
PATH=/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/C-01/negative-fd-zero/bin:$PATH /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources f27_sampler_controls_detect_growth_release_and_failure -- --exact --nocapture
```

[逐条输入/退出码/摘要](negative-fd-zero-run/command.json)；[原始输出](negative-fd-zero-run/stdout.log)。

### negative-tcp-zero-run

```sh
PATH=/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/C-01/negative-tcp-zero/bin:$PATH /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources f27_sampler_controls_detect_growth_release_and_failure -- --exact --nocapture
```

[逐条输入/退出码/摘要](negative-tcp-zero-run/command.json)；[原始输出](negative-tcp-zero-run/stdout.log)。

### ordinary-cancel-late-result-01

```sh
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test late_result_fence r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing -- --exact --nocapture
```

[逐条输入/退出码/摘要](ordinary-cancel-late-result-01/command.json)；[原始输出](ordinary-cancel-late-result-01/stdout.log)。

### ordinary-cancel-subagent-01

```sh
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test subagent_closeout parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap -- --exact --nocapture
```

[逐条输入/退出码/摘要](ordinary-cancel-subagent-01/command.json)；[原始输出](ordinary-cancel-subagent-01/stdout.log)。

### restored-controls-01

```sh
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources f27_sampler_controls_detect_growth_release_and_failure -- --exact --nocapture
```

[逐条输入/退出码/摘要](restored-controls-01/command.json)；[原始输出](restored-controls-01/stdout.log)。

## 尚待事项与下一命令

C与F46未联合独立验收。下一位实施者按RR3_F46_BRIEF仅修生产logging与必要内联回归；若接管C测试先让总控登记。本包释放两个测试文件所有权供总控协调。必须保留3、两原测试身份、160轮、失败UNKNOWN/拒绝逻辑。定向probe脚本当前硬编码历史输出目录且使用mkdir防覆盖，应复制到新F46证据目录并改新输出路径后重跑，不能覆盖本轮红。

修复后二进制重新构建及新证据目录就绪，再由全新联合验收者执行完整原资源命令并保存原始系列：

```sh
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t08_resources -- --nocapture --test-threads=1
```

全阶段producer与final gate由总控和全新独立终审运行；本包没有跑全阶段、发布优化/R10两小时1000任务对比、其他平台/真实商业供应商，不宣称性能收益或阶段可交付。DEBUG macOS服务资源正确性证据和进程内owner补证适用范围已分明。资源采样的未被本负载覆盖对象仍是边界，不以UNKNOWN填0，不扩大为证明任意负载无泄漏。
