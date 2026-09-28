# R02 最终收口 · repair-group10 R1 — A16 门禁 E5(3) 新诊断形态精确登记

- 时间：2026-09-28T17:37:38Z（本地 2026-09-29 01:37 +08:00）
- 唯一改动文件：`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`
  （改后 sha256 = `fb0662585c490ce3540c1dc0f8ba3f3ee5de480c6a13106e3ae17a50b2f04b2f`）
- 不 commit / 不 push。E5(2) seal 三件套白名单、E0/E1/E2-E4/E4.5、fail-closed 判定逻辑本身：均未改动。

## 1. 根因（取证结论）

gate-r2 的候选全量 npm test 中，round2/round3 各有一个失败块（round2 block 4、
round3 block 6）携带「补丁生成器拒绝脏树」+「guard 尾窗截断」+「CRLF 警告包裹」
组合，其 guard 尾窗行未命中任何已登记完整句形 → 文件分类出现 UNRECOGNIZED →
`FAIL: E5: unrecognized CANDIDATE failure cause`。

生产者机制链（全部读源码核实）：

1. `tests/round2-delivery-evidence.test.ts:255` / `tests/round3-delivery-evidence.test.ts:220`
   用 `execFileSync` 跑 `python3 …/create-delivery-patch.py`（round3 为
   `create-round3-patch.py`）；生成器 exit 1 时 Node 抛
   `Error: Command failed: python3 …`，stderr 全文进 message → FAIL 块。
2. 生成器 stderr = `git add`（临时 index）产生的 CRLF 警告 +
   `current source manifest does not match HEAD (uncommitted or untracked source
   changes): firstDiff=[…]`（脏树拒绝，json.dumps 3 元素）+
   `post-verification diff guard failed: {guard_output[-500:]}`
   （`create-delivery-patch.py:449` / `create-round3-patch.py:449`）。
3. 本仓库 guard 违例清单 ~2100 项（~65KB）>> 500 字符窗口 → `[-500:]` 前切
   mid-path：窗口 = 前截路径碎片（`udit-r17-cli-rust-targeted/typecheck-05/
   exit-codes.txt`）+ 完整 `  - path` 清单行。
4. guard 自身 `fail()` 是 `console.error(巨大消息); process.exit(1)`——Node
   不会在 exit 时 flush 未写完的管道输出，超过 macOS 64KiB 管道缓冲的部分
   **丢失**：实测捕获到的 guard 部分**恰好 65536 字节**，在
   `  - artifacts/rust-t` 处 mid-token 断尾（门禁块与生成器内嵌两路捕获断点
   完全一致，证明是 guard 进程侧的管道截断，非 vitest 截断）。
5. 块内窗口行重建长度**恰好 500 code points**（纯 ASCII，长度与 locale 无关）
   ——生产者 `[-500:]` 的字面几何。

## 2. 新登记形态（块 → 原因）

| 新形态（来源） | 结构化锚定式 | 归属原因 |
|---|---|---|
| guard-tail WINDOW（gate-r2 round2 block 4：`e5-candidate-blocks.txt`） | 同块首条 payload 行 = 生成器 wrapper（`python3 …/create-delivery-patch.py` 或 `…/create-round3-patch.py`）+ 精确前缀 `post-verification diff guard failed: ` + 碎片文法 `^[!-9;-~]*$`（可打印 ASCII、无空格无冒号→任何 ✗ 句尾必含空格或全角 `）`，句料不可能冒充路径碎片）+ 其后 ≥1 行且 ≥1 行**完整** `  - [!-9;-~]+` 清单行 + 重建窗口（碎片+\n+各行，无尾换行）**恰好 500 code points** | seal-coordinate-lag（窗口即 guard 违例清单尾，只有 non-audit-change 拒绝会打印清单） |
| guard-tail WINDOW（gate-r2 round3 block 6） | 同上（同一确定性窗口，两生成器嵌入同一 guard 输出） | seal-coordinate-lag |
| 同块伴生：脏树拒绝行 + CRLF 警告行（既有登记形态，本块内已正常命中，未改） | 完整句形 / firstdiff_ok，不变 | uncommitted-source-rejection |

登记方式沿用「完整句形/首字段+锚」纪律：前缀锚 + 同块生产者绑定 + 文法制约 +
精确 500 几何锚，**没有任何子串放宽**。窗口行在其块内被跳过（已解释正文），
验证通过则该块记 seal-coordinate-lag。

注意（显式边界，fail-closed 保持）：
- R6-F01 的裸尾窗 `post-verification diff guard failed: .py`（无后随清单行）
  仍 UNRECOGNIZED（`guard-tail-cut`、`same-block-known-plus-guard-tail` fixture
  验证不变）。
- 句中截断（`truncated-guard` fixture）仍 UNRECOGNIZED。
- 若未来某次 guard 输出总长落在 500..~600 区间、`[-500:]` 切在 ✗ 句内部，
  碎片含句料 → 拒绝 → 门禁红（无证据不登记，保持 fail-closed）。

## 3. E0s 自检 fixture（新增 8 个，均注释来源 gate-r2 实测块）

正控（真实/推导文本 → 分类正确）：
- `guard-window-r2-round2`：gate-r2 round2 block 4 原文（wrapper + 2/72 真实
  CRLF 警告 + 真实 firstDiff + 真实 500 点窗口）→ `seal-coordinate-lag,uncommitted-source-rejection`
- `guard-window-r2-round3`：gate-r2 round3 block 6 原文 → 同上
- `guard-window-complete-end`：推导变体（guard 未被管道截断、末行完整，窗口
  仍恰 500 点；非 gate-r2 字节，按生产者力学构造并注明）→ 同上

负控（单点变异 → 必须 UNRECOGNIZED 并使 gate FAIL）：
- `guard-window-off-by-one`：末行删 1 字符（500→499）→ `uncommitted…,UNRECOGNIZED`
- `guard-window-trailing-junk`：末行追加 ` Error: EACCES: permission denied` → 同上
- `guard-window-sentence-fragment`：碎片换成句料 `artifacts）:` → 同上
- `guard-window-wrong-producer`：同窗换 node-guard wrapper（生产者绑定失败）→ 同上
- `guard-window-missing-line`：删 1 行完整清单行（500→419）→ 同上

## 4. 验证（全部实跑，UTC 2026-09-28T17:2x–17:37Z）

| 验证 | 命令/载体 | 结果 |
|---|---|---|
| 语法 | `bash -n scripts/rust-tauri/r02_t08_legacy_entry_regression.sh` | 通过 |
| 复现基线（改前） | `/tmp/r02-final/work/harness-g10.sh`（sed/awk 提取真实 classify_file/extract_blocks）驱动 gate-r2 `e5-candidate-blocks.txt` | 复现 gate-r2 FAIL：round2/round3 = `UNRECOGNIZED,seal-coordinate-lag,uncommitted-source-rejection` |
| gate-r2 真块（改后） | 同 harness，改后机器 | 三文件全部已登记原因、**无 UNRECOGNIZED**：post-verification-audit-seal=seal；round2=seal+uncommitted；round3=seal+uncommitted |
| 现场重跑同态取证 | `env -u …proxy… npx vitest run tests/round2… tests/round3… tests/post-verification-audit-seal… 2>&1 \| tee /tmp/r02-final/g10-seal-raw.log`（脏候选工作区；Test Files 3 failed / Tests 6 failed，与门禁同态） | 其 blocks（真实 extract_blocks 提取）改后分类与 gate-r2 真块一致，无 UNRECOGNIZED |
| 全量 fixture 回归 | 提取脚本内全部 52 个 sc_expect fixture 重放（/tmp/r02-final/work/g10-fixtures/） | **52/52 通过**：44 个既有 fixture 判定逐字不变（含 R6-F01 三个尾窗/截断负控），8 个新 fixture 全部符合预期 |
| 门禁 E0s 段自跑 | 从改后脚本原文切出 E0s 分类器段（stub note/fail/EVIDENCE_DIR）执行 `/tmp/r02-final/work/run-e0s.sh` | `E0S-SECTION-EXIT=0`（脚本自身 fixture 文本 + sc_expect 全绿，含新 8 个） |
| 附加对抗钻演 | 吸收（合法窗块+裸 wrapper 块）→ 含 UNRECOGNIZED；窗内插入 EACCES → UNRECOGNIZED；wrapper 非首行 → UNRECOGNIZED；窗口 501 点 → UNRECOGNIZED | 4/4 fail-closed |
| heredoc 噪声修复验证 | 单独执行 CLASSES heredoc 块 | stderr 0 字节；3 行完整落盘（反引号内容不再被命令替换吞掉） |

## 5. 顺带修复（同文件内，属证据完整性，非放宽）

- gate-r2 stderr 里的 `line 530: ✗: command not found` 等 8 条噪声：CLASSES
  heredoc 原为**未加引号** `<<CLASSES`，正文中的反引号被 shell 命令替换执行，
  且归档台账 `e5-cause-classes.txt` 内被反引号包裹的术语被静默替换为空。改为
  `<<'CLASSES'`，唯一有意的展开 `${UNCOMMITTED_DIAG}` 改写字面量。
- `note "PASS E0s-gate-self-checks (45 … (legal \`]\`/\`,\`/…)"` 中两个反引号
  对同样触发命令替换（`]: command not found`）：改普通引号措辞，fixture 计数
  45→53。
- 两条 PASS/E5 结论 note 与头部 E5 语义注释、签名注释块同步登记新形态
  （`truncated/tail-window` 表述收窄为「截断句/未登记或几何不符的尾窗」）。

## 6. 环境观察（如实报告）

- 取证运行在 invoking 工作区直接执行三个封印测试（任务书指定；round2/round3
  因脏树在生成器拒绝处失败，未重写交付产物）。
- 会话期间工作区出现与本人无关的并发改动（如 `rust/crates/lingxi-service/
  tests/r00_management_leaves.rs`，手写测试辅助函数，应属其他修复组）；按
  「保留无关改动」未动。
- 本验证为本地钻演 + 真块驱动，不等于门禁全链复跑（未跑 E0/E1/E2-E4/E4.5/
  E5 全 npm test 全链，那是收口 gate-r3 执行者的事）；门禁第 3 轮实测仍是
  最终判据。
