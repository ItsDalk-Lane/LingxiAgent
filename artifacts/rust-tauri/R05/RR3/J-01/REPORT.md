# RR3 J-01 / F50 默认隔离准备修复

状态：**SELF_CHECKED，等待全新独立验收；R06_READY=false。** 未执行默认16、完整R02、完整主依赖复制或阶段终审；不把本报告当作这些检查的通过回执。

## 根因与改动

默认入口原先只做本地clone和rust/scripts/docs覆盖。node_modules被忽略、没有被clone带入，也没有准备步骤。读取G的四轮真实R02结果，每轮均为20命令、13PASS/7FAIL；其中五项缺Node前置：认证CLI/CLI_RUST/CLI_SESSIONS实际import ws失败，CLIENT找不到copy/node_modules/.bin/vitest，legacy复制node_modules失败。这五项不是ALF，也没有因此证明产品业务行为错误。A01/A13仍由H处理，旧G入口并行改写导致exit2/15行的失败不被本修复冲销。

本包只改默认负测入口，新增两个必要scripts辅助：r05_t08_prepare_node.py及r05_t08_node_selfcheck.py。总控已授权这两个辅助及共享Git对象的准备邻接。没有改Rust、R00/R02叶、package/lock/.gitignore、currentdocs、总控Git或系统设置。

准备发生在COPY状态记录、pristine快照、第一条control和verify-stage之前：

1. 校验来源与副本是真实独立Git根、HEAD相同、副本对象可达；读取**当前工作树**package.json/package-lock.json/.npmrc，并复制到副本，保留合法未提交根输入。可选.npmrc在来源删除时，副本对应删除旧文件。
2. 校验package与lock根声明；核对隐藏lock与权威lock的version/resolved/integrity/dependencies/optionalDependencies/bin，逐包检查实际package.json版本及可执行入口存在。非optional缺失拒绝；合法缺失optional逐项记录。实际Node满足候选engines，npm下限11.10源自现行.npmrc的min-release-age要求。记录实际Node/npm版本、路径和可执行文件摘要。
3. 对来源依赖所有普通文件记录SHA256/大小/模式，对内部相对链接记录目标；外逃/绝对链接和特殊文件拒绝。使用独立写时复制，检查无共享文件inode，复制后逐项与来源清单完全相同，再次核对来源没有变化。macOS使用clonefile；Linux使用FICLONE；不支持时仅在足够空间（总量加1GiB余量）下真正独立复制，否则明确失败。没有硬链接、没有指向用户依赖的可写目录链接、没有安装或更改声明。
4. 在副本中实际import ws并启动本机vitest；准备异常、不完整必要入口、版本/lock不符均非零停止。最终汇总前重新比对来源/副本依赖、根输入、工具摘要和Git可达性，漂移不准标全绿。仅依赖根下.cache/.vite/.vite-temp三个可重建输出目录不复制/不参与包内容摘要，副本自行写入；未排除任何候选artifacts/untracked，也未更改原来源绑定器。

磁盘前置邻接：默认clone的`--no-hardlinks`改为Git标准`--shared`。只读复用历史对象，工作树/index/HEAD仍独立，原overlay与来源绑定规则不变；避免约3.5GiB重复对象峰值。来源对象失效必须由准备或终末的真实git fsck拒绝，不静默复制失败继续。这个临时副本依赖来源对象存活，不能当永久独立归档；本轮不得清理来源对象。

## 内容可信边界

复用的是用户当前已安装依赖，**不是重新下载且重新认证的registry tarball**。隐藏lock根版本虽然是0.1.40-experimental.1，现候选是0.1.43，但不能仅因此拒绝：本轮实际1277个已安装包逐条匹配，另有4条未使用且不存在的旧packages/* extraneous元数据，98条缺失optional记录。旧extraneous只有无workspaces声明、对应路径不存在且两份lock记录一致才接受，并明确列出。

本轮读取、摘要并再次确认主依赖64760个文件/链接、1379047525字节普通文件内容和根输入未变。摘要证明所复制/所读取的本地内容一致，不证明上游供应链真实性；本地既有postinstall补丁保留。真实加载器、正式CLI及原测试证明本次必要路径可读，未冒称每个任意包的每个不可达文件都重新通过安装验真。

## 亲跑证据

主候选：codex/rust-tauri-migration，HEAD=b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b。实际Node v24.16.0，npm11.13.0，Darwin arm64。没有运行Cargo、共享NEG_TARGET或主Rust target。

核心命令：

```text
python3 -B scripts/rust-tauri/r05_t08_node_selfcheck.py --evidence artifacts/rust-tauri/R05/RR3/J-01/selfcheck-04
python3 -B scripts/rust-tauri/r05_t08_restore_selfcheck.py --evidence artifacts/rust-tauri/R05/RR3/J-01/i-restore-final
python3 -B scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py
bash -n scripts/rust-tauri/r05_t08_negative_gate.sh
```

| 真实检查 | 结果 |
|---|---|
| 小Git原默认clone，实际CLI help | exit1，明确Cannot find package 'ws' |
| 同旧copy实际vitest启动 | exit1，MODULE_NOT_FOUND |
| 新生产准备调用（直接提取默认脚本原调用） | exit0；92个当前真实包及传递闭包，2892文件/10内部链接、57905854字节；全部CoW，无普通复制 |
| 真实CLI help / 原CLIENT生产函数三条CLI叶 | help exit0；help/unknown/serve-unknown三条原checks全部成立，无用户home写入/服务启动 |
| 原tests/cli-args.test.ts，真正vitest run | 1文件、8 passed、0 failed；恢复后再1文件8 passed |
| 合法dirty package输入 | copy字节与当前来源完全一致，未盲用HEAD |
| 缺包清单/缺ws入口/错package-lock/错installed version/错hidden-lock | 全部目标拒绝，正常来源恢复后绿 |
| package及lock同时声明Node>=99 | 实际Node24在引擎检查处拒绝，非package/lock提前不一致 |
| 外逃依赖链接 | 拒绝 |
| 真实只读父目录复制失败→恢复权限 | 生产clone_dependencies实际PermissionError→实际复制正常；未替换复制函数 |
| source共享对象不可用→还原alternates | 生产准备内git fsck exit10、外层exit1→还原后准备exit0 |
| 共享clone来源Git | 主夹具.git所有文件/链接前后不变 |
| copy写自己的.vite缓存 | 来源未改、终末依赖核验通过 |
| copy改ws包内容→还原 | 来源字节未改；核验目标红→还原绿 |
| selfcheck-04 | exit0，42条实际命令、8条额外断言；不是42个业务场景，也不是默认16 |
| 最终dirty .npmrc删除补控 | 当前helper准备/核验均exit0；副本旧.npmrc确实删除；记录见commands-focused-final.json |
| I的原永久还原自检（不带production-sync） | exit0，35检查、0失败；不冒称重跑真实Rust同步 |
| B原永久自检 | exit0，15控制通过 |
| shell/Python语法 | 通过 |

准备与核验的每条argv/cwd/UTC/exit、工具路径/二进制摘要、lock/input/依赖清单和日志SHA存于各准备result.json、dependency-files.json及command-N日志；外层命令记录在selfcheck-04/commands.json、commands-final.json、commands-focused-final.json。小夹具只替代完整仓库/依赖集合，不替换Node、Git、ws、vitest、生产准备函数、原CLI入口或原CLIENT三条CLI检查；故障注入均在自有临时夹具，主树无注入。

原I的snapshot/reset、N06同步三个函数、B的run_n03/write_results等7段逐字节不变，16个record_case身份各一次、12个恢复注册文件完整；详见preserved-contracts.json。最终仍先完整reset再write_results；新依赖核验在最终reset之前，依赖内容不被reset修改。没有改任何负测目标、预期计数、过滤词或R02断言。

## 保留的失败、读取范围和后续

selfcheck-01（33命令）与selfcheck-02（39命令）均保留，是共享对象邻接前的阶段自检；selfcheck-03（42命令）是共享对象首轮通过记录。selfcheck-04绑定最终入口、准备器及永久自检等7个真实输入摘要，结束时再次确认相同。主元数据初读曾拒绝4条陈旧extraneous，后按真实当前lock结构细分并记录，没有改权威lock。首次I自检exit1是其精确终末顺序断言检测到新调用插入reset与write_results之间；已调整核验位置保留原顺序，原失败stderr保存，最终35检查通过。diff命令exit1仅表示存在差异，不是测试失败。

全文阅读RR1/RR2 MASTER、RR3 BRIEF/REVIEW_BRIEF/J_BRIEF、最新问题矩阵/HANDOFF、I-REVIEW-01报告；读取G-INTERRUPTION的真实索引及其四轮F50原命令/叶日志，40条命令日志和260条叶日志/JSON/文本摘要列在history-logs-read.json/history-leaves-read.json。扫描认证/client/CLI/legacy五条默认消费路径，保持其原业务断言、raw npm红、directed/E5及所有历史FAIL/中断结论。

selfcheck-01自有可重建夹具因空间清理，原日志/输入摘要/结果保留，回执own-fixture-cleanup.json；第二至四轮夹具保留。没有清理用户内容或其他任务缓存。

剩余必做：全新J独立审查应在当前真实完整依赖上亲跑默认准备和必要正负控；随后所有entry/scripts/docs读取输入静默冻结，新G02亲跑完整默认N01–N16和受影响R02正测，再按原流程进行新终审。这是待完成验证，不是豁免。Linux/Windows复制分支、完整主依赖复制、完整默认16、完整R02业务/legacy、LIVE/其他平台本包未运行。

交付后停止写入；不提交、不推送、不发布，不自行关闭F50或宣称R06_READY。
