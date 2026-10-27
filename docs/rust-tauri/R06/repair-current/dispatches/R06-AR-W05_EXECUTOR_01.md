# R06-AR-W05 / R06-AR-F05：一次性全新执行代理

唯一角色为 F05 上下文缓存修复。不是 F01 受阻审查的重试，也不能代替其独立验收。完成本 Task 后结束，不转任 Reviewer、Repair 或其它 Task。禁止 Git 提交/推送/reset/clean/发布；不能改主仓、其它代理候选或用户已有文件。

唯一产品工作区：`/Users/study_superior/.codex/r06-isolated-checkouts/w05-execution-source-01`，独立正常 clone/own `.git`。基线 `bc2ce51fd005ea75b9949156974e15d2761eb734`，已接受 F08；锁 `d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4`。只在核对实际 HEAD/diff/用户文件/工具链后动手。无 W01/W03/W06/W09/W10/W12 未接受产品覆盖，不能擅自复制它们或共享 target。

自己的证据 `artifacts/rust-tauri/R06/AR/W05-E01`、target `rust/target-w05-e01`、合成独立 home；own CoW node_modules。任务准备回执 W05-PREP-01 不是自检或验收。其它轮平台权限未知，不可借本 Task 更名重做它们的被阻动作。

## 全量阅读与原义务

完整读取 ENTRY-01 的 1219 行规格（F05 全部条款/C01—C04/模板/三层/门禁/公式），`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/` 原九共同资料、原 R06-T01/T06 全 Steps/交付及 A01/A02/A11/A12、所有关联 R00 叶/原断言/期限/owner及前置交接。原目录忽略时用 `rg --no-ignore`；10/08 human prompt 缺失是 SPEC_GAP，不能因此不读实际原09/23任务书。

现有基线与生产入口全部从源码判定：编译器/材料源/Skills port/Run/foreground/background/child；真实人格、记忆、技能安装/审批/启停/来源切换/卸载、配置/preferences、Session 属性/记忆开关/删除/归档/服务生命周期；请求及观测 API、原段注册/预算/常量模板和上游取消/usage。

`artifacts/rust-tauri/R06/AR/ENTRY-01/context-cache-execution-preparation.json` 是总控已读源线索。F01 执行者实际发现合法不同 Agent child 到达真正模型 wire，却复用了其它 Agent persona 的现行 cache；这属于 F05/EX03 的当前缺口。读对应冻结 W01-E01 交接及证据索引，自己独立复现，不继承 PASS、不复制 F01 产品。

原 T01 标准短提示词/同源观测、T06 人格/记忆/Skills/Pi hook 真实边界全部保持；动态/静态逐字段从原现役契约取证。已有观测、raw 归档、工具 pair、权限声明、子代理裁剪不因 cache 修复被弱化。完整 original scope 矩阵保留全原文/当前有限贡献/未验证，不自改到期归属，不把其余 EX03/技能包成功义务归R07或申报关闭。

## 必须完成的缓存与快照合同

1. 明确哪些字段允许按会话冻结，哪些必须下一 Run 生效；人格/记忆/Skills/preferences/Agent与Session设置逐字段列读源和所有实际修改入口。禁止整会话所有材料永久冻结。
2. 缓存身份包含真实主体、Agent、Session、child形态、实际材料与配置代次，或可证明等价的每 Run 快照索引。Session owner、actor、authorized reader 分开；原 LocalUser 广域访问合同不改，Device/Web 限本人。可信编译端口不得仅靠相同 session 字符串复用不同身份材料，不伪造当前HTTP开放任意Agent。
3. 一个真实 Run 使用单一确定的快照；材料并发更新不能改变已经发出的 Run 的 system/segment/tool目录/摘要。后续 Run 读取正确新代次。并发材料读取、版本核验和发布必须避免 A/B 混合状态；错误响亮传播，不 catch 后沿用假新版本。
4. 人格/记忆/技能/config/preferences 的真实服务及可信写入口接通失效；撤销、卸载、来源切换、Session 删除/归档和服务退出接通回收。直接合法材料文件更新按现役读取合同可见，不只新增一个没人调用的 invalidate。
5. cache 有明确有界淘汰/可观察计数；大量 Session/不同Agent/大量版本不造成另一种无界历史缓存。业务不依赖私有 tests hardcoded 特例，不能以缩短任意TTL假证明 next Run 生效。
6. 观测定位实际发送的 run/snapshot/model call；不能临时重新编译当前材料伪装已发请求。重启/淘汰/删除时合法历史观测若要求保留，则从真实持久化快照或原事件恢复；没有可查快照明确 NOT_FOUND，不造旧正文。不把当前 Session 最新一份当每个历史 Run 的观测。
7. 现役工具执行授权仍实时独立复核，旧 Run 冻结目录不让已经禁用/撤销权限的技能执行；新 Run 目录/材料与真实权限正确。
8. 无新抽象/依赖/迁移除非真实必要；优先复用原已有 durable 运行事实。若确需迁移先报告完整路径/实际原 schema；F03 已有待接受 opjournal v10，禁止自己碰撞版本/篡改旧迁移。不得改 registry/阶段图让原红变绿，该工作属于独立 F14。

共享路径变动必须列精确 owned 与消费者/STALE。后续组合须全新 integration/Review；不得宣布 F01/F03/F06 未接受的修复已经并入或通过。F06 parser 当前有平台受阻验收与真实反例，F05 cache 不能美化配置读取错误；只让合法配置的缓存行为成立不等于 F06 完成。

## 两层真实自检

先按上述原契约实跑旧红与正常对照，保留准备错误/旧失败；修真实根因，不能只改单个测试 expected。

- C01：真正二进制+认证HTTP+loopback模型，同 Session Run1→真实persona/memory/技能启停修改→Run2。捕获两次真实请求和按实际 Run 的观测，Run1不可改写，Run2新代次，禁用技能原执行权限拒绝。
- C02：可信宿主端口上的不同实际主体/Agent/Session碰撞，同子代理形态；合法 other-Agent 子代理用真实 parent/Runtime 关系。模型外部替身允许，被测编译器、网关、真实存储及组装不可 mock。跨身份例区分授权材料和私史，不收紧原LocalUser权限。
- C03：创建/使用/归档/删除超过上限的真实 Session。记录 cache数量/淘汰/删除后授权观测/重启历史快照，未测RSS=null，不能报峰值0；需保留的历史从真实持久事实取得，不靠测试echo。
- C04：材料读与发布之间实际并发屏障固定时序，多编译请求/真实更新/不同 Run；每个真正送出的 snapshotId/content/segments摘要能与原观测对账，不能请求A而只查到B。当前Run中途修改与下一Run生效另做正对照。

对抗性自检独立构造额外身份/变更/删除/源失败/重启/界限同族反例，并至少隔离还原旧仅(session,bool)永冻cache根因，使同一有效测试实际红，恢复当前候选实际绿。编译失败、guard触发、零匹配不是行为红。变异副本和自有缓存不能污染正式target；源码/输入/bin前后绑定真实。

所有实际修改入口逐一取证，人物/记忆/技能成功/失败、正常聊天与子代理、原T01/T06全部相关测试与F08实际投影回归按影响重跑。原上下文测试的旧“整个会话永远冻结动态字段”若违反原契约，需要更准确保持静态/本Run冻结、下Run动态生效断言；报告依据，不删除请求/观测同源/段形状/预算/权限/工具数与输入字节断言。

## 门禁/运行/交付

固定 Cargo绝对 `/Users/study_superior/.cargo/bin/cargo`1.98.1 `--locked -j8`；Node24.16/原锁。每 test binary 600秒/8192MiB守卫开启，workspace外层2400秒+有界开销，正式R05链按原19800秒总预算+有界开销，不统一600误杀完整链。宿主监督、真实父子退出/cleanup，不隐藏孤儿。

实际 fmt、workspace all-target clippy -Dwarnings、workspace no-fail-fast test、contracts、boundaries、registry，及适用原正式受影响链；没有亲跑只能 NOT_RUN，不继承他人成绩。已知 registry9vs7、F08旧严格事件列表/quota红仍 FAIL，不吞错/退役/删断言或改矩阵“过门禁”。基线失败不是自动豁免。

每命令记录 argv/cwd/有效home及来源/真实READY/原输入与所有有效非Rust/脚本/锁/bin before-after/实际exit/匹配数/守卫/平台/mock/原wire或DB/清理/时间。Cargo可能重建exe：保留真实不同窗口，必要时直接运行当前已建exe取得稳定窗口，不反填before。原测试自己建temp CLI home时不冒称外层env生效。

只新建合成隔离home；禁止真实用户数据/付费/真实对外操作。凭据/home/privatekeys/target/二进制/完整source archive只本地 LOCAL_ONLY_NEVER_GIT；安全manifest只必要脱敏wire/业务断言/日志/报告/指纹，不能整树交付。

最终逐项根因/全部受影响路径/修复/四C/原Steps-A-叶/新增真实缺口/两层自检/历史失败/STALE，冻结 owned/source锁/bin/hash 与safe manifest/HANDOFF。状态只能 READY_FOR_INDEPENDENT_REVIEW，未完成必需项照实保留。随后结束；总控另建全新独立Reviewer实际验收，不复用你或被平台中止的F01 Reviewer。任何实际平台中止必须记录原文、不改写重试绕过，只停对应链。
