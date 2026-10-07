# RR3 DELIVERY-FINAL-02 — 放行后最终精确交付清点

- 执行者：R05 RR3 DELIVERY-FINAL-02 全新空历史只读交付准备者；未参与任何实施/审查轮；未派子代理。
- 任务书：`docs/rust-tauri/R05/repair-current/RR3_DELIVERY_FINAL2_BRIEF.md`，全文读取；并全文/关键节读取 RR1_MASTER_PROMPT_2026-10-04、RR2_MASTER_PROMPT_2026-10-06、RR3_BRIEF/HANDOFF/PROGRESS/ISSUE_MATRIX、RR3_DELIVERY_FINAL_BRIEF（分类纪律蓝本）、DELIVERY-PREP-01/REPORT、DELIVERY-PREP-02/REPORT+REFRESH_RULES+merged-paths/decisions/classification-rules、DELIVERY-SPACE-01/REPORT+normalization-boundary.json、DOC-INPUT-BOUNDARY-01/REPORT、FINAL-04/STAGE_REVIEW.md、E-04/REPORT、E-REVIEW-05/REVIEW、F51/F52 迁移回执。中断的 DELIVERY-FINAL-01 采集仅作历史参考（其 inventory.jsonl 不当代数，本轮全部重新实测）。
- 边界：只写本 `DELIVERY-FINAL-02/` 目录；零 Git 写（全程 `--no-optional-locks`，未 stage/commit/push，未改 .gitattributes/配置/系统）；不删原件；未运行产品测试/构建/历史 driver；未读取真实用户 home/密钥。

## 一、结论

**权威枚举+全路径唯一分类完成：include 29,158 路径 / 2,490,626,419 bytes；localOnly 5,912 路径 / 1,218,175,981 bytes；UNKNOWN 0 条。** 163 条 CRLF 原证全部仍在且 raw/filtered SHA 与 SPACE-01 记录逐一相等，已全部标记 `--no-filters` 写入要求。Git 写入保守预算约 234 MB（4KiB 数据块取整），对当前 Data 卷 509 Gi 可用余量占比约 0.04%，容量充分（数据而非承诺）。NUL 列表可供 root 逐文件精确暂存；本报告不预写任何 Git 动作成功。

## 二、权威枚举（2026-10-07T23:3xZ 实测）

| 命令 | exit | 结果 |
|---|---:|---|
| `git --no-optional-locks rev-parse HEAD` | 0 | `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`（与任务书预期一致；分支 codex/rust-tauri-migration） |
| `git ls-files --others --exclude-standard -z` | 0 | **35,041** 条，目录条目 0（F51/F52 后绑定面无嵌套仓库/符号链接目录项），原始字节存 `enumeration-untracked.nul` |
| `git diff --name-only -z` | 0 | **29** 条 tracked 修改（无删除），存 `enumeration-modified-tracked.nul` |
| `git diff --cached --name-only -z` | 0 | **0** 条 staged（零暂存维持），存 `enumeration-staged.nul` |
| `git ls-files -s` 指纹 | 0 | SHA256 `3016f7ae…`（与 FINAL-01..04/E-04 基线一致 = tracked 集未变） |
| `git diff HEAD` 指纹 | 0 | 开工实测记录于 `git-initial-state.json`，收尾复核相等（见 §八） |

范围并集 **35,070 路径** = 35,041 untracked + 29 modified-tracked。任务书写"25 M tracked"为旧截点数字：实际 29 M = 25（PREP02 时点）+ L 1（`rust/crates/lingxi-service/tests/r04_t08_tool_matrix.rs`）+ M 3（`rust/crates/xtask/src/stage_map.rs`、`rust/crates/xtask/src/stage_maps/R04.json`、`scripts/rust-tauri/r04_t08_generate_stage_map.py`），与本轮枚举一致，如实按 29 处理。

## 三、分类总表（互斥覆盖，逐文件 SHA256/bytes/mode 见 classification.json）

| 桶 | 路径数 | bytes | 明细 |
|---|---:|---:|---|
| **include** | 29,158 | 2,490,626,419 | INCLUDE_PRODUCTION 27、INCLUDE_CURRENT_DOC 60、INCLUDE_EVIDENCE 28,727、INCLUDE_EVIDENCE_SNAPSHOT 344 |
| **localOnly** | 5,912 | 1,218,175,981 | LOCAL_BUILD_CACHE 3,442、LOCAL_ISOLATED_SOURCE 1,199、LOCAL_ONLY 1,074、LOCAL_RUNTIME_STATE 136、LOCAL_BINARY 61 |
| **UNKNOWN** | **0** | 0 | 无 |

与 PREP02 基线（19,734 路径）对账：**继承 19,659**（size+mtimeNs 与 merged-paths 记录相等，身份成立、角色未变，本轮仍全新实读 SHA256）；**变更 21**（7 份 repair-current 协调文档总控追加 + 12 份 tracked 现行文档 E-04 回填 + `r02_t08_legacy_entry_regression.sh`/`r05_t08_negative_gate.sh` 两脚本 H/I 轮编辑——全部重判为原类别，角色未变）；**新判 15,390**；**PREP02 后消失 54**（52 LOCAL_NESTED_REPOSITORY 目录项 + F52 的 `independent-validator-bin/git` 与 `python3`，全部对应 F51/F52 外迁，见 §六）。覆盖断言：29,158+5,912+0 = 35,070 = 枚举并集 ✓。

### PREP02 之后新材料的实际分类（任务书 PRECISION 项逐组落实）

| 新材料 | 新判路径数 | 判定 |
|---|---:|---|
| G-REVIEW-03 | 5,042 | INCLUDE_EVIDENCE 4,966 + SNAPSHOT 36（pristine 源基线）+ LOCAL_ONLY 5（p1-ticket.body）+ LOCAL_RUNTIME_STATE 35（local-token/home 快照） |
| FINAL-01/02/03/04 | 32/43/991/980 | 全部 INCLUDE_EVIDENCE（含 verify-R05 正式证据根、attempt-interrupted 中断现场、command-records） |
| E-04 | 16 | 全部 INCLUDE_EVIDENCE（含 2×16MB tree-snapshot、回填脚本） |
| E-REVIEW-04 / E-REVIEW-05 | 8 / 10 | 全部 INCLUDE_EVIDENCE |
| E-REVIEW-03（中断轮） | 34 | 全部 INCLUDE_EVIDENCE（历史中断原件保留） |
| F51-01 / F51-REVIEW-01 / F52-01 / F52-REVIEW-01 | 20/54/14/44 | 全部 INCLUDE_EVIDENCE（迁移回执/审查） |
| L-01 / L-REVIEW-01 / M-01 / M-REVIEW-01 | 6/18/1,267/634 | 全部 INCLUDE_EVIDENCE（含 M 两次 standalone verify-stage R04 全量证据） |
| J-02 / J-REVIEW-01 / J-REVIEW-02 / G-REVIEW-02 | 3,304/597/1,705/299 | 主体 INCLUDE_EVIDENCE + SNAPSHOT（pristine/snapshot-cmp-failure 源基线 12–13 项/组）+ LOCAL_RUNTIME_STATE 19（local-token/home 快照） |
| E-03 / G-INTERRUPTION 已在旧集，本轮补 E-03 67 条新文件、STORAGE-03 24、TASK0 6、DELIVERY-PREP-01 48、DELIVERY-PREP-02 36、DELIVERY-SPACE-01 8、DOC-INPUT-BOUNDARY-01 14、DELIVERY-FINAL-01 6 | — | 全部 INCLUDE_EVIDENCE（交付准备链证据） |
| E04 的 12 份文档变化 | 12（含于变更 21） | INCLUDE_CURRENT_DOC（E-04 回填，E-REVIEW-05 已独立 PASS；WORKER_MODEL_BOUNDARY 字节未变故不在其中） |
| M 的三文件 + L 测试文件 | 4（新判 4 tracked M） | INCLUDE_PRODUCTION（白名单生产/门禁数据面，FINAL-04 全链实测被测输入） |
| 新增白名单生产脚本 | 11 untracked | INCLUDE_PRODUCTION（A: run_output_sinks.py；B: r05_t08_mutate_pin/negative_gate_selfcheck；H: r02_run_output_regression、r02_t01_log_level_regression；I: r05_t08_restore_selfcheck；J: prepare_git_copy(+selfcheck)、r05_t08_prepare_node、r05_t08_node_selfcheck）+ C 采样器 `rust/crates/lingxi-service/tests/support/r05_resource_sampler.rs`（含于 rust/ 组） |
| 新增任务书/台账 | 18 | INCLUDE_CURRENT_DOC（repair-current，含本轮 RR3_DELIVERY_FINAL2_BRIEF.md） |

新材料安全扫描：新判路径中 **二进制 0**（≥8KiB 魔数全扫：无 Mach-O/ELF/ar）；真实运行时票据 5（p1-ticket.body，LOCAL_ONLY，仅登记 SHA/size 不输出原文）；运行时 token/home 快照 54（LOCAL_RUNTIME_STATE）；合成测试常量日志（如 a02-na-source-token.log，PREP02 同名先例 INCLUDE_EVIDENCE）不按 secret 词删除。旧排除（3,442 缓存、61 装备二进制、1,199 隔离源副本、1,074 LOCAL_ONLY）继承且原件本地保留。

## 四、163 条 CRLF 原证处置标记

消费 `DELIVERY-SPACE-01/normalization-boundary.json` 全部 163 行，本轮逐路径复验：**163/163 raw SHA256、filtered SHA256、字节数与 SPACE-01 记录全部相等（mismatch=0）**；163 条全部属 INCLUDE_EVIDENCE。输出 `crlf-no-filters-boundary.json`：每路径 `noFiltersWriteRequired=true` + raw/filtered 内容 SHA256 + raw/filtered blob SHA1 + raw/filtered 字节数对照。处置口径（登记不执行）：这些路径普通 `git add` 会经 `text=auto eol=lf` 改变 blob，须 `git hash-object -w --no-filters`（或 `git add --no-filters`）写入 raw blob 并以 `update-index` 登记 raw blob id，逐项回读核对 raw SHA；不改 `.gitattributes`/原件/全局配置。该方案仍需 DELIVERY-REVIEW-01 独立审查后方可执行。

## 五、Git 写入空间只读估算（不写对象、不 stage）

方法沿 SPACE-01：对 include 29,158 路径逐文件实测当前 Git 换行规则（binary=NUL@8k 检测；text=CRLF→LF）后的 filtered 内容，计算 Git blob SHA1（SHA1 仓库），`git cat-file --batch-check` 只读批量核对既有对象，缺失对象计 zlib level-1（当前默认 loose 压缩，无 core.compression 覆盖）长度并按 4KiB 数据块向上取整：

| 项 | 值 |
|---|---:|
| include 原始逻辑量 | 2,490,626,419 bytes（29,158 路径） |
| 唯一 filtered blob | 8,615（重复路径大量去重：candidate-source before/after、pristine 副本等） |
| 仓库已存在 | 861 |
| **缺失需新写 blob** | **7,754 / filtered 1,199,373,418 bytes** |
| loose 对象预算（level-1 / 4KiB 取整） | 205,163,658 / **205,164,544 bytes（≈205 MB）** |
| index（38,066+29,158=67,224 entries，v2 基础估算+lock 双写+扩展余量） | 9,384,352 bytes |
| tree（3,826 目录全按新增、逐对象 4KiB 取整的超保守上界） | 15,675,392 bytes |
| commit/refs/reflog/临时余量 | 4,194,304 bytes |
| **保守本地写入合计** | **234,418,592 bytes（≈234 MB）** |
| push 传输参照（缺失内容 level-6） | 182,157,535 bytes（smart HTTP chunked 通常不落整包本地盘，仅为量级参照） |
| 当前 Data 卷可用 | 546,534,588,416 bytes（509 Gi） |

**结论：≈234 MB 保守预算对 509 Gi 可用余量占比 ≈0.04%，容量充分。** 操作注意（沿 SPACE-01）：loose 对象 2,049 + 新增 7,754 将超过 gc.auto=6700，建议 root 在具体命令加 `-c gc.auto=0` 防自动重打包；既存 88 KiB 临时对象垃圾（`.git/objects/a0/tmp_obj_*`，SPACE-01 已记录）仍在，仅登记不处置；大于 GitHub 普通单文件限制的 134,457,480 bytes rlib 等维持 localOnly 不随本次交付。

## 六、引用边界与 localOnly 登记

`reference-boundary-index.json`（增量+引用，不重复展开 PREP02 大库存）：

1. **F51 外迁夹具**：56 嵌套 .git 目录（110,071 文件+121 symlink / 5,324,827,710 bytes）→ 仓库外 `LingxiAgent-RR3-localonly-fixtures/`；回执 `F51-01/RELOCATION-RECEIPT.json`（totals.all_digests_equal=true）。其中 52 项即本轮"PREP02 后消失"的 LOCAL_NESTED_REPOSITORY 目录项；另 4 项（J-02/J-REVIEW-01×2/J-REVIEW-02 的 copy/）系 PREP02 快照后新增、从未入旧清单，直接按回执登记。全部 localOnly=true、remoteOriginalAvailable=false。
2. **F52 外迁夹具**：`A-REVIEW-02/independent-validator-bin`（git 脚本+python3 symlink）→ 同外置根；回执 `F52-01/RELOCATION-RECEIPT-F52.json`；即本轮消失清单中最后 2 条。
3. **超限原件**：`C-F46-REVIEW-01/isolated/build/liblingxi_service.rlib`（134,457,480 bytes，SHA256 `ef146b51…`）等 61 项装备二进制维持 localOnly（身份继承 PREP02 local-originals，未重复全量重哈希，stat 相等）。
4. **本轮新增排除 59 项**（5 票据+54 runtime state）逐项登记实际 SHA256/bytes/driver 依据。
5. **旧 2,036 项排除原件**：引用 `DELIVERY-PREP-02/local-originals.json`，不复制副本。
6. **用户交付义务核对**：当前义务清单（HANDOFF `git_delivery_receipt_contract`：精确暂存清单、CRLF raw 交付、真实回执后置归档）中，本轮提供前两项；**真实未满足项=大 rlib/二进制原件远端不可取（remoteOriginalAvailable=false）**——若用户明确要求这些历史原件必须远端可取，现方案不能满足该部分，须另行解决渠道，不以"可重跑"冒称"同字节原件可得"。其余无真实未满足的交付义务。

## 七、产物清单（本目录）

- `REPORT.md`（本文件）、`delivery_final_02.py`（本轮 driver，只读枚举+分类+估算）
- `git-initial-state.json`、`commands.jsonl`（真实 argv/UTC/exit/输出 SHA）
- `classification.json`（35,070 行逐路径 path/gitState/type/mode/bytes/sha256/category/basis/classificationSource；严格 JSON 无重复键）
- `include-paths.nul`（29,158，NUL 分隔，root 逐文件暂存输入）+ `include-paths.txt`（人类可读，SHA/size/mode/类别）
- `local-paths.nul`/`.txt`（5,912）、`unknown-paths.nul`/`.txt`（0，空）
- `crlf-no-filters-boundary.json`（163 行 raw/filtered 对照+--no-filters 标记）
- `reference-boundary-index.json`、`space-estimate.json`、`summary.json`、`unknown-items.json`
- `enumeration-untracked.nul` / `enumeration-modified-tracked.nul` / `enumeration-staged.nul`（原始枚举字节）
- `self-receipt.json`、`MANIFEST.json`（自身新证据独立收据；MANIFEST 不自哈希，REPORT 由 MANIFEST 登记）

注意：`include-paths.nul` 供 root 后续逐文件暂存，**不含**本目录本轮枚举后才写出的产物（classification.json 等）——这些在 `self-receipt.json` 递增登记为 INCLUDE_EVIDENCE 候选，供 DELIVERY-REVIEW-01 审定后由 root 一并精确暂存；避免循环自哈希。

## 八、仓内前后稳定核验

- 开工（枚举时）与收尾（本报告写毕后）：HEAD 均 `b3ac0e6ae…`、staged=0、`git ls-files -s` SHA256 前后相等、`git diff HEAD` SHA256 前后相等（见 `git-initial-state.json` 与收尾复核记录于 self-receipt.json）——tracked 面零变化，本轮唯一仓内新增为 `DELIVERY-FINAL-02/` 自身产物。
- 读取期间逐文件 stat 稳定（35,070/35,070 无 missing）；QA：JSON 严格解析（object_pairs_hook 拒重复键）全过、NUL/txt 列表与 classification.json 逐路径相等、SHA256 随机抽样 5/5 相等、覆盖计数断言通过。

## 九、停止声明

分类与估算完成，UNKNOWN=0，无待人工判定项。本报告完成后停写；不执行任何 Git 写动作、不预写提交/推送成功。交 root 派 DELIVERY-REVIEW-01 全新独立审查；后续精确暂存（含 CRLF 路径 `--no-filters` 方案的独立审定）、commit/push 与远端读回由 root 在既有授权与回执契约下执行。
