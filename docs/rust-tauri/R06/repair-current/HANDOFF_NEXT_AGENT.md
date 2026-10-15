# R06 收口最终阶段交接（2026-10-14）

## 你是谁、从哪继续

你是 R06 收口的接力执行者。前一个 Agent 已完成 95% 的工作，剩最后一段：**C3 最小宿主 → 全量绑定 → verify-stage R06 正式门禁 → 推送 → R07_READY**。用户已授权最终提交并推送 `codex/rust-tauri-migration` 分支。

## 当前状态（HEAD `5ab3cee77`，树干净）

- **15/15 AR 发现关闭**：14 项独立审查 PASS（RV1–RV7 全新空历史子代理于冻结候选独立克隆亲跑锚点验证），F14 由正式门禁关闭（尚未执行）
- **194 READY / 85 GAP**（279 项叶子绑定）
- **阶段审查 CONDITIONAL_PASS**（STAGE-REVIEWER-R06-AR-R1）：C1 已修，C3 进行中（见下），C2=门禁 exit 0 即 F14 关闭
- 全量测试绿（lingxi-service 515/1 环境限、kernel 187/0、adapters 433/0）、clippy/fmt 净、契约测试 20/20
- GUI 真实链路已通过（用户配合两次原生弹窗点选；两叶以人工证据回执绑定翻 READY）

## 立即要做的：C3 五类到期最小宿主（阶段审查裁定 b 命中项）

前一个 Agent 的 C3 代理刚被中止（额度用尽），**没有落盘任何改动**。你需要重新执行：

### 组① R00-T02-LA-BC675C664910：config.yaml 完整 GET + 秘密掩码
- 断言原文：`python3 -c "import json; d=json.load(open('scripts/rust-tauri/r06_original_leaf_bindings.json')); print([r for r in d['items'] if r['id']=='R00-T02-LA-BC675C664910'][0]['assertions'])"`
- 语义源：现役 `server/routes/agents.ts` 的 GET `/api/agents/:id/config`（完整读回 config.yaml 真实内容、秘密字段掩码 `********`）
- Rust 已有 PATCH/PUT 面（`rust/crates/lingxi-service/src/agent_config_readback.rs`）——补 GET 处理器（pub async fn + lib.rs 路由注册 + auth.rs classify），测试扩展 `tests/r06_agent_config_readback.rs`

### 组② 知识来源约束五叶：R00-T02-LA-01E1E3CF7FA8、8A62470CC75D、9283371CDAD8、76E2F5AAF035、5F5365322CFF
- 共同语义：现役 `server/routes/knowledge.ts` 的 notebook 删除/来源清理链（删除接口连带清理所属来源、冻结引用保护）
- 阶段审查亲读源码确认 `knowledge.rs`/`knowledgetools.rs` 未实现。逐叶：读断言→实现（真实存储、参数绑定 SQL、不静默降级）→测试→bind 翻 READY
- 测试环境：`LINGXI_HOME=/tmp/<env>/home TMPDIR=/tmp/<env>/tmp`（r06_t07/r06_original_knowledge 套件需要）

### 组③ R00-T02-LA-269958F82F8A 重锚
- gap 文案过时（越权拒绝已在源码实现并有断言覆盖，属保守 under-claim）。核实后更新 gaps 文案为当前真实状态并 bind 翻 READY

### bind 翻绿模式（照既有先例）
```
# 参考脚本：artifacts/rust-tauri/R06/continuation-20261013/bind-gap-blocks-c1eab-batch1.py
# 核心：断言原文不可动、groups 带 proofLocations（用同目录 review-proof-anchors.py 的 test_location）、
# 全 FULL 才翻 READY、reviewed-original-ids.json 追加
```

## C3 完成后的收尾序列（按顺序，不可跳步）

1. **登记链**：`python3 -B artifacts/rust-tauri/R06/continuation-20261013/refresh-drifted-anchors.py` → `python3 -B scripts/rust-tauri/r06_stage_inputs.py --generate` → `python3 -B scripts/rust-tauri/tests/test_r06_stage_inputs_contract.py`（须 20/20 OK）
2. **干净全量绑定窗口**（期间禁止任何树改动/提交！）：
   ```bash
   rm -rf artifacts/rust-tauri/R06/continuation-20261013/full-binding-final
   python3 -B artifacts/rust-tauri/R06/continuation-20261013/execute-reviewed-proofs.py \
     --evidence artifacts/rust-tauri/R06/continuation-20261013/full-binding-final \
     --proof-leaf-ids artifacts/rust-tauri/R06/continuation-20261013/reviewed-original-ids.json
   ```
   全 PASS（~134 绑定，1-2 小时）
3. **提交 C3 成果**
4. **verify-stage R06 正式门禁**（C2 条件：exit 0 即 F14 关闭、R06 验收）：
   ```bash
   /Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- \
     verify-stage R06 --evidence /绝对路径/全新空目录
   ```
   内嵌 R05→R04→R03→R02 全链回归（timeout 14400s），整链预计数小时，后台运行+轮询
5. **账本终态**：AR_PROGRESS.json → `R06_ACCEPTED`、stage_gate → `PASS`；AR_ISSUE_MATRIX F14 → `DONE`；AR_HANDOFF.md 追加终态记录
6. **推送**：`git push origin codex/rust-tauri-migration` → 取远端 SHA 回执
7. **R07_READY 声明**

## 工具链与红线

- cargo 必须用 `/Users/study_superior/.cargo/bin/cargo`（锁定 1.98.1，`--locked`），CARGO_TARGET_DIR 用仓库外专用目录
- 不动 `.vscode/settings.json`；SQL 参数绑定；不静默降级；禁止 `#[ignore]` 或放松断言
- 审计封印坐标（`.sync-audit/`）不在本次范围
- 阶段审查完整报告：`artifacts/rust-tauri/R06/AR/STAGE-REVIEW-R06-AR-R1/stage-reviewer-r1/REPORT.md` + `VERDICT.json`
- 所有独立审查产物：`artifacts/rust-tauri/R06/AR/STAGE-REVIEW-R06-AR-R1/independent-reviews/`（rv1–rv7）
- 交接详细流水：`docs/rust-tauri/R06/repair-current/AR_HANDOFF.md`（2026-10-13/14 各段）

## 代理使用限额注意事项

每个子代理有 5 小时额度。大任务拆小派发（如 C3 三组可拆三个代理并行，文件面零交集时）；代理死后检查落盘状态再接管（前一个 C3 代理没落任何东西）。主控自己不要在绑定窗口/门禁运行期间做任何树改动。
