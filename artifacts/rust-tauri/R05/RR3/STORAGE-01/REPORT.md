# RR3 STORAGE-01 空间准备结果

结论：SPACE_INSUFFICIENT，当前不能安全启动下一轮默认 negative clone。任务范围内可安全删除的缓存已处理完，停止写入。没有运行 Cargo、测试或新代理。

- 删除前可用空间 2684772352 bytes（2.500 GiB）；删除后即时 2943180800 bytes；保留校验结束 2941267968 bytes（2.739 GiB）。
- 仅按清单删除 3236 个非可执行中间文件：逻辑长度 486645765 bytes，按唯一 inode 统计占用 256249856 bytes。磁盘即时净增加 258408448 bytes（246.44 MiB）。同盘 H 正在工作，净增受并行写入、硬链接/APFS共享块影响，不把逻辑长度虚称实际释放。
- 距 brief 的 3.5 GiB 独立 clone 基础需求仍差 816828416 bytes（778.99 MiB），还未包括编译余量。仅本 receipt 为保守空间判断采用 1 GiB 工作余量，4.5 GiB 观察目标仍差 1890570240 bytes（1.761 GiB）；不是实测编译峰值。
- 所有 5341 个保留文件/链接逐项前后校验一致，零缺失或变化。被删路径全部确认不存在。106 个虽属中间文件、但被历史清单路径/摘要引用的候选完整保留。

## 归属与仅有的两处删除范围

1. `/private/tmp/lingxi-rr3-a-review-01-target`（与 `/tmp/lingxi-rr3-a-review-01-target` 相同）：A-REVIEW-01/commands.json 中真实 Cargo 命令的 CARGO_TARGET_DIR；对应命令完成时间、exit、cwd、argv 摘录保存在 before.json.ownerProof。A-REVIEW-01/REVIEW.md 和 A-REVIEW-02/REVIEW.md 证明审查已结束。实际 xtask 与测试二进制保留。
2. `artifacts/rust-tauri/R05/RR3/I-REVIEW-01/permanent-final/own-target`：permanent-final/commands.json 原命令使用此独立输出根，I-REVIEW-01/REVIEW.md 已 PASS 结束。命令原件 SHA 和实际命令摘录在 before.json。真实二进制及原始命令、输入、序列、日志、源码、隔离故障正文全部保留。

逐文件限制：只删除以上两根内 incremental 的非可执行缓存，以及 debug/deps 的非可执行 .rlib/.rmeta/.o；没有按目录整删，也没有移除空目录。扫描 artifacts/rust-tauri 下 627 份历史 manifest/digest/sha256 文件，候选绝对/相对路径、文件名或内容 SHA 一旦命中就保留。逐文件 SHA、尺寸、inode、硬链接数、时间和 ownerRoot 见 before.json；删除时间见 deleted-files.jsonl。各根统计见 deleted-summary.json。

删除前两轮 lsof +D 均退出 1 且无 stdout/stderr，表示没有打开文件；ps 无这两个目标路径的活跃进程。主 rust/target 的 H 构建与旧 G 只读观察进程在快照中可见，均未中止或修改。检查命令、UTC、退出码和原始 stdout/stderr 存 before.json 与 pre-delete-checks.json。

## 保留与边界

主 rust/target、NEG_TARGET（/Users/study_superior/.cache/lingxi-r05-neg-target）、全部 G 证据、H-REVIEW-02、RR1/RR2、用户临时目录、Git 主库、生产源码、脚本与 docs 均未写入。新增文件仅本 STORAGE-01，删除仅上述明细。

I-01/manifest.json 引用 own-target 共 3290 处，其中 incremental 1160 处，因此四个 I-01 缓存全部原样保留。只读候选体积见 readonly-candidates.json，不能据此继续清理。H 主缓存仍独占，本任务未评估其删除条件。当前不存在本任务可以继续处理以补齐差额的候选。

5341 个前后摘要覆盖 A-REVIEW-01 全包、I-REVIEW-01 全包及外部 A target 中的所有保留文件（含所有在这两根受影响目录内的实际二进制）；不冒称对全部 RR3 或正在变动的 H/G 做总体冻结。原始 manifest 未改写，也未以摘要替代其要求的原件。

## 可复核的实际操作

UTC 2026-10-07T03:12:28.513494+00:00—2026-10-07T03:12:29.472576+00:00。

```text
python3 artifacts/rust-tauri/R05/RR3/STORAGE-01/prepare.py
python3 artifacts/rust-tauri/R05/RR3/STORAGE-01/cleanup.py
```

两条实际退出码均为 0。prepare.py 只读历史并写本包 before.json；cleanup.py 在删除前重新检查活跃进程、每个候选原始 SHA/inode/mtime，再逐文件 unlink 并持久记录，最后逐项校验保留原件。脚本原文保留。未执行任何 cargo clean、rm -rf、Git 写命令或系统改动。

结构化 receipt：before.json、pre-delete-checks.json、deleted-files.jsonl、deleted-summary.json、preserved-after.json、after.json。状态 SPACE_INSUFFICIENT；总控需要另行获得至少上述空间差额和实际编译余量后再启动。
