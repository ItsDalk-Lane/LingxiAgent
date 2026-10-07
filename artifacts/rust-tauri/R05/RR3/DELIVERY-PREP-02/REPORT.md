# DELIVERY-PREP-02：既有 768 条快照的逐项审定

结论：**768 条已逐项作出决定：257 条作为独立原始证据拟纳入，511 条仅在本地保留；没有笼统待定项。** 另纠正旧清单中同一实验工作副本的 541 条配套文件及 17 条临时票据响应，共 558 条。只新增本目录，未修改原件、历史清单、生产或现行文档；未暂存、提交、推送、删除、运行测试/构建或执行历史 driver。

这是 PREP01 旧清点的审定和合成，**不是最终冻结或完整安全扫描**。J02 及后续 G/E/FINAL 有自己的新增和变动，本轮没有把它们套入旧快照。最终交付仍须按 [最小刷新规则](REFRESH_RULES.md)重新实际枚举、分类与核对。

## 逐路径结果与依据

[decisions.json](decisions.json) 对每一条记录决定、规则、用途、源头/引用、实际字节/SHA、PREP01 大小和 mtime 比较、历史摘要与基线摘要比较。规则和 driver 依据完整列在 [classification-rules.json](classification-rules.json)，不是因后缀或 manifest 引用就接收或排除。

| 独立用途组 | 待定项数 | 决定 |
|---|---:|---|
| A-02 单独正常脚本输入；读取它们，故障另写 extra/repo 或 old-validator-snapshot | 3 | 纳入原始证据 |
| A-REVIEW-01 全量 snapshot，直接运行并变异 helper/candidate | 500 | 仅本地 |
| A-REVIEW-02 小型 snapshot，直接换旧函数、运行红、还原运行绿 | 3 | 仅本地 |
| B 实施/审查及 G 默认负测的独立 pristine | 44 | 纳入原始证据 |
| I/J 恢复回归的 pristine，含合法候选脏字节 | 77 | 纳入原始证据 |
| I 的 runner-pristine，独立于受测 runner-copy | 40 | 纳入原始证据 |
| snapshot-cmp-failure 已复制出的原始基线，cmp 故障未改变文件内容 | 7 | 纳入原始证据 |
| I 独立旧红工作副本 old-independent-copy | 8 | 仅本地 |
| I 独立旧红/新恢复共用 old-independent-pristine | 8 | 纳入原始证据 |
| mkdir 负控恢复腿实际创建的独立输入 | 1 | 纳入原始证据 |
| F46 修复前原文和 C 审查正常 frozen 原文 | 2 | 纳入原始证据 |
| E 只读契约审查的定向源输入 | 24 | 纳入原始证据 |
| R02 取证逐文件捕获的源/要求/配置/构建命令输入 | 44 | 纳入原始证据 |
| R02 取证保存的 HEAD/旧提交源码 | 7 | 纳入原始证据 |
| **合计** | **768** | **257 纳入 / 511 本地** |

“纳入原始证据”表示在证据目录中按文本阅读，不把它当生产入口、测试入口或当前候选。没有修改原文件的可执行权限，也没有运行这些留样。

恢复基线不是故障工作副本。negative_gate 从 COPY 向 EV/pristine 保存原始输入，故障写 COPY，reset 读 pristine；恢复回归的合法脏注释先于 snapshot，故不应用 HEAD 覆盖。snapshot-cmp-failure 的 cp 成功而替身 cmp 返回 2，当前原文与同轮 pristine 相等；保留该原文及失败日志不等于把失败记为成功。

E 的 24 项逐个与 source-read-index 和当时 input-before 摘要相同。保存复制动作的独立命令回执未找到，报告不虚称存在；定向目录结构、读取索引、完整 manifest 和审查驱动的实际 ROOT 读取/另一文档副本变异位置共同支持只读原文用途。TRIAGE external 下 20 项是已有负测副本的定向源码/权威配置摘录，由 collect.capture 保存读取前、捕获、读取后摘要；它们不是用户个人配置，也不是继续执行的整份副本。本轮只读仓库内摘存，没有访问原外部位置。

## 对旧纳入清单的必要补正

[supplemental-corrections.json](supplemental-corrections.json) 精确列出 558 条，原件不动：

- A-REVIEW-01/snapshot 中 536 条先前因扩展名不同而进入 INCLUDE_EVIDENCE 的复制文档/配套文件，与 500 条源码属于同一故障工作副本，统一 localOnly。它们是工作副本中的文档副本；原独立 REVIEW、正式报告、真实测试日志、原测量序列和历史 FAIL 仍留在旧清单的证据范围，不抹去失败。
- A-REVIEW-02/snapshot 的 1 条 .gitignore 和 I-REVIEW-01/old-independent-copy 的 4 条配置/脚本/表格同理。
- 17 条 p1-ticket.body 是真实运行时在合成测试环境签发的临时票据响应。生产 A13 脚本明确以临时 home 创建服务、签发票据；其中 4 条是取证保存的原响应复制件。只记录内容摘要、大小及来源，不输出票据原文，不把它说成真实用户密钥。请求、响应头、原测试日志与失败结论仍保留。

没有发现这 768 条属于用户个人配置的复制件，但这仅是上述来源审定，**不代表整个 RR3 的秘密或用户内容扫描已完整通过**。前置其余分类仅继承，未由本轮重审全部内容；未来新增或归属不明内容须按刷新规则另判。

## 合成清单与本地边界

[merged-paths.json](merged-paths.json) 包含 PREP01 同一批 **19,734 条路径**，保留 previousCategory、分类来源和继承时效说明。精确拟纳入列表为 [include-paths.txt](include-paths.txt)，共 **13,828 条 / 572,265,729 bytes**；[local-paths.txt](local-paths.txt) 共 **5,906 条 / 1,218,189,659 bytes**；[unknown-paths.txt](unknown-paths.txt) 为空。52 个嵌套仓库仍只计目录条目，其字节不是内部总大小。这些列表用于审阅，不能直接当最终 stage 命令输入，也未包含本轮自身或后续新文件。

[local-originals.json](local-originals.json) 在 PREP01 的 967 项基础上增加本轮 1,069 条排除原件，共 **2,036 项**。新排除原件均本轮实读 SHA/size、列 driver 与原引用并标记 localOnly=true、remoteOriginalAvailable=false；旧 967 项未重复读取大型 binary/cache，实际 stat 与 PREP01 全部相同，身份明确继承 PREP01，不能把 stat 相等说成本轮重新哈希。原件仍在本地，本轮没有交付或检查远端原件可达。

[references-with-delivery-boundary.json](references-with-delivery-boundary.json) 保留旧 31,282 条引用及历史摘要，并对本轮处置目标追加决定和本地边界。没有修改旧 manifest 来消除引用；原引用中的本地文件不会因保留 SHA 就变成远端原件。该索引继承 PREP01 所选正式材料的引用范围，非任意文本的完备解析。此前未逐项哈希的普通构建缓存和运行状态仍沿用旧分类，未冒称本轮逐字节验证；最后完整交付检查需按实际引用处理，不能把这份清单当最终引用闭合证明。

PREP01 已登记的大项边界继续有效：C-F46-REVIEW-01/isolated/build/liblingxi_service.rlib 为 134,457,480 bytes，普通 GitHub 单文件交付方案不能承载；另有 61 项实际 binary 及本机绝对工具链接。历史原件若被用户明确要求必须远端可取，现方案仍不能满足该部分；本轮没有引入 LFS、外部存储或发布，也没有将“可按 driver 重跑”冒称“能取得同字节历史原件”。

## 本轮核对与停止条件

所有 768 条及 558 条补正原件的大小/mtime 与 PREP01 相同。历史 manifest 与具体基线共 **2,582 条 SHA 比较，0 不同**；每条当前 SHA 都来自本轮实际文件读取。它们证明存档身份，不代表与今天仍由其他作者修改的生产源码相同，更不重新签旧测试结果。基线来源具体到当时 input-manifest、source-read-index、preservedBaseline、捕获前后记录或独立 pristine；无法提供额外复制命令回执的局部限制已明确。

JSON、精确集合、引用与自身摘要核对见 [QA.json](QA.json)；自身文件清单见 [MANIFEST.json](MANIFEST.json)，封口检查见 [finish-receipt.json](finish-receipt.json)。只读 Git 命令的 UTC、退出码及输出 SHA 在 [commands.json](commands.json)，分类器执行实录在 [classification-command.json](classification-command.json)。历史 driver 仅阅读，未执行；本轮无测试、构建、网络、系统改动、Git 写操作或原件删除。

本报告完成后停写。后续整合者按新增实际 J/G/E/FINAL 材料另建最终清点；本轮不更新 R05/R06 状态，不宣称产品放行。
