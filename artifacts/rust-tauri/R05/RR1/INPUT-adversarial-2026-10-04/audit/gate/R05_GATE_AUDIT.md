# R05 独立门禁、规格与准入审查

审查仓库：`ItsDalk-Lane/LingxiAgent`。固定候选：`d80737b6cb9186c8a18c0f35923aac00249d45c3`；实施前基线：`c549ff654508ab951e2cf39cf9d309fc9c6b8656`。本审查不修改产品源码、测试、报告或正式任务书，未 commit/push。

## 结论

**R05 不能据当前门禁 PASS 宣布完成。** 至少存在：6 个 R05 独占 OAuth 叶被基底测试代替完整行为；必需 C-ID 身份校验可被换名骗过；必须本阶段交付的代理/私有 CA 仍未完成；资源验证低于明确轮次要求且 FD 计数谓词失效。这里分别涉及实现缺口、验收器缺口和验证缺口，不能靠把报告状态改为 ACCEPTED 解决。

权威材料已经核对两份：2026-09-23 原始任务书（R05 8 个 Task/16 个 A-ID）及 2026-10-02 `Lingxi_R05_完整执行与验收提示词_2026-10-02.md`（100 个细化 C-ID、16 个门禁负测、§9 准入规则）。后者 §9 第 281 行只允许已授权规则内的外部实测延期。Reviewer 自己登记“后续网络加固”不是用户修改需求。

## 本轮亲自验证的内容

1. 从原专项文档提取 100 个 C-ID，与当前台账核对：原 100 个全在册；台账共 103，增加 `R05-T05-C11B`、`R05-T05-C13`、`R05-T06-C11B`。不是“整份台账忘记登记 9 个需求”，而是登记和实际 gate 校验脱节。
2. SHA256 逐文件比较当前 `rust/`、`scripts/`、`contracts/` 与 `T08-E01`、`REVIEW-T08/FINAL-R3`、`FINAL-WFR2-1` 三份 candidate manifest：**每份 483 个文件全相同，无缺失，无新增未绑定文件**。这些历史机器记录确实对应同一份关键源码，不能因 SHA 仍记旧基线就指控源码伪造。但不是本轮复跑结果。
3. 隔离 Rust 源副本执行 R05 全部 7 个 xtask 镜像测试：正常副本 **7/7 PASS**；仅将复制的 cid 表中 `R05-T02-C01` 改成根本不存在的 `R05-T99-C99`，再次执行仍 **7/7 PASS**。未修改任何 Rust 测试、原源码或真实工作树。
4. 从正式 `r05_t08_stage_suites.sh` 原样抽出 Python assembler，在复制的历史日志 fixture 上施加同一个 C-ID 换名，实跑 exit 0、`allCasesOk=true`、`allSuitesOk=true`，输出已不含原必需 ID。该 probe 是实际 assembler 执行，日志是 fixture；**没有声称完整 `verify-stage R05` 变异实跑通过**。
5. 从历史最终机器 gate 读取 6 个 R05-only 叶：均 `PASS`，均 `originalAssertionCases=[]`，均 `deferredToStages=[]`，reason 却称剩余义务会移动到后续阶段。
6. 读取真实默认格式 `lsof` 输出并应用与测试相同的行首数字谓词，结果为 0；默认输出首列是 COMMAND。本容器的 `/proc` 和 `lsof` PID namespace 不一致，不能将 Python 的 104 个 FD 与 lsof 目标直接作数值对账。结论只依赖格式与谓词不匹配、退出状态未检查这一实错。

## G01 — 阶段独占义务被套件整体成功替代，6 个 OAuth 叶错误放行

严重度：**阻塞完成**。实现缺口由凭证审查同时独立确认；这里说明门禁为何没抓住。

### 证据和根因

- `docs/rust-tauri/R05/r05_t01_build_scope_matrix.py:31–34,166–178`：D11 OAuth 叶均 R05-only；本阶段负责完整服务端 start/callback/poll/status/logout/custom modelId 管理；**没有 R07 剩余份额**。
- `scripts/rust-tauri/r05_t08_generate_leaves.py:49–66,96–111`：按首个 R05 Task 选通用 FAMILY，并无视该叶独占身份，一律写 `stage_share_satisfied` 和“后续阶段负责产品行为”。它甚至不消费上面的逐叶 scope 裁定。
- `scripts/rust-tauri/r05_t08_stage_suites.sh:273–292`：叶证据只问对应整套 suite 是否精确数量全绿，再复制为每叶 `actual=1`。没有核查该叶原始业务断言。
- `rust/crates/xtask/src/verify.rs:840–887` 对 `full_original_behavior` 有逐断言约束；`:919–935` 的 `stage_share_satisfied` 没有“无合法后续阶段不得遗留当前义务”的保护。
- `rust/crates/xtask/src/stage_map.rs:3079–3165` 不但只核对叶数量/ID，还要求所有叶都必须是同一种 share，固化了错误裁剪。
- 正式路由 `management.rs:586–590` 只有模型配置 reload、凭证状态、撤销；没有 OAuth start/callback/poll/custom-model 管理；CredentialStatus 没有可用模型数；OAuth 初次登录 helper 未接服务入口。

| R00 叶 ID | 必需行为 | 当前最终 gate |
|---|---|---|
| R00-T02-LA-16CEB6D12A6A | 添加 OAuth 自定义模型，刷新并返回新清单 | PASS，但只映射凭证 suite |
| R00-T02-LA-8060BE8AA02C | 删除 OAuth 自定义模型，刷新并返回新清单 | 同上 |
| R00-T02-LA-CFEC64F68DDE | 列出指定 OAuth provider 的自定义模型；非 OAuth 拒绝 | 同上 |
| R00-T02-LA-99D6C304D697 | 登录 start/callback/poll 到真实安装凭证与状态刷新；失败不伪成功 | 同上 |
| R00-T02-LA-CA0BF9A7AEA9 | 每 provider 的 loggedIn 与可用模型数 | 同上 |
| R00-T02-LA-FC80B6C4FBE4 | logout 删除凭证、清缓存、刷新模型列表，结果诚实 | 同上 |

### 完整修复与自检矩阵

- 逐叶恢复上表真实服务端能力，不要求提前做 R07/R08 UI；协议端可用本地受控认证服务器。
- 原 then 中每个成功/拒绝/失败副作用用正式服务入口验证，状态/模型表/磁盘持久化/实际后续请求相互对账。
- OAuth 涵盖 state、PKCE、过期、重复回调、取消、pending/done/error、revoke 与迟到登录/刷新竞态；模型管理涵盖空 ID、非 OAuth provider、权限、无效输入不改原清单、持久化失败、重启、跨 provider 隔离。
- 全 130 叶重新对照原 R00 断言、R05_SCOPE 和合法后续归属；本阶段能共享测试的机制可共享，但必须说明具体覆盖哪一条断言，不能以整套测试 PASS 替代叶行为。真正 R07 UI/壳义务仍保持合法延期。
- 阶段独占叶不得留下空承接阶段的 remainder；错将独占叶改为 partial share 的负测必须失败；删除任一叶断言的真实证据也必须失败。

## G02 — 必需 C-ID 可换成虚构 ID，全部镜像检查仍绿

严重度：**阻塞验收器可信度**。对应专项 §7、附录 C `R05-GATE-N01`、`N03`，以及全部 100 C-ID 精确映射义务。

`stage_map.rs:3239–3264` 只检查 91 个 ID 数量和 9 个示例 ID，没有逐个固定完整集合。`stage_suites.sh:219–240` 仅检查前缀 `R05-T` 和测试名，未验证 ID 属于权威集合。将非那 9 个示例之一的 `R05-T02-C01` 替换为 `R05-T99-C99`，所有计数和 test ownership 均保持不变，真实 7 个 Rust 镜像测试仍 PASS，assembler 亦 PASS。

- 正常/变异命令：`cargo test --manifest-path <隔离副本>/rust/Cargo.toml --locked --offline -p xtask --bin xtask r05_`。
- 正常：7 passed / 0 failed。
- 变异：7 passed / 0 failed；但必须 C-ID 已消失。
- 当前 103 个已登记 C-ID 中 91 个有直接 cid 表注册，12 个无直接注册；其中部分确有合法命令/共享测试，不能把“没有直接注册”全判漏测。但 gate 未以完整权威列表验证这些替代绑定；`T05-C12 NOT_RUN` 也没有阻断。

修复须将完整 100 个必需 ID 加 3 个已追加 ID 的身份、要求、直接测试或替代命令绑定都纳入同一 gate，验证精确集合与状态；拒绝未知 ID 替换、缺 ID、空命令、零匹配、忽略、未执行 PASS、任意降低要求。非 Cargo 检查可注册真实命令，不必为了凑数量制造单元测试。需要逐类负测：删直接 CID、替换为虚构 CID、删非直接绑定 CID、删/改名/ignore 测试、同步降低计数、将 NOT_RUN 离线义务伪装 PASS。允许多检查引用同一次有效执行，只需关系明确且按实际结果判定。

## G03 — 代理/附加私有 CA 是本阶段未完成义务，不能按 LIVE 延期

严重度：**阻塞完成**。这不是要求实际外部付费服务。

- 原始 R05 第 215 行要求统一系统/手动/直连、localhost bypass、企业根证书与 provider endpoint，第 221 行要求代理与证书测试。
- 专项 `R05-T05-C12:986–994` 明确受控端点下验证系统/手动/直连/NO_PROXY、有效/无效证书、测试私有 CA、显式授权与不降低 TLS。
- 专项 §9 第 264、281 行禁止延期本阶段实现，只有允许的外部实测可延期。
- 当前 `R05_BLOCKERS.md:18–22` 将此延期到无明确期限的“网络加固阶段”；验收台账 `T05-C12` 保持 OFFLINE_LOGIC / NOT_RUN，明确承认 Rust 没有旧 NODE_EXTRA_CA_CERTS 等价物。
- dispatch/OAuth 两客户端强制 `.no_proxy()`；当前 ProviderConfig 无该网络策略/附加 CA 面。凭证审查报告可提供更详尽生产代码定位。

修复须建立统一受控网络配置策略、显式附加 CA 入口、代理与 localhost/NO_PROXY 行为，并让模型、OAuth、媒体和资源下载按应有凭证范围使用它。离线起计数代理和临时 CA/HTTPS 服务器，验证正确路由、错误/过期/错误主机名证书拒绝、私有 CA 未批准拒绝/批准后成功、取消回收与无全局 TLS 降级。将其注册为持续门禁并移除虚假延期状态。真实 provider 登录仍可以按规则 BLOCKED_LIVE 至 R10。

## G04 — 资源门禁既未达到规定负载，又有 FD 计数失效

严重度：**阻塞必需验证**；当前证据不证明已发生生产资源泄漏。

- 专项 §8 第 256 行要求至少正常、长响应、100 次以上重复取消/错误、嵌套 worker、多会话负载，检查计数器/连接/task/FD/permit/临时文件稳态；`T08-C12:1335–1343` 要求原始采样和同条件对照。
- `r05_t08_closed_loop.rs:2239` 只有 12 次成功/HTTP500/幂等重放混合循环（另有 2 次热身和 6 次新增幂等请求），无重复取消、回调或多会话资源压力验证。
- 同文件 `:2212–2223` 对 `lsof -p` 默认输出按行首数字计数，默认第一列是 COMMAND；本服务名 `lingxi-service` 为文字，正常数据行全部被丢弃，`:2301–2305` 的 FD 上限/增长断言因而可恒绿。
- 没有检查 lsof exit 状态、stderr 或结构有效性；无法采样也可能变成 0。RSS 只检查最终输出能否解析，缺少真实统计字段；本 Linux 环境 PID namespace 导致 RSS 空输出，此部分是环境限制，不能据此宣称 macOS 生产代码泄漏。
- `R05_PERFORMANCE_RESULTS.json:41–47` 没有实际采样值，只写“断言失败才打印，通过即门槛内”；其直接日志引用还不是提交中存在的路径（完整 gate 的套件日志存在）。

修复须使用可靠机器可读 FD 采样，校验退出/数据有效性，受控打开/关闭 N 个 FD 的正反对照证明计数会增加/归零；不能把采样失败计0。按原最低负载和更严已有门槛完成 100 次以上取消/错误与长流、worker、并发会话，采样参与进程的资源、permit、连接、任务、临时文件；每个负载提供完整时间序列/环境/阈值/结果及清理末态。注入已知泄漏或假0采样必须使门禁失败。

## 信息性/流程收口，不单独夸大成代码阻塞

- `R05_REPORT.md` 和 `R05_HANDOFF.json` 仍称所有改动未提交、independent_review=PENDING；已有独立报告却写 PASS，实际远程已有 d80737b6 提交。这是当前状态与引用未收口，修复后统一更新报告、accepted_tasks、HEAD/候选哈希、review rounds、剩余义务。
- 历史审查者阶段门禁第一次因 macOS ALF 环境项 FAIL 后自行限定 PASS；后续 FINAL-WFR2-1 的完整机器记录确为 PASS，且关键源码 hash 相同。不要只抓第一次 FAIL 断言所有测试未运行，也不要把历史 PASS 当需求完整性的证明。
- 原始 16 个 A-ID 均保留且 REQUIRED；注册的 suite 运行/精确测试数/零匹配防护不是全无作用，只是无法替代语义对应和完整义务。
- LIVE 和继承平台条件须按原规则单列；不要求 R05 提前完成 R06 的完整上下文/记忆/知识库或 R07 UI。

## 最终修复后准入要求

一份固定候选上：全部 R05 实现与本阶段离线义务完成；完整 16 A-ID、100 基础 C-ID、3 个补充 C-ID、130 叶的本阶段真实份额明确且通过；合法后续份额保留负责人/阶段；上述新反例及原 16 项负向门禁通过；正常入口模型/文件/worker/媒体闭环与资源验证通过；完整 R05 gate 和 R04→R03→R02 回归真实成功；新的独立审查者逐项复核并解决全部阻塞；报告与候选绑定一致。只剩已允许 LIVE 或明确继承平台条件时，才可按规定限定状态交接 R06。

## 本轮证据索引

- `candidate-binding-comparison.json`：3 份历史 manifest，各 483 文件与当前源码相同。
- `six_r05_only_leaves.json`：6 个独占叶及机器 PASS/空承接阶段。
- `cid-mirror-control.log`、`cid-mirror-mutated.log`：实际 Rust 1.98.1 各 7/7 镜像测试。
- `cid_rename_probe.json`、`cid_rename_fixture/assembler-output.txt`：原 Python assembler 对历史日志 fixture 的换名反例；明确验证层级。
- `fd_counter_probe.json`、`fd_counter_raw_lsof.txt`：格式谓词与 PID 命名空间限制。
- `non_cid_registered_cases.json`：12 个无直接 CID 表注册项（区分合法间接验证与真实 NOT_RUN）。
