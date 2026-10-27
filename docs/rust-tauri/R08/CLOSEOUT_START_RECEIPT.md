# R08 收尾开工回执（closeout 会话）

1. HEAD=aa99d2ec1dae7f0aef5971bab049734f22fcf7d8（=审查基线，工作区干净）；分支 codex/rust-tauri-migration。
2. `python3 -B scripts/rust-tauri/r08_stage_inputs.py --check` → `inputs digest match`（exit 0；仅证输入摘要一致）。
3. 最新正式回执 verify-r08-final-15：命令 17/17、场景 16/16 机器 PASS；叶 20 PASS / 106 required_behavior_pending；candidateSourceBinding.stable=false、overall=FAIL（待证缺口如实表达）。
4. 阶段图登记 18 命令，其中 r08_a04_no_final_ws 未被任何场景引用（门不执行）；r08-a04-card-expansion.test.tsx 不在任何套件 argv 中。
5. 工具链核对：Node v24.16.0；rust-toolchain.toml 锁 1.98.1，须经 rustup（~/.cargo/bin 前置 PATH），Homebrew cargo 1.93.0 不读取锁定、禁用于本仓。
6. 外置任务书：未发现独立文件，本轮 /goal 提示词即任务书；R00 原账（ACCEPTANCE_MAP.json / FEATURE_STAGE_ACCEPTANCE.json）与 R08.json/回执已读。
7. 106 待验收叶分布实测：D01 聊天/WS 23、D02 会话 48、D03 工具/MCP 18、D04 设置 11、D13 终端 3、D20 CLI 3（全部 R00-T02-LA-* 叶）。
8. 本轮范围：任务A（验收基础设施）→B（恢复链）→C（客户端语义）→D（126 叶补证）→正式门 verify-r08-closeout-01；不发布、不合并 main、不迁真实用户数据。
