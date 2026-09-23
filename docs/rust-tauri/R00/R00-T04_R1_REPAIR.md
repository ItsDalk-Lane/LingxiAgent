# R00-T04 修复报告（R1 Repair：F01/F02/F03）

修复者：一次性 ZCode 修复任务（非原执行者、非后续验收者）。修复日期：2026-09-24。
修复依据：[R00-T04_REVIEW_R1.md](R00-T04_REVIEW_R1.md)（保持原样，结论未改）；任务规格为任务书 R00-T04 Steps/Deliverables 与验收 R00-A07/A08（REQUIRED）。

基线：分支 `codex/rust-tauri-migration`，HEAD = `ffcb85830ffdc75da5fe39b7a8cfddb4617f1aec`（= Task base = R1 tested HEAD，全程未变）。未创建分支/worktree，未 commit/push/PR/tag/release，未接触真实用户数据或外发 API。`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（总控账本）与 R1 验收报告本轮均未修改；除下列文件外无其他改动。

本轮改动文件（均为 T04 候选，不涉其他 Task 文件）：
`ENTRYPOINTS.json`、`STORES.json`（经生成器重生成）、`OWNERSHIP_CURRENT.md`、`R00-T04_REPORT.md`、`r00_t04_scan.py`、`artifacts/rust-tauri/R00/T04/{final-run-stdout.txt(由 .log 原字节重命名), deliverable-hashes.txt(重生成), scan-output.json(最终校验重写)}`，另新增本修复报告。

## F01｜BLOCKING｜.log 证据被 gitignore

- **根因**：执行者将最终运行 stdout 存为 `final-run-stdout.log`，命中 `.gitignore:95` 的 `*.log`，而该文件在 `deliverable-hashes.txt` 与冻结 8 文件聚合内，候选无法按登记指纹完整入库。
- **改动**：`mv final-run-stdout.log final-run-stdout.txt`（原字节，无内容改动——SHA-256 前后均为 `c4deafda678c898801527a43dfc4a90f05d08db482884fa7c77df8b288eca842`）；`R00-T04_REPORT.md` 交付表引用改 `.txt`；`deliverable-hashes.txt` 第 6 行路径更新并重算全部哈希；8 文件聚合重算（见 §候选聚合）。`.gitignore` 未动；未使用 `git add -f`。
- **验证**（只读，未实际 add）：
  - `git check-ignore -v artifacts/rust-tauri/R00/T04/final-run-stdout.txt` → 退出码 1（未被忽略）；重命名前同命令退出码 0 命中 `.gitignore:95 *.log`（复核 R1 证据）。
  - `git ls-files --others --exclude-standard artifacts/rust-tauri/R00/T04/` 列出全部 3 个产物文件；`git status --porcelain --ignored` 该目录下无 `!!` 项。
  - 8 份候选逐一 `git check-ignore` 均为 addable（见 §候选聚合后附验）。

## F02｜MAJOR｜ENTRYPOINTS 计数块与叙述自相矛盾

- **根因**：追加 EP-DEV-03（vite dev 开发链）后未刷新计数块与三处叙述；原校验器只验锚点不验计数，缺陷无护栏可见。
- **复算**（与 R1 一致）：entries 共 37 条 = active 35 + dormant 1（EP-DEV-01）+ residual 1（EP-DEV-02）；plugins_mcp total 5 / active 3；原声明合计 36 在任一读法下均不成立。
- **改动**：
  1. `ENTRYPOINTS.json`：新增 `entrypoint_status_counts`（`active:35, dormant:1, residual:1, total:37`）；`entrypoint_category_counts` 每类改为 `{active,total}` 双口径：desktop_shell 7/7、renderer_clients 2/2、http_ws_server 7/7、cli 3/3、bridge 3/3、schedulers 3/3、channels_dm 4/4、subagents 1/1、plugins_mcp **3/5**、tool_boundary 2/2。
  2. `r00_t04_scan.py`：新增 `validate_entrypoint_counts`——status 词表约束（active/dormant/residual）+ `entrypoint_status_counts` 与逐类 `{active,total}` 均由 `entries` 机械重算并比对声明值；接入 validate 为新检查 `A2_entrypoint_counts`（错误计入 fatal），stdout 摘要新增 `A2_counts_errors`；负向自检新增第 4 项 `tampered_counts_detected`（篡改 plugins_mcp active 计数必须触发失败）；docstring 同步。既有 A/B/C/D/E 断言结构未动。
  3. 叙述同步：`R00-T04_REPORT.md`（交付表"35 个现役入口 + 1 休眠 + 1 残留（合计 37 条）"、Step 1 分类列表 plugins_mcp 5（现役 3 + 休眠 1 + 残留 1）+ 合计句、负向自检 4 项、命令表加 A2 行）；`OWNERSHIP_CURRENT.md` §2 标题改"37 条登记 = 35 现役 + 休眠/残留各 1（计数由 --validate A2 检查机械校验）"、plugins_mcp 条目改"（5：现役 3 + 休眠 1 + 残留 1）"并按现役/非现役重排枚举。其余类别计数经复核 active==total，无需改动。
- **负向验证（实测）**：临时将 plugins_mcp `active` 3→4 后 `python3 -B docs/rust-tauri/R00/r00_t04_scan.py --validate` → 退出码 1，`FATAL: A2: entrypoint_category_counts[plugins_mcp] mismatch: declared={'active': 4, 'total': 5} recomputed={'active': 3, 'total': 5}`；恢复后 `cmp` 与改前逐字节一致，最终全量运行 `fatal_count=0`。
- **其余扫描断言不受影响**：最终运行 A=101 锚点 0 错、B=392 字面路由（470 注册位/50 文件）、C=69↔69、D=0 未分类/0 过期/146 token 文件、E=8/8、负向 4/4，与 R1 期数值一致（仅新增 A2 与第 4 项负向）。

## F03｜MINOR｜server-runtime-info 漏记 desktop 的删除性角色

- **根因**：生成器 `STORE_OVERLAY` 的 curated `processes` 将 server-runtime-info writers 写为 `["server"]`，漏掉 desktop 在 server 生命周期路径的 unlink（committed inventory 中 `desktop/main.cjs` 有 4 个 `remove-path` 位点归属该 store）。
- **改动**（注记落在生成器源数据内，重生成可保留）：`STORE_OVERLAY["server-runtime-info"].processes` → `writers: ["server", "desktop(unlink-only)"]` + `note`，note 逐点列明 4 个位点且明确"仅删除性 unlink，不创建/写入内容；内容唯一写者为 server"：stale/死内核探测清理 `desktop/main.cjs:1364/1368`、spawn 前清旧文件 `:1915`、`shutdownServer` 关停去留 `:6635`（与 EP-DESK-01 的 shutdownServer 叙述互证）。
- **叙述**：`OWNERSHIP_CURRENT.md` §3 跨进程分工段补 desktop 对 `server-info.json` 的仅删除性 unlink 一句；经程序重算，"server 写 62 项""desktop 内容写者 = 壳态 5 项 + OTA 列车"两个数字在新 STORES.json 下不变（desktop(unlink-only) 不计入内容写者）。
- **验证**：
  - 往返证明：将 overlay 临时回退为旧单行后重生成 → STORES.json SHA-256 = `e3b849427df8e5b20f66fbaf876bc39880b7ed1f34b9e194203ef78451fadc46`，与 R1 冻结候选逐字节一致（生成器/注册表/inventory 管线无其他差异驱动）；重新应用修复后 `diff` 仅 server-runtime-info 的 processes 块（writers + note）。
  - 字节稳定：连续两次 `--generate-stores` 重生成 `cmp` 逐字节一致；最终全量运行（默认模式含生成）后再验哈希仍为 `6067bdd3be7d0232c182d8f9c1beb147e1a5b59a3fa3ae137889f31a75d7b493`。

## 重跑验证汇总

| 命令 | 退出码 | 结果 |
|---|---|---|
| `python3 -B docs/rust-tauri/R00/r00_t04_scan.py`（默认=生成+校验） | 0 | STORES 69 项写入；A=101 锚点 0 错、A2 计数 0 错、B=392、C=69↔69、D=0/0/146、E=8/8、负向 4/4；`R00_T04_SCAN_OK`；重写 `scan-output.json` |
| `python3 -B … --validate`（F02 负向演示，计数故意改错） | 1 | `FATAL: A2: entrypoint_category_counts[plugins_mcp] mismatch…`；恢复后通过 |
| `npx vitest run tests/persistence-store-registry.test.ts tests/persistence-schema-tripwire.test.ts tests/http-route-security.test.ts tests/server-auth.test.ts tests/ws-scope.test.ts tests/device-registry.test.ts` | 0 | 6 文件 74/74 通过（与 R1 一致） |
| `node scripts/check-persistence-schema-fingerprint.mjs` | 0 | `170 watched sources; OK` |
| `git check-ignore`（final-run-stdout.txt；8 候选逐一） | 1×8 | 均未被忽略，可正常 `git add` |
| 语法检查：`python3 -c "import ast; ast.parse(…r00_t04_scan.py…)"` | 0 | 受环境限制替代项，见下"剩余限制"第 4 条 |

本轮改动仅元数据/打包/校验器，未触碰任何产品源码、依赖与测试源，上述回归范围与 R1 所列一致，无需扩大。

## 候选 8 文件与聚合 SHA-256

聚合算法与 R1 §1 相同（已先用修复前文件复核出账本值 `61d0a8ef…3300d` 确认口径）：按路径排序、每行 `SHA256␣␣路径` 换行拼接后取整体 SHA-256。验收报告与本修复报告不计入聚合。

| 路径 | SHA-256 |
|---|---|
| `artifacts/rust-tauri/R00/T04/deliverable-hashes.txt` | `155509ef9ab3b72e4c6bf06e606394ce0317bcc46f686ace053667d900867903` |
| `artifacts/rust-tauri/R00/T04/final-run-stdout.txt` | `c4deafda678c898801527a43dfc4a90f05d08db482884fa7c77df8b288eca842` |
| `artifacts/rust-tauri/R00/T04/scan-output.json` | `3fc4413371f99b8d135d6ba118c652cc38e97331524fed7e35e1e3d77a8cfdf1` |
| `docs/rust-tauri/R00/ENTRYPOINTS.json` | `f69e658f8b35b5a486a764872e27c82b599c84ef49404903f5b72f4b0b84d669` |
| `docs/rust-tauri/R00/OWNERSHIP_CURRENT.md` | `9d5d3b7a5b97286555b8be48c86e20148c4700ba9d4276395232736a420bd51a` |
| `docs/rust-tauri/R00/R00-T04_REPORT.md` | `8fb0731960491f6c7c2388fe50d70769057c517bde2ed2551b37a73cc187f909` |
| `docs/rust-tauri/R00/STORES.json` | `6067bdd3be7d0232c182d8f9c1beb147e1a5b59a3fa3ae137889f31a75d7b493` |
| `docs/rust-tauri/R00/r00_t04_scan.py` | `842dfe2d5cc4bc7e1f7dee1429d1e9792c6e9ef6d9a44981dc68d49d725855c2` |

**候选聚合 SHA-256：`888df87a31771c7eaea1bbcbeb367e5df070a069004c03a509f90c0cbc087087`**

注：`deliverable-hashes.txt` 沿用原清单结构列 6 个文件（不含 REPORT.md 与清单自身），与 R1 审查时结构一致，非本轮新增差异。

## 剩余限制与移交

1. **总账未同步（移交总控）**：`ORCHESTRATOR_PROGRESS.json` 中 `candidate_digest_sha256` 仍为修复前 `61d0a8ef…`、`candidate_note` 仍提及 `final-run-stdout.log`。账本由总控维护，本轮未触碰；重验通过后需由总控更新为 `888df87a…87087` 与新路径，否则账本与候选不一致。
2. **final-run-stdout.txt 为修复前最终运行的原字节**：按修复要求原字节保留（其 stdout 内容对应修复前那次默认模式运行）。修复后运行的完整 stdout 见本报告"重跑验证汇总"与重生成的 `scan-output.json`；若总控要求"final run"证据与修复后状态逐字节对应，可另行授权重捕。
3. **`python3 -m py_compile` 未执行**：本会话 Mimosa hook 拦截经 Bash 对仓库脚本做编译产物写入；以只读 `ast.parse`（通过）+ 脚本多轮实际执行（导入、生成、校验均成功）替代，语法与导入正确性已被覆盖。
4. **未扩大验证范围**：R1 报告 §6 的未验证范围（真实模型/平台账号/GUI/安装包等 LIVE 项）在本轮不变；本轮为打包/元数据修复，A07/A08 的实质结论未重做也未动摇。
5. ENTRYPOINTS 条目内容、锚点与 A08 链本轮未逐条重验（R1 已逐跳核实；本轮除计数块外未改动这些内容）。

## 结论

三项 finding 的修复与验证如上；候选 8 文件可整体进入版本库（无任何必需证据被 gitignore）。本修复者不自行判 PASS——状态：**READY_FOR_REVIEW**，交由下一个全新独立验收任务复核。
