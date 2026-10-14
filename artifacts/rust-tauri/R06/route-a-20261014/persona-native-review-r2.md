# 人格八叶与原生可用性修复 R2 独立复核

日期：2026-10-14。结论：**本轮限定范围 PASS**。上轮两项已复现问题都通过原独立反例与稳定对照；没有发现新的范围内阻断问题。此结论不表示 R06_ACCEPTED 或 R07_READY。

## 对象与隔离

在原独立树 `/private/tmp/lingxi-r06-review-persona-native`（detached 基座 2c8f8ddd0b75b4de61ebd047a0246a7f88f6d486）应用精确 R2 增量 `/private/tmp/r06-persona-r2-incremental.patch`，SHA256 `3d77b4c24b6a8dfded32a84b7bf260856d9a1c0676f226357952a5ece518de9f`。人格生产源码与完整正式测试源码都与当前主树 SHA256 相同。

原生可用性修复没有直接应用混有其它修改的整patch；从主树逐字复制 `require_native_write_sandbox`、`run_write`、`run_edit` 三方法与正式 unavailable 测试本体，复制后又逐字比较确保同源。没有夹带主树新的 ToolSuccess details 初始化，所以独立树 filetools 全文件 SHA 与主树不同；这里证明的是这三方法和真实调用链，不冒充其它主树变化已经受测。

没有修改主树、作者树、原 assertions、门禁、.vscode 或 .sync-audit；没有提交、推送。cargo `/Users/study_superior/.cargo/bin/cargo` 实际1.98.1，全部 --locked；合成HOME/LINGXI_HOME/TMPDIR/产品根沿用 `/private/tmp/r06-review-runtime`，只复用已安装cargo/rustup缓存，未读取真实业务数据。target仍为 `/Users/study_superior/.cache/lingxi-r06-builds/reviewer-target`，位于 /private/tmp 与 TMPDIR 允许范围之外，真实沙盒sentinel拒写位置有效。

## 两项问题的独立结果

### 人格材料竞态：PASS

原独立反例 `reviewer_sibling_material_change_is_refused_before_description_publication` 保留相同窗口：实际生产供应商请求已经发送后，外部用户修改另一AGENTS正文，再释放模型回答。现在实际 HTTP **409/persona_changed**；外部 `用户后改说明` 保持；目标identity仍旧字节；不创建description；事件数量不变。

本轮独立反例另加真实账本断言：已完成模型请求保留一行、Reported input23/output8、transport_attempts1、Succeeded、purpose auxiliary.summarize、origin persona.description、cause_ref agent:agentA，且session/run归属为空。这个Succeeded记录的是已完成且有用量的模型调用，不把本地提交冲突当成功写入，也不因409抹掉可能已收费的事实。随后没有外部改动的正常重试 **200/updated**，下一请求实际包含用户后改的新材料，正文与事件正常更新。用时2.66s，日志 `/tmp/r06-review-persona-r2-counterexample.log`。

### 原生 edit 不可用拒绝：PASS

原独立反例 `reviewer_edit_refuses_unavailable_native_sandbox_before_write` 相同 read→实际缺失助手→edit 序列，现在真实gateway返回 Failed/UpstreamUnavailable，错误文本含sandbox_helper_missing，原文件保持ORIGINAL、records=0、无临时残留。恢复真实系统助手后同一edit稳定对照Success，文件变为CHANGED、records=1。用时0.10s，日志 `/tmp/r06-review-native-r2-counterexample.log`。

4F0466F7C090原动作严格名为write，新增正式测试中的 write missing/untrusted/恢复成功/再次不可用与本轮read→missing→edit全部通过；edit是已修好的相邻变更入口一致性问题，没有将它伪装成原write叶自身失败。沙盒可用性probe不等于父进程native写入处于OS沙盒，路径/版本/原子性仍由真实FileTools/FileSafety负责。

## 新增13种来源变化与稳定对照：PASS

独立执行正式新增测试 `description_all_material_races_refuse_stale_commit_and_preserve_paid_usage`，实际读过其全部断言与生产快照实现，13种真实外部文件变化均409，13个无变化重试对照均200。

变体：另一AGENTS、另一identity、本次目标identity、Agent名称、yuan、Agent语言、用户名、全局语言、实际生效模板、最终样例、原先缺失的优先语言模板、新出现的自有正文、空目标所采用的identity模板。每项实测旧材料已进入请求、目标/简介不提交、外部后改保留、事件不增加、已完成模型账本不丢失，正常重试再更新简介和事件。用时34.19s，日志 `/tmp/r06-review-persona-r2-matrix.log`。

矩阵执行完才运行其它使用同一合成产品材料目录的回归，避免测试之间修改同名模板互相污染。

## 相关回归：55/55 PASS

- 原八叶具名检查及三个真实失败/回落检查：11/11（r06_persona_completion排除已单独跑的矩阵）。
- 原生文件工具：11/11。
- 真实OS沙盒：10/10。
- 助手生命周期：2/2。
- 上下文编译与下一Run：21/21。

日志 `/tmp/r06-review-persona-native-r2-regressions.log`。另有上面独立两反例2/2和矩阵1/1，所以本轮共58个顶层测试通过（矩阵含13拒绝+13稳定对照）。`git diff --check`通过。R2未修改头像实现；第一轮独立user/agent真实上传/删除I/O失败与原字节补偿证据仍在 `/tmp/r06-review-avatar-downstream.log`，本轮正式人格回归也重新覆盖原头像/权限/限额/ETag/链接/immutable拒绝链。

## 命令与复验坐标

环境前缀：

```sh
env HOME=/private/tmp/r06-review-runtime/home TMPDIR=/private/tmp/r06-review-runtime/tmp LINGXI_HOME=/private/tmp/r06-review-runtime/home LINGXI_PRODUCT_DIR=/private/tmp/r06-review-runtime/product CARGO_HOME=/Users/study_superior/.cargo RUSTUP_HOME=/Users/study_superior/.rustup CARGO_TARGET_DIR=/Users/study_superior/.cache/lingxi-r06-builds/reviewer-target /Users/study_superior/.cargo/bin/cargo
```

在独立树执行：

```text
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_persona reviewer_sibling_material_change -- --nocapture
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_persona_completion description_all_material_races_refuse_stale_commit_and_preserve_paid_usage -- --nocapture
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_native reviewer_ -- --nocapture
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_persona_completion --test r04_t04_file_tools --test r04_t06_sandbox --test r06_t01_context_compiler --test r06_agent_admin_lifecycle -- --skip description_all_material_races_refuse_stale_commit_and_preserve_paid_usage
```

独立反例路径：

- `/private/tmp/lingxi-r06-review-persona-native/rust/crates/lingxi-service/tests/r06_review_persona.rs`
- `/private/tmp/lingxi-r06-review-persona-native/rust/crates/lingxi-service/tests/r06_review_native.rs`

八叶逐叶正式测试名、sourceRaceTests名称、生产源码与反例SHA256、同源原生方法SHA256、日志及验证范围均在 `/tmp/r06-persona-native-r2-confirmation.json`，可供主控准备八叶绑定。绑定仍须按主控实际最终候选重验/阶段门禁规则推进，此文件不自动登记阶段验收。

限于macOS本机服务与loopback外部模型回答。未验证其它平台、真实付费供应商、桌面UI或正式打包；未跑全仓clippy/fmt或整阶段门禁。第一轮完整原assertions与历史FAIL证据保留在 `/tmp/r06-persona-native-review.md`，本报告取代它对这两个修复问题的当前状态判断。
