# RR3 / R02-TRIAGE-01 独立根因取证报告

取证者：rr3_r02_triage_01；本轮全新、未参与 RR3 实施或验收。结论性质：**定位完成，建议两项必需修复；未实施，未签修复 PASS，未作 G/FINAL 整体结论。** 所有本次新写入仅在本目录。未派代理、外发消息、修改生产/现行文档/总控台账、操作 Git 写入或系统许可；未构建、修改或运行共享 NEG_TARGET，也未改 negcopy.hTXzzN。历史 FAIL 与原规格全部保留。

## 1. 交总控的决定

| 现有稳定观察 ID | 判定 | 本 R05 是否必需 | 最小处理范围 |
|---|---|---|---|
| RR3-R02-OBS-A01 | 检查器环境依赖：把 INFO 选根诊断缺失算成“切根”；现场实际拒启、原数据及备用根均保持 | 是，修检查器和有效复证；不是修数据版本/存储行为 | `scripts/rust-tauri/r02_t01_service_smoke.sh` 的日志前置及必要回归；保留所有拒启/原文件/备用根断言 |
| RR3-R02-OBS-A13 | 产品脱敏误伤：真实 `request_id=req-<32hex>` 连同字段名被长 token 规则遮掉，真实关联断言失败 | 是，既有 R02 关联契约及 R05 诊断回归；不是 ALF，也不是仅负测属性 | `rust/crates/lingxi-service/src/redaction.rs` 及必要永久回归；如确需可信字段调用邻接，由总控登记 `lib.rs`/日志调用的唯一所有者 |

建议沿用这两个已登记 ID，将 TRIAGE_RUNNING 转为具名 OPEN/MUST_FIX，`requiredForR05=true`；如总控分配新 F-ID，保留这两个别名及旧失败引用。指派全新修复者，修后由另一位全新验收者复证；本报告不充当修后独立验收。机器可消费建议见 [registration-proposal.json](manifest/registration-proposal.json)。

## 2. 候选、工具链与读取边界

HEAD：`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`；分支 `codex/rust-tauri-migration`。工作树含其他包已存在改动，逐项保留；本报告不将 HEAD 视为含 F46 的已提交候选。`commands/candidate-*.json`、`worktree-current.*` 保存本次只读结果。

实际版本：rustc `1.98.1 (48a229cea 2026-09-01)`、cargo `1.98.1 (797e8a9bc 2026-08-05)`、Darwin arm64、Node v24.16.0、npm 11.13.0。根 `rust-toolchain.toml` SHA256 `eec3410410647a7d39f25371e73dda3ecfd5e7c10359090fff11e83053e11c37`；`rust/Cargo.lock` SHA256 `259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3`。均由本次实际命令/读取记录，不沿用工具版本猜测。

全文读取 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、当前矩阵/进度/交接；保存读取快照。原始共同任务书及 R05 专项在 RR1 INPUT 的 specifications；R02-A01/A13 原要求完整记录来自只读 R00 `ACCEPTANCE_MAP.json`，见 [original-requirements.json](input/original-requirements.json)。未声称另读未挂载的完整 R02 原任务书。

现场非冻结。本次主要取证 2026-10-07 01:56:49–02:14:09 UTC（精确时点逐文件见 loghash/commands），记录每个文件读取前后 hash、字节数和 mtime。首读时四份 a01/a13 stdout/stderr 均在同次读前后相同；截至后读这些四份字节仍相同，**只对这些已读文件及观察时点成立**。

观察到 N06 `run.log` 从 5621 字节增至 9953 字节，SHA 改变；RR3 总控矩阵/进度也有实际内容更新；隔离副本若干文件 mtime 改变而字节 SHA 相同。详见 [observed-changes.json](loghash/observed-changes.json)。读取过程中 N06 结果/退出文件落盘，不能倒推首读时结果已完成；更不能因此宣布全部 G 完成或所有证据恒定。当前矩阵出现 CLI 两项另失败，本次不对其完成归因或纳入本报告两项关闭。

## 3. A01：确切失败点与根因

原证：G-REVIEW-01/default16-01/n06-midgate-mutation/evidence/a01_smoke/{stdout,stderr}.log，及同层 A01 全部 leaf/健康/拒启/前后目录与原文件 SHA。完整字节已保存 `input/primary-before/`、`input/raw-before/`，原始路径及摘要见 loghash。

实际脚本 exit=1，未超时；leaf 原始 JSON 是 **11 项，1 项失败**：

- `a01-newer-data-epoch-refused`：expect=1、actual=1、ok=true；内层真实拒启 exit=2，无 READY。
- `a01-newer-data-epoch-explicit-error`：expect=1、actual=1、ok=true；有 `LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked`，并明确写 requires epoch 2 / kernel epoch 1。
- **`a01-newer-data-epoch-no-root-switch`：expect=1、actual=0、ok=false**。

不是“较新版本没有被拒绝”。源码脚本 `check_newer_refusal`（441–482 行）把多种条件合成一个 no_switch：原 stamp/数据不变、备用根不变、`effective_home=<原输入路径>` 和 `source=cli` 两段日志都在、原/备用根没有 DB/token、无 READY。

本次复算原文件前后 SHA 文本完全相等；stamp SHA 为 `fff3feede919307eab19131c6840eae2586f34591c8af945c5b99257432a08a3`，既存数据为 `c43fe175063ed3bec81dafcd0d914ec43e9e94bfee03dda38a6c35e79c054586`；备用根快照完全相等且为空。selected root 新增的仅实例锁、日志/tmp 脚手架，属于脚本明确允许诊断/锁；快照没有 DB、local-token。**导致 false 的可定位条件是 stderr 缺 `effective_home=` 和 `source=cli`**，不是前后数据差异。详见 [a01-recomputed.json](input/a01-recomputed.json)。

生产 `main.rs:405–414` 把两段诊断放在 `tracing::info!`；`logging.rs:347–350` 服从 RUST_LOG，未指定才默认 info。A01 未固定日志环境，却必需 grep INFO；A13 早已在自身 235–241 行明确固定 `RUST_LOG=info`，注释恰说明调用者 warn 会饿死证据。当前取证工具环境实际为 warn；同一个仍运行 G 的后续 R02 xtask PID 87199 用原生只读进程参数查询观察到 `RUST_LOG=warn`。N06 原进程已退出，**未捕获它当时完整环境**，不能伪造该冻结记录；N06 初次正常 service stderr=0 字节、拒启仅 ERROR/直接 marker，与该过滤原因一致。bash 环境查询因 argv/env 被重写不能见到值，首个 PID 已退出导致 errno22，准备失败亦保留，未将“没读到”当 unset。见 `input/caller-env-*.json`。

历史同契约正常对照：RR2/G-R2/negative-ev 下 N06、N16 run-a/run-b 的 A01 都有真实 **17 项、0 失败** 原 JSON；三种 home 入口拒启完整，原文件保持，INFO 包含选根及 source。本轮/HEAD/copy 的 A01 脚本字节一致。故这是既有检查器的环境前置缺口在当前环境暴露，**不是 F46 新造的数据版本问题**。历史对照只证明当时原契约，未冒称本次重新运行历史二进制。

最小建议：参考已存在 A13 做法，把检查所需日志级别显式设为合法前置；保留 no-switch 全部实物保护断言，最好失败时分项指出真实条件。不把移除 grep、降低 expected、将业务 FAIL 转 ALF，作为修复。

## 4. 版本、存储与迁移排除

A01 真实健康体：`dataEpoch=1`、wireProtocolMin/Max=1、serverVersion=0.0.0；版本权威 `lingxi-protocol::ContractVersions::R00_BASELINE.data_epoch=1`。种入的拒启样本是 stamp **schemaVersion=2、minimumReaderEpoch=2、committedDataEpoch=2**，要求 epoch2，较新拒启正确。schemaVersion=2 是印章编码版本，不等于当前 kernel data epoch，更不等于 SQLite 迁移版本。

`main.rs` 在实例锁/日志挂接后，先 `coordinate_data_epoch_startup`，成功后才 bootstrap auth/store；`epoch.rs` 的 fail-closed 及较新印章判断没有被 F46 改动。没有证据表明该运行在拒启后打开存储、写 auth 或改备用根。

实际 A07_A08 S4 三轮 inspector 原 JSON 全读并存档：`supportedVersion=7`、`userVersion=7`；compiledIn 与 receipts 的 v1–v7 名称/指纹逐项相同，v6 是 `model_call_usage_rr1_f21`，v7 是 `model_call_usage_rr1_f38_attempts_nullable`。RR2 FINAL 正式 R03 定向 R02 S4 快照同为7。源 migrations/权威 supported_version 和现行存储登记册对应已存在 v6/v7。见 [versions-recomputed.json](input/versions-recomputed.json)。本报告未重跑迁移，也未扩大为重签 F41；现有两项失败不需要改 epoch、lock 或迁移 SQL。

## 5. A13：确切失败点与产品根因

原脚本 exit=1，未超时；P1 真正常请求、P2 真 401/403/404/WS 拒绝、P3 真实 WAL 锁后 db_busy 503、P4 真损坏 stamp exit2 已到达相应边界。P3 实际状态是1个503、其余409，并非 stdout 的“200x0/503x1”已经列出全部负载；完整 `p3-status-codes.txt` 已保存，不改写为全200。

6 种预置/本轮实铸秘密的原扫描记录为 246 次读取 / 41 个目标文件、0 命中；原请求体/合法一次性发钥匙/票据响应按原脚本精确排除。取证独立复算可恢复的4种预置值及票据共5种，41个目标均可读、0命中；原 local token 随合成 home 清理，**未重新复算第6种**，保留原执行证据且明确限制，未捏造全6复验 PASS。

失败在之后的 CORRELATION：真实401体的 `details.requestId` 是 `req-2385be7efcccbb90c96ac40b6432d50b`，原始 stderr 有4条 AUTH marker，但对应行实际为：

```text
LINGXI_AUTH_REJECTED [token] method=GET path=/lingxi/v1/me status=401 Unauthorized reason=invalid_credential remote=127.0.0.1:51603
```

正式 `lib.rs:2571` 用宿主 RequestId 构造 `request_id={request_id}` 后走 `redact_line`；`inject.rs:75–100` 的真实 ID 为 `req-`+16随机字节的32个hex，单个 ID **36 字符**。`redaction.rs:124–126` 包含 `=` 的 token 类把字段名和 ID 连成 **47字符** 的 run；`find_long_random_token:617–645` 遇到 ≥40 整段替成 `[token]`。同问题影响 `request handled` tracing 行及 AUTH/TRANSPORT marker 的真实 request_id，并不是只漏复制一份日志。

真实日志保留：3份 log（3150/3575/201字节）各与原 retention manifest SHA/字节一致；其中 request handled 也已是 `[token]`，从文件中找不到该 ID。source home 已清理，不能重新证明当时 source 集合，只能复核保留文件与当时 source-bound receipt；真实 stderr 拒绝行也确实缺失ID。`inventory.txt` **不存在**：脚本在 correlation assert 退出，inventory 生成位于其后，不是另一个更早的保留失败。详见 [a13-recomputed.json](input/a13-recomputed.json)，完整请求体、响应、WS、manifest和每个日志均在 raw-before。

旧短 ID 测试不能保护此契约：`a13_normal_diagnostics_are_untouched` 用 `request_id=req-1`；另一个测试用不带字段名的16hex短 req；都没覆盖47字符正式赋值形状。

## 6. HEAD、F46 与历史判定

[source/head-working-copy-comparison.json](source/head-working-copy-comparison.json) 对相关脚本、main、redaction、inject、epoch、migrations、版本权威逐项记录 HEAD/working/copy SHA。所核文件中**只有 logging.rs 与 HEAD 不同**；其实际 F46 diff 仅把 open_current 移到 prune 前及增加启动保留数量回归，不改变过滤、RequestId生成、脱敏或存储版本。

A13 是早于 RR3 的真实问题：RR2 N06、N16 run-a/run-b 三份401体均有32hex真实ID，三个真实 stderr 的 AUTH marker均无该ID，三个 a13 stderr均有同一 correlation AssertionError。保留这些历史 FAIL，不能因负测汇总16/16而撤销它们。

R02 较早正常对照：`artifacts/rust-tauri/R02/audit-r17-g5/a13/redaction-scan/` 原401体 `req-267c498748e351d107464ccdd91c9ff2` 在真实 AUTH marker逐字存在，完整 retention manifest/logs可复算；该对照与当前都是36字符正式ID和相同“秘密全无、关联ID保留”契约。是历史原证，不是本次运行/冻结的旧二进制，旧binary hash未从该目录取得。

`git diff cdd213078 HEAD -- redaction.rs` 的实际改变显示，R05 d80737b6c 引入 `/`、`=` 的长 token 类以修复 provider base64 秘密漏网；RequestId生产长度在旧R02已是32hex。该加强把原安全诊断也吞掉。现 HEAD仍有它，F46没有改它。R02-T07 REVIEW_R1 F02历史允许的是当时不可达的provider形状缺口、要求R05引入provider前复核；**没有授权移除关联ID义务**。不能为修A13简单撤销 `/`/`=` 秘密防线，也不能全局放行任意 `req-` 前缀/用户载荷作为可信字段。

## 7. 本次最小探针及保留的准备限制

只使用已完成 C-F46-REVIEW-01 的 `isolated/build/lingxi-service`，不复制大target/.git，不编译。程序 SHA256 `7d9190d27d1b6a019abfbe97049d6855a3f225494124fee43ef8988e032ae78b`，59478432字节；与先前 isolated-normal-build/command.json 记录相等，原构建两条真实 rustc exit0、lock相同、logging源码hash相同。该程序是先前独立审查以现成依赖重新链接的main/library，**不是本次正式 cargo gate binary的替代身份**；只作根因探针，不作阶段PASS。前后binary hash相等，见 manifest/probe-binary-{before,after}.json。

真实 N06 程序身份另从 G 活跃观察记录摘取：A01 PID50757、01:48:17 UTC，shared-cache程序 SHA `90daa4e828f4f5103e9502928e93984ce087f16c431033e6d3eb0cc892a69a2b`；A13 PID59285/59393、01:53:09/12 UTC，SHA `d005248caa067367176b6212745ff5a17cb0442b72e301d81c0b0e648cf95e4d`。它们随中途kernel注释变异后的重build而不同，不能拿当前NEG_TARGET字节冒充两者或本探针。记录见 input/live-binary-selected.json；原观察文件也做读前后摘要。

探针命令实际执行：[minimal-probes.json](commands/minimal-probes.json)；源程序 [probe.py](source/probe.py)，全部 argv/日志/开始结束/退出码分别记录。

- CLI下 warn/info/unset，及 env/config-file下info，共5个较新epoch合成home：全部服务exit2、无READY、原stamp/数据保持、备用根保持、没有DB/token。warn隐藏选根INFO；info/unset能见到选根和source。
- **这5组不是原A01完整绿对照**：只允许在本取证目录写入，所以探针路径位于 `/Users/...`；原路径被用户路径规则及长token规则遮成 `/Users/[user][token]`，原“精确路径grep”仍false。该准备/邻接现象已原样保留，不改理想路径，不将0退出的探针驱动误称A01 PASS。原契约阳性证据采用前述RR2的真实短段临时路径原证；新修复者必须另在授权独立短段合成home重新亲跑完整检查。
- 另1个正式main回环服务、1个真实401，无模型/供应商调用：体中ID `req-90aad09f57adf29b55d5efc12f55e5a6`、assignment47；AUTH marker实际 `[token]`，文件诊断亦无该ID。SIGTERM exit0、进程reaped。该输入在认证边界合法到达，确认当前源码对应真实产品缺陷，不依赖N06变异。记录 input/request-correlation/result.json。

长路径诊断误伤是同一脱敏规则的必要邻接观察；本报告不将路径grep失败混作A01日志级别对照通过。修复者须保护原安全日志路径契约并继续脱敏个人路径及秘密；不要求改真实用户home/系统路径或数据迁移。

## 8. 真实正式链与 N06 属性分别处理

实际 stage maps及RR2 FINAL原子命令已核对：

```text
verify-stage R05
  r04_regression_gate: cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R04 --evidence {EVIDENCE}/R04_REGRESSION
R04
  r03_regression_gate: cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 --evidence {EVIDENCE}/R03_REGRESSION
  r04_rr1_repair_suites: bash scripts/rust-tauri/r04_rr1_g05_repair_suites.sh {EVIDENCE}/R04_RR1_REPAIR
R03
  7条真实R02定向producer：auth/storage/events/backup/recovery/full_chain/legacy
  repair_suites: bash scripts/rust-tauri/r03_g07_repair_suites.sh {EVIDENCE}/G07_REPAIR
  legacy使用 R02_LEGACY_REGRESSION_MODE=directed-no-seal-family（既有授权边界保留）
```

这里 **R03没有机械嵌套完整verify-stage R02**；它跑7条定向命令。R03原generator SUP-02另说明T08完整R02独立门禁义务。A01/A13不在当前正式R03这7条 argv 内，不虚称RR2 FINAL逐层已经执行这两脚本。完整R02注册的 A01/A13仍REQUIRED；本次N06恰实际执行完整R02，把真实缺口露出。不能因不是R03直接子命令就延期已确认R05关联诊断回归，也不能未经必要登记新加整套前序编排。

归属权威：原R00 R02-A01=无桌面启动；A13明确“预设敏感值全无，关联ID保留”；serve叶仍有启动/较新数据失败不切根要求。RR1 MASTER §2.3/§3.3/§5.3/§6.1和RR2 §一/§四G要求保留前序语义、关闭关联新必需缺口、负测目标性失败和正式有效闭包。因此两项不能新豁免到R06或统归ALF。既有directed/E5历史治理许可不涉及修改这两项契约，LIVE/平台延期也不覆盖本机离线行为。

N06原命令是隔离copy里 `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R02 --evidence <N06>/evidence`；等证据根出现后，在 `lingxi-kernel/src/lib.rs` 追加一个注释。读后实际 N06 exit1、`candidateSourceBinding.stable=false`，20个逐命令checkpoint均false，具名changed path只有该kernel文件，before/after摘要不同，runnerSourceBinding PASS。见 [n06-observed-binding.json](input/n06-observed-binding.json)。这是绑定目标红的独立证据；**A01/A13业务exit1既不能替代它，也不能被它抵消**。此处只报告已取得N06记录，不签整个G/默认16/FINAL完整结论。

## 9. 新修复与新独立验收的具体完成条件

### A01 检查器修复

唯一负责人：总控指派的全新R02检查器修复者。最小授权：A01脚本＋必要精准回归；不动epoch/migrations/原叶要求。新独立验收者须在新的隔离target/合成home/证据目录，以外层RUST_LOG=warn和info分别运行完整原脚本，均真实17项0失败；三种选根都取得拒启exit2、指定source/路径诊断、原stamp/既存文件字节不变、备用根不变、无DB/token/READY。所有准备失败保留。正向旧epoch正常启动与较新拒启要分开证明；不要改kernel epoch使较新样本误合法。

### A13 产品修复

唯一负责人：总控指派的全新脱敏修复者（可与A01成组；共享调用邻接由总控登记）。最小授权：redaction.rs内的可信诊断与秘密判别、永久回归；若需结构化调用点才加登记邻接，不新造日志系统、不改RequestId成短确定值取绿。新验收须：

1. 当前缺陷的生产36字符随机ID/47字符赋值旧红→修后同契约真实401体与AUTH marker逐字关联；正常request handled及TRANSPORT marker/WS/错误诊断必要关联保留；session/run不丢。
2. 继续完整6种秘密真实扫描及原请求/交付排除边界；`/`/`=` base64、混合大小写key、Bearer/URL/上游恶意内容仍遮盖，不用全局“req前缀可信”漏掉合成秘密；旧短ID测试之外增加正式形状。
3. 保留F46 open后≤max_files原上限与轮转/失败显式报告；A13新鲜retain3文件和source-bound SHA/inventory均真实生成，完整脚本exit0。
4. 重验A01受影响安全路径（短段合成临时路径以及授权的用户/长路径诊断范围），明确个人路径遮盖允许范围，不能让长token规则整段抹去必要诊断身份。探针这个长路径限制留在同根因邻接，不另行开放真实用户数据。
5. 按受影响输入失效规则更新二进制/证据绑定：logging/redaction及新的回归、受影响C/F46资源160轮/最终稳态、G绑定恢复control、新独立FINAL §5.3由对应新负责人执行；本定位报告不代替这些。

以下是留给修复者/新审查者的命令，**本取证没有执行构建或完整门禁**。先确认独立target无其他运行者及磁盘容量；不能用G的共享NEG_TARGET，所有证据路径采用新编号且非空拒绝：

```bash
R02_FIX_TARGET=/Users/study_superior/.cache/lingxi-r02-fix-01-target
PATH=/Users/study_superior/.cargo/bin:$PATH CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$R02_FIX_TARGET" RUST_LOG=warn \
  bash scripts/rust-tauri/r02_t01_service_smoke.sh artifacts/rust-tauri/R05/RR3/R02-FIX-01/a01-warn
PATH=/Users/study_superior/.cargo/bin:$PATH CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$R02_FIX_TARGET" RUST_LOG=info \
  bash scripts/rust-tauri/r02_t01_service_smoke.sh artifacts/rust-tauri/R05/RR3/R02-FIX-01/a01-info
PATH=/Users/study_superior/.cargo/bin:$PATH CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$R02_FIX_TARGET" \
  bash scripts/rust-tauri/r02_t07_redaction_scan.sh artifacts/rust-tauri/R05/RR3/R02-FIX-01/A13
CARGO_TARGET_DIR="$R02_FIX_TARGET" /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib redaction::tests
CARGO_TARGET_DIR="$R02_FIX_TARGET" /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib logging::tests
```

完整默认负测及最终§5.3仍归总控安排全新独立审查。不要以本报告、已有局部PASS或新脚本exit0提前宣布R06_READY或FINAL完成。

## 10. 交付清单

`REPORT.md`＋commands/原始stdout-stderr/exit、input/逐项原证快照与重算、source/只读取证工具及HEAD历史源码、loghash/读前后与observed变化、manifest/身份/登记建议/完整文件摘要。取证驱动exit0仅表示取证完成，受测服务拒启exit2、A01/A13原exit1、真实关联失败、环境读取失败和无效完整绿对照均各自保留。没有修复后的PASS签名。
