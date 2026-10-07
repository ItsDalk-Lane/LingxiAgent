# RR3 J-02：完整低占用默认准备与历史基线构造

状态：**SELF_CHECKED，等待另一位全新独立审查；R06_READY=false。** 本人只实施及自检，未关闭 F50，未作独立验收，未运行默认 N01–N16、完整 R02/R05、完整 npm/E5 或 Rust 构建。

## 结果与范围

此前两个真实 ENOSPC 均保留：J-REVIEW-01 的默认 HEAD clone 尚未到 Node 助手就失败；同轮 legacy 的历史 BASE checkout 也失败。本轮把完整的低占用准备接入这两个真实入口，最终均取得成功记录，没有稀疏检出、删除历史 artifacts、借当前工作树冒充历史版本或放宽原检查。

只修改四个授权 scripts 文件：新增 `prepare_git_copy.py`、`prepare_git_copy_selfcheck.py`；改 `r05_t08_negative_gate.sh` 的默认 Git 准备片段；改 `r02_t08_legacy_entry_regression.sh` 的 BASE 构造及对应说明。原 Node 助手 **逐字节未变**。没有改 Rust、package/lock、ignore、原任务叶、现行文档、主 Git 或系统设置；没有提交、推送、发布或派代理。

全部新证据在本 J-02 目录。`delivery-source.json` 给出最终四文件 SHA；两份 `*.j02.diff` 是相对于交接时已有 A/I/J 修改的本轮精准差异，不把先前作者改动算作本人改动。

## 准备方式与边界

新助手先从真实 Git 解析指定提交及全部 `ls-tree -rlz` 条目，不读取脏 index 来充当 HEAD。用 `clone --shared --no-checkout` 创建独立仓库；默认入口验证指定分支与来源 HEAD 一致；历史入口仅在新副本写独立 detached HEAD。`read-tree` 只在新副本建立准确提交的 index，没有 reset/clean/强制 checkout/新 commit。

所有 tracked 路径都物化，包括完整历史 artifacts。普通来源文件通过不跟随链接的目录/文件句柄读取，算真实 Git blob ID 和 SHA256；**只有实际 blob 相等**才使用独立 CoW，否则从目标提交的准确 blob 读取。缺来源文件照样恢复准确 Git 内容，特殊类型明确拒绝。模式按 Git 的普通/可执行位恢复；链接从 Git blob 保留原目标字符串，不跟随外部目标读用户内容。NUL 路径解析保留空格、tab、换行、反斜杠和中文路径。

macOS 真实调用 `fclonefileat`，每个成功 CoW 文件即时核验源/副本 inode 不同，再核完整 blob/模式。Linux 有 FICLONE 分支；仅在系统明确不支持 CoW 且空间足够时允许记录为 ordinary 的独立复制，空间不足或其他复制错误非零拒绝，绝不退为硬链接。本轮未测 Linux/Windows，不宣称跨平台 PASS。

前后核全部可能复用的来源文件（含脏/缺失状态）、来源 HEAD/HEAD 文件/index；末尾比较位于副本 Git 核验之后，能拒绝末尾并发漂移。副本 HEAD tree、index tree、完整 Git diff/status 及两次对象可达性全部检查。共享的是标准不可变对象存储，副本仍依赖来源对象存活；对象失效必须失败，不是独立长期归档副本。

目标必须不存在且与来源不相交；同根、祖先/子目录、已有内容、经过链接的目标均拒绝；证据不得写入来源 `.git`。legacy 的系统 TMPDIR 可返回 `/var` 别名，因此只把**本次 mktemp 已创建的父目录**转换为 `pwd -P` 后交助手，助手的链接拒绝不放宽。

原三目录 candidate overlay、Node 当前 package/lock/.npmrc 语义保持。新助手位于现有 scripts 全目录覆盖和 tracked+untracked 绑定范围中；实际 legacy `e0-candidate-binding.tsv` 明确含两个新助手的 SHA，原 source/copy 对称 cmp 已通过。没有增加共享工具路径来绕过输入绑定。

## 最终完整真实执行

候选 HEAD `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`。Darwin arm64；Node v24.16.0/npm11.13.0；Git/Python/Rust 锁定工具链的真实 argv、版本、可执行文件和 lock SHA 见 `toolchain.json`。Rust 仅查询版本，没有构建/运行 Rust 测试。

**最终默认生产前缀**：`actual-final-02/full-default-prefix.sh` 是当前默认脚本 SOURCE_HEAD 至 copy-ready 的原片段，只把 ROOT/COPY/EV 作为参数，没有替换被测准备函数。真实 04:01:23—04:03:09 UTC，exit0，记录在 `actual-final-02/full-default-command.json`。

| 内容 | 实际结果 |
|---|---|
| 完整 HEAD | 38,066 tracked，3,741,290,865 字节；37,887 CoW、179 准确 Git blob、0 ordinary、0 Git symlink（本仓当前 HEAD 无 symlink；链接分支由真实小仓覆盖） |
| 差异对象恢复 | 179 个非复用文件合计 5,208,246 字节；未借用脏内容或丢失文件 |
| 完整 Node | 1,277 安装包；64,760 文件/链接；64,645 CoW、115 内部链接、0 ordinary；1,379,047,525 普通文件字节 |
| Node 实际读取 | 副本内真实 import ws、真实 vitest --version 均 exit0 |
| 全文件独立性补核 | 当前完整副本 38,066 文件及 `.git/HEAD`、`.git/index` 共 38,068 对 inode 检查，无共享可写 inode |
| 全依赖末尾验证 | `node-final-verify-02/result.json`，04:05:40—04:06:04 UTC，exit0；来源/副本 64,760 条、根输入、工具、Git 可达性全部再核 |
| 来源保持 | 最终准备窗口主 `.git` 所有文件/链接逐项相等；跨随后 legacy 的末尾全量检查仍差异 0；主输入及默认副本输入全部相等 |

完整逐文件源清单、Git OID/模式/复制方法/SHA 与命令日志在 `actual-final-02/full-default-01/git-preparation/`；Node 全清单与真实工具读取日志在同层 `node-preparation/`。独立 inode 清单见 `independent-inodes.json` 及摘要；最终保持见 `final-preservation.json`、`final-inputs-preserved-02.json`。

Node manifest SHA 仍为 `a7194de703eabe3832f360f856d613e1ee1e4df7191250726c115ee6bb289408`，与 J 首审完整来源相同；工具记录也相同。本轮证明本地已安装内容一致、独立可写，不冒称重新认证 registry tarball。原 4 条无实体 extraneous 元数据、98 条缺失 optional、三个工具缓存排除规则保持原助手语义。

**真实原 legacy directed 入口**：`legacy-commands-final.json` 保存实际 argv/cwd；设置原有 `R02_LEGACY_REGRESSION_MODE=directed-no-seal-family`，直接执行最终默认副本里的原脚本。04:03:30—04:05:40 UTC，**exit0**。

- BASE 精确为 `201584f2917a7fd96d6ea603bdeddbd420082cfe`；8,880 tracked，1,562,337,119 字节；8,795 CoW、85 准确 Git blob、0 ordinary。
- 原 E0 ancestry、source/candidate 全镜像及对称 cmp、BASE HEAD/index/diff/status 无残留断言全部通过；不是仅助手自称纯净。
- 原 E0s：62 分类夹具；4 组真实 OS fd 发现/源副本对称检查、7 个非法根拒绝、24 组 Git 查询正常/异常/恢复控制，全部通过。
- 原 E1 默认入口/六项运行边界断言、E2 三次 TypeScript 检查、E3 core contracts、E4 两项边界检查、E4.5 renderer 构建均通过。
- **E5 完整 npm 与 seal-family replay 按原 directed 范围明确 SKIP，未执行，不标绿**。E5 分类函数及其 E0s 反例保持并实际运行；raw npm 历史红没有重分类。

完整证据在 `legacy-directed-02/legacy-entry/`，BASE 清单在 `e0-base-materialization/`。临时 source/candidate/BASE 目录由原 trap 删除，末尾确认不存在。默认完整副本仍保留，精确位置见 `actual-final-02/full-copy-path.txt`。

实际空间只报告文件系统截点：最终默认前缀前 1,761,566,720、后 1,640,165,376 字节；BASE 物化前 1,580,879,872、后 1,558,667,264 字节。它们不是本操作独占物理占用/释放量，未用 st_blocks 总和伪造物理空间，也不保证后续大构建所需空间。

## 永久回归与保持性

最终 `python3 -B scripts/rust-tauri/prepare_git_copy_selfcheck.py --evidence artifacts/rust-tauri/R05/RR3/J-02/selfcheck-03`：**31 条断言、49 条记录的真实外层命令、0 ignored/filtered**，不是 49 个业务场景。默认片段与 legacy BASE 片段来自当前生产脚本，Git/文件系统/CoW 均真实；小仓库是替身边界。

覆盖完整 tracked 旧 artifacts、普通/可执行/原链接、特殊字符路径、staged+unstaged、来源缺失/不同于 HEAD；与普通 clone 对照 HEAD/tree/index/status/全文件字节和模式；修改 copy 文件/index/HEAD 后 source 全内容保持；准确历史 BASE；真实缺 Git 对象、共享对象失效、查询非零或带错误 stderr、实际目的目录拒写、早期及末尾来源漂移、非法目标、特殊文件，以及各项恢复绿。macOS 合法 `/tmp` 系统别名在真实 legacy 片段中成功，直接把 helper 目标指向链接仍被拒绝。故障腿仅用 Git 命令代理注入错误/权限/确定性漂移，未替换物化函数；所有夹具在仓外且本轮自行清理，回执保留。

另亲跑现有永久检查：`a-fd` 4 场景/7 非法根/24 Git 查询控制通过；`i-restore` 35 检查、0 失败；`b-negative` 15 控制通过。最终原 legacy 又亲跑了一次实际 E0s/真实 OS 发现器。低成本语法检查通过；未运行 production-sync Rust 构建，不把 I 同步真实大负载算作本轮亲跑。

`preserved-contracts-final.json` 证明以下逐字节保持：negative 从 pristine 到最终结果的整个后半段（含 12 文件还原、N06 同步、N01–N16 和 B 动态计数）；legacy binder/发现/排除/候选完整 cp/cmp 整段；legacy 原纯净断言至全部 E0s/E1–E5 的整段；Node 助手；OS 发现器永久回归。

`prior-review-input-comparison.json` 逐项核 J 首审的 504 个输入，502 个相等，差异仅两个本轮改动的 shell 入口。`node-evidence-reuse.json` 明确记录不变的 Node 助手/永久负控/根输入/真实 Node 调用片段/依赖和工具 SHA。因此既有 Node 负控证据只按这些实际不变输入引用；本轮新的默认 Git 准备、完整新依赖、legacy 受影响部分已重新执行。未把旧默认 clone 失败改写为通过，也未把旧最小 auth/client 业务结果虚报成本轮新跑。

## 保留的中间结果、限制与下一步

`selfcheck-01` 是初版 30 断言/46 条记录外层命令（另有一次未纳入该计数的直接 fsck 负控）；`selfcheck-02` 补末尾漂移与统一命令回执，31/49；最终 `selfcheck-03` 加入系统临时目录别名的真实调用，31/49。早期 smoke 只作为探索保留，不进入最终计数。

三次完整默认准备均有真实日志；最终使用 `actual-final-02`。第一版助手之后补晚期漂移检查；第一次真实 legacy 在 `/var` 别名处 exit1，是本轮接线兼容缺口，原 `legacy-directed-01`/stderr 保留。修正仅解析本次已创建的 BASE 父目录，最终新目录复跑成功。未挑选成功日志覆盖失败。

J-REVIEW-01 原两处 ENOSPC 的真实文件和 SHA 记录在 `historical-failures-retained.json`；正确默认 clone 原件为该审查根目录的 `clone.log`，不是假定的子目录路径。旧 J/G/ALF/原 npm 红与中断原样保留。本轮只清理自己可重建的小夹具及两个被后续运行取代的默认副本，分别有精准 cleanup receipt，原日志/输入/结果清单不删。

本包目前无剩余已知准备阻塞，**只到 SELF_CHECKED**。Linux/Windows 分支、默认 N01–N16、完整 R02/R05/FINAL、完整 npm/E5、本轮真实 auth/CLI_RUST 服务业务均未新跑；旧有效业务可按 502 输入和依赖相等范围复用，不扩张为完整阶段通过。既有 H02 service/resource 证据未重编译或污染。

下一步由另一位新 J 审查者亲跑最终默认前缀、完整依赖/末尾核验、历史 BASE 与原 directed/真实发现器及正负控；包级独立通过后再新 G02、E/FINAL。主 Node 及 Git 未修改，系统权限未修改。`process-closeout.json` 确认本轮已无匹配的执行进程；所有执行工具已退出。报告和最终 manifest 写完后停止全部写入，交总控接手，R06_READY 保持 false。
