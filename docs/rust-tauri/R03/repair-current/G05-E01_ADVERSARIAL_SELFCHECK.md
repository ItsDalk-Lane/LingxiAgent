# R03 修复轮 G05-E01 对抗性自查（F06：真实执行输入被静默截断为 2000 字符）

- 执行代理：EXECUTOR-REPAIR-R03-G05-E01。日期：2026-09-30。
- 方法：先在隔离 git worktree（`/tmp/lingxi-r03-g05-redbase`，detach 于候选起点 `d56e6883d`，未含本轮修复）运行最终版测试取得红基线；再在修复后的工作区复跑。Provider 替身只记录实际收到的输入文本，回复为固定常量（不携带输入信息），一切断言不依赖回复文本。

## 红基线（攻击在修复前成立）

- `input_payload_fidelity`：**5/5 FAILED**（0 passed; 5 failed），退出码非 0。逐条失败点即审查反例：2001/3000/8000 字符与 Unicode 长请求的 Provider 实收输入 = 前 2000 字符投影，尾部要求丢失；同前缀异尾部请求被截断后内容仍可区分（digest 全量）但**执行内容**与判定内容不一致。证据：`adversarial-selfcheck/red-baseline-input_payload_fidelity-full.log`（摘要 `...-summary.txt`）。
- `input_budget_refusal`：**编译失败**（E0425 `MAX_SUBMISSION_INPUT_BYTES` 不存在、E0599 无 `InputTooLarge` 变体）——修复前不存在任何显式超限拒收路径，>1 MiB 输入会被受理并截为 2000 字符静默执行。证据：`adversarial-selfcheck/red-baseline-input_budget_refusal-compile.log`。

## 逐攻击窗口

### A1｜尾部与前缀相反标记（C01 对抗变体）

- 攻击：请求前缀反复声明 `IGNORE_TAIL_USE_ONLY_ALPHA_K3.`，末尾放相反指令 `|TAIL_DIRECTIVE_OMEGA_{len}_7QXZ_END`；若截断仍存在，"真实任务"（尾部）被丢弃而前缀胜出。
- 观测（修复后）：前台与后台（1999/2000/2001/3000/8000）Provider 实收输入与原请求逐字符相等——OMEGA 尾部完整在场；断言基于记录文本而非回复。
- 是否推翻修复：否。命令 `cargo test --locked -p lingxi-service --test input_payload_fidelity c01_`，退出码 0。证据 `normal-selfcheck/g05-suites-post-fix.log`。

### A2｜同前 2000 字符、异尾部（C02 对抗变体：误合并/摘要漂移）

- 攻击一（同 id 异尾部）：同 explicit requestId、共享前 2000 字符、尾部不同 → 若摘要只覆盖截断投影，两条会被当作同一请求合并/重放。
- 观测：返回 `DuplicateRequestConflict`，Provider 计数不变、不新增 run——digest 覆盖全量输入，两条不同请求不被误合并。
- 攻击二（异 id 异尾部）：两个 id 各自执行，Provider 记录两次全量输入，且 `normalized_request_digest_hex` 对两份**实收**内容的摘要不同（摘要对应实际执行内容，非投影）。
- 是否推翻修复：否。命令 `cargo test --locked -p lingxi-service --test input_payload_fidelity c02_dedup`，退出码 0。

### A3｜CRLF / emoji / 组合字符规范化陷阱（C02）

- 攻击：中文+非 BMP emoji+组合序列+CRLF 混合长请求从两条入口提交；若执行链隐式规范化（如 CRLF 折叠、NFC 重组）或按"字符"截断拆开组合对，内容即被静默改写。
- 观测：两入口 Provider 实收与原请求**字节相等**；组合字符对（e+U+0301）精确横跨历史 2000 切点时仍完整（旧投影在该点丢重音符——红基线用例 `c02_adversarial_combining_pair_straddling_the_historical_cut` 修复前 FAIL）。唯一允许的规范化仍是 dedup 摘要的 CRLF→LF（契约既有声明），且方向是"同内容判定同一"，不改变执行载荷。
- 是否推翻修复：否。命令 `cargo test --locked -p lingxi-service --test input_payload_fidelity c02_`，退出码 0。

### A4｜超限只发生在日志摘要时误拒合法输入（C03 对抗变体）

- 攻击：提交"超过历史日志摘要界（2000）但远在预算内"的合法输入（2001、100_000 字符）；若把日志摘要上限误接到受理拒收，合法长请求被错误拒绝。
- 观测：两者均受理、全量执行、各恰好一次模型调用与一条 durable run。修复后日志只记 `input_chars`/`input_bytes` 计数、不记内容，不存在"日志摘要超限"这一拒收来源。
- 是否推翻修复：否。命令 `cargo test --locked -p lingxi-service --test input_budget_refusal c03_log`，退出码 0。

### A5｜超限载荷的"假受理"面（C03）

- 攻击：1 MiB+1 字节输入分别从前台、后台提交（带 explicit requestId）；观察是否出现假 Run、started 写入、模型调用、工具派发或残留 id→run 绑定（幽灵 replay）。
- 观测：两入口均 `InputTooLarge { bytes, limit_bytes }`（数字与实际一致）；runs 计数 0、`status != 'queued'` 计数 0、Provider 0 调用、工具 0 派发；随后同 id 合法内容重试**真实受理**（fresh，非 replay）——超限拒绝不留任何绑定，G04 两阶段绑定语义未被破坏。
- 是否推翻修复：否。命令 `cargo test --locked -p lingxi-service --test input_budget_refusal c03_over`，退出码 0。

### A6｜"换个魔法数"检查（红线 4）

- 攻击面：修复是否只是把 2000 改成更大数字继续截断？
- 反证：`sessions.rs` 中已无任何 `take(N)` 投影（`grep -rn "take(2000)" rust/crates` 仅剩新测试文件的文档注释与测试构造）；执行载荷直接是 `submission.input` / `submission.input.to_string()`；上限只在受理头部以 `InputTooLarge` 显式拒收出现，预算数字 = 传输层既有 1 MiB body/frame 上限（非新造）。

## 结论

六类攻击窗口在修复前均可成立（红基线 5/5 FAIL + 编译缺失），修复后全部不可复现；未发现推翻修复的观测。两层自查（普通+对抗）均为 PASS。
