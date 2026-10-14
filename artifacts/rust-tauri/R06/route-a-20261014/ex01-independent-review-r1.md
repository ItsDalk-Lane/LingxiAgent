# EX-01 全新独立审阅：NEEDS_REPAIR

本次不能关闭 EX-01。正常手动/fresh 持久压缩、真实配置消费、失败保全及取消保护有独立运行证据；但材料/工具的最终同代保护存在一个真实提交竞态。它使 fresh-compact 在保存等待期间材料或工具已变更后，仍返回成功并保存旧刷新快照。两条反例同一根因，记为一个 P2。不签 R06_ACCEPTED / R07_READY，R06 仍未验收；不改门禁、断言、台账、主树、作者树，不提交/推送。

## 候选与方法

- 实际独立候选：`/tmp/lingxi-ex01-independent-review`，HEAD `2c8f8ddd0b75b4de61ebd047a0246a7f88f6d486`，应用 `/tmp/lingxi-ex01.patch`。
- EX01补丁 SHA256：`422320189356e299d3a6ecb1ce705be3ab14e0efb4d9068f59d6a3806380b63c`。未额外应用主控 persona/native 修改；基座自带已有提交内容。
- 亲读当前本地 AGENTS.md、R06-EX-W01_SPEC.json，以及现役 core/session-defaults.ts、core/session-compaction-runtime.ts、core/session-compactor.ts 的实际设置、手动入口、关注指令与硬截断处理；再读候选 Rust 入口、配置、历史、保存、运行链。
- 独立测试复用作者 HTTP/供应商/启动辅助设施，但新增实验步骤与断言。HTTP认证、ModelGateway、AuxiliaryExecutor、driver、ContextCompiler、SQLite与模型历史均为生产实现；只替身远端模型。并发是实际外部 SQLite 写者、真实 PUT 配置、外部配置文件编辑或实际 ToolRegistry 生命周期变更，没有 mock 被验收核心。
- 环境：cargo `/Users/study_superior/.cargo/bin/cargo`，版本 `1.98.1 (797e8a9bc 2026-08-05)`；所有 cargo 均 `--locked`。独占仓库外 target `/Users/study_superior/.cache/lingxi-r06-builds/ex01-review-target`。LINGXI_HOME `/tmp/lingxi-ex01-review-env/home`，TMPDIR `/tmp/lingxi-ex01-review-env/tmp`。
- 保留独立实验：`/tmp/r06-ex01-independent-review-tests.rs`；原运行文件 `/tmp/lingxi-ex01-independent-review/rust/crates/lingxi-service/tests/r06_ex01_independent_review.rs`。

候选核心文件 SHA256：

| 文件 | SHA256 |
|---|---|
| compaction.rs | b5dcc0201dc05423667caaef524cc3f1bdf29d6a650e441394af1a2a30880917 |
| history_content.rs | be508bceca367e35a15b7e8d2b64db51b4d16cdcd385a1c4848d44bd7d066faf |
| model_history.rs | 80f0a30f648e4be216d4feafa18e6be0321d7b2c594ffb3bf37ad6874f9da789 |
| runs.rs | 4ba565e2198381e946f2768a291c043cd6198415ad13734c691df835fb7cc4c7 |
| adapters/storage/session_tree.rs | 98c5ad972b59da82bc07a8c497dfccc0f029c23366d646cd1828b9410104d2ee |

## P2：刷新快照检查与真实提交之间允许材料/工具跨代

精确位置：`rust/crates/lingxi-service/src/history_content.rs:1675`（1650–1677复检材料、目录、设置，1695仍保存之前快照，1700异步提交）；`rust/crates/lingxi-service/src/model_history.rs:852`（856–879提交前还有多次异步DB操作，896–909最终 allowed 闭包只裁决取消）；`rust/crates/lingxi-adapters/src/storage/session_tree.rs:249`（最终事务核对head/revision/session/cancel，未核对材料/工具/设置）。

独立实验，材料腿1064–1107、工具腿1109–1153：

1. 真 execute 创建4轮长历史，真 PUT 设置 keepRecentTokens=1；经真实 fresh-compact HTTP 触发摘要。
2. 远端替身返回摘要。独立外部SQLite连接轮询，必须看到该会话 `auxiliary.summarize` usage 已落盘，然后取得 `BEGIN IMMEDIATE` 写锁；锁内必须确认 manual档案COUNT=0，确保原请求尚未提交成功。
3. 等250ms后，实际数据库查询在队列中100ms超时，证明生产写者已等待外部锁。此时修改材料：将助手配置姓名 `EX01-OLD-NAME` 改为 `INDEPENDENT-AFTER-USAGE-NEW-NAME`。工具腿改为对实际目录调用 `set_availability` 禁用 `tool:first-party:edit`，目录世代从25进到26。
4. 释放外部写锁。正确行为应409、原有效历史不变；实际两腿都200、`ok:true,fresh:true,degraded:false`，并产生持久manual档案。
5. 材料档案的 `refreshedContext.render` 仍为 `你叫EX01-OLD-NAME`；工具档案仍为catalogGeneration25，包含已经禁用的edit声明。结果确实是保存旧快照，不只是返回字段错误。

材料腿单跑复现一次、与工具腿一起再次复现；两次都在首个可观察窗口取得反例，没有以随机失败代替达点证明。日志 `/tmp/ex01-review-late-commit.log`、`/tmp/ex01-review-late-commit-confirm.log`。确认日志结果 `0 passed; 2 failed`，失败断言409与实际200。

影响：原规格 D62A81 要求按当前Agent配置重建提示词和工具快照，这一条件在最终保存前变更时不成立。下一Run会重新采集材料，不据此声称下一Run永久使用旧设置或真的执行禁用工具；问题是当前刷新压缩成功结果及持久刷新凭据并非同代，旧历史已被成功压缩投影覆盖。

修复边界：把材料/工具/设置快照同代校验纳入实际保存事务的最终裁决，和取消phase及分支CAS一致；对生产配置/目录/材料世代或锁明确线性化时机。不能只在HTTP入口增加一次重读，之后的异步读和排队仍会留下窗口。必须保留上述真实写锁反例与409/原史保全断言；并验证取消成功与档案提交不能同时发生。

## 独立通过证据

作者新增6个场景在本候选由审查者重新执行6/6，通过而非引用作者数字：手动禁自动仍可压缩、focus真实供应商线体、非假Run来源和usage cause_ref、下一Run、重启、源删除后fork、fresh当前材料/工具、真实PUT keepRecent/reserve/enabled、禁自动预算不足不发送、坏请求/权限/未知/忙、供应商错误、CAS同head不同revision、摘要途中材料变更、SQLite实际写失败、未知manual来源和篡改保留尾部拒绝、显式硬截断。

额外独立实验：

- 摘要过程中真实PUT reserveTokens变更与实际ToolRegistry禁用，均409 `compaction_context_changed`，原history/head/revision/run计数完全相同。测试991–1016。
- 写锁先阻塞摘要usage结算时取消/外部材料变更，均409并保全历史。测试1018–1062。**这两个早期探针不能证明最终提交窗口**；原测试名含commit/recheck，只用于描述已观察结果，不以命名代替证据。
- 另一个真正usage落盘后、manual档案不存在且提交等待的取消探针1155起：调用同一生产取消域成功赢得裁决，释放写锁后HTTP409 `compaction_commit_refused`，原history/head/revision/run计数逐字一致。日志 `/tmp/ex01-review-commit-cancel.log`，1/1通过。这个探针使用生产取消域API控制竞争；HTTP取消认证由作者6场景的真实HTTP取消复跑覆盖。

独立重跑的既有回归（均原断言）：

| 套件 | 结果 | 日志 |
|---|---:|---|
| r06_ex01_manual_compaction | 6/6 | /tmp/ex01-review-author-tests.log |
| r06_ar_f01_history | 8/8 | /tmp/ex01-review-regression-tests.log |
| r06_session_history_content | 12/12 | /tmp/ex01-review-regression-tests.log |
| r06_t02_closed_loop | 2/2 | /tmp/ex01-review-closed-loop-product.log |
| r06_t02_compaction | 26/26 | /tmp/ex01-review-remaining-regression.log |
| r06_t03_session_tree | 14/14 | /tmp/ex01-review-remaining-regression.log |
| adapters session_tree_r06t03 | 18/18 | /tmp/ex01-review-adapter-tree.log |

合计原有86场景独立复跑通过；新增独立正向4场景通过，另2场景是确认失败的材料/工具晚期竞态。不能用86/86覆盖独立反例。

二进制closed-loop首次2/2失败：外置target无法向上找到产品lib，合成home缺产品素材；Run在模型调用前failed.provider_error。未改测试或断言，明确提供 `LINGXI_PRODUCT_DIR=/tmp/lingxi-ex01-independent-review/lib` 后同候选2/2通过。首次失败日志仍保留，不计为最终通过，也不将补环境误写成代码修复。

## 复现命令

在 `/tmp/lingxi-ex01-independent-review` 执行，统一前缀：

```sh
env LINGXI_HOME=/tmp/lingxi-ex01-review-env/home TMPDIR=/tmp/lingxi-ex01-review-env/tmp CARGO_TARGET_DIR=/Users/study_superior/.cache/lingxi-r06-builds/ex01-review-target /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_ex01_independent_review independent_post_usage_ -- --nocapture
```

上面过滤器包含材料、工具和晚期取消三场景，预期当前候选2失败+1通过。单独取消：过滤 `independent_post_usage_precommit_cancel`。早期3实验：在添加晚期实验前曾使用 `independent_` 过滤，日志 `/tmp/ex01-review-independent-tests.log` 为3/3；现文件完整重跑这个过滤会包括确认失败的晚期场景。

原6场景：同前缀 `--test r06_ex01_manual_compaction`。回归命令：同前缀分别 `--test r06_ar_f01_history --test r06_session_history_content --test r06_t02_closed_loop --test r06_t02_compaction --test r06_t03_session_tree`；首次closed-loop失败使后两套未执行，随后用 `--test r06_t02_compaction --test r06_t03_session_tree`补跑。二进制closed-loop有效复跑命令在统一env中再加 `LINGXI_PRODUCT_DIR=/tmp/lingxi-ex01-independent-review/lib`，仅 `--test r06_t02_closed_loop`。Adapters命令使用 `-p lingxi-adapters --test session_tree_r06t03`。

## 范围与未验证面

严格Run档案仍先检查真实输入锚（model_history.rs:495–516），manual来源要求独立action、禁止sourceRun/attempt，并比对原史digest及保留后缀（68–147）；旧history完整性/工具opaque/源删除fork兼容由原F01真实运行回归覆盖，未扩大旧缺档案工具历史白名单。源删除后的manual fork由真运行6场景覆盖，非伪造新Run。

本次未新增“提交CAS已赢、数据库COMMIT尚未完成”取消失败的专门故障注入；该臂只做源码核对，晚期取消实验覆盖取消先赢。未实跑断电/磁盘满、跨Windows/Linux、真实付费供应商、正式打包。slash接管、实验开关、完整WS accepted→succeeded/noop/failed及旧UI接线不在此次服务核心实现/验证中，不能将相关整叶翻READY。未对主控同批persona/native集成候选作总验收；主控需在修复后重新整合、复核并按原78逐项完成。
