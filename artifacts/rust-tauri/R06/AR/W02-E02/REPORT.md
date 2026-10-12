# R06-AR-W02 / F02 — W02-E02

状态：READY_FOR_INDEPENDENT_REVIEW（仅执行代理交候选，不是独立验收PASS）。

恢复现在先核对当前权限、文件内容和本会话实际写入来源，再准备临时文件。提交时交换两个文件，保留实际被替换的对象；遇到外部改动时回换，若又发生竞争，保留两个实际对象并明确报“不确定、可能已写入”。错误和重新启动均不擅自重做恢复。

## 候选和授权

基线 HEAD `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`，分支 `codex/rust-tauri-migration`。精确15文件及逐文件SHA见 `candidate-summary.json`，差异见 `candidate.diff`。`own-inputs-freeze1.json` 保留先前冻结；本次绑定 `own-inputs-pre-final.json`，最终15文件内容与冻结完全一致。用户settings SHA保持 `0f72eb0a2549c95973c2aa8abe54c9aa44491fb9ca2a1c6e437404655d36b3d3`。没有Git写、远端操作、真实用户数据或付费供应商调用。资源测试守卫保留。

固定 cargo 1.98.1，所有cargo `--locked -j8`。实际命令、开始/结束、退出码、测试数、隔离 `LINGXI_HOME` 与日志SHA见 `final-command-results.json`；日志独占目录 `attempts/final-attempt5/`。每次服务启动只使用自己创建的temp CLI data-home，专项日志打印实际 home/DB，CLI覆盖任何已有env。早期单命令未显式设process LINGXI_HOME，但真实服务仍为自造CLI home；不将它描述为相同的env证据。最后runner逐命令另建 `/tmp/lingxi-w02-e02-*` 环境，真实服务仍另建独立home。

共享工作区含其它任务尚未独立通过的改动；最终运行属于组合运行。W02本15文件不引入W12 knowledge NOFOLLOW/版本保护代码，不把W12当已通过依赖。独立验收必须在2e基线仅复制15文件亲跑；基线知识库原正常open没有W12引入的 `/var` 链接回归。当前系统别名正对照成功只是组合运行，不替代单项基线运行。本记录没有静默复制其它Task产品。`lib.rs` 当前差异均是本任务接线，其余修改/用户已有内容保留。

## 生产调用与同族修复

- bootstrap共享 `FileSafety` 接给实际 FileTools、ResourceIO、SessionService；真实gateway write和真实HTTP write均建立见证。见证按主体类别/主体ID/session/路径绑定当前fd的dev/ino/大小/mtime/ctime/hash，旧checkpoint哈希、历史轮询、读取和checkpoint不会制造应用写入证据。
- 当前session授权由现有SQLite写队列重新读取，并在完整恢复callback期间持有队列；预提交及交换后继续核对共享ResourceAccess。内部Mutex只串行本服务操作，**不声称阻止外部进程**。不存在新schema迁移。
- Snapshot沿现有DirFd NOFOLLOW逐组件打开，拒绝最终链接、祖先链接、目录身份变化，前后fd元信息稳定才认快照；新增快照路径8MiB上限及 `take(cap+1)`，拒绝大文件和增长，不改变旧普通大文件read流义务。见证文件64KiB上限。
- 先0600临时创建、写入、checked权限复制和sync，再核验当前路径；准备journal并保留候选，macOS `renameatx_np(RENAME_SWAP)`、Linux `renameat2(RENAME_EXCHANGE)`提交。交换之后复核实际移出的inode/mtime/size/mode和有界字节；不匹配则回换。回换前核验当前新对象，并复核回换实际移出的对象；第二竞争产生unknown而不是删除未知文件。原始对象从不自动unlink。
- 交换发生后任何错误保持unknown/`write_may_have_occurred`；prepared/unknown/completed journal和保留对象不在启动时盲重放。交换前失败删除的只有本操作已知temporary；已prepared对象保留。未支持原子交换的目标平台拒绝危险已有文件恢复，不退回普通rename覆盖。
- DirFd原已有能力复用；补漏close、state child dir NOFOLLOW0700、state files0600、checked fchmod，不再忽略复制权限失败。state写亦沿句柄，外部把来源目录替换链接不能伪造见证或导致跟随外部目录写入。
- 真正rewind/retry/preview/checkpoint统一当前授权、来源和字节校验；checkpoint存储内容还校验原始hash/size。历史恢复走真实ResourceIO；expected-version提前持有snapshot，并在commit使用同一快照，后续变动真实409。
- 通用文件/ResourceIO在宽授权home根下仍不能读取/写入 `file-write-witnesses`、`file-recovery-journal`、`.lingxi-recovery-*`；对包含内部根的父目录write也拒绝，防trash/move整个private状态。fsread、preview资源、资源下载、bridge下载和SessionFile登记补同边界。旧sidecar仍可保留原ID，但映射私有原件的实际内容路由403。ResourceIO list不泄露保留对象，原回归断言未放松。
- `list_session_file_hashes` 仅是旧历史索引，未作为恢复授权；旧kernel `judge_file_restore` 无生产消费者。本轮不凭其单元测试绿声称已验证新链。无配置FileTools兼容fallback保留；扫描生产唯一注册在bootstrap，已设置共享FileSafety。R04/T06/T08的手工未配置工具harness只证原兼容，不证明生产新边界。

## 四C逐项自检

| C-ID | 期待 | 实际及证据 | 状态 |
|---|---|---|---|
| C01 | 应用A→checkpoint→B可恢复；外部C/旧hash不得覆盖 | `ar_f02_c01*`实际HTTP write/checkpoint/rewind target=A、retained=B；随后外部写历史B被拒。`native_filetools*`真实gateway写入，主体或session不匹配拒绝 | 本机自检通过；独立NOT_RUN |
| C02 | 当前授权及最终/祖先链接不能沿旧授权恢复 | 真实DB撤销授权；最终/祖先链接sentinel=B；删除重建/hardlink、来源目录链接、宽授权根及legacy私有资源均真实检查 | macOS自检通过；其它平台NOT_RUN |
| C03 | 读取后、最后核验后外部原地写/替换必须保留 | 外部python3子进程在before_replace、commit_boundary分别写C或replace inode；final=C。最后窗口交换保留并回换，backup=A。反向交换第二竞争D时final=C、retained=D、unknown | 本机确定性自检通过；不是全外部进程排他保证 |
| C04 | 部分写/sync/提交后错误、多文件强停、重启诚实 | temp_write真实PARTIAL临时载荷+ENOSPC注入；permission_copy/temp_sync/before_replace均target=B；after_replace target=A、retained=B且unknown。3个真实子进程exit73，重启journal/bytes不变，双文件只首个已交换 | 本机注入/进程停止通过；物理满盘、断电、真正取消API NOT_RUN |

真实结果的路径与收据逐条保存在 `final-selfcheck.log`：正常恢复target A、保留B；late竞争target C、保留A；第二竞争target C、保留D、`unknown_original_retained`；提交后失败target A、保留B、`write_may_have_occurred`；强停commit_boundary target B/保留A，after_replace target A/保留B，multi首文件A/保留B且第二文件仍B。重启不改这些字节或journal。unknown决不能按“failed且零副作用”理解。

专项每轮15 declared = 14父检查函数 + 1无env时直接返回的crash child helper。父检查启动3个真实child（exit73）；不能把helper无操作或子进程停止重复算完整验收PASS。两轮15+15、原套件70、adapter32共132 declared Rust passed，其中两个helper无操作；实际崩溃检查为两轮各3个子进程，不追加测试数量。ScriptedProvider只替代供应商Final回包，磁盘、SQLite、HTTP鉴权、gateway、恢复/历史生产逻辑没有mock。

## 原Steps、交付物与A-ID

| 原条款 | 期待与实际检查 | 本轮状态/证据 |
|---|---|---|
| T03 S01 | create/list/search/pin/archive/restore/delete/fork/retry/edit/rewind原入口保留；management_surface、store admin/search、fork/retry/rewind真实回归 | 关联本地回归已运行；原桌面/分页/广播份额NOT_RUN |
| T03 S02 / A05 | ID和共同历史保留，新写独立，权限不能跨分支扩大；`fork_copies_history_with_stable_ids_and_writes_stay_independent`、cross_principal、adapter fork/授权检查 | 服务/存储自检通过；桌面消费及LIVE NOT_RUN |
| T03 S03 | retry新Run不覆盖旧审计结果；retry_resets_branch、target_resolution、busy、file_rollback原回归 | 关联真实服务回归通过；客户端两段重发语义仍原限制 |
| T03 S04 / A06 | 真实checkpoint版本/来源/当前授权；外部修改冲突且不能伪称外部副作用回滚 | 全四C+retry/rewind原断言通过，receipt `externalEffects=not_rolled_back` |
| T03 D01-D03 | SessionService、树/分支索引、回退检查生产接线 | 真实生产链与存储回归；无新独立交付验收 |
| T05 S01 | 登记、stage/materialize、preview、cleanup、引用授权保留 | session_files/resources/resourceio/preview/fsread/bridge真实套件；private状态所有相关消费者拒绝 |
| T05 S02 / A09 | sf_旧ID映射/可读/未授权拒绝，不输出远端未授权path | `a09_import_legacy_sidecar_preserves_ids_and_readability`、fork文件ID、resources票据/授权及私有legacy资源403；真实各本地服务面通过；实际桌面/LIVE所有客户端NOT_RUN |
| T05 S03 | 存在/hash/MIME/大小/所属/授权/引用一致 | reference_checker、登记/inspect/route、snapshot损坏与主体session见证测试；本机通过 |
| T05 S04 / A10 | 缓存清理保权威文件及活跃引用，不删恢复原件 | `a10_cleanup_keeps_delivered_external_and_cleans_cold_managed`、cold_live_reference、warm、bootstrap_cleanup原断言通过；未知保留原件永不自动清理 |
| T05 D01-D03 | ResourceService、SessionFile迁移、文件引用检查器 | 原真实生产回归通过；不宣称补齐原延后providers/桌面转换 |

完整131个Task叶成员（T03 74、T05 57）及每个原R00断言/到期标识/锚点见 `original-leaf-coverage.json`；关联原Steps/A/交付物及其它原义务原文见 `original-obligations-coverage.json`。原deferred/share以及桌面、广播、供应商、mount/url成功、docx/xlsx、watcher/sweep、长运行cleanup原未验份额继续NOT_RUN，不能把关联套件绿升级为每叶完整独立PASS。没有修改原任务范围或总控原始记录。

## 失败历史与旧行为对照

`old-red.log` 在未修产品上真实运行1项，exit101：恢复HTTP200/receipt restored，却将外部sentinel B改成A。这是有效旧行为红；新同一不覆盖断言通过。没有把0运行编译失败当红。该日志原始argv/start/end未写入，不能伪造早期时间；日志实际HTTP结果和原失败原样保留。

其余全部原日志及runner原json保留；`failure-history.json` 用实际唯一路径解析旧滚动文件并核对SHA：check1/2临时编译错误，check3/4/5成功；new1缺title fixture4fail/new2 5pass；exchange1共享W12 NOFOLLOW别名启动5fail/exchange2 canonical5pass；full1 filehistory响应fixture错误1fail；final3 native复用旧checkpoint fixture1fail；runner1 cwd错误0tests，runner2路径alias fixture8fail/资源list1fail/瞬态canon编译，runner3 14+14+70通过但新guard缺参数编译，runner4专项0tests crate-private helper误用，regression70/adapters32/clippy通过。上述均非最终候选验收。修正fixture没有删原契约断言；资源列表新增私有保留件拒绝是真实同族生产修复。两次冻结均保留。

证据构建脚本第一次在clippy尚未结束时主动assert拒绝封存，未产生Ready结果；待实际6命令全部完成后重跑，不把该证据脚本失败记产品/行为测试失败。

## 限制及其它任务影响

- macOS亲跑。Linux交换代码未执行；Windows/reparse及其它平台编译/运行NOT_RUN，已有文件恢复在不支持安全交换平台明确拒绝，不宣称跨平台通过。普通读取大文件流保留，新增可恢复写/快照超过8MiB明确拒绝。
- 无法锁住任意外部进程或持有原inode的写句柄；交换保存实际对象，后核验和安全回换能捕获已测竞态，持续竞争可能留下unknown和恢复对象。并发外部改同一对象的后续字节仍属于外部进程行为。不能声称瞬态目标从未改变或所有竞态都无副作用。
- 只复制POSIX mode，未验证完整ACL/xattr；物理ENOSPC、断电/fsync硬件语义、真正cancel API均NOT_RUN。临时初始0600和权限复制失败拒绝已测。
- 未知原件和journal永不自动清理，磁盘占用会增长；本轮没有新增自动恢复/清理接口，需保留证据交独立review判断。通用工具不得删除/伪造这些对象。OS同用户直接改可信服务状态不是应用API授权边界，未承诺OS沙箱。
- R04/T06/T08旧手工未配置工具harness仅兼容证据；W12未独立通过，不作为W02依赖通过。旧T03仅hash不存内容/旧hash集合判来源/盲普通write恢复，以及早期仅pre-rename复核的论证均STALE；旧全仓历史绿不绑定本15文件。旧T05叶延后/份额及完整生产平台要求保留，没有替它们补签PASS。
- 影响T03 retry/rewind/checkpoint/preview和T05 FileTools、ResourceIO版本写、filehistory、读/预览/引用/桥下载保护；其它Task产品未改。独立review尚未运行；不能以本报告自签关闭F02或R06整体验收。
