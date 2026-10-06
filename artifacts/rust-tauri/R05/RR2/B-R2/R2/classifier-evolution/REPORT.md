# B-R2 R2 轮 2：E5 分类器 patch-too-large 形态演化登记 — 记录与验证

日期：2026-10-06；工作包 B-r2（第 2 轮修复智能体）。
对象：scripts/rust-tauri/r02_t08_legacy_entry_regression.sh（工作树，shasum-256
703e443d9bb9829766b0a4d450e38160be93bda9f15d17290aca95a5182427d5）。
完整 r02_t08 门禁跑未执行（由总控在静默窗口执行）；本目录只做分类器判定路径的
重放验证与 E0s 自测段重跑。

## 一、历史核实结论（登记前置条件，已通过）

待登记失败族 = round2-delivery-evidence.test.ts 块4（R10-09 重放）与
round3-delivery-evidence.test.ts 块6（现场重放），python3 生成器 wrapper 之下。

1. 该族从未绿过（任何 candidate 端全量 E5 运行）：
   - R02 收口时代仅有的两条通过 E5-cause-classification 的链
     （final-candidate-fc9971898df7a9e8 / e876171c5cd0a6f8，
     `grep -rl 'PASS E5-cause-classification' artifacts --include=summary.txt`
     全库仅此两处）中，两块位以**已登记形态**失败并被接受：
     wrapper + firstDiff（uncommitted-source-rejection）+ guard-tail WINDOW
     （seal-coordinate-lag，repair-group10 登记）→ e5-candidate-causes.txt
     记 seal+uncommitted；gate-r2 的 a29e5adbb404ad70 是登记前的失败证据。
   - R03 时代各 verify-stage 链（STAGE-REPAIR-G01-F01/F03、T08-E01 两链）：
     round2 块4 仍为 firstDiff+WINDOW（登记形态），round3 块6/7 自 R03 起
     演化为 `patch replay failed: error: patch too large` 且当时诚实判
     UNRECOGNIZED → 这些链 E5 全部 fail-closed（无一含 PASS
     E5-cause-classification）；G01-F02/G07-E01 更早失败（非族新红等）。
   - R04 时代通过链 REVIEW-T08/FINAL-R3（2026-10-04）为 directed 模式：
     summary 明载 `SKIP E5 … BY SCOPE: directed mode requested`，
     E5 从未运行，块位也非绿。
2. 形态演化原因核实：树增长使重生成交付补丁超过 git 自身 apply 上限
   （git apply.c MAX_APPLY_SIZE = 1024*1024*1024-1 ≈ 1023 MiB；F44 实测
   `git diff --binary --full-index 67dee5d2 HEAD` = 3,671,013,696 B 未压缩）。
   两个冻结生产者的真实输出形状（与当前 HEAD 源码逐行核对）：
   - round2 create-delivery-patch.py:359 raise RuntimeError（未捕获 →
     python3 打印 6 行 Traceback：File 帧第 457 行 <module> /
     第 359 行 replay_and_verify，源码行逐字一致，末行完整句
     `RuntimeError: patch replay failed: error: patch too large`）；
   - round3 create-round3-patch.py:342 raise SystemExit（字符串消息 →
     python3 裸行 `patch replay failed: error: patch too large`，无 Traceback）。
   证据：s5-full-4/ev/legacy-entry/e5-candidate-blocks.txt 块4/块6、
   F44/f44-failure-blocks.txt、F44/f44-three-suite-run.log（57891-58151、
   59063-59192 行）。

结论：核实通过——失败族早已登记（R5-F01/R6-F01/repair-group10 演进史）、
从未绿过；本轮为其补充形态演化识别。诚实注明：patch-too-large 具体形状自
R03 起一直被判 UNRECOGNIZED（fail-closed），本轮是总控授权的同族新形态登记，
非"恢复既有接受态"。

## 二、登记实现（只改 r02_t08_legacy_entry_regression.sh）

- 头注释登记段：新增 "R05 RR2 B-R2 R2 … FORM-EVOLUTION registration" 块
  （同族演进关系、MAX_APPLY_SIZE 原因、R03 起未登记的诚实历史、证据指针）。
- 分类器 classify_file：块首生成器 wrapper 绑定 + 各自生产者的严格形状——
  round2 要求连续 6 行 Traceback 且 File 帧引用与 wrapper 相同的脚本路径
  （457/<module>、359/replay_and_verify）+ 完整末句；round3 要求裸句整行相等；
  句尾必须是 git 完整拒绝文本 `error: patch too large`。截断句、其他 git
  错误、跨生产者组合、变体帧、node-guard wrapper、裸 wrapper 一律仍
  UNRECOGNIZED（fail-closed 纪律不变）。判定入 seal-coordinate-lag
  （登记红，npm 原始 exit 仍为 1，绝不转正式绿）。
- e5-cause-classes.txt 登记册两条目同步扩充；E5 文档块与三条 PASS note 同步。
- E0s 自测新增 9 例 fixtures（2 真实正例 + 7 负例：截断句、其他 git 错误、
  外来 SystemExit 文本、跨生产者双向、变体帧、node-guard 生产者）。
- 未改任何断言/门禁判定逻辑（E5 的 UNRECOGNIZED→fail 路径原样）。

## 三、验证（命令与结果）

1. bash -n：通过。
2. 分类器重放（replay-harness.sh：从脚本逐字提取 extract_blocks/classify_file
   等函数后仅调用分类路径；每目录 blocks.txt + verdicts.txt 留档）：
   - f44-three-suite-replay/（F44 修复 ENOBUFS 后的真实三件套日志，当前
     真实形状）：块1=AssertionError、块2/3/5=guard ✗、块4/6=patch-too-large
     演化形态 → 六块全 seal-coordinate-lag，三文件类集合无 UNRECOGNIZED，
     OVERALL=仅登记类失败的分类治理态（非 fail-closed）。
   - s5-full-4-replay/（F44 修复前旧日志）：块4/块6=seal-coordinate-lag
     （演化形态生效）；块1/2/3/5=`Error: spawnSync git ENOBUFS` 崩溃形态
     保持 UNRECOGNIZED——崩溃非登记诊断，诚实 fail-closed（该形态已由 F44
     在测试侧修复，当前真实形状见 f44-three-suite-replay）。
   - r02-final-fc9971-replay/ 与 r02-final-e8761-replay/（R02 收口两条通过
     链回归护栏）：与各自归档 e5-candidate-causes.txt 逐字一致
     （seal / seal+uncommitted / seal+uncommitted）——既有登记形态零回归。
   - r03-g01f01-replay/（R03 链对比）：round2 块4 仍 seal+uncommitted；
     round3 块6 由历史 UNRECOGNIZED 变为 seal（即本轮登记的演化，其余不变）。
3. E0s 自测段重跑（e0s-rerun/）：按脚本行段逐字提取（449 fail、465 note、
   501-549 bind_worktree、568-590 validate_run_output_unit、610-790 sinks+
   declared-root、930-1479 SEAL_FAMILY+ledger+分类器、1481-2779 E0s 段）在
   隔离 EVIDENCE_DIR/WORK 执行：exit 0；62 例分类器 fixtures 全过（含 9 例
   新增 ptl fixtures：两正例 seal、七负例 UNRECOGNIZED；repair-group10 原
   8 例 window fixtures 结果逐字不变），F42 绑定 fixtures 全过。
   stdout.log/stderr.log/e0s-self-checks.log/summary.txt 留档。

## 四、边界与遗留

- 未跑完整 r02_t08（含 E5a 全量 npm test 与 base 重放）——按指示留总控
  静默窗口执行；本轮验证覆盖分类器判定路径与 E0s 自测。
- 生成器本体（artifacts/f1-f12-repair/**，受审材料）未改；补丁体积问题
  （≥3.67GB > git MAX_APPLY_SIZE）依旧存在，登记只让该已知族失败可分类。
- s5-full-4 旧日志中 ENOBUFS 崩溃块保持 UNRECOGNIZED 属预期（F44 已修）。
