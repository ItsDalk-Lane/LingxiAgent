# EX03 当前 GitHub 工具入口补齐

现行原文已核对：R06-EX-W03_SPEC 的 reported_gap/proposed_real_checks，以及 lib/tools/install-skill.ts 的整体开关、allow_github_fetch、安全审查/一次token、完整目录安装且仅当前助手启用。该入口仍现役，未复活废弃能力。本包只补本次指定服务缺口，不宣称整个EX03或阶段验收完成。

隔离树基线：484504bb4cb28ec4378918745e510e6408bb99cb；7份净源码，逐文件基线/候选SHA在 github-ex03-manifest.json。
候选聚合：3dabe4a1d165189394447500ee326731e1866d9aa830f6e71d5eb60fb06a4970。
补丁：fc5202c9410a95854aabd3b41e6b16a56058877b0f04f9a91f78e00a074c3ae9。
Main/QuickChat冻结包/门禁/登记未改，未提交/推送/安装依赖，新target为零；仅复用获分配的service缓存，现已释放。

改动：
- memorytools消费实际开关，只有enabled与allowGithub都真才下载；校验绑定助手后操作，原Github恒拒分支移除。
- 严格GitHub源解析，固定codeload/{owner}/{repo}/zip/HEAD（保留原TS HEAD语义），保留tree的包子路径；拒绝伪host、凭证、query、越界和编码混淆。
- 沿现役NetworkPlane与EgressGuard：公网地址检查/固定DNS、无重定向、无供应商凭证，32MiB读取上限与15秒总时限；只新增archive MIME分支，原media MIME、上限与错误文案保持。
- 复用PreparedPackage的ZIP/.skill校验和RAII清理，支持GitHub子目录包，本地工具入口也复用完整包准备；未引入新解压依赖。
- 正常guard审查和风险一次token未绕过，包内容变化后旧token拒绝；下载/审查取消及提交前取消明确失败。关闭审查时明确标注跳过，不伪称审查通过。
- install_skill恢复原Review权限合同：operate也需实际确认，read_only/无人值守子代理仍由既有规则拒绝，权限未扩大。
- 模型工具的安装先在同盘隐藏目录准备再提交，同名合并保留旧额外文件；失败/提交前取消不暴露半包，失败恢复时保留原材料且明确报告。只改当前助手启用清单，default-enabled:false不自动给其他助手启用。

最终真实测试：
`CARGO_TARGET_DIR=/private/tmp/lingxi-ex01-target CARGO_INCREMENTAL=0 /Users/study_superior/.cargo/bin/cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service --lib github_tests -- --nocapture`

github-tests-frozen.log：6/6 PASS，0.15秒（正常重编28.55秒）。真实TCP HTTP归档替身，固定GitHub URL经过生产解析/开关，再由仅cfg(test)传输地址替换接本地合法endpoint；核心Egress下载/解压/审查协议/工具执行/安装/网关/R03审批均生产代码，没有模拟安装核心。生产二进制没有替换入口，Github不能由输入改为私网地址。

硬断言包括：
1. enabled/allowGithub关闭零请求；安全判定一次；完整SKILL/references/scripts逐字；另一助手配置不变；runtime_skill_catalog仅当前助手可用。
2. 风险拒绝零安装，真实一次token接受，换包后旧token拒绝；302不跟随、坏ZIP、下载和审查中取消零安装。
3. 伪host/越界/编码混淆零网络；safety=false不调用guard且明确标注；本地.zip/.skill安装仍可用。
4. 真实生产注册 + R03驱动：operate先waiting审批且零请求，Reject后零安装；Approve后1次真实HTTP下载和真实安装；read_only再调用零新增请求，真实工具结果Failed。
5. 越界ZIP、展开超过32MiB、目标是原有文件均拒绝；文件逐字不变且池中无隐藏半成品。
6. 实际公网生产路径尝试记录，见下面限制；该检查只证明安全拒绝，不是公网成功验收。

最终正向证据：
`/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-github-install-cddf71222b1b6ad8/proof.json`
最终真实R03审批证据：
`/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-github-install-3855ecb9e9c9e793/driver-proof.json`

原回归 github-regression.log：r06_t06_skills 6/6 PASS（审查/token、R04工作区拒绝、助手绑定、HTTP白名单/覆盖顺序/只读/bundle）。
旧egress回归 github-egress-regression.log：7/7 PASS。
严格service --lib --tests Clippy -D warnings：github-clippy-frozen.log PASS17.33秒。
净补丁重放7文件逐SHA一致：github-replay-result.json。

基线失败：github-baseline-evidence/probe.rs是从冻结基线原Github if块原样提取的独立Rust执行探针（仅text_success返回类型简化为字符串，不冒称完整服务运行）。enabled=true/allowGithub=true时仍返回“not enabled”，raw-failure.log实际exit101。完整候选成功由上述真实核心/真实驱动证明。

限制必须保留：本机实际DNS将github.com解析198.18.0.152、codeload.github.com解析198.18.0.16；真实生产下载尝试被既有公网保护在拨号前拒绝。没有添加私网放行、替代IP或关闭TLS。因此公网GitHub成功下载仍是NETWORK_REFUSED，不能记公网PASS。正向成功证明来自规格允许的真实HTTP替身。安全判定端口使用现有SkillGuardPort注入safe/suspicious/unavailable等确定结果；本包未重新调用付费guard模型，不冒称当前真实供应商验证。Windows/Linux运行与正式打包NOT_RUN。

合并仅精确hunks：lib只给skills绑定已有deps.network_plane；egress不碰socket_addr_is_guarded声明，可与Root资源URL的pub可见性小hunk共存；其余skills/memorytools模块归本包。不覆盖Root当前资源/QuickChat或其他作者更改。
