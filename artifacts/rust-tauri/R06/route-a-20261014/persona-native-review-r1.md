# 人格八叶与原生 write 可用性补丁独立审阅

日期：2026-10-14。审阅状态：**FAIL / needs-fix**。结论：正常行为与已测失败腿通过，但发现一个人格材料一致性缺陷及一个相邻原生 edit 入口缺口。不能据此签 R06_ACCEPTED 或 R07_READY。

## 审阅对象与边界

- 原基座：2c8f8ddd0b75b4de61ebd047a0246a7f88f6d486。
- 仅在独立 detached 树 `/private/tmp/lingxi-r06-review-persona-native` 应用 `/private/tmp/r06-persona-completion.patch`（SHA256 `9040141da4d7f2e1a16603637779259cefdc92917fd5bdc63efe020597864de9`）与 `/tmp/r06-native-write-20261014/native-write.patch`（SHA256 `96c7f0166f74c3df4a7389793c5794064633033278219a3c826a3aa41c1045b8`）。没有修改主树、作者树、生产补丁、原 assertions、门禁、.vscode 或 .sync-audit；没有提交、推送。
- 现役基准读取：`server/routes/agents.ts`、`server/routes/avatar.ts`、`core/agent-manager.ts`、`core/agent.ts` 及生产 Rust router/auth/persona/roster/native files/sandbox。
- cargo 为 `/Users/study_superior/.cargo/bin/cargo`，实际版本 `1.98.1 (797e8a9bc 2026-08-05)`，所有测试 `--locked`。合成 HOME/LINGXI_HOME/TMPDIR/产品材料均在 `/private/tmp/r06-review-runtime` 内；没有读取真实业务 HOME 数据。只复用现有工具缓存 CARGO_HOME/RUSTUP_HOME。
- `CARGO_TARGET_DIR=/Users/study_superior/.cache/lingxi-r06-builds/reviewer-target`，明确在 /private/tmp 与 TMPDIR 允许范围外；沙盒拒写 sentinel 不是因为位置已被允许而虚绿。

## 确认发现

### P2：简介提交只检查目标正文，另一人格材料在模型生成期间改变仍提交过时简介

定位：`rust/crates/lingxi-service/src/persona.rs:583-588`；`description_for` 在 256-269 读取两份人格材料，sourceHash 在 282-290 绑定双方内容，但返回后只对 `name` 对应的目标正文做 before/current 比对。外部 writer 不受 PersonaService 的 HTTP 写入锁约束。

真实反例：先持久写 `identity.md=原始身份`、`AGENTS.md=原始说明`。调用真实认证 HTTP PUT identity，新身份带审阅标记；供应商 loopback 替身收到实际生产请求后，作为外部用户将 AGENTS.md 改为 `用户后改说明`，再交还模型回复。实际请求包含 `新身份\n\n原始说明`，当前服务返回 **200 + descriptionRefresh=updated**，新身份与旧来源 hash 简介均提交，并广播 `agent-updated`；磁盘 AGENTS.md 已是 `用户后改说明`。下一消费者现读 description.md，因此会沿用与当前人格材料不一致的简介。

独立检查 `reviewer_sibling_material_change_is_refused_before_description_publication` 期待在材料变化时 409 且不提交正文/简介/事件，实际 **FAIL，200 != 409**。日志 `/tmp/r06-review-persona-counterexample.log`。反例源码仅在独立树 `tests/r06_review_persona.rs:929` 起；外部模型只回答和触发外部 writer，生产 HTTP/网关/存储/落盘/事件均真实。

修复建议：生成前捕获全部参与来源校验的材料；模型返回后复检同一快照（至少 identity、AGENTS、配置/替换参数、实际回落模板），发生变化则明确冲突或重新生成，不能用旧材料生成结果发布成功。该缺陷影响 4870805E3A3D、856B28902999 的描述刷新真实性。

### P2：沙盒不可用拒写规则只进入 write，edit 仍可写同一文件

定位：`filetools.rs:1867-1878`（新增检查仅 run_write），`filetools.rs:1976-1977`（run_edit 直接开始 ResourceOp::Write），两者随后共享真实落盘与产物链。

真实反例：正常 read 读取 ORIGINAL，随后将系统助手定位 override 指向实际不存在的 `missing/sandbox-exec`，再通过生产注册工具与真实 gateway 调用 edit（有效 edits[] 参数）将 ORIGINAL 替换 CHANGED。实际 **ToolOutcome::Success**，实际磁盘为 CHANGED，并含实际产物 ResourceRef。独立检查 `reviewer_edit_refuses_unavailable_native_sandbox_before_write` **FAIL**。日志 `/tmp/r06-review-native-counterexample.log`；反例源码 `tests/r06_review_native.rs:1866` 起。

该测试仅替换助手定位，不伪造拒绝或模拟生产写入。与原 write unavailable 新测试同一 seam。建议原生变更的共同前置条件同时覆盖 write/edit，避免同一可写文件在沙盒不可用时可经 edit 修改。

范围校准：4F0466F7C090 的叶动作严格名为 write，所以本反例不能伪装成该叶 write 请求本身失败；该叶新增的 unavailable write 路径确实通过。这里报告的是相邻写入入口不一致，是否构成该叶验收阻断应由主控按既定范围判断，不能借此扩大原断言或虚造 gap。

## 独立验证结果

1. 两份精确补丁的原有三套件：`r06_persona_completion` 11/11、`r04_t04_file_tools` 11/11、`r04_t06_sandbox` 10/10，共 **32/32 PASS**。日志 `/tmp/r06-review-persona-native-test.log`。人格测试 14.04s，文件工具 0.78s，真实沙盒 2.52s。不是引用作者已有 PASS。
2. 新增独立真人格并发反例：1 FAIL，详见发现一。
3. 新增独立 edit unavailable 反例：1 FAIL，详见发现二。首次编写反例用了错误的旧参数形状，被生产 gateway 明确拒绝；修正为真实 edits[] 后才复现写入成功。旧失败不作为缺陷证据。
4. 新增真实头像下游失败检查 `reviewer_role_avatar_actual_downstream_failures_are_explicit_and_restore_bytes`：**1 PASS**（2.74s）。对 user 与 agent role，真实第四格式 webp 加 macOS uchg 后分别 POST、DELETE；四条实际 I/O 拒绝均 HTTP500、明示错误，前面已删格式恢复、原字节全部保持、事件未增加、没有临时残留。GET 在实际目录占据头像文件位置时500明确报错。role 无效/缺 agentId/未知助手的 GET/DELETE 实际400或404，事件未增加。日志 `/tmp/r06-review-avatar-downstream.log`。
5. `git diff --check` PASS（生产精确补丁与独立测试）。未重复跑全仓 clippy/fmt 或整阶段门禁，未作其它平台/真实付费供应商/桌面 UI/正式构建证明。

通用测试环境命令前缀：

```sh
env HOME=/private/tmp/r06-review-runtime/home TMPDIR=/private/tmp/r06-review-runtime/tmp LINGXI_HOME=/private/tmp/r06-review-runtime/home LINGXI_PRODUCT_DIR=/private/tmp/r06-review-runtime/product CARGO_HOME=/Users/study_superior/.cargo RUSTUP_HOME=/Users/study_superior/.rustup CARGO_TARGET_DIR=/Users/study_superior/.cache/lingxi-r06-builds/reviewer-target /Users/study_superior/.cargo/bin/cargo
```

在独立树执行的 cargo 参数：

```text
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_persona_completion --test r04_t04_file_tools --test r04_t06_sandbox
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_persona reviewer_sibling_material_change -- --nocapture
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_native reviewer_ -- --nocapture
test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_review_persona reviewer_role_avatar_actual_downstream_failures -- --nocapture
```

## 九叶逐项核对（本审阅结果不替代叶封印）

| 叶 | 实际核对 | 状态/限制 |
|---|---|---|
| 4870805E3A3D | AGENTS持久、ishiki同文件；字符串/空串/坏类型、无权、未知助手；成功事件实际HTTP读回；模型简介、实际下一Run供应商请求消费简介 | 常规全部通过；发现一阻断描述材料一致性 |
| 856B28902999 | identity同上；失败模型500、原正文及事件不变；真实简介文件拒写后原身份补偿 | 常规全部通过；发现一同类影响 |
| 7C699FD2A740 | public正文、空串、两地址同效、事件、坏body/权限/未知助手；真实目录错误与链接拒绝零事件 | 已测行为通过，未见新缺陷 |
| 7F0F3F76E38A | png/jpeg/webp真实HTTP，jpg规范、旧四格式清理、助手列表hasAvatar、新事件；>1MB允许、>15MB413；无权/坏格式/坏base64/链接拒绝 | 已测行为通过；独立role下游POST真实500及原字节补偿 |
| BB4CD1E4CD43 | 现役四格式与legacy四格式全部清理，无头像重复DELETE200，真实事件与权限拒绝；uchg部分删除500补偿 | 已测行为通过，未见新缺陷 |
| 7570BE1F2527 | role user/agent定位与agentB隔离，实际格式，旧格式清理共享生产函数，角色输入/权限/链接/实际I/O拒绝 | 已测行为通过，独立两role真实POST失败腿通过 |
| 4A56F26EDA32 | user/agent DELETE→GET404，重复删除200，另一agent保留；角色输入/权限/链接/实际I/O拒绝 | 已测行为通过，独立两role真实DELETE失败腿通过 |
| E4BEE8E6974A | 图片真实bytes/MIME/ETag，匹配304空body，无头像404，auth先于条件请求；链接与实际目录I/O500拒绝 | 已测行为通过，未见新缺陷 |
| 4F0466F7C090 | 真实write冲突/越权/符号链接/原子性旧套件；实际missing与untrusted助手不可用时原字节/父目录/临时文件不变；恢复系统助手write成功，再失效拒绝无成功缓存 | unavailable write真实PASS；发现二是相邻edit入口缺口，不能说成该叶write失败。此前SessionFile登记供应商链本轮未重跑 |

账本/错误身份核对：辅助模型调用使用真实 AuxiliaryExecutor、同一 dispatch/credential/network/quota，summary未配置才显式chat回落；已发送的供应商失败不切路，content_filter后HTTP500与失败账本行均真实。账本purpose=auxiliary.summarize、origin=persona.description、cause_ref=agent:agentA，未伪装会话/Run归属。缺network明确500、无请求、正文与事件不变。HTTP错误当前为明确的 error 文本，不新增结构化原错误 code。无模型时返回descriptionRefresh=model_unavailable属作者显式诊断边界，本审阅未假称这种情况下已生成新简介。

原生沙盒真实性：可用性检查实际验证系统助手元数据与执行能力；macOS双腿probe已跑且真实沙盒套件32项中的10项通过。此patch不把父进程native写入移入OS沙盒；仍由FileTools/FileSafety保障路径/版本与原子性。没有把probe通过等同于父进程受OS containment。

## 完整原 assertions（从现行绑定直接提取）

### R00-T02-LA-4870805E3A3D

- 成功：执行“写助手 AGENTS.md”后，持久写入正文、刷新描述并广播 agent-updated；/ishiki 同一文件
- 拒绝/边界：body.content 必须为字符串，包括空字符串；不应错误扩大写入或披露范围

原 Then：持久写入正文、刷新描述并广播 agent-updated；/ishiki 同一文件

### R00-T02-LA-4A56F26EDA32

- 给定目标与相应权限，执行“恢复指定用户或 Agent 的默认头像”后核对：移除该 role 的自定义头像文件，再读取返回 404
- 输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果

原 Then：移除该 role 的自定义头像文件，再读取返回 404

### R00-T02-LA-4F0466F7C090

- 磁盘内容与请求一致
- 冲突/越权不写
- 并发版本冲突、路径越权/符号链接逃逸或沙盒不可用时拒绝，不覆盖用户后改。

原 Then：文件字节改变并登记会话文件产物。

### R00-T02-LA-7570BE1F2527

- 给定目标与相应权限，执行“上传指定用户或 Agent 的头像”后核对：删除该 role 的旧格式头像并写入新图片，返回实际格式
- 输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果

原 Then：删除该 role 的旧格式头像并写入新图片，返回实际格式

### R00-T02-LA-7C699FD2A740

- 成功：执行“写公开助手说明”后，写入 AGENTS.public.md 并广播 agent-updated；兼容旧地址同效
- 拒绝/边界：body.content 字符串，可为空；不应错误扩大写入或披露范围

原 Then：写入 AGENTS.public.md 并广播 agent-updated；兼容旧地址同效

### R00-T02-LA-7F0F3F76E38A

- 成功：执行“上传头像”后，删除旧格式头像并写入新的图片，刷新助手列表并广播 agent-updated
- 拒绝/边界：base64 data URL 仅 png/jpeg/webp；15MB body limit；不应错误扩大写入或披露范围

原 Then：删除旧格式头像并写入新的图片，刷新助手列表并广播 agent-updated

### R00-T02-LA-856B28902999

- 成功：执行“写助手身份文档”后，持久写入 identity.md，刷新描述并广播 agent-updated
- 拒绝/边界：body.content 字符串，可为空；不应错误扩大写入或披露范围

原 Then：持久写入 identity.md，刷新描述并广播 agent-updated

### R00-T02-LA-BB4CD1E4CD43

- 成功：执行“删除助手头像”后，删除所有头像格式并广播 agent-updated
- 拒绝/边界：即使没有头像仍返回 ok；不应错误扩大写入或披露范围

原 Then：删除所有头像格式并广播 agent-updated

### R00-T02-LA-E4BEE8E6974A

- 给定目标与相应权限，执行“查看指定用户或 Agent 的自定义头像”后核对：返回图片及 ETag；未设置返回 404，ETag 相符返回 304
- 输入无效、权限不足或下游失败时应返回明确错误，不呈现成功结果

原 Then：返回图片及 ETag；未设置返回 404，ETag 相符返回 304

## 源码坐标

精确合并候选生产源码SHA256：

- persona.rs `ba0e79d58c8e0cce643c2d197f3e891669c2b3e99bb7947213f632797af682a3`
- filetools.rs `23130898fdbf5f82229c8d248521445215f003836539170f33e2c56f0e427a7c`
- sandbox.rs `276feae6510cbd33b3d3ce64a8e24bee3c43d88c52d3e137d6a052595524f9b2`

独立反例源码SHA256：

- r06_review_persona.rs `74695d89aa00af4cd7bff08a36b6c71c9cfedda556796112d236924fbf967a78`
- r06_review_native.rs `af3dfd503f3d6721ccd26141756e1e989e0e25a2d6b4c4dac043b09964935492`

注：反例源包含复制的作者fixture与原tests；运行时用reviewer_过滤，仅执行独立新增案例；复制并未更改作者原测试。两个反例文件完整路径为 `/private/tmp/lingxi-r06-review-persona-native/rust/crates/lingxi-service/tests/r06_review_persona.rs` 与 `/private/tmp/lingxi-r06-review-persona-native/rust/crates/lingxi-service/tests/r06_review_native.rs`。工作树、target与日志保留待修复后同一审阅任务复验。
