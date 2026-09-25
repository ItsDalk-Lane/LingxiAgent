# R01-T08 /tmp 清理记录（2026-09-25，ZCode:R01-T08-exec-r1）

- 范围：R01 各轮（T01–T08 执行/评审/修复）在 /tmp 的克隆、构建缓存、日志与夹具副本，
  共 **74 项、约 36GB**，逐项清单见 `r01-tmp-inventory.txt`（删除前 `du` 快照；
  总量记录于 `r01-tmp-total-size.txt`）。
- 删除前保留动作：T08 本轮 handshake 证据（transcript.jsonl/server.log）已复制入
  `artifacts/rust-tauri/R01/T08/gates/`；T06 SHASUMS 引用的 /tmp 二进制哈希已录入
  `../disposition/t06-binaries-disposition.json`。
- 明确未触碰（非 R01 范围）：`/tmp/lingxi-r00-*`（R00 各轮）、`/tmp/lingxi-clone`、
  `/tmp/lingxi-baseline*`、`/tmp/lingxi-c26-*`、`/tmp/lingxi-fulltest-*`、
  `/tmp/lingxi-typecheck-*`、`/tmp/lingxi-f02-*`、`/tmp/lingxi-confirm-head.txt`、
  `/tmp/lingxi-debug-profile`。
- 与 RR-T08-F1 的关联：被删的 `/tmp/lingxi-r01t02-target`（共享构建缓存）与
  `/tmp/r01t02-review-clone`（评审遗留克隆）正是「check-generated 错绑遗留克隆树」
  的实体。错绑事实与两轮对照日志已留证于 `../gates/t02-check-generated.log`（错绑轮）
  与 `../gates/t02-check-generated-fresh-target.log`（全新 target 正确轮，exit 0）；
  删除残留只是清理，不消除门禁脚本缺陷本身（修复归属见 RISK_REGISTER RR-T08-F1）。
- 本任务自留工作目录（评审期间保留，评审后可删）：`/tmp/lingxi-r01-t08-rust-target`
  （cargo 测试/生成门禁的本轮 target 缓存）。
