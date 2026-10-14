# R06 Agent deletion R4 — 坏收据与实物矛盾拒绝

结果：原33回归+追加3项检查共36通过；独立原完整探针未改，2/2通过；service lib+R3/R4测试严格Clippy通过。源码冻结于2026-10-14 15:42 UTC（北京23:42）。不改主树、不提交、不动原断言/门禁/登记/0B状态。

## 最小净增量

- 独立树：`/private/tmp/lingxi-r06-persona-completion`
- 相对R3 939049013ed797cc84bc0d32756f925b86cb3fb14f751cb68653e23159bf1aed：`/private/tmp/r06-agent-delete-r4-incremental.patch`
- R4 SHA256：`e420a319cdeba28eacbef2f01e2cd5ffccfded2d1cb3087d0547e191e49b43f5`
- 2文件前后SHA：`/private/tmp/r06-agent-delete-r4-source-sha.json`
- R3+R4全部7个候选文件最终冻结SHA：`/private/tmp/r06-agent-delete-r4-frozen-source-sha.json`
- R4基线2文件保全：`/private/tmp/r06-agent-delete-r4-base`；对该基线git apply --check通过。
- 只改 `agent_deletions.rs` 统一实物验证和启动整批预验证，`r06_agent_delete_completion.rs` 仅尾部追加3项测试。程序核对原R3测试全文仍为最终文件字节前缀，所有旧断言未改。R3其它文件冻结SHA保持。

## 已确认根因与修复实效

原独立前置是明确的人为收据损坏：实际顶层uchg阻止rename，生产写出aborted且公共A完整、data从未存在；再把收据损坏为committed/logicalDeleted=true。旧恢复仅在prepared检查实物，随后相信坏committed记录，返回completed并停用仍现役A。没有把此人为损坏包装成正常rename成功证据。

新validate_deletion_materials是所有读取/恢复/GET/cleanup/当前删除settle的统一检查。真实rename回调也在永久关闭Dream核心之前检查。发生矛盾或路径为符号链接/普通文件时明确500，不能先stop、改prefs、清data、签completed。实际启动先读回验证全部收据，再逐条恢复，避免后面的坏收据造成恢复只执行业务动作一半。

|收据状态|公共助手目录|本事务data|判定|
|---|---|---|---|
|prepared|只有其中一处存在|与公共目录互斥|无rename或rename后checkpoint前，允许后续按真实位置推导|
|aborted|可有或后来被另一次真实删除|必须无data|允许历史失败记录存在；不重停核心/改引用/清理|
|committed|必须不存在|可以存在，或已清完|前者待清理，后者真实清完但completed checkpoint尚未发布，均正常|
|completed|必须不存在|必须不存在|只认可两处数据实际都无|

所有非缺失路径必须为真实目录；public目录符号链接（含断链）/普通文件、data符号链接/文件都会拒绝。未发布元数据安全清理保持R3行为。没有添加假事务、没有增加配置/依赖，也没有把合法committed data已清完窗口误判为损坏。

## 原反例保全与证据

- 原外部审阅报告/源文件 `/tmp/r06-agent-delete-r3-independent-review.md`、`/tmp/r06-agent-delete-r3-independent-experiments.rs` 保持不变。
- 原完整探针在修复候选临时编译执行后移出测试目录保存为 `/private/tmp/r06-agent-delete-r4-independent-original-probe.rs`。它与原文件SHA逐字一致：`9078a63f82f5c13ad009e114f3a2ae93ab5e14391dfa08f10c0e850c0ee7ab92`。
- `/private/tmp/r06-agent-delete-r4-independent.log`：2/2 PASS。原损坏committed反例cleanup=500、公共A仍存在、staged不存在、A整理status=200/coreAvailable=true。原top-only uchg字节保全（配置/facts/longterm/memory/today）完整断言保持，解除故障后真实五阶段Dream succeeded。
- 独立probe复制原fixture中不用的辅助定义产生3条测试自身unused警告；未改该探针以清警告。正式候选指定lib/正式新增测试的strictClippy单独通过，不把探针警告冒充strict通过。

## 最终回归

`/private/tmp/r06-agent-delete-r4-regression.log`：exit0；原agent lifecycle2 + 删除R3原7/追加R4 3（10）+ Dream17 + Memory7，共36通过。真实HTTP/磁盘/SQLite/生产供应商HTTP替身/实际bootstrap与全服务restart，未mock被验收生命周期或存储。

追加3项：
1. 真顶层uchg生产出aborted后，分别人为损坏为committed和completed。真实GET、cleanup均500；实际恢复核心入口拒绝、真实ServiceState bootstrap也以contradiction原因失败。公共A配置、五份记忆、偏好以及坏收据逐字不动，原运行A status200。恢复原aborted后GET200，A实际5请求Dream成功。
2. 真完成删除后人为在公共A位置注入目录symlink和普通文件：GET/cleanup均500，链接目标用户文件字节不变，B核心200。移除矛盾前置后正常completed cleanup200。
3. 真completed后注入矛盾data：GET/cleanup500、data文件不动。移除该人为前置并设置合法committed/data已清空窗口，整个服务restart正确恢复completed；A不重建，B真实五阶段Dream succeeded。

第三项初次新增fixture未给B写入待整理来源，真实核心按empty-memory拒绝，原succeeded断言失败；补充B真实独立来源后通过，未改断言或生产处理。原33断言保持。

`/private/tmp/r06-agent-delete-r4-clippy.log`：cargo clippy --locked -p lingxi-service --lib --test r06_agent_delete_completion --no-deps -- -D warnings exit0。

cargo固定 `/Users/study_superior/.cargo/bin/cargo`，合成HOME/TMP/LINGXI_HOME，target仓库外，CARGO_INCREMENTAL=0。未运行其它平台、正式打包/真实供应商费用或整阶段门禁。本轮是明确坏收据前置及路径同族检查，不宣称恶意外部writer持续竞态全部已验证。

Settings/UI作者已同步：正常API字段不变；500保留错误/旧pending，不视作completed。最终冻结SHA供其复制独立UI树必要后台前置，UI补丁不重复后台。

0B483D7F6A43自动ticker全叶仍OPEN/PARTIAL，此R4不实现或裁决该原义务。
