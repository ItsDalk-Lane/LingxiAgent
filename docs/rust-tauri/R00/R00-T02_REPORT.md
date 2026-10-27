# R00-T02 第十版修复候选交接

**状态：R9-F01 的修复候选已生成，待全新 R10 审阅者独立裁决；本报告不判 R00-A03/A04 或 R00-T02 PASS。** 分支 `codex/rust-tauri-migration`；Task Base/HEAD 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`。未改产品源码、任务书、总控账本、R1–R9 独立审阅或根因报告；未提交推送。用户原工作区的旧任务书删除及总控进度改动保留原状。

## 当前候选

- 832 个生产登记、736 个保留 F-ID（HTTP 491、其他 245）、736 个一对一补充场景、24 域；四组归属结构差集为空。所有场景仍为 `SPECIFIED_NOT_EXECUTED`。
- 五条供应商叶的 G2/G3 正源、生成库存与逐叶场景已区分权威 `provider-catalog.json`、条件性本地 `provider-plugins`、条件性 `models.json` 投影、焦点助手 YAML 刷新和内联凭证后续请求助手 YAML 保存。纯供应商请求不进入请求助手显式保存分支，但成功刷新可重写焦点 YAML；失败时目录可能已变，不宣称原子回滚。`PUT /api/config` 的 null 项归 remove 叶，仅另含全局字段时写 `preferences.json`。
- `r00_t02_source_gates.py` 在 F-ID 之外核对五叶结构化存储条件、正源/生成件的结果与失败断言、15 个配置请求样例，以及现役源码中目录→模型刷新→无 ID 焦点刷新→请求助手显式保存的调用顺序。R10 反例包括目录缺失、任何 YAML 均不写、A/B 混淆、插件或模型必写、内联凭证漏 A YAML、另一条路由 remove 错挂 save、刷新失败谎称回滚；原 R6/R7/R8/R9 反例保留。详见 [R00-T02_R10_REPAIR.md](R00-T02_R10_REPAIR.md)，其 SHA-256 为 `6b84b6e47a2f76651baed75edde512db37c8881d1f7e6ab4ef0cc878cffb0fba`。
- UI 矩阵仍为 26 页、177 动作、718 条逐叶源码事实；Agent/Providers 页面未被本轮修改。旧整文件 SHA 不适用于新候选，旧页面审阅只能用于未变化子树，不能证明五条供应商叶已通过 R10 独立审阅。

## 检查

| 命令 | 实际退出码 / 结果 |
|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --write` | 0；832 登记、24 域、四组结构差集为空 |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py` | 0；`CHECKS_OK` |
| `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --negative-checks` | 0；R10 正例接受、14 个 R10 错例输出 `NEGATIVE_DETECTED`，原反例保持触发；日志 `/tmp/r00_t02_r10_negative.log` 不在冻结候选内 |
| `python3 -B -m py_compile docs/rust-tauri/R00/r00_t02_inventory.py docs/rust-tauri/R00/r00_t02_source_gates.py` | 0 |
| `node --check docs/rust-tauri/R00/ask_user_ast_gate.cjs` | 0 |
| `npx vitest run tests/config-scope.test.ts tests/provider-catalog.test.ts` | 0；2 文件、21 项通过 |

这些是静态候选和现役代码的定向验证；未执行 Rust/Tauri 实装、真实供应商、跨平台安装包或旧用户数据演练。`R00-A03/A04` 和 `R00-T02` 的 R10 裁决须由未参与修复者给出。

## 冻结指纹

以下 **20 份非报告候选**按文件名排序，每行 `SHA-256␠␠文件名\n` 再取 SHA-256，聚合值为 **`972317079442203a8da67609ddcfc8661b304361934fcbd9db999be0c5244ad1`**。独立只读审阅、R10 修复说明和本报告均不参加聚合。

| 文件 | SHA-256 |
|---|---|
| `ENTRYPOINT_COVERAGE.json` | `76fe6e2b9aaae914aaff21be6f731ba8d537b944ced6412ebca4f547c8313000` |
| `EXCLUSIONS.md` | `df55a8a4d6183626c00ac27e6c0452fb20cdd3cfe781c45d2777fbff06900666` |
| `FEATURE_INVENTORY.json` | `48141268281fcb854f1674d53f526c5a6149e20504c8a2c818f3ed2abed11039` |
| `FEATURE_STAGE_ACCEPTANCE.json` | `699ef808d35d0185681e712372cf6d6585caf0e3958daab646586f0b355072a9` |
| `R00-T02_BRANCH_CLOSURE_AUDIT.json` | `07fb8db02e9cbb71e72c69f13ea8c40e265a00f11aea3ee7eb84404fce0584cb` |
| `R00-T02_BRANCH_SEMANTIC_CONTRACT.json` | `e3f097eb42113b432c01c973f4bfad33711adb886f8311d889e1469b5107459c` |
| `R00-T02_NONHTTP_AUDIT_CORE.json` | `c43a8c288b3762fd5a2b65c07af0759600c3fbfd5022cf9c35b6578aec7fde5c` |
| `R00-T02_NONHTTP_AUDIT_SPLITS.json` | `fa7c3e36be0119cf4af93ad7c5fd57081e4e534bdf529fa425bd2211bcfcda10` |
| `R00-T02_NONHTTP_AUDIT_TOOLS.json` | `92f1b720330cccf967981f10fa3b09b6be02bef2ab6a9be4c36ab1ea7bc16971` |
| `R00-T02_NONHTTP_AUDIT_UI.json` | `491ab3289b180d9ec698ebdbc3245e0d4a581158d7326688db5a71ea3189810c` |
| `R00-T02_R8_CLOSURE_LEDGER.json` | `da2586cc99d02710bb9444903fc21d585f37612ddfbeba507a2d35dc67999d7b` |
| `R00-T02_SEMANTIC_AUDIT_G1.json` | `41f3d6672fe3e750ccf454cd27210f887b4f34a3791babf17b464eba1312e5e3` |
| `R00-T02_SEMANTIC_AUDIT_G2.json` | `7be7d4686b686046b2d5c1a8f4cc9f0ad840ddb51e0d1d283e4b1a6de766f7e2` |
| `R00-T02_SEMANTIC_AUDIT_G3.json` | `10bf3ef49268654d8a59f18b489e9ece1fc455984ef1caeab67d7892bd96b735` |
| `R00-T02_SOURCE_BRANCH_AUDIT.json` | `b35fa70570907e355f006f43d3820fc5f34a594598c11f70624ad3c2975e6e7f` |
| `R00-T02_UI_ACTION_MATRIX.json` | `2b54d59d4a9d1217eac6e6654aee0229517d657346c11adaf1f4819ee26d311a` |
| `R00-T02_UI_SOURCE_BRANCH_CONTRACT.json` | `85376ca2848ccb5d3d2d5e7489f7b61a974569ec8fe31135ae1bbfac8729b20d` |
| `ask_user_ast_gate.cjs` | `637b325dd2b11653b7e452d255c214b8e9440fcee83eb706f488eba9c563619d` |
| `r00_t02_inventory.py` | `b95e4bb800bb95beab06363cd34267d8a999a806d3438b1d4c5e98fbb301f97d` |
| `r00_t02_source_gates.py` | `1c54e90af08c1ae145253b00e6033ac747782e2e0c785c7b6d9377308ba0813c` |

本报告不自引用；候选改动后须重新计算。
