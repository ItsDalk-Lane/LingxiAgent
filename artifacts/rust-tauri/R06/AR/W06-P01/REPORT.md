# R06-AR-W06 / F06 修复 P01

状态：**READY_FOR_INDEPENDENT_REVIEW**。一次性修复代理 `/root/ar_w06_repair_01`，未参与 E01 或 R01。没有自签独立 PASS；新的 R02 Reviewer 必须亲验第三层。

R01 HIGH 根因已在本轮自检关闭：根与敏感分区的非字符串 YAML 键不会再被当作缺字段恢复默认。本候选选择 R01 明确允许的“对不支持类型响亮拒绝”，没有声称将现役 js-yaml 复合键归一化为字符串。普通字符串键、block/flow、引号/BOM/CRLF、alias/merge 和原默认保持；未知字段内部数据保留。拒绝响应含 config.yaml 与 unsupported YAML key type，运行失败且零 provider 请求。

## 改动与根因全族

相对 E01，仅改统一 `agent_config.rs` 和三个专属测试；其余11 owned内容未变，没有加依赖、改Cargo、改消费者特例、写lib/shared/knowledge/session_tree、rootmeta或用户设置。

统一入口在读取默认值/更新之前，检查root和agent/memory/experience/skills/workspace_context的全部直接键；merge展开前递归检查所有来源（包括被显式覆盖的来源），展开后复核。字符串重复由真正YAML parser拒绝；字符串与复合键的规范化冲突在不支持键类型边界拒绝，无法借默认或set掩盖。

全族路径见 family-matrix.json：context_compiler主材料/roster；memory语言/flags/experience/update；memorytools执行时复核及skill启用；persona locale/yuan；skills读取/策略/名单/启停/删除引用/孤儿预览/runtime目录；agent_memory统一读写和原子patch；skill_library其它Agent引用。产品都继续复用同一入口，错误传播保留。

## 旧红到新绿及三层

1. 第一层逐项自检：最终固定副本四C 13/13，原影响回归77/77；四C逐条结果在 results.json。所有原测试名称/断言/数量保留，新增有意义测试3项：adapter全族拒绝与patch字节保护、未知内部值/merge保存保真、真正binary全族wire与shared预览。
2. 第二层对抗性自检：未修产品先亲跑 red-family-03，实际3测试、1 passed/2 failed；普通字符串配置真实binary对照正常，17主配置复合键/merge反例仍默认或完成、peer复合enabled预览错误列共享孤儿。采用同族断言修产品后全绿；最终固定副本19个业务样本全部符合期待，22个adapter键类型/alias/merge/复合重复样本拒绝，真实patch不改原字节。真实Gateway工具保留普通paused检查并新增3复合/merge拒绝，tenet不落盘。
3. 第三层独立审查：未执行，交新R02。13或77只是执行数量，不代替原语义完整验收。

所有失败尝试保留：red-family-01编译失败，不能算业务旧红；red-family-02测试正对照错误读了flags层级，修测试后采用03取得有效正对照与业务红。green-family-01随后增加原文件patch保全/READY断言，摘要STALE，仅辅助；最终采用固定副本 final-*。旧flow/引号基线红的R01/E01证据已核对，未冒称本轮重建完整旧binary。

## 固定候选与本人亲跑

HEAD/base 2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d。总控按15准确SHA复制绑定W06-CANDIDATE-02；本人最终只读 `/Users/study_superior/.codex/worktrees/r06-w06-review2/LingxiAgent`，使用该副本独立rust/target，不含W02/W12产品。开始/结束15SHA与主仓冻结一致，见 binding-before/after、owned-files.json。

绝对Cargo1.98.1/rustc1.98.1；编译与xtask均--locked -j8；fmt原生格式检查无--locked/-j参数，实际命令保留。guard8192MB/600s未关闭，有受监督父进程。每run新合成LINGXI_HOME；binary READY逐样本验证实际canonical home且source=cli。外部模型端点全是loopback替身，生产parser/gather/compiler/HTTP/Run/Gateway/数据库/组装均真实。

- final-four-c-01：exit0，13 passed。原10binary样本+新增18主样本及peer预览；gather/真实memorytool/保存字段。
- final-regression-01：exit0，77 passed。T01 service/kernel/closed_loop；T06 memory/skills/skill_context/hooks；adapter agent_memory。
- final-clippy-01：workspace/all-targets/-Dwarnings exit0。
- final-fmt-01：all --check exit0。
- final-contracts-01、final-boundaries-01：各exit0。

命令/start/end/exit/test_count/passed/failed/锁摘要/fixture摘要/15SHA/logSHA/binarySHA都在每run JSON及 results.json；非测试命令的test_count0不当作零匹配测试通过。实际test可执行文件摘要见 executable-binding.json。实际service binary SHA256：406279b9ecabfc8d1ce3678c322ae6900064f00dd2f156d893be77d5ba3d47c1。

## 原条款、未验与STALE

完整保留354原记录（10 Steps、6交付、4 A、263 R00断言、31相关修复Step、28 C、12旧review义务）及101叶组。original-obligation-review.json逐项追加本轮配置份额、证据、原owner/deadline及未验状态，历史R01评价单独保留；不拿本项关闭F05缓存、F07完整工具目录、F08或EX。

T01五Step：组成、artifact同源、静态前缀、中英文/子代理、冻结模板相关回归亲跑，配置份额已修；完整F01/F05/F07/EX份额仍OPEN。T06五Step：事实/compiled/经验/修订、隔离、技能审核/名单/策略、四hook、Gateway越界拒绝相关回归亲跑；完整记忆调度/生命周期、所有技能流程、worker/Pi与父子授权仍保留原到期义务。六交付物保留，不接受完整原Task。A01 wire/观测、A02 golden/segment/超限、A11隔离/工具绑定、A12 token/TTL/内容变更和越界/symlink两层仅接受配置影响自检份额，不扩大为全部工具wire预算或完整业务生命周期PASS。

未跑完整workspace tests、正式R06门禁、R05→R02阶段级闭包、其它OS、正式打包、真实供应商/LIVE。F14/F15须按最终组合候选重绑定与回归；本项独立77不替代阶段闭包。E01/R01旧候选及中间green-family测试摘要对本新候选STALE；后续产品/测试/配置修改使受影响结果重新STALE。

## 数据与边界

仅合成临时资料/loopback。真实cleanup只调用preview；普通共享引用预览空，复合peer明确400；预览后共享SKILL.md存在，未调用删除业务。每binary子进程SIGTERM成功wait、stub stop/join、仅自建临时fixture清理；wrapper新空home清理。历史W06-R01证据manifest逐条核验无变化。用户.vscode/settings.json仍是初始0f72…b3d3。无Git写、提交、推送、发布、真实资料、付费、外发或R07。

## 精确15 owned摘要

| 文件 | SHA256 |
|---|---|
| rust/Cargo.lock | 9e91ff754118fa274c95363e3a9f3d0c802667b179c2576bd10e45fc146b6506 |
| rust/crates/lingxi-adapters/Cargo.toml | 9b9704e81f2df10816614b00bee59b498cada62f59348512e888851a8ac2769e |
| rust/crates/lingxi-adapters/src/storage/agent_config.rs | 0501492fad5bc7f1f7022754cec7885f44bff5d7a4f3b16d0ca1f7d71ff9e78c |
| rust/crates/lingxi-adapters/src/storage/agent_memory.rs | f5d4e33b22404b7d42f4dc583a819deacfa43424eca4fbc2b20515f118f36b49 |
| rust/crates/lingxi-adapters/src/storage/skill_library.rs | a88c5bd73851fedb111cb845c4b55b8eec0ffca122a74aca0d0657ccf16f1dde |
| rust/crates/lingxi-service/src/context_compiler.rs | c31120dac95715d577b75277a268e9723abf18257c39478951582da64068e54b |
| rust/crates/lingxi-service/src/memory.rs | 13ec97760e51b732c97c2cee90031c0ef4b5362110901777225370760a700f9c |
| rust/crates/lingxi-service/src/memorytools.rs | 94fe1aafb37794bd3493eaab20b58b5b42438ff40f32cfab3a5d7fb69cd5463d |
| rust/crates/lingxi-service/src/persona.rs | e2c81d4ce35a925da258e30a3419b9f245b164d41d8c72720a316948f2028b73 |
| rust/crates/lingxi-service/src/skills.rs | ad597c498070e16794a7751329f353b2bb5abefaff41d568f4683f2d358680cd |
| rust/crates/lingxi-service/tests/r06_t01_context_compiler.rs | c8ee1915777a5f9f15d8c87955bfc1ced0e5887920e282d58bf28938e83c82be |
| rust/crates/lingxi-adapters/tests/agent_memory_r06t06.rs | c13e2d1145f5b9447f918d20721f849bad8915062eb4a75247fd6782913e3275 |
| rust/crates/lingxi-service/tests/r06_ar_f06_config.rs | b80b1b0b8b4d78f81d68df3797718f0725268cfcc1de16b7719c92409530d276 |
| rust/crates/lingxi-service/tests/r06_ar_f06_binary.rs | 2273e67b7bc3860a8a039d72cc3eb3e5a27962bee810b048e3ab1f5c9ce0426b |
| rust/crates/lingxi-adapters/tests/r06_ar_f06_config_preservation.rs | b577ded0d7875cd424617969512476c2402b4e11bfcab24fb457b601b10f74d0 |
