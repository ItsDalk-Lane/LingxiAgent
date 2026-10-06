# R05 凭证、模型路由与网络边界独立对抗性审查

## 结论与范围

**本范围结论：FAIL，存在阻止 R05 完整验收的真实实现缺口。**

- 仓库：`ItsDalk-Lane/LingxiAgent`；candidate HEAD：`d80737b6cb9186c8a18c0f35923aac00249d45c3`；R05 比较基线：`c549ff654508ab951e2cf39cf9d309fc9c6b8656`。
- 本报告主要覆盖 R05-T01/T02/T05，触及 T06 共用网络实现；不重复其他审查者负责的协议保真、worker 授权与 usage 结论。
- 需求同时核对原任务书 `R05_模型协议、凭证、流式处理与最小完整闭环.md` 和用户的 `Lingxi_R05_完整执行与验收提示词_2026-10-02.md`，不把代码中自行登记的延期当作用户批准。
- 全部生产源码只读，无仓库代码、测试或报告修改，无 commit/push。测试位于此目录的外部 Cargo crate，依赖真实仓库 crates；循环回本机的测试服务器仅替代外部模型/HTTP 服务，凭证、路由、协议适配、egress 均为真实实现。
- Linux x86_64；rustc `1.98.1 (48a229cea 2026-09-01)`；cargo `1.98.1 (797e8a9bc 2026-08-05)`。以下 10 个“要求正确行为”的反例测试全部失败，证明当前实现违反对应断言；这不是仓库现有测试失败数量。
- 命令：`source /workspace/scratch/9b4397d87a13/audit/runtime/env.sh` 后执行 `cargo test --manifest-path /workspace/scratch/9b4397d87a13/audit/credential_network/Cargo.toml --locked -- --nocapture`。实际退出码 101，10 failed、0 ignored，执行测试阶段 0.28 秒。
- 证据：`src/lib.rs` SHA256 `e7ec004c15c65fe734aacf7e1aa2ff2a46caddb3f8bbcb81ed62b810c54c5405`；`probes.log` SHA256 `1f0fb8f15f496b50b77473e6f489004de7504797100d564d7206b98e201b6756`。

## 已确认发现

### CN-F01：热更新跨代次读取凭证，401 重试把新钥匙发送到旧端点

**严重度：BLOCKING；实测。需求：R05-T01-C05、T01-C04/A01、T02-C10 的凭证作用域。**

位置：

- `rust/crates/lingxi-service/src/management.rs:649–656`：先 `credentials.reload(&plane).await`，再 `gateway.reload(plane)`，并非同一个原子快照。
- `rust/crates/lingxi-service/src/credentials/mod.rs:396–426`：resolve 只按 `route.provider` 取当前材料，完全不校验 `route.config_generation`，也不绑定 route 的旧端点。
- 同文件 `:500–509`：遇到 401，只要当前静态钥匙不同即返回 `Refreshed`。
- `rust/crates/lingxi-adapters/src/models/provider.rs:195–226,240–257`：route 只解析一次，401 后取最新凭证继续 `execute_route(..., &route, &fresh, ...)`，仍使用旧 endpoint。
- `rust/crates/lingxi-adapters/src/models/gateway.rs:134–159`、`provider.rs:221–223`：compat hints 另外查询当前快照，与 route 同样可能跨代次。

真实网络触发：

1. provider `main` 配置端点 A 和合成钥匙 `dummy-old-key`，请求实际到达 A。
2. A 暂缓返回；把该 provider 更新为端点 B 与 `dummy-new-key`，执行真实两个 reload 方法，顺序与管理面相同。
3. A 返回 401；生产 `GatewayedProvider` 发起一次刷新后重试。
4. **A 的第二个真实 HTTP 请求头是 `authorization: Bearer dummy-new-key`。**

测试：`reload_during_401_must_not_send_new_key_to_old_endpoint`。另 `old_route_must_not_receive_new_endpoint_credential` 在无网络条件下直接验证旧 route 收到新 key。

影响：正常编辑供应商配置即可形成端点/认证混用；凭证可能泄露给已经撤换的端点。不是仅在极窄 pre-send 时序出现：旧请求等待 401 的整个期间都能触发。

修复要求：把身份、端点、协议、认证引用、模型能力、compat 与策略统一绑定同一配置代次；重试只能使用在当前作用域仍有权使用的凭证。配置更新后可明确拒绝旧路由或遵守可撤销的旧快照，不得把新材料套在旧端点。单独调换两个 reload 的顺序不能修复。

自检：A→B 改端点+key；仅 key 轮换；仅 endpoint 变化；协议/auth kind/compat 改变；provider 删除再添加；401 在变更前/中/后；刷新中撤销；多 provider 同时更新。逐次捕获真实请求头/地址/config generation，断言无未授权跨代次混搭、撤销仍即时有效。

### CN-F02：凭证句柄代次复用，轮换后有效、撤销后可复活

**严重度：BLOCKING；实测。需求：R05-T02-C12，撤销相关 C05/A04。**

位置：`credentials/mod.rs:611–616`（变更配置重新 seed cell）、`:828–831`（新 cell 的 generation 固定 1）、`:685–705`（句柄记录 provider+generation）、`:750–771`（解析只比较 generation 后交付当前材料）。

两个实测：

- `old_handle_must_not_resolve_reloaded_secret`：generation=1 的句柄签发后更换 key；新 cell 又是 generation=1，旧 handle 返回 `Ok(Bearer("dummy-new-key"))`。
- `removed_then_readded_provider_must_not_revive_old_handle`：签发 handle → revoke → reload 删除 provider → reload 重建同名 provider；原句柄重新返回新 key。

根因：数字代次只在某个 cell 内单调，handle 把它当作 provider 整个生命周期内唯一的身份。cell 被替换/删除重建后，代次发生 ABA 复用。

修复要求：凭证身份需有跨 cell 生命周期不复用的 epoch/instance 标识，或在替换/删除/撤销时原子失效旧 registry 条目；handle 还必须绑定可信调用者/Agent 作用域，而非仅 provider 名字。`mint_handle/resolve_handle` 当前签名不接收任何 principal，不能将“未知 provider 拒绝”视作已经验证两用户权限隔离。

自检：上述两个反例；API key ↔ OAuth ↔ authHeader ↔ none；同名 provider 删除重建；过期、篡改 expiry、跨 provider、跨用户/Agent；旧 refresh 在 cell 替换前/后返回；新 handle 可用而所有应失效旧 handle 被拒绝。严格区分：现已实测的是代次复用，不宣称已从外部 HTTP 利用跨用户漏洞。

### CN-F03：模型没有工具/图像能力声明承载，A02/C06 被缩小成协议族检查

**严重度：BLOCKING；配置结构+真实请求实测。需求：R05-A02、T01-C06、T01 实施步骤 2/4。**

位置：`models/config.rs:32–43,180–187` 的 provider/route binding 无工具支持、图像输入支持字段；`model_exchange.rs:755–760` 的 route request 不携带请求能力；`models/gateway.rs:225–237` 只检查 protocol family 是否服务 operation class。`r05_t01_model_plane.rs:806–840` 的 C06 测试实际上把 chat 绑定到 ASR 协议，不是模型未声明工具或图像能力场景。

两个真实协议入口测试：

- `undeclared_tool_capability_must_make_zero_requests`：模型配置中没有工具能力声明，传入一个合法 tool declaration，实际 HTTP 请求数为 1，要求为 0。
- `undeclared_image_capability_must_make_zero_requests`：模型配置中没有图像能力声明，传入 host 提供的 image 输入，实际 HTTP 请求数为 1，要求为 0。

**证据边界：这里证明“配置中没有能力声明却未拒绝”，不从合成 model 名字推断任何真实模型的能力。** 原规格明确以未声明能力作为前置条件。独立 closed-loop 审查者另以正式 service 二进制复现 tools 外发，可作更强产品入口证据。

修复要求：建立与 provider+model+operation 一致的能力承载和前置验证，覆盖输入/输出模态、工具、推理支持和上下文/输出预算。按真实注册/配置来源迁移能力；不能仅用 provider 品牌或协议族代替模型能力，也不能通过假定所有 chat 模型支持全部能力完成。

自检：明确声明支持、明确不支持、未声明三态 × tools/image/operation × chat/auxiliary/worker；不支持和未声明需要 0 个外部请求及明确错误；不得偷偷删 images/tools 后发文本，也不得换模型；已有正常模型组合继续通过。

### CN-F04：OAuth 登录与模型管理的 R05 服务闭环缺失

**严重度：BLOCKING；生产调用图+原需求确认，非“缺真实密钥”。需求：T02 步骤 1、T02-C08，六个仅归属 R05 的 R00 叶。**

位置：

- `management.rs:586–590` 模型管理路由只有 reload、credentials status 和 revoke。
- `credentials/mod.rs:305–324` OAuth store 从现有文件种入；`:396–423` 无 token 即 NotLoggedIn。
- 同文件 `:104–105` 生产 service 对 OAuth 模块的实际调用是 refresh；`:885,908` 私有 `install_tokens` 只由 refresh flight 调用。
- `models/oauth.rs:460,627,651,732,833` 实现了 device-code/PKCE 流程函数，但 production service 无调用。`r05_t02_oauth_flows.rs` 直接调用这些函数，不证明登录后安装进同一 CredentialService。
- `credentials/mod.rs:220–233` status 无可用模型数量；Rust production 查无 OAuth custom model list/add/delete 实现。
- `docs/rust-tauri/R05/CREDENTIAL_FLOW_MATRIX.json` 自述把“OAuth login flow DRIVING”放到 later stage；这没有覆盖原叶要求的后端 start/callback/poll 状态和模型刷新。

原 R00 叶要求：

| 叶 ID | 需要的可观察行为 |
|---|---|
| R00-T02-LA-16CEB6D12A6A | 添加 modelId 到 provider 注册表，刷新并返回新清单 |
| R00-T02-LA-8060BE8AA02C | 删除 modelId，刷新并返回新清单 |
| R00-T02-LA-99D6C304D697 | start 指引；手输码 callback；设备码 poll done；完成登录后刷新 provider，失败不报告已登录 |
| R00-T02-LA-CA0BF9A7AEA9 | 每个 OAuth provider 的 loggedIn 与可用模型数 |
| R00-T02-LA-CFEC64F68DDE | 指定 OAuth provider 的模型 ID |
| R00-T02-LA-FC80B6C4FBE4 | 删除凭证、清认证缓存并刷新模型列表 |

这些叶在 `R05_SCOPE_MATRIX.json` 的 `r00_execution_stage_ids` 均为 `["R05"]`。已有 revoke 可删除 OAuth token，但没有完整登录/模型状态闭环。

修复要求：把已实现登录状态机接入有认证、权限、取消、过期、并发与重启语义的 Rust 服务入口，成功结果通过同一个 CredentialService 的原子安装与撤销 fence 写入。补齐上述模型注册表及状态行为；保留当前 UI 交互形式，不能要求真实付费/真实 OAuth 授权才能做确定性离线验收。无 OAuth 初始配置后新增 OAuth 的 store 初始化/持久化亦需涵盖（bootstrap 目前仅 needs_store 时开库）。

自检：每种现役 flow 在服务入口完成 start→callback/poll→credential ready→调用模型→refresh→logout→restart；正确/错误/过期/重复 state；重复 callback 零重复安装；登录中撤销/配置变化；新增/删除/列表模型及数量准确；非授权用户 0 写入；每个 R00 叶有独立具体断言，不能统一映射“CredentialService 套件全绿”。

### CN-F05：系统/手动代理与显式私有 CA 未实现，自行延期违反原 R05 必需项

**严重度：BLOCKING；源码结构与原需求确认。需求：R05-T05 步骤 1、必须交付代理/证书测试、T05-C12。**

位置：`models/dispatch.rs:98–105` 与 `models/oauth.rs:161–165` 均固定 `.no_proxy()`；`ProviderConfig` 只有 protocol/endpoint/auth，无冻结网络策略或附加 CA。`R05_INTERFACE_EVOLUTION.md:487–504,588–590` 和 `R05_BLOCKERS.md:19–23` 把代理/私有 CA 推到后续网络加固。

原 09-23 阶段规格明确统一“系统代理/手动代理/直连、localhost bypass、企业根证书”；10-02 C12 的前置明确包括系统/手动/直连/NO_PROXY、有效/无效证书及测试私有 CA。没有查到用户批准这些 OFFLINE 必需项延期；允许 LIVE 真实凭证验证最迟 R10，不等于网络实现与本机私有 CA 测试可延期。

本项不声称 TLS 已关闭：当前默认拒绝无效证书是正确方向，但只是 C12 的一条腿。操作系统根信任也不能替代显式网络配置与可复核的附加 CA 通道。

修复要求：实现统一且可冻结的路由策略及显式可信 CA 来源，让 chat/OAuth/辅助/媒体/资源下载读取一致策略；符合现役 localhost/NO_PROXY 语义，保留默认 TLS 验证；配置错误明确拒绝。不能以环境代理可能不可靠为由永远强制直连，也不能在故障时退回不验证证书。

自检：计数代理+受控源服务验证系统/手动/直连/NO_PROXY/本地 bypass；合法公网链、测试私有 CA 未授权拒绝/授权通过、错主机名、过期/无效证书；同表覆盖 OAuth refresh、chat、operation 和 download；总预算/取消包括代理连接/TLS；每条模式捕获目的地，秘密不在代理诊断/导出泄露。

### CN-F06：egress 只检查 URL 字面 IP，域名解析到回环仍建立连接

**严重度：BLOCKING；真实 TCP 实测。需求：T05-C11（DNS/redirect 与权限一致）。**

位置：`models/egress.rs:322–363` 只在 host 是 IPv4/IPv6 字面量时做受限地址检查，https 域名直接通过；`:355–356` 注释甚至明示把 DNS rebinding 视作 offline-undecidable gap；`:371–379` 随后使用普通 reqwest client 重新解析/连接，无已校验 DNS 地址钉住。

`domain_resolving_to_loopback_must_not_be_connected`：allowlist 仅包含 `https://authorized.example.invalid/v1`；下载 `https://localhost:<随机端口>/private-image.png`；真实 OS 解析 localhost，未授权 127.0.0.1 listener 接到 TCP 并读到 TLS 握手字节。

**验证边界：本测试证明未授权内网连接已发生；没有关闭 TLS、没有供应商凭证发送、没有伪称成功读取私有 HTTPS 内容。** 具有合法证书但 DNS 指向私网的域名造成的完整内容访问是该缺口的后果风险；本轮未进一步搭建受信 TLS 域名。

修复要求：实际解析并验证所有候选地址，对连接使用通过检查的结果，处理多地址、IPv4/v6、代理解析及重定向，不留下“检查一个地址、客户端另外解析”的窗口；显式授权本地模型 origin 的例外需保持有范围，不能扩大到任意内网资源。

自检：localhost/普通域名→回环、RFC1918、link-local、IPv4-mapped、混合公网/私网记录、解析改变、跨 origin 跳转；哨兵目的地 0 连接；显式授权本地服务正向通过；SNI/Host 与 TLS 验证不被 IP 钉住破坏；下载始终无供应商认证头。

### CN-F07：非 2xx 错误 body 没有总预算与读大小约束

**严重度：BLOCKING；时限实测，大小缺口源码确认。需求：T05-C01/C03，T06 辅助调用预算。**

位置：`models/dispatch.rs:138–148` 得到响应头后直接 `response.text().await.unwrap_or_default()`，既不传绝对 deadline，也无大小 cap；此共享函数被全部 chat 和 `models/operations/mod.rs:237–246,268–281` 使用。该文件 `:326–338` 的 `classify_error_response` 又把 `None` 传给 bounded reader，并吞掉读错误。

`error_body_must_obey_total_deadline`：模型替身迅速返回 401 响应头及 1 个字节，声明还有 body 后保持连接；deadline=现在+50ms，外层观测 250ms 时真实函数仍未返回。因此首字节有时限不代表整个请求有时限。

影响：错误响应会继续占用配额/连接直到别层更长 idle/cancel；遇到不断输出的大错误 body 会全量缓冲。不能因为某些 service 调用的上层另有取消，就把共享网络契约标 PASS。具体正式 run 的最长滞留取决于上层时限，本测试没有宣称每个入口永远挂起。

修复要求：统一对成功/错误/redirect/解析失败 body 使用有界读取，涵盖剩余绝对预算、空闲、取消与有限错误片段；保留原错误类别，但不能吞掉 body 超时/截断/读失败形成假完成。视频回退等特殊错误分类必须携带原 deadline。

自检：401/403/429/5xx/3xx 每类分别 stalled body、slow trickle、超大body、中断、合法小body；总期限已快耗尽、取消与EOF竞争；chat/auxiliary/worker/operation/OAuth/资源下载逐入口；记录 socket、permit、真实请求数与时长，不能盲重试或错误地记账多次。

### CN-F08：脱敏只覆盖部分错误路径，协议错误可携带完整 key，截断可泄露 key 前缀

**严重度：BLOCKING；两条实测。需求：T02-C09，秘密不进入 kernel/事件/可见投影。**

位置：

- `models/dispatch.rs:355–359` 先 `.chars().take(512)` 再按完整凭证匹配 scrub。
- `models/credentials.rs:190–198` scrub 只做原文 exact replace。
- `models/openai_completions.rs:303–306,345–350` 协议解析/流错误直接返回 kernel `ProviderTurn::Failed`，没有通过当前 auth 的材料脱敏；`:626–635` 把供应商返回的未知 tool name 放进错误。

实测 1：`truncation_must_not_expose_credential_prefix` 把合成 key 放到错误片段第 505 个字符开始，截断后的 `secret-c` 不再匹配完整 key；随后再经过 service `redact_line` 仍残留。没有用真实凭证。

实测 2：`protocol_error_echo_must_not_carry_complete_secret_into_kernel`：真实 HTTP 已接收 synthetic key `dummy-cobalt-key`，测试模型在 SSE 的未知 tool name 回显该 key；GatewayedProvider 返回 `ProviderTurn::Failed`，message 包含完整 `dummy-cobalt-key`，再次调用 service pattern redactor 仍未去除。这个测试在收口指令前已提交运行，故最终反例数从 9 增到 10。

修复要求：在明确的宿主边界统一处理全部错误及诊断输出，先对材料脱敏再截断；当前 credential、刷新时涉及的旧/新材料及安全编码表示都要有覆盖，避免只保护非2xx的body excerpt。不能通过删错误说明掩盖协议问题，也不能修改模型正常用户内容的语义来假装脱敏。

自检：secret 跨 512 截断边界、各 offset；未知tool/finish_reason、schema/type错误、SSE内错误、HTTP body/headers、OAuth错误、URL编码/JSON转义后的回显、二次 refresh 的旧/新材料；扫描 kernel 结果、DB、事件、诊断、导出、worker env/参数和产物，只有受控凭证库存储允许存在真实材料。保留有意义且不含秘密的错误。

## 修复后整体验收矩阵

1. 从同一个候选树读取原 R05 A-ID、10-02 C-ID、R00 适用叶；每项有明确公开行为、测试命令、请求/存储副作用计数和实际结果。A02 不能用错协议族样例代替；OAuth 六叶不能合并成“通用凭证套件存在”。
2. 先让本目录的 10 条要求断言在修复后通过；迁入仓库时保持行为要求，不能删除/放宽断言。对 scratch probe 因正式接口演进产生的编译变更只允许更新调用方式。
3. 加入各发现下的同根因矩阵，尤其真实管理面 reload→401→重试、登录→持久化→logout→重启、DNS→目的地连接和 error-body budget。
4. 所有测试用真实 Rust Gateway/CredentialService/adapter/HTTP 客户端及隔离 store；外部供应商用受控服务。要求正式 service 管理入口测试，不能只直接调用内部函数证明已经接线。
5. 重跑受影响的 R05-T01/T02/T05/T06，以及协议/worker/闭环与 gate 的必要回归；按原任务书重新运行阶段 gate、无Pi文件工具闭环、取消/恢复和静态边界检查。测试数不是放行依据，所有 REQUIRED 行为都必须有证据。
6. 修复候选绑定 HEAD/tree/lockfile/toolchain/config/fixture，使用未参与实现的新审查者做完整阶段复验。外部 LIVE 真服务可以按授权要求登记 BLOCKED_LIVE；这里确认的能力、OAuth后端接线、proxy/CA、DNS与错误body安全属于可本机离线实现验收项，不应延期放行。

## 未定结论、限制与不重复项

- `install_tokens` 与多 provider reload 构建/换表之间还有可疑时序，但本轮未另建屏障复现，**不计入已确认 finding**；在 CN-F01/F02 重做一致快照/代次时应纳入并发压力与确定性时序矩阵。
- 当前 handle API 无 principal，原 C12 的两 Agent/用户作用域证据不足；本报告没有据此宣称某个现有外部 HTTP endpoint 已可盗取任意用户凭证。
- DNS 测试观测到 TCP/TLS 握手，未获取受信 HTTPS 私有内容；TLS 默认验证仍开启。
- 未调用任何真实付费模型/登录服务；这与本轮 FAIL 无关。
- worker/usage 审查者另证实 operation 排队不计入 deadline、拒绝前零 HTTP 却记 transport attempt，以及资源路径授权使用问题；属于其报告，本报告仅建议与 CN-F07 同表回归，不重复列为新的网络反例。

