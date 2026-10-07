# R05_REPORT — 模型协议、凭证、流式处理与完整任务闭环（RR1 修复轮版本；§10 为 RR2 增补）

- 生成：R05-T08-执行者（RR1 WP-T08），2026-10-06。阶段：R05（T01–T08 全部八任务，RR1 对抗修复轮 F01–F39 之后）。§10 由 R05 RR2 收口（WP-F）于 2026-10-06/07 增补——RR2 轮现状与阶段状态以 §10 为准，§1–§9 保留 RR1 轮历史原文。
- 基线：分支 `codex/rust-tauri-migration`，HEAD `d80737b6cb9186c8a18c0f35923aac00249d45c3`（R05 首轮实施提交）；**RR1 全部修复为该 HEAD 之上的未提交工作树改动**（含 T01–T07 各工作包与 T08 本包；无 commit/push 授权，全程零 git 写操作）。
- 审查事实：首轮实施（HEAD d80737b6）被 2026-10-04 对抗审查判 **NOT_ACCEPTED**（F01–F28 问题矩阵，见 `docs/rust-tauri/R05/repair-current/RR1_ISSUE_MATRIX.json`）；本报告描述 RR1 修复后的候选状态。首轮报告的全部 PASS 记录是**同一审查基线上的历史事实**，但按 N16 规则（生产源码/协议/配置/lock/生成器/gate 变化 ⇒ 受影响证据过期），其结论**不继承**为 RR1 候选的门禁通过——最终门禁与阶段审查必须在 RR1 候选上重跑。
- 本报告只陈述真实运行过的命令与结果。

## 1. 阶段状态（§9 格式；RR1 候选当前真值）

```text
offline_gate:            PENDING（RR1 候选的最终 verify-stage R05 待跑：F24–F27 修复已自检绿，阶段门禁由收口在冻结候选上执行；首轮 PASS 见 §5 历史，已按 N16 过期）
independent_review:      PENDING（RR1 轮独立审查未开始：T01–T07 已逐任务独立验收 PASS，T08 与阶段终审待全新审查者）
live_verification:       BLOCKED_NOT_AUTHORIZED（RR-BLK-CREDENTIALS，最迟 R10；R05_LIVE_VERIFICATION.json）
platform_verification:   macOS arm64 = RR1 各包自检/验收实测平台；Windows = 未验证（继承 R04）；Linux = 未真机验证
stage_readiness:         READY_FOR_INDEPENDENT_REVIEW 的前置＝阶段门禁通过（材料就绪；本报告即按该状态整理）
release_state:           NOT_IN_SCOPE
R06_READY:               false（待阶段门禁+独立终审）
```

## 2. RR1 修复概要（按工作包；逐项详见 repair-current/RR1_ISSUE_MATRIX.json）

- **WP-T01（F01/F02/F29）**：逐模型能力声明与发送前检查（tools/image 三态、零请求拒绝）；配置代次/401 重试一致解析（冻结 route+材料，端点永不收新钥匙）。INDEPENDENT_PASS。
- **WP-T02（F03/F04/F05/F30/F31）**：凭证实例 epoch/handle 绑 principal、六 OAuth 独占叶服务端闭环（login.rs 一次性事务）、脱敏次序+编码变体+宿主边界。INDEPENDENT_PASS（F31 为叶偏差登记，随 F25 消费）。
- **WP-T03（F06–F10/F32）**：Anthropic 占位签名/空文本、Google functionCallPart 锚点+并行分组、DeepSeek reasoning 载体中心化 fail-closed、TurnOrigin 三入口强制、Responses/Codex 原位顺序。INDEPENDENT_PASS。
- **WP-T04（F11/F12/F13/F33）**：整批准入（batch_admission，schema/ID 冲突零副作用）、逐族终结严格化（过程内容不产 final）、normalize_final_message 单一投影（实时/final/快照/持久化/重启同源）。INDEPENDENT_PASS。
- **WP-T05（F14/F15/F16/F35）**：NetworkPlane 统一策略（system/manual/direct、NO_PROXY+环回 bypass、显式 CA）、egress 解析候选钉住、错误 body 有界+全程 deadline。INDEPENDENT_PASS。
- **WP-T06（F17–F20/F36/F37）**：附件 canonical ResourceScope+no-follow 读取、媒体任务双身份+路由快照、settle 三 fence、system-speech 统一监督（真实 macOS /usr/bin/say）、say argv 注入修复。INDEPENDENT_PASS。
- **WP-T07（F21/F22/F23/F38/F39）**：调用事实五态记账（not-sent attempts=0、取消臂落行、abandoned RAII）、usage 严格解析+诊断不回显、Gemini thoughts 口径、fence 臂永久钉子。INDEPENDENT_PASS（R3 任务级验收）。
- **WP-T08（F24/F25/F26/F27，本包）**：
  - **F24**：正式组合根四工具（read/write/edit/exec_command+write_stdin）+ 受控单操作 worker（config `workers` 封闭节→register_worker_tool 生产调用点+Available 居住化+`--model-global-permits`）；T08-C11 嵌套链（permit=1、3 真实 HTTP、父子 usage JOIN、env 白名单无密钥、SIGTERM worker 回收）全部走正式二进制，零 ServiceDeps 注入。
  - **F25**：独占叶防线（share 且无后续阶段=FAIL+单测红绿双向）；六 OAuth 独占叶按 full_original_behavior 逐断言钉 rr1_f04 具名测试（leafcase 测试级行）；scope 矩阵 R4 full 化；130 叶逐项重核。
  - **F26**：r05_required_cids.tsv 权威登记册（103=100 原始+3 追加）；assembler 预跑+xtask 镜像双处精确集合校验；三表重建（27 套件 362 集成测试全量唯一归属；93 cid+10 command 绑定）；变异四例红。
  - **F27**：采样器（lsof -F fn 机器可读+p 记录身份+退出码；失败=UNKNOWN 非 0）+正负对照（+3 FD 精确可见/kill+reap 后报错）；120 次取消/错误负载（预算看门狗取消 60/60 连接回收、60 次 500 错误、15 正常、10 长响应、15 嵌套 worker、双会话；重启 recovery 消解全部 dangling 行且零重执行）；原始时间序列（lingxi.r05-f27-resource-series.v1）随门禁落证据目录。
  - **F28**：本报告及配套文档的 RR1 真值收口（见 §6）。

## 3. 变化与影响面（RR1 增量）

- 新增生产模块：`lingxi-adapters/src/models/{batch_admission,network}.rs`；`lingxi-service/src/credentials/login.rs`；组合根（lib.rs）正式注册 process tools 与 config 声明 worker；`--config` 闭合键集新增 `workers`；CLI 新增 `--model-global-permits`。
- 新增测试套件（纳入 pin 表）：service `r05_t05_network`(21)/`r05_t06_rr1_media_resource`(17)/`r05_t06_rr1_system_speech`(10)/`r05_t07_rr1_usage_ledger`(15)/`r05_t08_production_tools`(6)/`r05_t08_resources`(2)；adapters `r05_t03_rr1_replay`(33)/`r05_t04_rr1_batch_terminal`(9)/`r05_t07_rr1_usage_strict`(7)。
- 存储迁移：v6（台账五列）→v7（transport_attempts nullable，旧行原值保留）；migration 测试随套件。
- 门禁侧：`r05_required_cids.tsv`（权威登记册）、`r05_t08_rebuild_tables.py`（三表重建）、leafcase 测试级行、xtask 镜像 8 项（含精确 CID 集合、独占叶防线、scope 矩阵核验）。
- 旧产品零改动维持：RR1 未触 `desktop/ shared/ package.json`（工作树 diff 可核）。

## 4. 验收与映射（RR1 重登记后）

- 附录 A 16 个 A-ID 全部保留（阶段图 REQUIRED 场景）。
- 附录 B 103 个 C-ID：**93 个由 cid 表逐测试唯一归属**（含 RR1 新增的 T05-C12 代理/CA 与 T08-C11 worker 嵌套链——首轮为 NOT_RUN/场景级的两项现均有 cargo 实测）；**10 个 command 绑定**（T01-C01/C12、T05-C04/C06/C09/C10、T06-C11B、T08-C10/C13/C14——各指向已注册阶段图命令，共享有效一次执行，生产者见 R05_TEST_MAP.json）。登记册 `r05_required_cids.tsv` 由 assembler 预跑+xtask 镜像双处精确校验（换名/删除/伪造即红，变异已证）。
- 130 R00 绑定叶：6 独占叶 full_original_behavior（逐断言具名测试）、124 share 叶（laterShare 归属明确）、5 合法 deferred（R1/R2 规则）。
- 已登记延期：仅 LIVE 全家（RR-BLK-CREDENTIALS，最迟 R10）。首轮的 T05-C12 延期已由 F14 关闭（R05_BLOCKERS §3 历史记录）。

## 5. 门禁与命令（真实运行记录）

### 5.1 RR1 候选自检（本包与各工作包，rustup 代理 1.98.1）

| 检查 | 命令 | 结果 | 证据 |
|---|---|---|---|
| F24 套件 | `cargo test --locked -p lingxi-service --test r05_t08_production_tools -- --test-threads=2` | 6/6 ok（修复前同套件 6/6 FAILED exit 101） | `RR1/WP-T08-E01/old-red-f24-on-unfixed-tree.log` |
| F27 套件 | `cargo test --locked -p lingxi-service --test r05_t08_resources -- --nocapture` | 2/2 ok；原始序列落盘 | `RR1/WP-T08-E01/`（series 路径见测试输出） |
| xtask 镜像 | `cargo test --locked -p xtask --bin xtask r05_` | 8/8 ok | — |
| verify.rs 单测 | `cargo test --locked -p xtask exclusive_leaf` | ok（红绿双向） | — |
| 邻接回归 | closed_loop 11/11、binary_wiring 2/2、worker_model 10/10、r04_t02 13/13、r04_t08 10/10、config 单测 33/33 | 全 ok | — |
| 变异验证 | 隔离 /tmp 副本 7 例（换名 CID/删登记行/伪命令/伪 share/删 leafcase/assembler 预跑/采样器回退） | 逐例红+恢复绿，主树零接触 | §2 WP-T08 摘要与 RR1_ISSUE_MATRIX 各 testEvidence |

### 5.2 阶段生产者（27 套件）与最终阶段门禁

- `bash scripts/rust-tauri/r05_t08_stage_suites.sh artifacts/rust-tauri/R05/RR1/WP-T08-E01/R05_SUITES`：结果以 `R05_SUITES/summary.txt`、`r05-cases.json`、`leaf-cases.json`（137 案例含 13 测试级）、`required-cids.txt`（预跑身份校验）、`f27-resource-series.json`（原始时间序列）为准（本轮实跑记录）。
- 最终 `verify-stage R05`（含 R04→R03→R02/RR1 闭包与 16 负测+RR1 新增负测）在冻结候选上由收口执行；其结果回填本节后方可宣布 offline_gate。
- 首轮（HEAD d80737b6 前身 c549ff654 脏树）的 FINAL-WFR2-1 verify-stage PASS 与 82/82 生产者绿为**历史记录**（`FINAL-WFR2-1/verify-R05/`、`T08-E01/verify-R05/`），按 N16 因生产源码与 gate 变化不继承为 RR1 结论。

## 6. 文档一致性（F28 本轮收口）

- 本报告、R05_HANDOFF.json、R05_TEST_MAP.json、R05_PERFORMANCE_RESULTS.json、R05_NEGATIVE_GATE_REPORT.md 已更新为 RR1 真值（PENDING 状态如实、延期仅 LIVE、首轮 PASS 标注过期不继承）。
- R05_ACCEPTANCE_LEDGER.json：T05-C12 由 NOT_RUN 改为 RR1 实测（r05_t05_network）；T08-C11 由场景级改为 cid 拥有（r05_t08_production_tools）；其余条目证据指向以 R05_TEST_MAP/三 TSV 为准。
- ORCHESTRATOR_PROGRESS.json 的回填与最终归档回执：**待阶段独立 PASS 后**由收口统一执行（总控 §3.3/F28 规则；不在本报告内预写）。

## 7. 未测与限定

- LIVE 秬实供应商（未授权，最迟 R10）；Windows/Linux 平台（未验证，继承）。
- 间歇环境项 R05-ENV-ALF-UNSIGNED-TEST-BINARY：`r00_management_leaves` LAN 腿（192.168.3.5）在部分窗口停驻（T03/T05/T07 轮均有登记）；处理不变：按台账登记，不放宽门禁、不改测试、不当回归修。
- RR1 期间各审查者登记的 LOW 级测试缺口（F32/F33/F34/F37/F39 同类）均已由后续轮落地为永久腿；当前矩阵无未落地登记项（F34 归 WP-T04-R3，非本包）。

## 8. 回退

- RR1 全部改动为工作树未提交差异；回退=丢弃对应文件修改（migration v6/v7 只增列不覆盖旧值；workers 为新增闭合键；--model-global-permits 为新增旗标）。

## 9. 交接

- 见 `R05_HANDOFF.json`（RR1 版：接口、R06 可消费范围、延期登记、审查状态）。

## 10. RR2 修复轮增补（2026-10-06/07；上方 §1–§9 为 RR1 轮历史记录，原文保留不改写）

- **候选**：分支 `codex/rust-tauri-migration`，HEAD `ad5ec4e9853a51ed929f1e2e077b97d41c951572`（=origin）+ 未提交 RR2 工作树改动；无 commit/push（收口后由总控按既有授权统一处理）。
- **RR1 终审遗留五项全部关闭（任务级）**，逐项独立验收（全新上下文审查者，非实施者）：
  - F41（A）：登记册补登 v6/v7+等式自检 S0；`A-R2/REVIEW-r1.md` PASS（指纹三方一致、S0–S4 全绿、三变异精确红）。
  - F42（B，两轮+微修复轮3）：运行输出归属重写+E5 分类器 patch-too-large 形态演化登记+note 转义修复；`B-R2/R2/REVIEW-r2.md` 四项（F42/F44/B-r2/B-r3）全 PASS；r1 验收 full 轮 E5 unparseable 复析为 vitest worker 负载抖动（非逻辑缺陷）。
  - F34（C）：永久腿+阳性对照+映射登记；`C-R2/REVIEW-r1.md` PASS（两种审查者自建变异恰新腿红）。
  - F43（D）：(b)(c) 修复 PASS + **rust_test_workspace 首次完整绿窗 exit 0（115 组 ok / 1476 passed / 0 failed，含 r00 1/1）**，`D-R2/REVIEW-r1.md`+`D-R2/workspace-green-window-REVIEW-r1.log`；实施期「绿窗 BLOCKED」文案已按事实刷新，ALF 环境项登记保留（重链接后的新二进制实例可能需用户再次 Allow）。
  - F31（E）：OAuth 列表面恢复 404；`E-R2/REVIEW-r1.md` PASS（38/38、404 body 逐字一致零存在性 oracle、变异红绿双向）。
  - F40+三追加 C 行（F，本收口）：T05-C11B/T05-C13 按 RR1 矩阵 F14/F15/F16 与 F08/F10/F14/F35 的 independentReview 翻绿（RR2 改动未触及 egress/compat/golden，证据未过期）；T06-C11B 以 D 绿窗关闭。
- **RR2 新发现 F44**（seal 三件套 ENOBUFS 遮蔽诊断）：4 文件纯 maxBuffer 选项修复，经 B 包 REVIEW-r2 §5 PASS；生成器 patch-too-large 由 B-r2 分类器登记处置（登记红不转正式绿）。
- **G-INT/G-NEG DONE**：I01–I11 全 PASS（`G-R2/I-MAPPING.md`，无新 F-ID）；原 16 负测 16/16 fail-closed+RR2 新增反例 6/6（`G-R2/NEG-GATE-RR2.md`）。
- **完整链与门禁窗口（RR2 候选实测）**：r02_t08 仓库根完整链 `s5-full-5` exit 0（GREEN，登记红如实）；workspace `cargo test --locked --workspace` exit 0（1476/0，D-R2 REVIEW 绿窗）。
- **资源口径（§四F）**：原始记录=160 混合轮=60 预算取消+60 错误+40 其他（15 正常+10 长响应+15 worker）；恢复前 active≠已终结、重启已消解≠永久泄漏（原始序列 `RR1/INDEPENDENT-9/verify-R05/R05_SUITES/f27-resource-series.json`，stubDropped=60）。
- **阶段状态（RR2 候选当前真值，§9 六元组口径）**：

```text
offline_gate:            PENDING（RR2 候选的正式 verify-stage R05 待 FINAL-01 亲跑；workspace 绿窗与 r02_t08 完整链为窗口级实测，不代位正式门禁）
independent_review:      任务级全部通过（六工作包+F44 独立验收 PASS、G DONE、三追加 C 行收口）；阶段级终审 PENDING（FINAL-01）
live_verification:       BLOCKED_NOT_AUTHORIZED（RR-BLK-CREDENTIALS，最迟 R10；不变）
platform_verification:   macOS arm64 = RR2 各包实测平台；Windows = 未验证（继承）；Linux = 未真机验证
stage_readiness:         READY_FOR_INDEPENDENT_REVIEW
release_state:           NOT_IN_SCOPE
R06_READY:               false（待 FINAL-01 阶段终审；终审通过并完成封印/提交流程前不得翻 true）
```

- **历史记录边界**：§1–§9 的 RR1 叙述与 RR1_INDEPENDENT-9 FAIL 结论为历史事实原文保留；本节不覆盖、不改动其内容。
