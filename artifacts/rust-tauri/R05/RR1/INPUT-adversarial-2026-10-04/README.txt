灵犀 R05 对抗审查证据与复现包｜2026-10-04

结论：冻结候选尚未完成R05；先执行同包prompt中的完整修复总控提示词，不能直接开始R06。
仓库：https://github.com/ItsDalk-Lane/LingxiAgent
分支：codex/rust-tauri-migration
被审HEAD：d80737b6cb9186c8a18c0f35923aac00249d45c3
比较基线：c549ff654508ab951e2cf39cf9d309fc9c6b8656
本次只读审查，仓库工作树始终干净，无commit/push，无真实收费/OAuth账户请求。

阅读顺序
1. prompt/：可整体复制执行的R05修复总控提示词，28项修复、完整原验收身份、逐项自检和阶段放行规则。
2. audit/credential_network/report.md：凭证、能力、OAuth、代理/CA、DNS、期限和脱敏。
3. audit/protocol/PROTOCOL_AUDIT.md：协议重放、签名、工具批次、过程/final、消息顺序。
4. audit/worker_usage/R05_T06_T07_ADVERSARIAL_REVIEW.md：资源读取、媒体身份/取消、预算和usage。
5. audit/closed_loop/R05_CLOSED_LOOP_AUDIT.md：正式二进制及实际文件/HTTP/重启证据。
6. audit/gate/R05_GATE_AUDIT.md：独占叶、C-ID换名、资源验收、历史源码绑定。
7. audit/runtime/runtime-audit-result.json：82次登记运行的254测试，253通过、1资源采样环境受阻；每条选用日志及SHA256。
8. specifications/：此次用户原始任务书相关文件与10月2日完整专项提示词，未改写。R06文件只供核对前置和边界。

证据等级与正确解释
- 协议probe：13测试，1正常对照通过、12正确契约断言失败。
- 凭证/网络probe：10正确断言失败。
- worker/operation/usage：两套新增audit_测试分别3失败、5失败。
这些退出101是缺陷反例，不是编译错误；有跨域重复，不能相加当独立缺陷数。
- 正式二进制的binary-result/process-only-result/batch-result及HTTP快照，直接证明真实文件和持久化行为。
- 本次原runner退出1，未把253个测试通过冒称整个R05门禁通过。RSS测试因PID namespace与/proc不一致无法采样，不证明产品内存泄漏。FD谓词与默认lsof格式不匹配是另一项已确认验证器缺陷。
- C-ID换名：实际运行原/变异7个xtask镜像测试；Python assembler输入为历史日志fixture，未声称完整变异verify-stage实跑。
- 三份历史manifest各483个文件与当前生产源码相同，不能因旧testedSha字面不同就否定其同源码证据。
- Google编码/签名与官方契约比较未调用收费API；DNS反例仅证明未授权TCP/TLS连接；macOS say问题为源码确认，尚待有效平台实测。

复跑说明（给编码智能体）
保留原始证据不改写，在隔离目录做工作副本。包内未附编译器、构建缓存、node_modules、完整仓库副本、临时home数据库或本地认证令牌。源代码从上述仓库取得；审查旧红应使用冻结HEAD的隔离checkout，不要回退或覆盖用户现有分支。

若把仓库放在解压目录内名为LingxiAgent，与audit目录同级，则三个Cargo.toml中的../../LingxiAgent路径可以直接解析。使用Rust 1.98.1及包内Cargo.lock，第三方依赖保持锁定。依赖已准备后可加--offline。

cargo +1.98.1 test --manifest-path audit/protocol/Cargo.toml --locked -- --nocapture --test-threads=1
cargo +1.98.1 test --manifest-path audit/credential_network/Cargo.toml --locked -- --nocapture
cargo +1.98.1 test --manifest-path audit/worker_usage/Cargo.toml --locked --test operations_audit audit_ -- --nocapture
cargo +1.98.1 test --manifest-path audit/worker_usage/Cargo.toml --locked --test usage_audit audit_ -- --nocapture

上面是复现现有缺陷的命令，原HEAD预期是失败，不是要求把测试预期改成当前错误行为。修复后将永久回归纳入仓库正式gate。

三个Python正式二进制probe保留原始源码和原路径，运行前只在工作副本中把ROOT改为隔离证据输出目录，BIN改为对应候选实际编译出的lingxi-service；原ROOT/BIN并不代表你的机器路径。gate脚本中的环境/仓库路径同理按真实环境调整。各报告引用的原绝对路径以audit/后的相对路径对应包内文件；不是每个临时home文件都归档，关键HTTP/结果/源/日志已保留。

重要：修复F01后，其他旧probe若没有新要求的能力声明，会在更早边界拒绝。必须按完整提示词补齐合法前置，并证明命中了所测的schema、ID、重试、规范化或取消边界。不能仅因零HTTP/零文件或测试不再到达缺陷就宣布修复。

秘密与隔离
包中错误反例含dummy-*等合成秘密和失败时泄漏的测试标记，这是原始证据；没有真实provider凭证。受控服务收到的授权样本、输入夹具及旧失败证据，与修复后宿主应保持无秘密的输出分开审查，不能全局排除测试目录骗过扫描。

MANIFEST.json记录每个归档文件的SHA256及字节数。索引中的历史源码路径、原运行时间和工具结果按原样保留；不要为美化结果重新生成旧失败日志。
