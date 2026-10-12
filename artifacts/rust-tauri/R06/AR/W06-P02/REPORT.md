# W06 / P02 部分交接

状态：**PARTIAL_HANDOFF_SUPERSEDED_PENDING_NEW_ROOT_CAUSE**。不签READY或PASS。总控依据不可变规格§5.3发现两轮同族FAIL后漏先派全新ROOT-CAUSE；已要求停止P02，由全新ROOT-CAUSE形成全路径矩阵后再派全新P03。本人不复用为P03或Reviewer。

停止前已完成的改动全部保留，未提交、未暂存、未推送。主仓HEAD仍2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d，分支codex/rust-tauri-migration。review3固定副本未写入、未构建、未测试。主仓还有他人W02/W12/W08混合改动，本轮主仓运行只能作中间证据，不能替代隔离最终候选。

## 已做与真实旧红

在修改产品前，亲跑review2真实binary：F01标准合并/普通关闭正对照成立，但字面<</深层字段提升/未知future层级保存违约；现役js-yaml检查105断言87成立18失败，exit1。F02三个未知tag样本真实flags/Run/wire/PATCH错误接受，15拒绝/字节保护断言全部失败，exit1。每子进程实际binary SHA为406279b9ecabfc8d1ce3678c322ae6900064f00dd2f156d893be77d5ba3d47c1，READY/canonical synthetic home/source=cli/guard/真实DB及wire均留记录。源绑定见red-source-binding.json，旧红探针含22样本，不把wrapper的Rust test_count0当零匹配测试绿。

统一agent_config改为官方完整YAML实现：granit-parser完整事件图只校验根/敏感分区键与真正merge全部来源（包括被覆盖来源），保留scalar style/tag与anchor身份；serde-saphyr严格tag、重复键、标准merge/alias语义及成熟serializer实际读取/保存。字面<<保存会正确加引号，再次读取仍普通键；没有自制行扫描/文本preprocess。未知标准兼容值/深层结构保留；unknown custom tag全局拒绝，标准!!str/!!bool/!!map/!!seq正对照保留。

新增serde-saphyr=1.3.0及同版granit-parser=1.3.0。锁仅增加8必要registry包和adapters依赖边，所有既有registry包版本/内容不变，见parser-dependency-review.json；保留现有serde_yaml_ng给公共Value和独立测试。官方来源与接口记录均保存。

同族真实Gateway发现install_skill在enable_for_agent失败后仍虚报已启用；总控明确允许原owned memorytools最小修复。有效red-install-gateway-02：正常安装/启用字面键对照正确，三个unknown-tag配置均实际安装且虚报已启用，业务断言exit101。现已加入安装前统一配置验证，后续enable失败明确报告“已安装，但启用失败”，不回删安装成果。该修复初检有正反例。

三专属测试仅追加，原字节逐份仍是完整前缀；两个旧跟踪测试全字节未变。没有减少原10样本、77回归名称或断言。新增7测试：adapter四项、真实binary一项（24主样本+peer预览+新session往返）、Gateway两项。

## 中间结果、失败与未验

- initial-four-c-03：exit0，实际20tests（adapter9/service config8/binary3）全部通过，包含四C、R01复合/merge源、R02字面键/tag、补充mixed merge/literal、alias key、非具体!、%TAG、真实Gateway安装和配置PATCH保存后新session Run。
- 最后只按项目edition2021恢复原测试字节/格式，产生精确新SHA；没有在该最终格式SHA重跑。因此initial-four-c-03标STALE_FINAL_BINDING，不冒称精确最终候选已验证。
- initial-four-c-01真实失败暴露F05：同一session配置PATCH后flags=false，但旧冻结prompt仍含记忆。该失败保留，F05继续OPEN/STALE。后续新session Run只能证明F06保存/重新采集正确，不能叫完整同session下一Run生效。
- self-adapter-02的mixed merge/literal独立serde_yaml oracle自身误报duplicate，属于断言来源故障；保留原日志。initial-four-c-02的alias key夹具少空格造成unknown anchor，修正有效输入后继续；不当产品旧红。
- adapter-boundary-03/04进一步暴露本轮非具体!键被源码错拒，按官方Tag(empty handle/suffix !)修正，初检03已覆盖。
- red-install-gateway-01的preference夹具被home helper覆盖，正常对照失败，不能算旧红；修正夹具后02才是有效业务旧红。
- 两次官方源码准备失败（Python CA与初次tar成员路径）单列setup-attempts.json，不算旧红。

没有执行最终固定副本四C/原77回归、fmt、clippy、contracts、boundaries。没有重建旧行扫描基线探针，仅核验R01/P01/R02历史manifest共226份无漂移。没有执行P02 registry、workspace全测、R06正式门禁、R05→R02闭包、其他OS、打包、LIVE、共享删除或R07。

354条原记录和101叶组全部保留，逐条追加P02部分实测/推断/未验状态；原owner/deadline/剩余范围不变。不关闭完整T01/T06、A01/A02/A11/A12或任何未验原叶。F05/F07及R02—R05受影响配置/运行/网关证据须最终复验，F14/F15集成重绑定仍待原owner。此次新库/写配置/安装错误行为变化使先前精确候选结果STALE。

## 交给新ROOT-CAUSE/P03

先按§5.3由全新ROOT-CAUSE读原规格、两轮FAIL、P02真实反例与现候选，形成完整全路径矩阵。P03随后检查本轮改动是否充分/最小、真实语义往返与隐藏来源/tag全部消费者，再必要修复、复制核定review3固定副本并使用自有target完成全部最终四C/新增家族/77/fmt/clippy/contracts/boundaries。最后另建全新Reviewer。本轮产品/测试已停止修改，不自签放行。

所有Cargo实用绝对/Users/study_superior/.cargo/bin/cargo，实际1.98.1，适用构建测试--locked -j8；cargo1.98.1字面路径不存在，已通报总控。锁生成仅目标package update，无既有包升级。每run合成home，guard8192MB/600s与可监督父子归属未关闭。外部模型仅loopback应答替身，当前parser/compiler/gather/HTTP/Run/Gateway/storage/message assembly真实。共享仅preview，真实用户资料/settings无写。失败中断的自建binary夹具已按唯一原日志路径清理记录，其他正常夹具/子进程按原代码清理。

## 精确15 owned SHA

- `rust/Cargo.lock` — `bd7c0082ca055cec4b0b3cdedb78a186a39270d0cef3b83651065bd9b2e3f3c1` （本轮改变）
- `rust/crates/lingxi-adapters/Cargo.toml` — `540ee130e5b01e631ff10fb1555fbf71d127c611647c54d53df4d58da20b81e9` （本轮改变）
- `rust/crates/lingxi-adapters/src/storage/agent_config.rs` — `e1028398d74f37003e3cd6ad0de3b2d3ba8d83676a6801cf861a291219b25208` （本轮改变）
- `rust/crates/lingxi-adapters/src/storage/agent_memory.rs` — `f5d4e33b22404b7d42f4dc583a819deacfa43424eca4fbc2b20515f118f36b49` （保留）
- `rust/crates/lingxi-adapters/src/storage/skill_library.rs` — `a88c5bd73851fedb111cb845c4b55b8eec0ffca122a74aca0d0657ccf16f1dde` （保留）
- `rust/crates/lingxi-service/src/context_compiler.rs` — `c31120dac95715d577b75277a268e9723abf18257c39478951582da64068e54b` （保留）
- `rust/crates/lingxi-service/src/memory.rs` — `13ec97760e51b732c97c2cee90031c0ef4b5362110901777225370760a700f9c` （保留）
- `rust/crates/lingxi-service/src/memorytools.rs` — `ea9c5f66fe101d138d1d8d21321f710e9579e7db1482ee0a603b998e960b54b9` （本轮改变）
- `rust/crates/lingxi-service/src/persona.rs` — `e2c81d4ce35a925da258e30a3419b9f245b164d41d8c72720a316948f2028b73` （保留）
- `rust/crates/lingxi-service/src/skills.rs` — `ad597c498070e16794a7751329f353b2bb5abefaff41d568f4683f2d358680cd` （保留）
- `rust/crates/lingxi-service/tests/r06_t01_context_compiler.rs` — `c8ee1915777a5f9f15d8c87955bfc1ced0e5887920e282d58bf28938e83c82be` （保留）
- `rust/crates/lingxi-adapters/tests/agent_memory_r06t06.rs` — `c13e2d1145f5b9447f918d20721f849bad8915062eb4a75247fd6782913e3275` （保留）
- `rust/crates/lingxi-service/tests/r06_ar_f06_config.rs` — `ae9580489d7734c2c8109255e7ff35035d87c4479b7aedd54166544963e455d6` （本轮改变）
- `rust/crates/lingxi-service/tests/r06_ar_f06_binary.rs` — `865cfffcf52e6836cc693e2c2a2718e24c70d754b91166d2723a08d459506d91` （本轮改变）
- `rust/crates/lingxi-adapters/tests/r06_ar_f06_config_preservation.rs` — `a0122d6e61711cf4c9627e05dba04d4494ebb6d642e25a688aa5362b7757a8af` （本轮改变）

15项摘要：3ec4429a5ebc12e5057c655c7166d6cce0738e653cd4f05a0b9a39e0d97100fe。保护settings仍0f72eb0a2549c95973c2aa8abe54c9aa44491fb9ca2a1c6e437404655d36b3d3。完整差异partial-owned.diff，每命令argv/start/end/exit/输入/平台/候选/锁/实际binary与日志摘要见results及逐attempt JSON。
