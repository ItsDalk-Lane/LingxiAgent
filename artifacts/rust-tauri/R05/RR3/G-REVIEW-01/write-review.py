import pathlib,json,datetime,re
E=pathlib.Path(__file__).resolve().parent
def load(p):return json.loads((E/p).read_text())
a=load('actual-evidence-audit.json');bind=load('binding-manifest-audit.json');cmds={x['name']:x for x in load('supplement-commands.json')};head=load('source-before.json')['head'];prod=load('full-producer-restoration-audit.json');pair=load('n16-pair-binding-audit.json');boundary=load('final-input-boundary.json')
assert load('extra-complete.json')['originalInputsEqual'];assert a['all12Restored'];assert prod['commandExit']==0 and prod['runs']==91 and prod['passed']==427 and prod['failed']==0
now=datetime.datetime.now(datetime.timezone.utc).isoformat();rows=[]
notes={1:'删R05-A16，镜像点名缺项；0通过/1失败',2:'故意零匹配，cargo为0但生产者因running=0拒收；91组/426通过/0失败，只有注入筛选为0',3:'权威唯一24→23，matches=1/mutations=1；0通过/1失败，精确还原1通过',4:'真实无料断言失败且吞退出；91组/426通过/1失败，生产者计数检查点名拒收',5:'真实旧根预置后拒收not empty',6:'真实R02中改kernel；20/20不稳且只点名kernel，runner PASS；overall FAIL',7:'删R04回归命令，点名references unknown command，非编译失败',8:'接线反转；binary 1通过/1失败，service lib 358/0对照绿',9:'固定done；runtime nonce 0通过/1失败/11过滤',10:'call ID错配；binary 1通过/1失败，点名call_bin_1',11:'StopUnconfirmed压为成功；诚实目标0通过/1失败/118过滤',12:'持有model permit；嵌套目标1通过/1失败/8过滤',13:'删除R04 deferred basisKind；实际map解析拒绝',14:'增LIVE登记；离线镜像0通过/1失败',15:'合成秘密进入错误文本；无料目标0通过/1失败'}
for r in a['result']['cases']:
 n=int(r['case'][-2:]);rows.append(f"| N{n:02d} | {r['exitCode']} | {notes[n]} | 默认真实记录目标红 |")
rows.append('| N16 | 默认未收尾 | 两个真实R02均已完成，摘要不同/各20稳/runner PASS；原reuse未运行 | 缺生产case行；补证单列 |')
cs=[]
for k,v in cmds.items():
 count='；'.join(v.get('counts',[])).replace('|','/') or '无Rust测试summary（按实际命令/结果JSON判断）';cs.append(f"| [{k}](supplements/{k}/command.json) | {v['exitCode']} | {count} |")
rl=[]
for x in bind['results']:
 if x['stage']=='R02':
  failures=[f"{c['key']}={c['exitCode']}" for c in x['commands'] if c['status']!='PASS'];rl.append(f"| {x['result']} | {x['overall']} | {x['stable']} | {len(x['checkpoints'])} | {', '.join(failures)} |")
text=f"""# RR3 G-REVIEW-01 独立验收：FAIL

默认全16未成功完成，不能接收当前候选。实际生产入口exit **2**，只生成 **15** 条N01–N15记录，N16两真实R02完成但旧根拒收/生产汇总未收尾。运行期间另包修改了主入口及生产输入；新旧脚本各自bash -n均0，运行中脚本流在Python片段处报shell解析错误。此非目标错误不得算N16有效红。所有旧输入目标与恢复补证完成后仍保留本轮FAIL。`R06_READY=false`，正式FINAL §5.3未执行。

## 身份、继承与入口

验收者rr3_g_review_01，独立空历史CLI exec --ephemeral，thread `01a113f6-7e90-7051-b75f-8f28fd53ee99`，未参与B/A/C/F46或RR3实现/先前验收，未派agent、未对外发消息。真实派发/argv见[身份回执](reviewer-identity.json)及dispatch；不虚称collaboration子线程或后端精确模型版本。

完整读取G/review/RR3 brief、RR1/RR2 MASTER、RR3矩阵/进度/交接、A/B/C/F46报告、新独立报告及指定RR2/G-R2历史原证；[读取和历史审计](reading-and-history.json)、[历史层级/manifest核验](reference-audit.json)保留原文摘要。继承16A、100+3C、130适用叶、I01–I11、N01–N16及原§6.1/6.2；不实施R06、不重做原28项。

候选HEAD/初始远端 `{head}`，codex/rust-tauri-migration加39文件脏树overlay。macOS arm64；绝对rustup入口 `/Users/study_superior/.cargo/bin/cargo`，cargo/rustc1.98.1、所有Cargo构建/测试--locked，离线；Node24.16.0/npm11.13.0。工具实际argv/UTC/exit/可执行hash见[environment](environment.json)。

本人亲跑唯一生产默认入口：

`bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/RR3/G-REVIEW-01/default16-01`

开始 `{a['default']['startUTC']}`；结束 `{a['default']['endUTC']}`；exit2。[真实command](default-command.json)、[原console](default-console.log)和[原case行](default16-01/case-results.tsv)不改写、不补造case-results.json。自有生产clone `/Users/study_superior/r05t08-work/negcopy.hTXzzN`；初始913明确文件与主树逐字相同，486实际执行输入单列[sourceCOPY边界](source-copy-boundary.json)。不声称913覆盖整个Git树；真实R02 binder另覆盖38083实际条目。

## 必须交总控处理

**MF-G01：运行窗口未冻结，当前候选缺默认完整16验收。** 原入口SHA `4853e129e2d0cf7dbd6c93c8c158186fad23e6f231a03ff64f140cca61e0042f`；其后主树脚本、service lib.rs/redaction.rs及R02 smoke变动，真实[漂移](source-drift-default-failure.json)、[最终边界](final-input-boundary.json)、[双语法对照](script-syntax-controls.json)。运行入口不能边执行边改，也不能把只固定内层copy误称外层生产入口冻结。待H/I停止写并由新独立角色验收后，记录实际输入冻结清单，由**另一全新G reviewer**在新目录亲跑当前生产默认N01–N16和受影响恢复/反例；不得把本轮15行+N16补证拼成生产16/16。本人不修生产对象、不代验H/I、不连续复审自己的FAIL。

旧reset漏kernel确已核实：N06追加注释保留至N07–N16-A，N16-B带两段注释。SHA原始 `8f6595e9be39865f74a91817089ab1d6b016beb84f31a6ed08ca58b96229d3e5`、N06后 `0a5f0c3aee15f437cae03cca5eb182680f6e57aa64e3dcd95fbb5561a243720f`、N16后 `cecd3f054673c853992b2c5c9e0e4c7ef1763561b905b65a233a01f1eef471e0`。只改变身份，不宣称无害注释是功能缺陷，亦不把后续带注释输入冒称原候选。[逐次实际hash观察](mutation-observations.json)和[12文件恢复](full-restoration.json)证实精确还原。总控另登记F49/I新修复属另一轮，本报告不提前签其PASS；原brief允许限定恢复补证的边界未被改成虚构功能缺陷。

## 默认各项的真实结果

| 实际身份 | exit | 目标原因/实际数量 | 判定 |
|---|---:|---|---|
{chr(10).join(rows)}

controls实际xtask8/0、binary2/0，N03恢复1/0，均exit0。N02只有刻意注入的一组零匹配；有效红是生产者拒绝0匹配，不能把cargo0/零测试称断言绿或无关红。N04唯一真失败和RUN_EXIT=0变异由源hash/原日志/实际计数绑定，另独立直跑同具名断言101→精确恢复1测0。其余行为负测均非零匹配、真实目标FAILED；parse/root边界按具名拒绝判断。没有用无关编译/早认证失败替代上述目标。

## 绑定、唯一身份及恢复补证

N06原R02 **20/20 checkpoint false**、changedPath仅kernel、runner PASS、overall FAIL；业务命令13/20 PASS与此独立。补充只在自有copy把R02 map临时设为明确RX单命令，以**原生产main**亲跑：全部命令PASS/0的normal→中途改kernel后命令仍PASS/0而overall FAIL/exit1→字节恢复normal PASS/0。这是生产!stable强制FAIL分支证据，**RX受控编排不是R02/R05业务gate**。临时map及kernel逐字还原后显式cargo重build原runner，不能新helper/map复用旧runner。全部实际结果与source/binary/log/UTC见下表及[map恢复](controlled-map-restoration.json)。

N16 A摘要 `{pair['legs'][0]['digest']}`，B `{pair['legs'][1]['digest']}`，实际不同；跨两份38083条目只有kernel不同，两腿各20/20稳/runner PASS。[独立重算摘要](binding-manifest-audit.json)按生产domain/长度编码重算所有before/after manifest，文件hash/计数/唯一路径与引用相等。[pair核对](n16-pair-binding-audit.json)保留完整差异。原默认旧根腿缺失；[独立旧根拒收](supplements/N16-used-root-refusal-supplement/command.json)实际exit1/is not empty，未追记生产汇总。

恢复绿覆盖xtask121、N08/N10二进制2、N09 nonce1、N11诚实1、N12 permit2、N04/N15无料1及全部91组生产者。原始kernel及所有12快照文件逐项hash相等；额外R02 map/CID/required/leaf表也精确恢复。最终copy与初始913范围及486执行输入相等，**当前main执行输入不相等**；见[最终边界](final-input-boundary.json)。自有补证所有临时变异及前后输入hash在逐命令JSON中，未写主树。

| 真实补证命令（完整argv/cwd/UTC/input/binary/loghash在链接） | exit | actual counts |
|---|---:|---|
{chr(10).join(cs)}

完整恢复producer实际 **91 runs/427 passed/0 failed/0 ignored**；27 suites+64 exact lib pins，93 cid-owned+10 command-bound=103 required C-ID，130 leaves/137 leaf cases。各执行测试恰一个CID，不缩规格。[独立数量审计](full-producer-restoration-audit.json)、[原生成文件](supplements/producer-restored-full/suites/)及完整原始资源序列保留。

## I01–I11完整映射（只旧冻结隔离输入，不签新H/I候选）

以下suite均本轮恢复producer实际执行；具体原测试名、counts、CID/叶对应来自真实日志与权威四表，不把列举等同正式全阶段放行。旧RR2 I-MAPPING原证保留，当前亲跑取本目录suites。I06审批补证和I10新C/F46具名证据单列。

| 项 | 要求→断言→有效证据 | 结果/边界 |
|---|---|---|
| I01 | model_plane24：f29_management_reload_during_inflight_401_never_leaks_the_new_key、f02_resolve_dispatch_freezes_compat_and_capabilities_with_the_route、跨provider隔离；credentials38 concurrent401/旧handle/删除重加不复活；64 exact中reload/late-writeback/catalog代次 | 旧输入PASS |
| I02 | credentials38 rr1_f04六叶：PKCE/device登录→真实调用、add/remove/list（非OAuth404）、status模型数、logout、restart迟到事务/撤销fence；oauth_flows21刷新/过期/scrub，真实磁盘/HTTP断言和13独占叶绑定 | 旧输入PASS |
| I03 | replay33+goldens3+protocol12；签名/reasoning/opaque族间隔离及顺序；runtime_nonce真实工具→下一请求，C03并行call配对/C12已确认exchange续接；N09/N10真实目标红/恢复绿 | 旧输入PASS |
| I04 | batch_terminal10（含F34 unclosed腿）+streaming18：合法write+read(path=123)整批零副作用、冲突ID拒整批、重发收敛/独立ID各执行/跨轮合法复用、截断零执行 | 旧输入PASS |
| I05 | closed_loop11：五方final/实时/快照/DB/重启、零重执行、崩溃诚实；batch/streaming非final、reasoning/opaque/mood/truncation断言；c16_a09_midstream_cancel_releases_the_connection_and_settles_once | 旧输入PASS |
| I06 | production_tools6真实四工具/subprocess/globalpermit1/子环境无凭证/SIGTERM回收；worker_model10授权purpose/回调trace/未知停止，usage_ledger15父子物理对账；本轮I06-existing-approval和I06-existing-worker-permission具名现有R04测试补审批/取消/重启/不可绕权限 | 旧输入PASS，非正式R04完整gate |
| I07 | network21 proxy/direct/NO_PROXY/privateCA/TLS/DNS pin/redirect；credentials c10凭证不跟redirect；timeouts service9/adapters13总预算、错误体上限、refresh/operationsqueue、egress exact拒绝 | 旧输入PASS |
| I08 | media_resource17授权路径/叶及父symlink、host/jobID区分、旧任务reload不改绑、cancel poll/download无迟到登记、registered product sha；operations/speech原suite保留 | 旧输入PASS |
| I09 | usage_trace9、usage_ledger15、strict7/families14、persistence5：外部HTTP/物理尝试/父子/owner查询/重启相等，UNKNOWN不填0、未发不记已发、失败/存储异常不漏账、无秘密；原S4/schema登记底座历史证据保留 | 旧输入PASS |
| I10 | 本轮完整资源2/2、160轮、54真实进程树点、15 live-worker峰值与释放、61 owner点/45稳态，真实permit/waiter/run/TCP/tmp/日志≤3；新C/F46测量假FD/TCP0各101→恢复0、六次真实旧日志顺序红→还原绿；普通取消两个具名测试及crash/replay下述映射 | 旧输入PASS；当前main C321两处已变，须新边界消费 |
| I11 | 默认N01–N15真实目标红；N16两真实绑定+独立reuse补证；F25/F26新CID/缺登记/bogus command/exclusive share/缺leafcase目标红→精确恢复绿；新A fd永久反例+三控/完整checkpoint；B41坏锚点/坏汇总；C假0测量负控 | **默认全16整套FAIL**；不缩为15项规格 |

I10取消归属必须区分：60预算408在重启前仍running（cancelDanglingActive60/cancelSettled0），连接已回收；重启消解60且provider零重执行，不能冒充普通取消后同实例恢复。普通取消用新C/F46独立 `subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap` 1/1（7过滤）和 `late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing` 1/1（4过滤）；媒体两cancel2/2，crash-effect/stream/request-replay/callback-replay各1具名。历史只有父PID的18点不代表全进程；新证为真实PPID树54+生产owner61两类边界。G自己完整raw SHA `{prod['resource']['sha256']}`，115资源点/160逐轮完整保留，不裁峰值。

[初始引用审计](reference-audit.json)及[隔离copy同输入](isolated-copy-reference-equality.json)确认A19/B4/C321对应冻结旧输入；新A2 2475、B641、C/F46 533 manifest全相等。A1仅17未变输入复用121及内外各50 checkpoint，新A2已覆盖两处shell改动，不整体复用旧1563广义清单（[广义差异](reuse-a1-input-audit.json)及[实际17](reuse-a1-targeted-input-audit.json)）。A受控7/8/15/20层全部checkpoint与真实fd反例均不是业务gate。当前main A19仍相等、B4不等/C321两项不等，[最终引用](reference-audit-final.json)拒绝把旧证签给新候选。

B单N03其他15 NOTRUN原样保留；历史RR2真实16/16及N03当时隔离24→23原证保留[历史负测审计](historical-negative-audit.json)，没有延期当前N03或移到R06。旧F31/F34/F41/F44交叉反例只保留历史原证，不虚称本轮重跑。

## 全部业务失败与历史边界

| 原始R02结果位置 | overall | binding stable | checkpoint数 | 全部非PASS commands/exit |
|---|---|---|---:|---|
{chr(10).join(rl)}

这些业务FAIL与N06/N16绑定属性正交：a01原数据/epoch字节未变且fallback没创建，但原严格home清单因instance/log/tmp新增而失败；a13 auth marker找不到error body requestId；auth及CLI/client/legacy多项因生产clone未复制node_modules而缺模块/缺vitest（原具体JSON/stderr全部保留）。不统称ALF、不以其overall FAIL证明绑定、不把“数据未变”重签原FAIL。总控另H/F47/F48定位修复为新输入，本G不自修、不签其PASS。

历史RR2正式R05 FAIL5/7 stabletrue；R04 FAIL6/8、8/8不稳；R03 FAIL14/15、15/15不稳，分别只指父stdout；真实JSON递归读取并保留。raw npm候选exit1/3文件6失败、baseline0不改绿；directed-no-seal-family E0–E4.5绿/E5合法跳过只在原授权边界，完整s5-full-5治理GREEN保留其raw红，不当npm正式全绿。D确切对象BLOCKED及准备操作不被G不同hash管理面通过替代。E/D及H/I并行更新如实列final边界，整个树从未由本轮冻结。

## 证据和限制

[actual-evidence-audit](actual-evidence-audit.json)记录所有原log/counts/hash/已执行cargo路径和结果；[默认live binary](live-binary-observations.json)、[补证live binary](supplement-live-binary-observations.json)保存真实PID/PPID/argv/文件SHA，UTC是**观测时间**不假称精确launch；逐补证command JSON为实际start/end。runner13项编译来源与当前copy源码/4map/helper两端一致；Cargo.lock、工具链、contracts/schema、测试/support/fixture/配置与authority四表摘要在输入清单内。未捕获的瞬时PID不凭路径猜SHA。

只写本目录、自有clone/夹具及生产脚本自然输出；无主树注入/源码/文档/authority/Git写、无系统权限/过滤器变更、无提交推送/外发/agent。只清本轮自己的重复Git对象（改自有alternate共享只读主库，HEAD/tree/refs/index/status一致）及已停止的自己CLI独立编译缓存，真实binary原路径/sha、source与证据保留；逐文件SHA/身份/空间回执见own-storage-receipt和own-cli-cache-*，未删其他用户历史/副本、未清共享NEG_TARGET。

平台仅macOS arm64 debug、本scope；不外推Windows/Linux、正式打包、LIVE供应商、最终全部Tokio任务或全阶段门禁。最终交付时间 `{now}`。本轮FAIL交总控按MF-G01安排新冻结/新审查，G自身恢复及补证完成不替代新默认全16；[MANIFEST](MANIFEST.json)覆盖本目录封存证据（动态dispatch单独界定），所有真实失败保留。
"""
(E/'REVIEW.md').write_text(text)
print('REVIEW written FAIL')
