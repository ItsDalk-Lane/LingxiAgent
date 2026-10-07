# RR3 DELIVERY-REVIEW-01 — DELIVERY-FINAL-02 交付清单全新独立审查

- 审查者：R05 RR3 DELIVERY-REVIEW-01 全新空历史只读交付审查者；未参与任何准备/分类/实施/包级审查轮；未派子代理。
- 任务书：`docs/rust-tauri/R05/repair-current/RR3_DELIVERY_FINAL_REVIEW_BRIEF.md`（其中"DELIVERY-FINAL-01"指代本轮被审分类，实际对象为 `DELIVERY-FINAL-02/`；"磁盘很紧"约束已解除）；并全文/关键节读取 DELIVERY-FINAL-02 全部产物、DELIVERY-PREP-01/02 REPORT、DELIVERY-SPACE-01 REPORT+normalization-boundary.json、FINAL-04/STAGE_REVIEW、E-04/REPORT、E-REVIEW-05/REVIEW、F51/F52 迁移回执、最新 RR3_PROGRESS 台账。
- 边界：只写本 `DELIVERY-REVIEW-01/`；全程零 Git 写（主库只读、`--no-optional-locks`，未 stage/commit/push，未改 .gitattributes/配置/系统）；未删原件；未跑 Cargo/历史 driver；未读用户 home；Git 写实验只在隔离微型库（`/private/tmp/rr3-delivery-review-01/crlf-lab`，localOnly、不提交）。

## 一、结论

**PASS，无 mustFix。** 12 项亲验全部通过（§二–§五）；隔离 CRLF 实验证实精确归档方式成立（§六）；空间估算独立复算成立（§七）；正反控制 METHOD_VALIDATED（§八）。`include-paths.nul`（29,158）与 `crlf-no-filters-boundary.json`（163，--no-filters 方案）可交 root 按既有授权执行精确暂存；后续 20 件 DELIVERY-FINAL-02 自身产物（self-receipt/MANIFEST 登记面）由 root 暂存时以当刻枚举收口。4 条非阻断观察见 §九，均不构成 mustFix。本审查不预写任何 Git 动作成功。

## 二、权威枚举与覆盖（亲验）

- 亲跑（2026-10-08 UTC）：HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`（分支 codex/rust-tauri-migration）；untracked **35,061**、modified-tracked **29**、staged **0**（exit 全 0）。
- 与 FINAL-02 快照（35,041+29=35,070 并集）对账：**快照后新增恰 20 条 = DELIVERY-FINAL-02 自身产物**（与 self-receipt outputs 18 条 + `MANIFEST.json`/`self-receipt.json` 互注册设计完全吻合）；**消失 0 条**；29 M 集合逐路径相等（含 L 的 `rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs` 与 M 的 `stage_map.rs`、`stage_maps/R04.json`、`r04_t08_generate_stage_map.py`）。
- 指纹复算：`git ls-files -s` SHA256=`3016f7ae…`、`git diff HEAD` SHA256=`a38e5d7a…`，与 `git-initial-state.json`/收尾 self-receipt 逐字节相等；commands.jsonl 中枚举 stdout SHA 与落盘 `.nul` 文件 SHA 一致。
- classification.json：严格解析（拒重复键）35,070 行、路径唯一 0 重复；**行集 == 枚举并集**（extra 0 / missing 0）；include 29,158/2,490,626,419 B 与 localOnly 5,912/1,218,175,981 B 互斥且覆盖全并集；UNKNOWN 0；`include-paths.nul`/`local-paths.nul` 与分类集合逐路径相等；35,070/35,070 文件当前在盘。

## 三、抽样与类别依据（亲验）

- **include 侧 63/63 通过**（要求≥40）：INCLUDE_PRODUCTION 全 27（含 M 三文件+L 测试文件、B/H/I/J/A/C 新增白名单脚本、`r05_resource_sampler.rs`）+ CURRENT_DOC 8 + EVIDENCE 24 + SNAPSHOT 4，逐文件实读 SHA256/bytes/mode/type 与清单相等，basis/classificationSource 完备（继承条目标明 size+mtimeNs 身份，新材料条目标明目录所有权判定）。
- **localOnly 侧 15/15 通过**：五子类各抽 3，SHA/bytes/mode 相等（票据/token 仅核 SHA 不输出原文）。
- 新材料逐目录统计与 REPORT §三**逐项相等**：G-REVIEW-03 5,042（4,966 E+36 S+5 LOCAL_ONLY 票据+35 RUNTIME）、FINAL-01..04 32/43/991/980 全 EVIDENCE、E-04 16、E-REVIEW-04/05 8/10、F51/F52 四组 20/54/14/44、L 6/18、M 1,267/634、J-02 3,304（3,291+13 S）、J-REVIEW-01 597（+13 RUNTIME）、J-REVIEW-02 1,705（+6 RUNTIME）、G-REVIEW-02 299、PREP-01/02 48/36、SPACE-01 8、DOC-INPUT-BOUNDARY-01 14、DELIVERY-FINAL-01 6。
- 继承/新判账目：classificationSource 指向 PREP02 merged-paths 19,659 条，随机 25/25 size+mtimeNs 与 merged-paths.json（19,734 行）相等（身份成立）；非继承 15,411 = 新判 15,390 + 变更重判 21（7 协调文档+12 E-04 tracked 文档+2 H/I 脚本）；E-04 `changed-files.json` 12 份改动全部 INCLUDE_CURRENT_DOC，字节未变 2 份（WORKER_MODEL_BOUNDARY/INTERFACE_EVOLUTION）正确不在其中。
- **正式 FAIL/中断原证未被吞**（逐路径在 include）：FINAL-01/02/03 STAGE_REVIEW+STRUCTURED_SUMMARY（offline_gate=FAIL 历史）、FINAL-03/04 ATTEMPT1-INTERRUPTED 177/369 文件、FINAL-04 command-records/attempt1-partial-manifest-sha256.txt、G-REVIEW-02 全 299 条、G-REVIEW-03 default16-01 中断现场（2,072 条中仅 16 条运行态票据/token 依边界排除）、M-01 attempt-1-concurrent-writes（overall=FAIL 原因现场）、e5-candidate-npm-test 日志、RR2/FINAL-01 verify 输入摘存。

## 四、localOnly 边界与外置夹具（亲验）

- LOCAL_BINARY 61/61 魔数复核全为真二进制；`C-F46-REVIEW-01/isolated/build/liblingxi_service.rlib`（134,457,480 B）本轮**全量重哈希 = `ef146b51…`** 与登记相等。
- 运行态材料全部在 local：`p1-ticket.body` 22 条全 LOCAL_ONLY、`local-token.json` 7 + `home-*.json` 96 全 LOCAL_RUNTIME_STATE（合计与 136+5 登记吻合）；include 侧对 ticket/token/home/`.git/`/node_modules/`.DS_Store`/`.env`/`id_rsa`/cookie/password 模式命中 **0**。
- LOCAL_BUILD_CACHE 3,442（own-target 等）/LOCAL_ISOLATED_SOURCE 1,199（isolated 源副本）/LOCAL_ONLY 1,074（snapshot 故障工作副本等）与 PREP02 边界一致；include 无任何已知故障工作副本路径（copy 命中均为 copy-delete-rename TSV 证据与生产 prepare_git_copy 脚本）。
- F51 外置夹具：回执 56 entries/110,071 文件+121 symlink/5,324,827,710 B、`all_digests_equal=true`；外置根实存 **57 项**（56+1）。抽 `A-02_copy-delete-rename` 目的地独立复算：33 文件+1 symlink（st_size=138 计入合计 34,456 B，与回执逐数相等）、嵌套 .git 保留。F52 回执（git+python3，绑定面终量 69,126 全普通文件）与消失清单最后 2 条吻合。
- "消失 54" = 52 LOCAL_NESTED_REPOSITORY + independent-validator-bin 2，reference-boundary-index 逐条登记且**无一仍在盘/在分类**；对账 19,659+21+54 = 19,734（PREP02 基线）✓。

## 五、include 安全扫描（亲验）

- 根域受限：include 顶层仅 `artifacts/…/RR3/`（29,071）、`docs/rust-tauri`（60）、`rust/crates`（13）、`scripts/rust-tauri`（14）；无 desktop/shared/根级杂项/用户文件。
- 全量 29,158 文件 8 字节魔数扫描：仅 4 个 `.gz` 证据存档（3 个 PREP02 继承 + E-03/before-documents.json.gz 1,035,414 B 新判；`.gitattributes` 显式 `*.gz binary`，按二进制原字节入库，非可执行）。REPORT §三"二进制 0"的口径为"无 Mach-O/ELF/ar"，与事实相容（见 §九观察 1）。
- 342 个可疑名命中逐类核销：credentials.rs 等为 pristine/快照源码证据、R05_SUITES 命中为 cargo 测试**文本日志**（.log）、`a02-na-source-token.log` 为合成测试常量日志（PREP02 同名先例 INCLUDE_EVIDENCE）。

## 六、163 CRLF 原证与隔离微型 Git 实验（亲验）

- 逐字段对照：`crlf-no-filters-boundary.json` 163 行与 SPACE-01 `normalization-boundary.json` 路径集相等，raw/filtered 内容 SHA256、blob SHA1、字节数 **6 字段 ×163 全等（mismatch 0）**；raw 合计 18,703 / filtered 17,908 与 SPACE-01 总量相等；`noFiltersWriteRequired=true` 163/163；163 条全在 include（INCLUDE_EVIDENCE）。
- 独立重算抽样 12/12：raw SHA256+SHA1、filtered（CRLF→LF）SHA256+SHA1、字节数与两份清单全部精确相等。
- **隔离微型 Git 库实验**（`/private/tmp/rr3-delivery-review-01/crlf-lab`，与主库规则同源 `* text=auto eol=lf`，仅合成样本，列 localOnly 不提交）：
  - E1 普通 `git add`：blob=`83db48f8…`（18 B，无 CRLF）≠ 原始 blob=`b87108ab…`（21 B 含 CRLF）——**普通规范化确实改变原证字节**；
  - E2 `git hash-object -w --no-filters` + `git update-index --add --cacheinfo`：index blob == 原始 SHA1；`git cat-file` 回读与原文件**逐字节相等**（SHA256 `96e4d66c…`）；
  - E5/E6 `git update-index --really-refresh` exit 0、status/diff 不标记 modified、工作树字节未动；E7 `git commit` 后 `HEAD:path` blob 仍为原始字节（CRLF 保留）——**提交链不脱节、报告 hash 与远端 blob 不脱节**；
  - 如实记录的注意点：raw blob 入 index 后，未来对这些路径的 checkout/renormalize 会按 eol=lf 改写**工作树**字节（blob 本体不受影响）；故 root 逐项核对应按 REPORT 口径用 `ls-files -s` blob id + `cat-file` 回读，不应依赖工作树 status。
- 结论：`--no-filters` 精确归档方式**成立**，与 SPACE-01/FINAL-02 的登记口径一致；不改 .gitattributes/原件/全局配置的前提下可保原字节。

## 七、Git 写入空间估算复核（亲验）

- 以同规则（NUL@8KiB 二进制判定 + CRLF→LF）独立重算 include 全量 filtered blob：**唯一 8,615 / 仓库已存在 861（真 `cat-file --batch-check`）/ 缺失 7,754**——与 space-estimate.json 三项全部相等（blob 宇宙精确复现）。
- 预算算术：205,164,544（loose level-1 4KiB 取整）+9,384,352（index）+15,675,392（tree）+4,194,304 = **234,418,592 ✓**；150 个缺失 blob 抽样 level-1 实压 11.5%（< 总体口径 17.1%，即申报值不低估）、level-6 10.0%（申报 15.2%）。
- 当前实测 Data 卷可用 515 Gi（≥ 申报时 509 Gi）；2,049 loose + 新增 7,754 > gc.auto 6,700 → `-c gc.auto=0` 建议成立；既存 88 KiB tmp 垃圾仍在、仅登记（count-objects 原样复现）。结论"≈234 MB 对 509 Gi 占比≈0.04%、容量充分"成立。

## 八、低成本正反控制（隔离清单，未触任何真实交付文件）

隔离副本 6 文件 + 迷你清单，7 案例：正常通过；故障 copy（同路径换内容）、路径越界（`../../etc/hosts`）、字节漂移、文件缺失、重复条目**分别被精准拒绝**；还原后复验通过——**METHOD_VALIDATED**。如实记录：首轮控制中 P2 案例失败系**本人探针**未还原 N3 漂移注入（审查者自身缺陷，非被审对象问题），修正后复跑 7/7 全绿（commands.jsonl 两轮 exit 均存档）。

## 九、非阻断观察（不影响 PASS）

1. REPORT §三"新判路径中二进制 0"的括号口径为"无 Mach-O/ELF/ar"；新判中实有 1 个 gzip 证据存档（E-03/before-documents.json.gz，1.0 MB，`*.gz binary` 属性保原字节，非可执行、非误判）——建议后续措辞区分"可执行二进制"与"压缩存档"。
2. `reference-boundary-index.json` 的 f51 `entriesSample` 三条目的地名用了下划线形态（`artifacts_rust_tauri_…`），实际回执与磁盘为连字符形态（`artifacts_rust-tauri-…`）；权威回执（F51-01）与磁盘一致，仅为样例字符串渲染不精确。
3. include 路径目录（含祖先）我计 4,090 个，tree 预算按 3,826 计；差额 ≈1.08 MB，在 234 MB 保守预算与 509 Gi 余量下无关判定（超保守上界意图不变）。
4. DELIVERY-FINAL-02 的 20 件快照后产物中 `MANIFEST.json` 与 `self-receipt.json` 互相登记（自哈希规避设计，REPORT 已声明）；root 暂存时须以当刻枚举把二者一并收口，避免只按 self-receipt outputs 表取 18 件。

## 十、产物与停止

- 本 `REVIEW.md` + `commands.jsonl`（真实 argv/exit/UTC/输出摘要；两处 exit=1 为审查过程如实存档：控制首跑探针缺陷、merged-paths 解析中止后内联重跑）。验证脚本与隔离库均在外置 `/private/tmp/rr3-delivery-review-01/`（localOnly，不提交）。
- 本审查未改任何生产/脚本/现行文档/证据/清单原件；不执行也不预写 commit/push 成功。下一棒（root，既有授权）：按 `include-paths.nul` 精确暂存（163 CRLF 路径用 --no-filters+update-index 并逐项核 index blob SHA）、以当刻枚举收口 DELIVERY-FINAL-02/DELIVERY-REVIEW-01 自身产物、commit/push 与真实远端回读按 HANDOFF 回执契约归档。完成后停写。
