from pathlib import Path
import json,hashlib,datetime,re,subprocess
ROOT=Path.cwd();E=ROOT/'artifacts/rust-tauri/R05/RR3/E-01';D=ROOT/'docs/rust-tauri/R05'
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def dump(p,d):Path(p).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
b=json.loads((E/'before.json').read_text());inputs=json.loads((E/'inputhash-before.json').read_text());after={p:sha(ROOT/p) for p in inputs['files']}
assert after==inputs['files'];dump(E/'inputhash-after.json',{**inputs,'capturedAt':now(),'files':after,'sameAsBefore':True,'qualification':'E文档前后本清单生产/测试/构建输入相等；非最终终审来源绑定，不虚造tested SHA。'})
source_specs=[('rust/crates/lingxi-kernel/src/model_exchange.rs','pub struct ModelRouteRequest',15),('rust/crates/lingxi-kernel/src/model_exchange.rs','pub struct ModelTurnInput',46),('rust/crates/lingxi-kernel/src/model_exchange.rs','pub enum ExchangeItem',24),('rust/crates/lingxi-kernel/src/model_exchange.rs','pub struct TurnOrigin',15),('rust/crates/lingxi-kernel/src/lib.rs','pub struct RunContext',13),('rust/crates/lingxi-kernel/src/usage.rs','pub struct ModelUsageQuery',14),('rust/crates/lingxi-kernel/src/ports.rs',"fn next_turn<'a>",9),('rust/crates/lingxi-kernel/src/ports.rs','fn query_model_call_usage(',7),('rust/crates/lingxi-adapters/src/models/credentials.rs','pub trait ProviderCredentialPort',9),('rust/crates/lingxi-adapters/src/models/auxiliary.rs','pub struct AuxiliaryRequest',10),('rust/crates/lingxi-adapters/src/models/auxiliary.rs','pub async fn complete(',18),('rust/crates/lingxi-service/src/operations.rs','pub struct OperationCallContext',8),('rust/crates/lingxi-service/src/operations.rs','pub async fn embed(',8),('rust/crates/lingxi-service/src/operations.rs','pub async fn rerank(',8),('rust/crates/lingxi-adapters/src/models/operations/embedding.rs','pub struct EmbeddingRequest',11),('rust/crates/lingxi-service/src/lib.rs','Arc::new(workermodel::LedgerWorkerCallbackTrace::new(',6),('rust/crates/lingxi-service/src/runs.rs','let turn_input = ModelTurnInput {',13),('rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs','async fn rr1_f21_operation_context_carries_session_run_and_cause()',53),('rust/crates/lingxi-adapters/src/storage/migrations.rs','version: 7,',4),('rust/crates/lingxi-protocol/src/handshake.rs','pub const WIRE_PROTOCOL_MIN_SUPPORTED',7),('rust/crates/lingxi-protocol/src/wire.rs','pub const EVENT_SCHEMA_VERSION',1)]
sources=[]
for path,needle,n in source_specs:
 lines=(ROOT/path).read_text().splitlines();i=next(i for i,s in enumerate(lines) if needle in s);sources.append({'path':path,'line':i+1,'endLine':i+n,'sha256':sha(ROOT/path),'excerpt':'\n'.join(lines[i:i+n]),'role':'SOURCE_INSPECTED_NOT_NEW_TEST_EXECUTION'})
dump(E/'source-evidence.json',{'capturedAt':now(),'sources':sources,'boundary':'接口按当前声明及实际生产调用核对；ModelTurnInput旧注释None today与当前runtime不同，以runs.rs实际deadline接线为准。样例摘自原受控测试，本E未编译或执行它。'})
# 阅读索引只说明实际消费方式，不冒称逐行审完几MB账本或全部历史日志。
read_paths=[]
for name in ['RR3_E_BRIEF.md','RR3_BRIEF.md','RR3_REVIEW_BRIEF.md','RR1_MASTER_PROMPT_2026-10-04.md','RR2_MASTER_PROMPT_2026-10-06.md','RR1_BRIEF.md','RR1_FINAL_REPORT.md','RR1_PROGRESS.md','RR1_LEAF_DEVIATIONS.md','RR2_BRIEF.md','RR2_ISSUE_MATRIX.json','RR2_PROGRESS.md','RR2_HANDOFF.md']:
 p=D/'repair-current'/name;read_paths.append({'path':str(p.relative_to(ROOT)),'sha256':sha(p),'method':'全文/分段阅读；JSON为完整顶层及10条issue全字段' if p.suffix=='.json' else '全文分段阅读'})
for p in (ROOT/'artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/specifications').rglob('*.md'):read_paths.append({'path':str(p.relative_to(ROOT)),'sha256':sha(p),'method':'全文分段阅读原规格（只读，不按R06任务书启动实现）'})
h=json.loads((D/'R05_HANDOFF.json').read_text())
for p in h['rr3_current']['evidence_refs'].values():read_paths.append({'path':p,'sha256':sha(ROOT/p),'method':'报告/映射全文；JSON与原始资源全对象解析、有效回执/关键日志/哈希核对；不冒称逐行审完所有cargo日志'})
for path in ['artifacts/rust-tauri/R05/RR2/D-R2/probe/zz_lanprobe.c','artifacts/rust-tauri/R05/RR2/D-R2/probe/zz_lanprobe.py','artifacts/rust-tauri/R05/RR2/D-R2/probe/probe-suite.log']:read_paths.append({'path':path,'sha256':sha(ROOT/path),'method':'全文只读'})
read_paths.append({'path':'docs/rust-tauri/R05/repair-current/RR1_ISSUE_MATRIX.json','sha256':sha(D/'repair-current/RR1_ISSUE_MATRIX.json'),'method':'顶层基线/计数/独立终审及E相关F27/F28/F40/F42、继承关闭与证据入口全字段；未冒称全文逐条人工重新验收所有F-ID'})
dump(E/'READING.json',{'recordedAt':now(),'entries':read_paths,'owned_before_refs':[str((E/'before'/p).relative_to(ROOT)) for p in b['owned']],'large_ledger_method':'现行大型账本完整JSON解析，历史字段/字节前缀全量比对；语义只查本E相关当前块及引用，不重新验收全部旧用例。'})
cmds=json.loads((E/'commands-03.json').read_text());assert cmds['allExitZero']
changed=[p for p,v in b['owned'].items() if sha(ROOT/p)!=v];dump(E/'changed-files.json',{'recordedAt':now(),'owned_changes':[{'path':p,'beforeSha256':b['owned'][p],'afterSha256':sha(ROOT/p)} for p in changed],'optional_interface_evolution':'未改变接口，无必要新增演进项，保留原文件。','write_boundary':'本E全部写入是上述13份现行文档及E-01自身非dispatch；总控RR3台账/生产/pins/历史任务书/系统/Git未写。'})
checks=[]
for r in cmds['commands']:
 try:
  for line in (ROOT/r['stdout']).read_text().splitlines():
   d=json.loads(line)
   if 'assertions' in d:checks.append(d)
 except json.JSONDecodeError:pass
report='''# RR3 E-01 文档实施报告

执行者：`rr3_e_impl_01`，由用户安排的外部新CLI `exec --ephemeral` 轮。没有调用collaboration spawn、没有派代理或对外发送，不虚称新线程工具spawn成功。本轮仅R05；**SELF_CHECKED，独立E验收PENDING**，不是独立PASS。

## 1. 当前交接截点

`stage_readiness=NOT_ACCEPTED`，`R06_READY=false`，`offline_gate=FAIL`，最新完成阶段 `independent_review=FAIL`（RR2/FINAL-01），RR3新审查PENDING；LIVE=BLOCKED_NOT_AUTHORIZED，release_state=NOT_IN_SCOPE。

- A-02仅shell Git查询异常拒绝补修，24/24自检绿待新独立复验；A-REVIEW-01 FAIL保留。
- B/F45已B-REVIEW-01独立PASS、无包级mustFix，不等于当前默认全16通过。
- C/F27与F46新CLI联合独立验收已运行，但本交接结论仍PENDING；F46完整160、2/2自检绿。旧C2/2漏检的最终4日志超3保留为FAIL事实。
- D-01真实非回环0/1/0/0、exit101、BLOCKED；精确系统操作只准备未执行，最终重链需新身份。
- E文档自检已完成，交另一全新独立E验收；G默认16、新FINAL均**NOT RUN**。不造最终结果路径或tested SHA。

本轮按用户指定状态截点交接。实施期间总控自有RR3矩阵/进度及外部CLI dispatch日志另有更新，记录观察而不写这些文件，不将新结果追写成当前签收。终审真实最终结果由另一新文档轮补录。

## 2. 改动前后

| 范围 | 原问题 | 本轮最小完整纠正 |
|---|---|---|
| 报告/独立审查/阻断/负测说明 | RR1/RR2尚未终审、旧candidate/uncommitted、仅ALF等历史易被误读为当前 | 前置当前块，保留原文；报告§11写最新RR2 FAIL、R04全部8和R03全部15checkpoint漂移；A/B/C-F46/D/E/G/FINAL区别明确 |
| HANDOFF | usage v5旧消费值、§6.2字段与样例不足、阶段READY旧值 | 真实wire1/event1/data_epoch1/schema7及锁文件hash；宿主ctx、gateway/credentials、消息/工具结果/opaque来源、aux/embed/rerank/query真实字段与调用片段，错误/unknown/取消/恢复/预算；accepted_tasks空、只R05 |
| 三大型进度/验收/测试账本 | 旧任务级PASS不能代当前阶段接受，I10误把408当普通取消恢复 | 保留旧条目和排序、只追加RR3元数据及引用；I10两个具名测试新C回执与408重启消解分开 |
| 性能/LIVE材料 | 旧资源PASS/“offline全部已过”与新漏检/阻断矛盾 | 原阈值不放宽；160/54binary/61owner及真实RSS/FD/log/TCP原始值、负控/清理记录自检；当前offline FAIL，LIVE/平台原许可完整保留 |
| WORKER/MODEL_USAGE语义 | 生产Noop旧值、物理请求每次一行、操作绝无session/run | 核对生产LedgerWorkerCallbackTrace、逻辑call一行及transport_attempts、embed/rerank可选可信宿主上下文，未知NULL不当0 |
| ORCHESTRATOR | 根current_task停R04，R05占位NOT_STARTED/READY | 根当前观察HEAD/R05任务状态与阶段NOT_ACCEPTED统一，旧占位另存；所有非R05stage/task及其他根key保持相等 |

共13份现行文档修改；`R05_INTERFACE_EVOLUTION.md`无需新增接口演进、未修改。权威pins/CID/leaf/scope、生产源码、原任务书、历史证据、其他包key均不改。主树已有A/B/C/F46变更保留，未清理用户或其他包文件。完整改动清单及前后hash见 [changed-files.json](changed-files.json)，原文快照见 [before.json](before.json)。

## 3. 本人实际执行的文档检查

真实命令参数、cwd、开始/结束、exit、检查输入前后hash及stdout/stderr hash均在 [commands-03.json](commands-03.json)，日志在 `checks-03/`。所有15条exit0，其中8条是工具链/HEAD只读查询，6条文档检查和1条限定文档 `git diff --check`。

'''
report+='| 命令（仓库根执行） | 实际结果 |\n|---|---|\n'
for c in checks:report+=f"| `python3 artifacts/rust-tauri/R05/RR3/E-01/verify.py {c['check']}` | exit0，{c['assertions']}项断言PASS（E自检） |\n"
report+='''| `git diff --check -- <14份所有权清单>` | exit0，只检查许可文件 |
| rustup代理rustc/cargo `--version` | exit0，1.98.1 |
| node/npm/python3 `--version`、`uname -sm` | exit0，v24.16.0/11.13.0/3.14.3，Darwin arm64 |
| `git rev-parse HEAD`及origin跟踪引用 | exit0，均b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b |

检查内容：7份JSON含重复key检查；所有新增Markdown本地链接/当前JSON消费源码和证据引用可定位，无历史链接警告；跨7份JSON current块完全相等；大型账本全部原字段/原始字节前缀和历史Markdown逐字保留；ORCHESTRATOR非R05完全相等；5份权威表摘要不变；外部dispatch原前缀保留。接口38项核对包括7个结构全部字段精确相等、真实embed/rerank签名、生产Ledger接线、正常操作样例、旧deadline注释与实际runtime区分。原始证据逐项核对包括真实哈希、完整160负载/54+61点、全部日志≤原3、owner45稳态归零、伪FD/TCP各101及恢复0、两普通取消有效回执。

### 3.1 失败与修正记录（不抹去）

[首次commands.json](commands.json)保留15条真实检查：JSON/链接/跨文档/证据/diff已exit0，history/source两条exit1。history错误是对正在由外部维护者追加的dispatch及总控台账错误要求整文件不变；改为验证dispatch开工原始字节前缀仍保留，总控外部变化只记录观察、不把它冒充“未变”。其原hash未重拍，许可的权威表仍严格等式。source检查错误是给实际使用import名称`RerankRequest`的签名写了过长限定名；读取实际签名后修正文档检查器，源码及消费字段不改。第二轮全通过，旧失败日志不覆盖。

实施脚本首次源目录遍历误先遍历大target，尚未写现行文件，停止的只是本E自己的遍历进程；改为目录层先排除target后执行，实际完成输出为13个owned改动、423输入。无生产/系统权限操作。`implement.py`为一次性实施过程记录，不应再次执行（追加键已有会拒绝），不是独立验收入口。

## 4. §6.2消费取证与生产输入绑定

[READING.json](READING.json)列继承brief/规格、RR1相关F字段、RR2完整矩阵、各必读报告/映射/探针及实际原始JSON消费方式；不冒称逐行重验几MB旧账本或所有cargo日志。[source-evidence.json](source-evidence.json)列当前源码行号/字段片段与原文件SHA。HANDOFF最小正常embedding片段来自永久具名受控测试：session/run/attempt/cause_ref真实参数，input文本+dimensions2，向量[0.25,0.75]，prompt5/total9不能猜output4；样例依赖该测试boot隔离state及loopback前置，明确不是独立可执行程序或LIVE调用。本E未重新编译/运行样例。

'''
report+=f"观察HEAD={b['head']}。范围限定摘要 `{inputs['digest']}`，423个输入逐项前后相同，见 [inputhash-before.json](inputhash-before.json) 与 [inputhash-after.json](inputhash-after.json)。算法为排序的UTF-8 `sha256  repo-relative-path\\n` 行再SHA256；范围rust除target/.git、scripts/rust-tauri除__pycache__、根rust-toolchain及共享版本。包含测试/构建，不包含文档/artifact/主树其他目录，不冒充xtask全工作树冻结摘要或最终candidateSourceBinding。真实Cargo.lock SHA `{sha(ROOT/'rust/Cargo.lock')}`；工具链和依赖锁另写HANDOFF。\n"
report+='''
RR2 FINAL第四层R02实际脚本r02_legacy_regression exit0，E0–E4.5 ALL GREEN，E5 SKIP BY SCOPE（directed-no-seal-family）；不是独立R02 verify-stage JSON，原始summary/stdout/exclusions/sinks哈希已纳入HANDOFF。RR2 FINAL正式结果：R05 5/7 FAIL顶层7/7稳定，R04 6/8 FAIL、0/8稳定（外层stdout），R03 14/15 FAIL、0/15稳定（父层stdout）；runner本身PASS不能抹去binding失败。原结果SHA保留在现行报告/HANDOFF，旧“只剩ALF”历史不修改。旧raw npm candidate1/base0、3文件6失败仍红；合法directed/E5 SKIP与严格patch-too-large登记范围保留，生成器本体未修、不能新增豁免。LIVE最迟R10及既有平台延期不扩缩；本机资源自检不等于Windows/Linux或正式四层通过。

## 5. 未执行与停止写入

没有运行cargo测试、workspace、N01–N16、verify-stage、npm test、LIVE、其他平台、系统Allow/remove/add/unblock、签名、Git提交/推送/封印或R06实现。本轮仅源码、既有原始证据、文档自检。A2、C/F46及E新独立结论仍PENDING，D BLOCKED，G/FINAL NOT RUN；必需事项留在R05，不以接口已可读推导R06_READY=true。

本轮最终只读验证及证据清单完成后，**停止全部文件写入**。正式静默窗口不写主树或artifact，后续最终真实结果由另一新文档轮补录。交新独立E验收者：先读RR3_E_BRIEF/RR3_REVIEW_BRIEF及原§6.1/6.2，再读13份现行改动、前后快照、source/inputhash/commands/REPORT；亲自验证，不以此自检代替独立PASS。总控RR3台账仍由其唯一所有者推进。
'''
(E/'REPORT.md').write_text(report)
print('Final REPORT prepared; actual checks',[(c['check'],c['assertions']) for c in checks]);print('Unchanged scoped inputs',inputs['count'],inputs['digest'])
