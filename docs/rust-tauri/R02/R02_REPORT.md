# R02｜Rust 独立服务、存储与事件基础 — 阶段报告

按 91 交付模板填写。本文是**执行者口径的阶段候选报告**：阶段状态
READY_FOR_REVIEW；最终 PASS/ACCEPTED 归总控另派的独立验收代理。

## 1. 阶段与结论

**READY_FOR_REVIEW**（T01–T07 各任务经独立对抗验收 PASS(R1/R2/R3)；
T08 为本阶段收口任务，执行者自评 READY_FOR_REVIEW，待独立验收）。

16/16 基础验收场景由收口执行者以真实命令全量复跑（xtask verify-stage R02，
overall PASS），状态与证据指针见 `docs/rust-tauri/R02/R02_ACCEPTANCE_LEDGER.json`。

## 2. 范围

- 本阶段授权：R02 任务书 8 项任务（T01 workspace/组合根 → T08 交付与门禁）。
- 与基线差异：TASK_BASE（=T07 收口提交 574198916…）之后仅 T08 的未提交工作树
  （§10 全集）；获准变更 = 任务书 R02-T08 五步 + RISK_REGISTER 明确归属 R02 的
  RR-T08-F1 硬化 + 移交修正清单（T07 F01–F04 / T02 F02 / T04 F01/F02 /
  T06-R2 F05 惯例 / FINFO1），逐项对质表见 R02-T08_REPORT §7。
- 不包含：真实供应商连接、真实高权限工具、生产默认入口替换、新 UI、
  数据迁移（归 R08）。

## 3. 源码

- 起始 SHA：`5741989165fe7e04c9a58a9d35c7747d3599d274`（= 分支
  codex/rust-tauri-migration 远端 HEAD；工作区开工时干净）。
- 终点：同 SHA + T08 未提交工作树（17 个跟踪文件修改 + 11 个未跟踪新文件/
  目录；工作树 digest `94f95e7a53dac99f5cfacea2e6eca3043da6ff20afb261cf9c68b7388ce32332`，
  9707 条目，明细 `artifacts/rust-tauri/R02/T08/working-tree-digest.txt`，
  计算器 `docs/rust-tauri/R02/r02_t08_worktree_digest.py`）。
- 依赖锁摘要：`rust/Cargo.lock` 5c43390c894421e6（T08 仅 +7 行 xtask 成员块，
  零新增第三方包/版本）；`rust-toolchain.toml` eec3410410647a7d（1.98.1）；
  `package-lock.json` e54a16fe14f15b47（未动）。

## 4. 环境

- macOS 27.0.0 arm64；rustup 1.29.1 + 锁定 rustc/cargo 1.98.1（组件
  rustfmt/clippy）；cargo 全程 `--locked` + 离线 + 专属
  `CARGO_TARGET_DIR=/tmp/rust-target-r02-t08`；网络命令剥代理。
- 测试数据：全部 mktemp 合成 /tmp home；合成敏感值；无真实账户/数据。
- 外部替身：无（本阶段所有验收均为本机确定性场景；mock 边界在各任务报告）。

## 5. 完成项（逐 T-ID）

| 任务 | 交付与关键实现 | 代码定位 | 独立验收 |
|---|---|---|---|
| R02-T01 | workspace + 组合根；health；依赖树桌面零依赖证明 | `rust/crates/lingxi-service`、`rust/Cargo.toml` | PASS(R1) |
| R02-T02 | 配置/路径优先级、0700 布局、实例身份与单写者锁、启动诊断 | `src/config.rs` `paths.rs` `instance.rs` | PASS(R1) |
| R02-T03 | HTTP/WS 认证与资源范围（loopback token、票据、Origin/Host 守卫、速率） | `src/auth.rs` `transport.rs` `ws.rs` | PASS(R1) |
| R02-T04 | StoragePort：新 runs.db 迁移、有界单写者队列、同事务终态+事件、真实故障注入 | `rust/crates/lingxi-adapters/src/storage/*` | PASS(R1) |
| R02-T05 | 事件顺序/快照/断线续读（envelope、snapshot+游标、显式 detach） | `src/events.rs` | PASS(R2) |
| R02-T06 | 在线备份、graceful shutdown、epoch 闸 fail-closed（PROD-DEFECT-1 闭口）、启动恢复 | `src/shutdown.rs` `epoch.rs`、adapters `backup.rs` | PASS(R2) |
| R02-T07 | 结构化 error/causeId/requestId、脱敏+轮转日志、8 上限旗标、注入时钟/ID | `src/redaction.rs` `logging.rs` `inject.rs` `lib.rs` | PASS(R1) |
| R02-T08 | 真实二进制全链冒烟；xtask 三命令+负向；RR-T08-F1 硬化；移交修正收口；R00 map 覆盖层；启动/关闭手册 | `rust/crates/xtask`、`devgate.rs`、`scripts/rust-tauri/r02_t08_*`、`docs/rust-tauri/R02/{SERVICE_START_AND_SHUTDOWN.md,R02_IMPLEMENTATION_MAP.json}` | 本报告（READY_FOR_REVIEW） |

## 6. 行为变化

对用户可见的变化：**无**（R02 新栈不接入任何生产入口；Electron/Node 默认
启动、行为、数据根零变化——R02-A16 证据）。新栈专属行为（服务二进制、
HTTP/WS 面、退出码、资源上限）是 R03+ 的基础面，尚未面向用户。

## 7. 验收（逐 A-ID）

16 场景全 REQUIRED。执行方式 = `cargo run -p xtask -- verify-stage R02
--evidence artifacts/rust-tauri/R02/T08/verify-stage`（overall PASS；每场景
真实命令/真实退出码/时长/日志落盘）。逐项命令、预期/实际、退出码、日志：
**R02_ACCEPTANCE_LEDGER.json**（机器可核）+ R02-T08_REPORT §6（含 A15/A16
细节与 xtask/F1 负向矩阵）。要点：

- R02-A15 全链（启动→认证→写→订阅→关闭→重启读取）：PASS；无遗留子进程/端口
  （pgrep/curl 证据在链内）。
- R02-A16 不影响旧入口：PASS；旧入口面 diff=空、typecheck×2+边界门禁绿、
  全量 npm test 失败族⊆预存在封印族（失败清单 0 个 T08 文件）。
- 负向面：xtask 未知阶段/空集/缺证据/超时/假成功（exit0 无证据）共 8 类
  单测 + 2 类 CLI 负向 + 1 次实战（run1 缺证据 FAIL，已归档）；
  RR-T08-F1 修复前错绑绿/修复后拒绝 6 件套。

## 8. 安全与数据

- 权限负向：伪造身份/Origin/Host/过期票据/跨主体（A05/A06 + T07 对抗变体）。
- 真实进程：全部二进制级脚本 + A15 双进程链；关闭后遗留进程/端口检查为零。
- 单写者：OS 文件锁权威 + 陈旧记录归档接管（A03）。
- 迁移/回滚：epoch 闸 fail-closed（损坏不自动丢弃，A12）；在线备份可恢复
  （A11）；新栈与旧栈数据根分离，旧栈零改动（ADR-004 + A16）。
- 敏感面：脱敏器镜像 + 全量扫描（A13）；token 0600；日志 0600/0700。

## 9. 完整映射

机器可核映射链：

- 功能/入口/存储盘点：R00（三份 map，封存）+ R02 覆盖层
  `R02_IMPLEMENTATION_MAP.json`。
- 任务→场景→测试→结果→证据：`R02_ACCEPTANCE_LEDGER.json`（16 场景）
  + 各 `R02-T0x_REPORT.md` 的 §验收/§门禁 + `R02-T0x_STORAGE_REGISTRY.json`。
- 阶段图（xtask）：`rust/crates/xtask/src/stage_maps/R02.json`。

未覆盖集合：R00 补充场景中归属 R03+ 的部分（未展开即未执行，见各阶段
任务书）；R02 阶段无未覆盖的 REQUIRED 场景。

## 10. 已知缺陷

1. **审计封印坐标落后**（预存在，R01 同款；非本阶段引入）：seal 族测试在
   当前 HEAD 红（失败清单=已提交 R01/R02 交付文件，0 T08 文件）。修复归属
   封印工作流（需授权提交后按 PROGRESS 封印流程推进），不是代码缺陷。
2. **FINFO1**：contentSha 无法仅由矩阵重算（已按授权补说明于 PROTOCOL_SPEC
   §10；登记 RR-T02-FINFO1 维持 OPEN→修复待独立验收确认后由总控关闭）。
3. **RR-T08-F1**：已按 RISK_REGISTER 建议方向双保险硬化并留负向证据；
   登记状态 OPEN→修复待独立验收确认后由总控关闭。
4. 新栈已知边界（不阻塞）：Hub 流注册表 4096 上限的跳扇出兜底（R03+ 复核）、
   脱敏器 `/`/`=` 边界（R05 provider token 前必须复核，已入 R02_HANDOFF
   unresolved_items）、跨平台（Windows 权限/轮转）未验证（R09/R10 关卡）。

## 11. 未执行 / BLOCKED

无 BLOCKED 场景。未执行项（均非本阶段必需）：真实 Electron GUI 启动验证、
跨平台、真实供应商（R05）、真实打包安装（R09/R10）——见 R02-T08_REPORT §8。

## 12. 回退

- 新栈回退：停止 lingxi-service 进程、删除合成 home 即可（无生产数据写入；
  新栈从不指向真实用户目录——无默认根）。
- 门禁回退：RR-T08-F1 硬化不改变门禁语义（绿↔绿）；三脚本默认 target 派生
  可被显式 CARGO_TARGET_DIR 覆盖。
- 本次后新增数据：全部在合成 /tmp home 与 artifacts/，回退不丢失任何用户数据。

## 13. 独立审查

由总控另行指派（本阶段各 T 已分别经独立对抗验收；T08 与阶段级验收待派）。
建议重点见 R02-T08_REPORT §10。

## 14. 下一阶段（R03）

- 允许范围：R03 任务书（运行状态机、并发、取消与恢复）。
- 必须输入：`R02_HANDOFF.json`（interfaces/资源上限/错误码/注入面）、
  `R02_IMPLEMENTATION_MAP.json`（R00 原件+覆盖层）、
  `SERVICE_START_AND_SHUTDOWN.md`、xtask 阶段图机制（R03 建图时按
  stage_maps/R02.json 模板新增并注册 STAGE_MAPS）。
- 不允许开始：R04+ 任务；任何生产入口切换；真实数据迁移。

## 15. 远程 / 发布

未获准、未执行。无 commit/push/PR/tag/release。
