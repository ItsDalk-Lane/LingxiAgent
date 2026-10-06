# R05 RR1 WP-T01 R2 — F29 执行报告（2026-10-04）

执行者：R05-T01-修复-r1（全新上下文修复智能体）。范围：仅 F29 —— 把「真实管理入口 reload 与在途 401」组合场景固化为 lingxi-service 永久仓库测试。**不改任何生产行为。**

## 改动

- `rust/crates/lingxi-service/tests/r05_t01_model_plane.rs`：新增 `mod rr1_f29`（单测试
  `f29_management_reload_during_inflight_401_never_leaks_the_new_key`）。组合
  `c05_reload_through_management_surface_swaps_atomically` 的真实服务面
  （`boot_with_config` 组合根 boot + `lingxi_service::run` + `http_post`）与
  `rr1_f01_f02` 的 gated 401 stub 形状（raw-TCP 屏障 stub，请求 1 记录后 parked，
  401 释放；后续连接捕获并应答 poisoned SSE 使旧红可观察）。
- 台账：`RR1_ISSUE_MATRIX.json`（F29 状态流转）、`RR1_PROGRESS.md`。
- 本证据目录。

## 场景与断言（对 F29.remaining 逐条）

1. 真实组合根 boot + `lingxi_service::run` 服务面；在途 `execute_for` parked 在 A 的 401
   （first_seen 屏障证明请求 1 先带着旧钥匙 `Bearer dummy-real-entry-old-key` 抵达 A）。
2. 停车期间重写 `--config`（B 端点 + 新钥匙 `dummy-real-entry-new-key` + `stub-model-v2`），
   `POST /lingxi/v1/models/reload`：断言 200、`ok=true`、**`generation=2`**、网关
   `config_generation()==2`。
3. 释放 401 后：
   - **A 收到的请求总数恰为 1 且所有请求只含旧钥匙**（任何含新钥匙即红——泄漏检查放在
     最前）；
   - **gen-1 model call（`{run1}-mc0001`）无伪造 delta**：run 1 的全部
     `model_call_delta` 事件属于 mc0002；
   - **任何 B 派发属全新 model_call_id**：run 1 恰两次 model_call_started，
     `mc0001=(main, stub-model-v1)`（停车的一代）→ `mc0002=(main, stub-model-v2)`
     （有界重试以全新身份取当前代次再路由，非在途改写）；B 的 wire 请求体 `model ==
     "stub-model-v2"`；
   - run 1 经重试 completed.with_final，final 是 B 的二代答案，绝非 A 的 poisoned 文本；
   - **reload 后新 run**（run 2）经 B+新钥匙 completed.with_final（`served_identities ==
     [(main, stub-model-v2)]`；B 恰 2 个请求、全部 `Bearer dummy-real-entry-new-key`、
     绝无旧钥匙）。

## 旧红对照（隔离副本，已删除，不进主树）

- 隔离副本：`git archive d80737b6`（冻结 HEAD，即 F02 修复前形状）解包至
  `/tmp/lingxi-f29-headcopy-*`，追加同一测试模块；唯一接口演进适配是 plane 去掉
  `capabilities` 字段（HEAD 的 config schema 无该载体且 deny_unknown_fields 会拒绝；
  F29.remaining 明示「无 capabilities 字段的 plane」形状）。行为断言逐字未动。
- 结果：**exit 101**，泄漏被当场抓获——
  `the OLD endpoint A captured the NEW endpoint's key on request #1 … authorization: Bearer
  dummy-real-entry-new-key`（见 `old-red-f29-frozen-head.log`）。副本验证后已删除。

## 自检（全部经 `/Users/study_superior/.cargo/bin/cargo`，rust-toolchain.toml 锁定 1.98.1）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t01_model_plane rr1_f29 -- --test-threads=1` | 0 | 1/1 通过（另 3 次复跑均绿） |
| `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t01_model_plane` | 0 | 24/24 通过（23 既有 + 1 新增） |
| `cargo fmt --manifest-path rust/Cargo.toml --all --check` | 0 | 通过 |
| `cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-adapters -p lingxi-service --all-targets -- -D warnings` | 0 | 通过（初跑红于 needless_lifetimes，已修） |
| 隔离副本 `cargo test … --test r05_t01_model_plane rr1_f29`（冻结 HEAD 形状） | 101 | 预期旧红：A 第 2 请求携带新钥匙 |

日志：`green-f29.log`、`green-r05t01-model-plane-full.log`、`fmt-check.log`、
`clippy.log`、`old-red-f29-frozen-head.log`。

## 边界核对

- 无生产代码改动（`git diff` 仅 `r05_t01_model_plane.rs` 一个文件相对上一轮新增本模块）；
  无依赖变更；无 git commit/push。
- 全部合成凭证（`dummy-real-entry-*`），loopback stub，隔离数据目录；
  无真实供应商/真实 OAuth/真实密钥。
- 单一权威未动：reload 仍走 management.rs → CredentialService.reload（盖
  `upcoming_generation`）→ gateway.reload 的生产接线；测试只消费真实入口。
- 状态：F29 → IMPLEMENTED，待新独立审查者复核（含与 §5.2 I01 集成核验联动）。
