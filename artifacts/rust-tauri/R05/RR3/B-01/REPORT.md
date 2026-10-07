# RR3 工作包 B — F45 / R05-GATE-N03 实施与自检报告

状态：**SELF_CHECKED / READY_FOR_INDEPENDENT_REVIEW**。实施者 `/root/rr3_b_impl_01`，未作独立验收结论。时间见各 receipt（UTC；本机时区 Asia/Shanghai）。候选 HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`，使用该 HEAD 加创建副本时的未提交工作树 overlay；其他工作包仍在推进，最终集成候选须另行冻结、独立复验。未 commit/push，未写总控台账。

## 结果与根因

旧脚本把 `svc:r05_t01_model_plane` 固定写成 7→6；当前权威 `r05_stage_pins.tsv` 记录为 24，故旧 sed 零匹配、没有实际注入。RR2 G 在隔离副本做过有效 24→23、历史原16项16/16依然保留；该历史记录不代表旧主树脚本可复跑。本轮修的是 **F45：N03过期锚点可复跑缺失**，不改权威表、镜像期望、生产实现或既有计数。

改动三文件：

- `scripts/rust-tauri/r05_t08_negative_gate.sh`：N03调用动态注入器；成功注入才运行目标镜像；红必须出现目标套件、drifted、具名 FAILED 与实际1失败/0忽略；逐字节还原并精确运行镜像确认1/1绿。新增 `[EVIDENCE_DIR] --case N03`，默认仍运行N01–N16；新汇总检查实际身份集合完整且唯一，单项结果明确其他15项未运行。隔离副本改为每次新建唯一目录，避免覆盖历史副本或并行运行副本。
- `scripts/rust-tauri/r05_t08_mutate_pin.py`：权威表唯一目标解析；从当前数值取 old、新值为 old−1；只替换计数字段一次，保留其他行及空白；保存 old/new/行号/matches=1/mutations=1/前后SHA256。零匹配、重复/冲突匹配、非法数字或不能继续降低的计数直接非零且不写目标表或成功receipt。没有硬编码24。
- `scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py`：实际调用注入器验证当前数及另一数值、缺失/重复/冲突/坏格式/最低数；从主脚本逐字提取汇总函数验证单项、空集、错项、重复项、坏判定、完整16项与缺项。

## 真实执行与退出码

| 执行 | 退出码 | 实际观察 / 证据 |
|---|---:|---|
| `bash -n scripts/rust-tauri/r05_t08_negative_gate.sh` | 0 | `bash-n.log` / `bash-n-exit-code.txt` |
| `python3 scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py`（最终版） | 0 | **15项检查**均符合预期；拒绝腿实际exit1，注入与合法汇总腿实际exit0；`selfcheck-2.log` |
| 主脚本首次 `… B-01/n03-rerun --case N03` | 1（未接受） | 动态24→23真实发生，镜像真实exit101；我新增精确名称误写 `stage_map::tests`，实际应为 `stage_map::map_tests`，红点名判BAD、还原零匹配被严格拒绝。完整失败证据保留于 `n03-rerun/`、`console.log`，未冒称有效结果 |
| 主脚本修正后 `… B-01/n03-rerun-2 --case N03` | **0** | 正常 **8 passed / 0 failed / 0 ignored / 109 filtered**；动态24→23恰一次；目标镜像 **0 passed / 1 failed / 0 ignored / 116 filtered，exit101**，点名套件与drifted；逐字节还原后精确镜像 **1 passed / 0 failed / 0 ignored / 116 filtered，exit0**。`case-results.json`=`scope:N03, cases:1, allRefused:true, controlsGreen:true`，其他15项明列未运行 |
| `python3 artifacts/rust-tauri/R05/RR3/B-01/phase-driver.py` | **0** | 额外亲跑精确正常→注入→目标红→还原绿四腿，退出码 **0/0/101/0**；保存每腿命令、时间、来源/输入清单、lock/fixture/binary/log SHA256；红腿只改变权威表输入，恢复腿输入摘要完全相等。`binary-bound-phases/result.json` |

真实目标命令（所有cargo使用绝对路径1.98.1、`--locked`、离线模式）：

```bash
/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p xtask --bin xtask stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites -- --exact
```

镜像从隔离副本的真实TSV读取输入。红失败为断言 `the R05 pin table's suite registrations drifted (dropped, added, or re-counted)`，左右套件集明确显示 `svc:r05_t01_model_plane` 为23与24；没有编译失败、空过滤或提前失败冒充目标失败。

## 来源、隔离与边界

成功副本：`/Users/study_superior/r05t08-work/negcopy.Gz0rkR`；首次失败副本：`…/negcopy.tV1ROh`。采用只读本地clone + rust/scripts/docs overlay，主树无故障注入。自检锚点变异仅用 TemporaryDirectory。成功副本表已还原，与其 pristine及主树表逐字节相同；三个B文件也与主树逐字节相同（`candidate-summary.json`）。

每腿源清单覆盖副本 rust（排除target）、scripts/rust-tauri、docs/rust-tauri、contracts、rust-toolchain.toml；按排序JSON计算输入摘要。正常与还原均为 `b9215837486197e6be72304fc20410d488368a9ad7429055f6557d63d67fd57f`，变异为 `f3dbc9f413721bb942f5c60b8c6736e2e9f664b9bcbc5f773392232844b3e8f7`，唯一差异 `docs/rust-tauri/R05/r05_stage_pins.tsv`。二进制三腿同SHA256（运行时读表，因此不需要改二进制）：`52b03f6097b5ab8a4de9d8a34bb93bc0efb26a4a816e034a5930a04e873e5452`；Cargo.lock=`259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3`。完整细项见 `binary-bound-phases/*-receipt.json`、输入清单与 `evidence-sha256.json`。

本轮受影响的N03已真实重验，新增汇总逻辑已做合法/缺失/重复/空集自检。**没有重新注入其他15项，不把历史16/16覆盖或冒称本轮16/16**。RR2原有效16/16保留于 `artifacts/rust-tauri/R05/RR2/G-R2/NEG-GATE-RR2.md` 与 `negative-ev/`。未执行完整workspace/正式阶段门禁/LIVE/其他平台，F45自检不表示R05阶段放行。

## 交给新的独立验收者

下一命令：

```bash
python3 scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py
bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/RR3/B-REVIEW-01/n03 --case N03
```

验收者须新建自己的证据目录、实际运行，核对old/new/恰一次变异/具名目标红/恢复1/1绿，另以缺失与重复锚点核实拒绝、不写成功receipt；确认默认16项身份保留、单项不冒称其他15项PASS。实施者不自签独立结论。
