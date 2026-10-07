# RR3 A-REVIEW-02：F42 第二轮全新独立验收

结论：**PASS（A1/A2 同包）**。首轮唯一 mustFix 已关闭，两处生产校验均明确拒绝无法可靠完成的 Git 查询；合法新鲜输出仍通过，已登记内容仍拒绝。没有发现本轮新的 mustFix。该结论只关闭 A/F42 包级问题，不是完整 R05 阶段放行，不签 R06_READY。

审查者：`rr3_a_review_02`，未参与 A 实现、修复或首审。实际入口为已安装 Codex CLI `exec --ephemeral` 空历史，thread_id=`01a113dc-ea73-75b0-abd0-3745ae142665`。由现存 `dispatch/request.json` 与事件首行核实，未虚称 collaboration spawn 成功；未派代理或外发消息。dispatch 原样保留、未覆盖。

全文读取 RR3_A_R2_BRIEF、RR3_REVIEW_BRIEF、RR3_BRIEF、RR1/RR2 MASTER、RR3 ISSUE_MATRIX/PROGRESS/HANDOFF、A-01/A-02 REPORT、A-REVIEW-01 完整 REVIEW；亲读首审 validator-fault-results 和六份原始正常/故障/恢复日志。首轮 FAIL 与所有历史失败保留。

## 输入与权限边界

HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支=`codex/rust-tauri-migration`。`input-manifest.json` 与 `delivery-inputs.json` 逐项绑定19个 A 输入，收尾19/19相等；明确 C 和 E 并行，**不宣称总工作树冻结**。19项包含锁、工具链、忽略规则、xtask源码/测试/四层图及两脚本/共享发现器；没有用整个工作树摘要替代它们。

本轮只写本目录证据、报告、隔离夹具。没有修改主树源码、其他包、总控文档/台账、Git状态或系统；隔离夹具仅 init/add 建立真实索引，没有 commit。注入全部在本目录隔离副本或回归自己建立的临时夹具，主树没有注入后还原。未复制 target，未删除历史或用户内容，新增证据约9 MiB。

工具：绝对 `/Users/study_superior/.cargo/bin/cargo`、rustc，均1.98.1；Node v24.16.0、npm11.13.0、Python3.14.3、macOS27.0.1 arm64。`binary-manifest.json` 保存实际工具路径、解析路径、字节数/SHA及真实锁定cargo/rustc文件；版本日志独立保存。没有新Rust构建/测试，复用的是原新独立审查明确 `--locked` 的有效运行。

## 亲跑结果与目标红证

命令 argv/cwd/UTC 起止/exit/log SHA 见 `commands.json`，永久回归每条查询另有其 `validator-query/results.json`。`verified-counts.json` 从本轮原始记录重算，没有复制作者统计。顶层准备失败不计产品红证。以下全部受影响 shell/发现器/E0s 均为本审查者亲跑。

| 验证 | 实际结果 | 本轮原证 |
| --- | --- | --- |
| 首审旧两生产校验 + 当前正确永久断言 | exit1；实际24条，正常/恢复8条通过，16故障全部未满足明确拒绝。不是编译错误或提前拒绝，真实合法目录和Git索引已抵达两个生产函数 | `validators-old-red.log`、`validators-old-red/validator-query/results.json`、`validator-mutation.json` |
| 精确还原当前字节后的生产校验 | exit0，24/24，ignored0/filtered0；两函数×tracked/fresh×正常/四故障/恢复。四故障为exit2+stderr、exit2空输出、exit0+stderr、exit2+误导stdout，16条全部具名 `git ls-files query failed` 拒绝 | `validators-restored-green/`、其日志；快照已与当前主树脚本逐字节一致 |
| 额外独立校验 | 29/29：20条正常/信号终止/成功码二进制stderr/无法执行/恢复，外加9条非法根与链接拒绝。合法根通过、tracked拒绝；真实Git索引，只有故障查询使用隔离PATH | `independent-validator-results.json`、对应29份命令日志 |
| 当前真实OS fd永久父stdout+child/stdout+旧untracked JSON，及三对照 | 两次完整运行各exit0、4/4，ignored0/filtered0；仅子sink、旧tracked、无旧文件对照均通过；7非法根、tracked sink、lsof故障拒绝 | `discovery-green/`、`discovery-restored-green/`、各日志 |
| A2旧发现器目标反证 | exit1；两校验24/24先通过，然后旧父DIR吞child旧JSON，改旧JSON后 before/after 字节仍相同，精确失败于“旧证据变更…被发现器吞掉” | `discovery-old-red.log`、`discovery-old-red/old-untracked-parent-child/{sinks.txt,before.tsv,after.tsv}` |
| 独立父/子/孙、多sink、内外根、合法脏树 | 6组全部exit0，每组6个真实stdout/stderr fd；所有层增长绑定稳定。内/外×fresh/旧untracked孙/旧tracked孙；有旧孙时三个祖先都禁止DIR吸收，自己的新鲜evidence子DIR可独立获证 | `independent-03/results.json`、每组`sinks.txt`和baseline/growth清单 |
| 四类输入增改删改名、旧证据及source/copy对称 | **100次**变异均检出，100次源/副本绑定字节相等，两侧恢复均回到基线；源码、脚本、配置、stage map各自content/add/delete/rename，旧JSON另外4次 | `independent-03/` 每次独立TSV、每组results；不覆盖成单一最后结果 |
| 独立发现器异常 | ps/lsof/git各静默exit7、误导stdout+exit9，共6条，均非零拒绝；Git故障有真实仓库内sink确保抵达归属查询 | `independent-03/discovery-faults.json`及各stderr |
| 生产E0s完整片段 | 原shell exit0；**61条expected/actual判定+1缺块拒绝=原62项**，另1合法全绿对照。默认真实fd4组、24查询及根/FILE/binder全部通过 | `e0s.log`、`e0s/e0s-self-checks.log`、`e0s/f42-real-fd/`、`e0s-counts.json` |
| 原classifier/binder/cmp与语法 | classifier和binder与首审输入逐字节相同，所有cmp行相同；bash -n、两Python源码语法检查exit0 | `e0s-preservation.json`、`shell-syntax.log`、`python-syntax.log` |

旧两校验只在 `snapshot/` 替换为首审保存的原函数，两处均有旧/新函数SHA；永久断言保持当前字节。保存目标红后还原原字节，再运行绿。旧发现器单独指定历史脚本，仅用于隔离目标反证；恢复真实当前发现器再跑4组绿。所有有效排除集合来自真实OS fd发现器，没有手传理想FILE、扩大到整个artifacts、忽略全部untracked、删cmp或重拍快照接受漂移。

E0s脚本提取当前生产四函数、完整classifier和整个E0s区段，只补目录、临时目录清理与note/fail打印。它不运行E1–E5/npm，也不冒充完整正式legacy gate。完整自检已执行成功，统计器修正后只重计保存日志，没有为挑绿重跑生产自检。

## A1 未变输入与新独立证据复用

按本轮授权，直接把首审隔离快照和当前主树逐字节比较，而非只信 A-02 声明：指定17个A1输入全部相同，包含共享发现器及全部xtask源码/测试/阶段图/Cargo锁和工具链；见 `a1-reuse-input-equality.json`。两处本轮改动脚本明确不在复用集合，受影响shell行为已全部新跑。

亲读原始8层JSON，重新统计内外各7/8/15/20、各50 checkpoint：每条stable=true、changedPathBytesHex空、error空、全部commands/overall PASS；亲读xtask原始121 passed/0 failed/0 ignored/0 filtered日志和实际命令。见 `a1-reused-checkpoints.json`。此外重核首审A1旧行为50不稳/overall FAIL与字节还原50稳/PASS；最深20个commands本身均PASS但overall FAIL，不能以命令绿遮盖绑定失败。

来源身份原证为同一个已编译测试runner正常0→只改helper字节101、点名 `run_output_sinks.py`→不换runner还原0。亲读三份原日志、mutation、binary身份和对应实际命令；没有声称本轮新运行或新构建。27个被复用原证的SHA还与首审自有manifest相等，8层记录另外与原checkpoint index相等；见 `a1-reference-manifest-audit.json`、`a1-reused-red-restored.json`、`binary-manifest.json`。

`main.rs` 的 `if !stable` 独立强制 overall FAIL 原分支仍在，逐字节相同；本轮对此为源码审计，不声称执行了正式main入口。RX四层记录用真实Scope/verify/run_command及真实进程，但RX映射和有界printf负载只证明编排/绑定，**不是正式R05/R04/R03/R02业务gate**，其runner身份另外引用真实旧runner拒绝证据。

## 准备失败、限制与交付

准备失败全部保留，见 `preparation-failures.json`：第一次独立夹具把副本放进源树导致递归复制；第二版断言误禁止了可独立获证的新鲜evidence子DIR；第三版全部有效。E0s生产shell第一次已exit0，其后统计器误把含合法绿对照的63行全当62条expected判定而退出1；两次修正脚本定位失败产生KeyError，最终正确独立重计62+绿对照。它们均不是产品红证，没有藏掉或计PASS。首个准备命令顶层UTC未包装，原始子case UTC已保存，明确不补造顶层时间；所有有效验收命令都有真实UTC。

本轮有效记录639条命令，含快照、夹具准备与检查调用，**不是639个独立测试**；完整日志摘要校验通过，准备失败另列。`manifest.json` 绑定本目录交付文件，排除自身及持续由派发者写入的dispatch目录；不覆盖dispatch。主树19项最终相等，历史首审FAIL及其红证未改。

未执行完整workspace、正式嵌套stage gate、N01–N16、Linux/Windows或LIVE；这些仍由总控安排冻结后全新FINAL亲跑。本轮没有将其他包/环境FAIL改为PASS，也不自签阶段R06_READY。A1/A2同包独立PASS，无mustFix，可由root更新A/F42包级闭合；阶段NOT_ACCEPTED/R06_READY=false边界保留。
