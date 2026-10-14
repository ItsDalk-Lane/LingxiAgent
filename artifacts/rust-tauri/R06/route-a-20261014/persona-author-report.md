# 人格八叶实现交付（2026-10-14）

工作树：/private/tmp/lingxi-r06-persona-completion；detached 6d1933279。未提交/推送，主工作区未改。patch：/private/tmp/r06-persona-completion.patch。

## 实现

4870805E3A3D、856B28902999：AGENTS.md / identity.md 成功持久化后产生 agent-updated；ishiki 同文件。真实 AuxiliaryExecutor 经同一网关/凭证/网络/配额生成 description.md，summarize 槽未配置显式回退 chat；供应商失败不换路，发生在正文提交前且明确500。sourceHash 复用相同材料避免重复生成；写空文档后简介使用实际模板回落源。简介写入失败补偿恢复原人格，临时文件清理，拒绝不广播。生成期间正文已被别的 writer 改动则409，目录/链接变化复检后拒绝。无模型时正文仍能写，响应显式 descriptionRefresh=model_unavailable，不伪造简介（与旧TS best-effort的功能边界对齐并增加可见诊断）。

7C699FD2A740：公开说明正文/空字符串合法持久化；public-ishiki 双向同效；成功后发布事件，未知助手/坏body/权限/链接/实际I/O错误零成功事件。

7F0F3F76E38A、BB4CD1E4CD43：头像按现役 avatars/agent.* 布局新写，兼容读取/清理此前候选根目录 avatar.*。data 与 dataUrl 均可接受，png/jpeg/webp，jpeg规范为jpg；响应含format+ext。旧格式全部清理，失败恢复原字节；DELETE无头像幂等ok，实际第四格式删除失败会补偿前面已删格式。列表现读 hasAvatar，成功发布 agent-updated。

4A56F26EDA32、7570BE1F2527、E4BEE8E6974A：新增 /lingxi/v1/avatar/{role}；role=user|agent，agent必须显式?agentId=...，user独立 user/avatars/user.*。读为chat，写为settings.write；坏role/缺agent/未知agent及权限均明确拒绝。GET图片+MIME+ETag（MIME和内容digest）、匹配304空body、无头像404。两条avatar上传路由单独15MB，不扩大普通请求1MB上限，真实验证>1MB成功与>15MB拒绝。删除/上传不串用户或另一助手。链接目录/文件拒绝，不读取/删除其外部目标。

下一消费者：agents_admin 列表现读已生成 description、hasAvatar；context_compiler 的团队花名册现读 description.md（剥sourceHash注释），真实后续Run供应商请求含更新简介，不靠事件冒充刷新；材料发布epoch覆盖人格+简介提交。

## 修改文件与主控整合

src/persona.rs（完整服务+角色头像），src/agents_admin.rs（列表现读字段），src/context_compiler.rs（RosterEntry.summary现读），tests/r06_persona_completion.rs（新增），src/lib.rs（/avatar/{role}三动词；两条avatar方法路由15MB上限），src/auth.rs（角色头像chat/settings.write策略）。未触碰其它代理compaction/model_history/memorytools/ToolSuccess；无依赖/锁文件/schema/门禁/台账改动。

lib/auth patch只在原persona/avatar段附近，可与主控compaction段合并。不修改agent_config_readback.rs。

## 逐叶真实测试映射

- 487080：original_487080_agents_write_refreshes_actual_description_and_next_roster
- 856B28：original_856b28_identity_write_refreshes_actual_description_and_next_roster
- 7C699F：original_7c699f_public_write_and_legacy_alias_publish_only_after_persistence
- 7F0F3F：original_7f0f3f_agent_avatar_upload_cleans_formats_refreshes_next_list_and_event
- BB4CD1：original_bb4cd1_agent_avatar_delete_cleans_all_formats_and_is_idempotent
- 7570BE：original_7570be_role_avatar_upload_selects_explicit_agent_and_user_without_crosstalk
- 4A56F2：original_4a56f2_role_avatar_delete_restores_defaults_and_preserves_other_target
- E4BEE8：original_e4bee8_role_avatar_returns_bytes_etag_304_and_404_under_actual_permissions
- 额外真实边界：persona_description_uses_explicit_chat_fallback_only_for_unconfigured_summary_slot；persona_writes_refuse_actual_filesystem_failure_and_symlink_without_publication；real_immutable_file_failure_rolls_back_persona_pair_and_partial_avatar_cleanup（macOS真实uchg保护：成功正常对照+写/删除真失败+回滚；守卫解flag后清理）。

所有测试走服务真实HTTP认证与生产router，磁盘真实落盘；描述的供应商仅为loopback网络回答替身，网关/配额/账本/人格/编译器/下一Run全部真对象。辅助调用账本检查真实成功/失败purpose+origin+agent因果，不伪造session/run归属；事件发布还经真实 /app-events HTTP读回。拒绝前后文件与事件对比。

## 验证与限制

指定工具链 /Users/study_superior/.cargo/bin/cargo 1.98.1，--locked；HOME/TMPDIR/LINGXI_HOME/LINGXI_PRODUCT_DIR为 /private/tmp/lingxi-r06-persona-runtime 合成目录；CARGO_TARGET_DIR=/private/tmp/lingxi-r06-persona-target-198。CARGO_HOME/RUSTUP_HOME显式使用既有只读工具缓存路径。早期错用了PATH中的Homebrew1.93，已告知主控并以指定工具链重新执行，旧结果不充当最终门禁证据。

R1修订前相关四套件：新增11+原人格2+助手生命周期2+上下文编译21，全通过（36）；本轮只按主控要求复验受影响的新增11，未重复其它25项。严格clippy：cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_persona_completion --no-deps -- -D warnings，通过。fmt --all --check、git diff --check通过。日志：/private/tmp/r06-persona-final-test.log、r06-persona-final-exact-source-test.log、r06-persona-final-clippy.log、r06-persona-final-fmt.log。

上述是macOS本机服务证明，未冒充其它平台、真实付费供应商或旧Electron UI操作；app事件依现有HTTP游标日志接口，未新增WS宿主。未登记READY、未运行R06整阶段门禁、未声明R07_READY；独立review与主控整合后的最终候选重验仍由主控完成。

## 主控复核后的 R1 收紧

- description_for 缺 network_plane 时明确500，响应含 network configuration unavailable，不创建直接网络配置。供应商与凭证失败仍拒绝，不新增换路。正常fixture显式注入生产网络plane；既有chat回退检查内增加缺配置真实HTTP腿：外部请求0、原正文不变、事件不变。
- 人格原子写、头像写的临时文件清理不再 let _ 吞错：不存在属于正常清理结果，其它失败附加 temporary cleanup failed。临时文件仅在本次 create_new 成功后清理，避免删除未获本次所有权的同名对象。
- 简介提交失败后的回滚预检失败保留原提交原因并附加 precheck 错误；头像原字节补偿也用同一原子写，并逐个附加补偿失败。生产 persona.rs 已无 let _ 吞错清理。
- 本轮只修改 persona.rs 与 r06_persona_completion.rs；没有summary-store/dream-ticker夹带。

R1最终验证：11/11 PASS（66.02s），指定cargo1.98.1严格clippy与fmt检查通过。最终patch SHA256：9040141da4d7f2e1a16603637779259cefdc92917fd5bdc63efe020597864de9。
