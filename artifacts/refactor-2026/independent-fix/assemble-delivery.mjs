import fs from 'node:fs';
import process from 'node:process';
import console from 'node:console';
import path from 'node:path';
import crypto from 'node:crypto';
import {execFileSync} from 'node:child_process';
const D='docs/refactor-2026/independent-fix', A='artifacts/refactor-2026/independent-fix';
const read=p=>JSON.parse(fs.readFileSync(p,'utf8'));const write=(p,x)=>fs.writeFileSync(p,JSON.stringify(x,null,2)+'\n');
const full=read(A+'/main/full-tests-delivery.json');
const commandIndex=A+'/main/commands.jsonl';
const commands=fs.readFileSync(commandIndex,'utf8').trim().split('\n').map(JSON.parse);
const fullCommand=commands.find(c=>c.id==='engineering-full-tests-delivery');
const suiteByPath=new Map(full.testResults.map(s=>[path.relative(process.cwd(),s.name),s]));
const testsFor=(files)=>files.flatMap(file=>(suiteByPath.get(file)?.assertionResults||[]).map(t=>({file,name:t.fullName,status:t.status})));
const fullEv=[A+'/main/engineering-full-tests-delivery.out',A+'/main/engineering-full-tests-delivery.err',A+'/main/full-tests-delivery.json'];
const regressions=[...read(D+'/F01_RESULT.json').regressions];
for(let i=9;i<=13;i++)regressions.push({id:'C'+String(i).padStart(2,'0'),status:'PASS',scope:'真实HTTP/授权/路由/解析；引擎副作用边界替身，非供应商',production_entry:['server/routes/sessions.ts /sessions/new','server/routes/sessions.ts /sessions/new-detached','server/session-create-input.ts'],tests:testsFor(['tests/session-create-input-contract.test.ts']),original_requirements:['P01-T05','P01-A07','P08-T03'],evidence:[A+'/f02/red-corrected-fixture.log',A+'/f02/green.log',...fullEv],command_index:A+'/f02/commands.jsonl',assertions:{C09:['损坏JSON与非对象body拒绝','invalid_json/invalid_body','创建/模型/元数据/历史/谱系副作用为0'],C10:['cwd/mount/folders类型','目录元素类型','memory/agent/currentAgent/thinking/project类型','错误不转换默认'],C11:['detached相同校验','permission/fork/history自身字段','拒绝在写入前'],C12:['空body/{}','字段null','目录过滤/默认','正常agent/记忆/权限'],C13:['只读账户拒绝','无授权device拒绝','合法写主体也须校验']}['C'+String(i).padStart(2,'0')]});
regressions.push(...read(A+'/f03/F03_RESULT.json').cases);
for(let i=20;i<=23;i++)regressions.push({id:'C'+i,status:'PASS',production_entry:['scripts/check-core-contracts-strict.mjs','tsconfig.core-contracts.json',...read(D+'/STRICT_SCOPE.json').checkedFiles],tests:testsFor(['tests/core-contracts-strict.test.ts']),original_requirements:['P01-T04','P01-T06','P01-A05','P01-A06','P01-A12','P08-T02','P08-A03'],evidence:[D+'/F04.md',D+'/STRICT_SCOPE.json',D+'/CROSS_REVIEW_F02_F04.md',A+'/main/final-contracts.out',A+'/f01/typed-callers-regression.log'],command_index:commandIndex,assertions:{20:['实际根入口及编译闭包','真实消费者','后台/媒体真实批次绑定受检'],21:['真实可空query','身份错用','错误attempt类型','真实终态分支缺失','生产句柄批次不能重写'],22:['新增any逐项核对','无忽略或假声明','旧动态边界精确清单','第三方unknown运行时校验'],23:['无文件全局诊断','损坏/缺失配置','缺精确include','缺源码导入','正例零诊断']}[i]});
const ext=[
['C24','FAIL',['P08-T04'],fullEv,'全量退出码1，原失败不改绿；其他工程检查分别见commands'],
['C25','BLOCKED',['P07-T04','P07-A02','P07-A07'],[A+'/f05-gui/result.json',A+'/f05-gui/GUI_REPORT.md'],'真实GUI部分通过；中文IME合成等未验证，不以粘贴替代'],
['C26','BLOCKED',['P07-T06','P07-A10'],[D+'/C26-local-soak.md',A+'/f05-soak-extended/C26_RESULT.json'],'真实浏览器与任务循环及资源窗口已采集；完整资源无界增长判定见最新扩展报告，不以短循环冒充长期结论'],
['C27','BLOCKED',['P08-T04','P08-A06'],[A+'/main/ci-head-readonly.out'],'候选未提交，审查HEAD Actions 0；无push/PR授权，无其他三平台环境'],
['C28','BLOCKED',['P04-T08','P06-T05','P06-T06','P06-A14'],['docs/refactor-2026/P04/ACCEPTANCE_MAP.json','docs/refactor-2026/P06/ACCEPTANCE_MAP.json'],'无真实供应商费用/账号操作授权；仅本地协议替身，无真实成功率'],
['C29','BLOCKED',['P08-T05','P08-A07','P08-A10'],[A+'/f05-package/result.json',A+'/f05-package/PACKAGE_REPORT.md'],'新macOS arm64 --dir包：独立server、真实桌面任务、PDF、内置浏览器、PTY通过；安装器/其他平台/终端GUI仍缺，最终源码与生成元数据绑定快照'],
['C30','BLOCKED',['P08-T05','P08-A08','P08-A09'],[D+'/C30_RESULT.json',A+'/f05-upgrade/result.json'],'源码升级/半写故障/回退14项通过；未覆盖产物激活失败和全内容逐项一致'],
['C31','PASS',['P08-T07','P08-A12'],[D+'/FIX_ACCEPTANCE_MAP.json',D+'/BLOCKED_ITEMS.md'],'124场景/68任务逐项保留原文与子断言，阶段按必需阻塞汇总'],
['C32','PASS',['P00-T01','P00-T05','P08-A13'],[D+'/SOURCE_MANIFEST.json',D+'/EVIDENCE_SHA256.txt',commandIndex],'命令绑定实际HEAD/工作区字节/Node/锁文件；原始失败保留，未复制审查摘录作为本轮运行']
];
for(const [id,status,original_requirements,evidence,note] of ext)regressions.push({id,status,original_requirements,evidence,note,command_index:commandIndex,production_entry:'见关联分包报告中的真实执行入口'});
regressions.sort((a,b)=>a.id.localeCompare(b.id));
for(const regression of regressions){if(Number(regression.id.slice(1))<=23){const files=[...new Set([...(regression.test_files||[]),...(regression.tests||[]).map(t=>t.file)])];regression.final_candidate_run={command_id:'engineering-full-tests-delivery',overall_exit_code:fullCommand.exitCode,source_manifest:fullCommand.sourceManifest,source_digest:fullCommand.sourceDigest,assertion_results:testsFor(files),evidence:fullEv,note:'条目测试结果与全量门禁分开：全量exit1不能改为exit0'};}}
const catalog=read('Lingxi_Refactor_Taskbooks_2026-09-21/acceptance-catalog.json');
const extra={
'P00-A09':['tests/log-redactor.test.ts','tests/model-call-payload-redaction.test.ts'],
'P07-A04':['tests/history-read-directory-cache.test.ts','tests/history-pagination-run-continuity.test.ts'],
'P07-A06':['desktop/src/react/__tests__/chat-performance/p07-stream-buffer-wallclock.test.ts'],
'P07-A08':['tests/history-read-directory-cache.test.ts'],
'P01-A07':['tests/session-create-input-contract.test.ts'],
'P02-A02':['tests/f01-task-write-contract.test.ts'],'P02-A03':['tests/f01-task-write-contract.test.ts'],'P02-A11':['tests/f01-task-write-contract.test.ts'],
'P05-A01':['tests/history-run-outcome-edges.test.ts','tests/conversation-export.test.ts'],
'P05-A03':['tests/stream-route-consumer-isolation.test.ts'],'P05-A04':['tests/stream-route-consumer-isolation.test.ts','artifacts/refactor-2026/P05/logs/P05-ACCEPTANCE-counterexample.test.ts'],
'P08-A03':['tests/core-contracts-strict.test.ts'],'P08-A02':['tests/history-protocol-compat.test.ts','tests/stream-route-consumer-isolation.test.ts']};
const override={
'P05-A14':['BLOCKED','协调器顺序/错误保护单测通过，但缺迁移中实际kill进程后恢复的当前候选证明；源码半写故障不是该场景。'],
'P07-A03':['BLOCKED','懒加载并发/失败/重试当前测试通过；没有证明正在初始化时取消的传播结果。'],
'P07-A06':['BLOCKED','墙钟合并窗口正文/文件/结束通过；未在同一尾窗口故障场景证明错误与附件共同保留。'],
'P00-A01':['PASS','入场工作区干净；未reset/clean，旧审计测试改写文件有备份并按原字节恢复。合成用户未提交夹具场景未另造，实际工作区保护有日志。'],
'P00-A02':['PASS','实际HEAD等于审查HEAD；研究基线到当前差异保留原P00清单，本轮按实际源码定位。'],
'P00-A03':['FAIL','业务数据均隔离；首次Electron缓存因通用data末级映射到既有用户Application Support/Data，未证明哨兵完整不变。后续唯一目录已验证并清理，不将初次隔离缺口隐去。'],
'P00-A04':['PASS','范围维持原P00排除项，本轮没有重引入旧专用调度或本地模型管理。'],
'P00-A05':['PASS','先红后绿及全量exit1原始保存；环境/授权不足单列BLOCKED。'],
'P00-A06':['PASS','本轮交付检查覆盖编号完整、必需状态向上汇总及证据引用；旧evidence-check另真实执行。'],
'P00-A08':['PASS','比较器同输入/少样本假优化selftest本轮重跑，拒绝不等价工作量；无当前性能收益声明。'],
'P00-A09':['PASS','当前脱敏契约测试与合成fixture；没有导出真实凭证或原始真实用户内容。'],
'P00-A10':['PASS','无关UI优化未实施；资源/隔离异常及审计失败显式保留。'],
'P06-A03':['BLOCKED','确定性参数/工具链通过；真实模型选择与按需发现行为没有费用授权。'],
'P06-A04':['BLOCKED','授权对象契约通过；真实模型语义选择未获授权验证。'],
'P06-A05':['BLOCKED','协议拒绝和权限契约通过；真实模型澄清行为未获授权验证。'],
'P06-A07':['BLOCKED','固定危险动作拒绝断言通过；真实模型抗注入行为未获授权验证。'],
'P06-A12':['PASS','真实模型验证明确BLOCKED，不填成功率。'],
'P06-A13':['BLOCKED','安全错误结构契约通过；真实模型纠错行为未获授权验证。'],
'P06-A14':['BLOCKED','真实旧新配对留出集未获费用/账号授权；没有以固定协议结果冒充。'],
'P07-A01':['BLOCKED','原配对基准作为历史，未把旧数字作为候选收益；新GUI尚缺IME完整性能/正确性证据。'],
'P07-A02':['BLOCKED','真实Electron及新包接收任务完成；未故意延迟可选初始化并立即提交，不能外推原场景完整PASS。'],
'P07-A04':['BLOCKED','当前目录/分页内容正确性通过；没有重跑当前候选首建与热分页分别测量报告，旧时间仅保留历史。'],
'P07-A07':['BLOCKED','持续流真实复制/选中/滚动/展开完成；真实中文IME合成仍缺证据。'],
'P07-A08':['NOT_APPLICABLE','本轮没有新增CPU worker，原CPU_OFFLOAD_DECISION明确无热点不引入worker；目录缓存不作为worker取消/崩溃证明。既有宿主工作进程和全功能验证另归P08-A04。'],
'P07-A09':['PASS','未因网络等待引入worker，无网络提速承诺；无关性能优化未执行。'],
'P07-A10':['BLOCKED','真实浏览器/任务/取消循环已执行；时长、RSS趋势和缓存上限分别报告，完整无界增长排除仍有限。'],
'P07-A11':['BLOCKED','比较器selftest拒绝减少工作量假优化，安全套件保留；没有运行关闭安全校验的故障注入性能分支，不能替代原场景。'],
'P07-A12':['BLOCKED','完整IME交互和长期资源结论未闭合，不能据局部快项判整个性能验收通过。'],
'P08-A02':['PASS','原全量8条旧协议因旧树缺席跳过；随后独立旧锁安装和两文件8/8 HTTP/实际消费者补验通过，另有本轮原权限/旧字段兼容回归；不等于完整旧GUI。'],
'P08-A01':['PASS','旧不安全写入口/缺省吞错/it.fails已退出；实际入口及两类边界检查重新执行。'],
'P08-A04':['BLOCKED','功能分层证据保留；真实GUI、账户、跨平台等缺项不合并为全功能PASS。'],
'P08-A05':['BLOCKED','源级升级探针对权限/会话做有限读取；未对所有设置组合在候选安装产物重启逐项验收。'],
'P08-A06':['BLOCKED','无候选四平台CI，无push/PR授权。'],
'P08-A07':['BLOCKED','独立新.app及server无开发依赖运行通过；正式安装器与全部目标平台未覆盖。'],
'P08-A08':['BLOCKED','源级升级已跑，未完成产物升级及资源/观测/设置全字段比较。'],
'P08-A09':['BLOCKED','源级半写/旧代码回退通过，非真实安装产物激活失败注入。'],
'P08-A10':['BLOCKED','新包无token拒绝通过；完整普通/远程越权副作用矩阵仍为源码层，不能算包内全覆盖。'],
'P08-A11':['PASS','无提交/push/PR/tag/发布/真实模型费用；一次性测试密钥销毁，未用生产签名。'],
'P08-A12':['PASS','总体BLOCKED且保留FAIL，未写全部完成。'],
'P08-A13':['FAIL','新持久化指纹已生成，历史原始日志保留；四个旧审计坐标失败尚未闭合，不改封印/白名单。'],
'P08-A14':['PASS','F01/F02/F03真实复现后修复；F02/F04跨包独立审阅另捕获两缺口并修复，范围与限制均保留。']};
const scenarios=[];
for(const p of catalog.phases){const h=read('docs/refactor-2026/'+p.id+'/ACCEPTANCE_MAP.json');const hist=h.cases||h.acceptance_cases;for(const a of p.acceptance_cases){const old=hist.find(x=>x.id===a.id)||{};const files=[...new Set([...((JSON.stringify([old.test_path,old.supporting])||'').match(/(?:tests|desktop\/src\/react\/__tests__|artifacts)[\w/.-]+\.test\.[cm]?[jt]sx?/g)||[]),...(extra[a.id]||[])])];const tests=testsFor(files);const hasFailed=tests.some(t=>t.status==='failed');let status=hasFailed?'FAIL':tests.length&&tests.every(t=>t.status==='passed')?'PASS':'BLOCKED';let reason=tests.length?'当前完整运行中的实际用例结果；只证明下列测试层级，不推广为GUI/真实模型/跨平台。':'该历史手工/基准证据未绑定当前候选，不复制旧PASS；相关有限证据或缺项见修复条目。';if(override[a.id]) [status,reason]=override[a.id];const linked=regressions.filter(r=>(r.original_requirements||r.taskbook||[]).includes(a.id)).map(r=>r.id);const assertions=a.expect.split(/[；，]/).map(x=>x.trim()).filter(Boolean).map((text,i)=>({id:a.id+'.'+(i+1),text,status,evidence:tests.length?fullEv:[D+'/FIX_REPORT.md'],limitations:reason}));scenarios.push({id:a.id,name:a.name,setup:a.setup,action:a.action,expect:a.expect,status,reason,mandatory_assertions:[{id:a.id+'.setup',kind:'required_setup',text:a.setup,status,evidence:tests.length?fullEv:[D+'/FIX_REPORT.md'],limitations:reason},{id:a.id+'.action',kind:'required_action',text:a.action,status,evidence:tests.length?fullEv:[D+'/FIX_REPORT.md'],limitations:reason},...assertions],repair_regressions:linked,production_entry:old.production_entry||old.execution||'见阶段原始调用点清单与本轮报告',historical_reference:{path:'docs/refactor-2026/'+p.id+'/ACCEPTANCE_MAP.json',status:old.status,test_names:old.test_names||old.test_name||null,not_current_execution:true},current_tests:tests,evidence:tests.length?fullEv:[D+'/FIX_REPORT.md']});}}
for(const scene of scenarios){if(scene.id==='P08-A02'){scene.independent_current_runs=[{evidence:A+'/history-compat-final/result.json',command_index:A+'/history-compat-final/commands.jsonl',tests_passed:8,skipped:0,exit_code:0}];scene.evidence.push(A+'/history-compat-final/compat-eight.log');} }
const aggregate=statuses=>statuses.includes('FAIL')?'FAIL':statuses.includes('BLOCKED')?'BLOCKED':'PASS';
const phases=catalog.phases.map(p=>({id:p.id,original_gate:p.gate,status:['P04','P06','P07','P08'].includes(p.id)?'BLOCKED':aggregate(scenarios.filter(a=>a.id.startsWith(p.id)).map(a=>a.status)),cases:scenarios.filter(a=>a.id.startsWith(p.id)).map(a=>({id:a.id,status:a.status})),tasks:p.tasks.map(t=>({id:t.id,title:t.name||t.title,status:'MAPPED',original_taskbook:'Lingxi_Refactor_Taskbooks_2026-09-21/'+p.file,repair_regressions:regressions.filter(r=>(r.original_requirements||r.taskbook||[]).includes(t.id)).map(r=>r.id),acceptance_cases:scenarios.filter(a=>a.id.startsWith(p.id)).map(a=>a.id),note:'本轮只补F01—F05差异；任务完结资格从阶段全部必需场景和gate汇总，不自动继承历史完成'}))}));
const taskCases={"P00":[[1,2,3],[4,10],[7],[6,9],[5],[8],[10],[1,2,3,4,5,6,7,8,9,10]],"P01":[[4,8],[2,3],[1,10],[5,6,9],[7,8],[3,4,12],[1,2,11]],"P02":[[1,3],[2,13,14],[4,5,6,12],[7,8],[9,10,11,15],[1,7,8],[13,14],[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]],"P03":[[1,8],[2,9],[1,3,4,5],[5,6,7,10],[8,11,15],[12,13,14],[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15],[9,10,15]],"P04":[[15],[1,2,3,4,5],[5,6,7],[6,8,9],[10,11,12],[13,14],[15,16],[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]],"P05":[[1,2,9],[2,13],[3,4,5,6],[7,8,9,16],[10,11,12],[14,15],[1,3,4,7],[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]],"P06":[[1],[2,3],[8,11],[6,9,10],[4,5,7,12,13,14],[13,14],[1,2,3,4,5,6,7,8,9,10,11,12,13,14]],"P07":[[1,9],[2,3],[4,5],[6,7],[8,9],[10],[1,11,12]],"P08":[[1,2],[3],[4,5],[6,13],[7,8,9,10],[14],[11,12,13]]};
for(const phase of phases)for(const [i,task] of phase.tasks.entries()){task.acceptance_cases=taskCases[phase.id][i].map(n=>phase.id+'-A'+String(n).padStart(2,'0'));const states=task.acceptance_cases.map(id=>scenarios.find(a=>a.id===id).status);states.push(...regressions.filter(r=>(r.original_requirements||r.taskbook||[]).includes(task.id)).map(r=>r.status));task.status=aggregate(states);task.status_scope='对应原场景与本轮回归的验收汇总，非声称重做历史实现';}
write(D+'/FIX_ACCEPTANCE_MAP.json',{schema_version:1,generated_at:new Date().toISOString(),candidate:'未提交工作区，见SOURCE_MANIFEST.json',overall_status:'BLOCKED',status_rule:'场景逐条报告；必需FAIL/BLOCKED禁止阶段整体完成。阶段BLOCKED可包含已经明确的FAIL，详见场景。setup/action列为必需条件，expect分号/逗号子断言保留；已知局部证据不满足原动作的条目降为BLOCKED，并列出缺失。',catalog:'Lingxi_Refactor_Taskbooks_2026-09-21/acceptance-catalog.json',regressions,phases,scenarios});
const failed=full.testResults.flatMap(s=>s.assertionResults.filter(t=>t.status==='failed').map(t=>({file:path.relative(process.cwd(),s.name),name:t.fullName,messages:t.failureMessages})));
write(D+'/FIX_RESULT.json',{schema_version:1,overall_status:'BLOCKED',engineering_gate:'FAIL',implementation_status:{F01:'PASS',F02:'PASS',F03:'PASS',F04:'PASS',F05:'BLOCKED'},observation_O01:{status:'RESOLVED',finding:'TaskRegistry是best-effort运行可见性，原持久成功反馈缺口成立；新增显式durable诊断。DeferredResultStore权威持久失败原有保护复验通过，不将注册表失败推导为外部动作必需重试。',evidence:D+'/F01_RESULT.json'},start:{branch:'docs/knowledge-closeout-2026-09-21',head:'b0e9118427e16206dcd25ecfb566d766ed476727',tree:'b43a2a1528c5d8f91884aae1e57ae5bd92788b76',dirty:false},end:{head:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),tree:execFileSync('git',['rev-parse','HEAD^{tree}'],{encoding:'utf8'}).trim(),dirty:true,source_manifest:D+'/SOURCE_MANIFEST.json'},environment:{node:process.version,platform:process.platform,arch:process.arch,lock_sha256:crypto.createHash('sha256').update(fs.readFileSync('package-lock.json')).digest('hex'),dependency_evidence:A+'/main/lock-installed-versions.out'},full_test:{command:fullCommand.command,exit_code:fullCommand.exitCode,files:full.testResults.length,passed:full.numPassedTests,failed:full.numFailedTests,skipped:full.numPendingTests,total:full.numTotalTests,failed_assertions:failed,evidence:fullEv,scope:'本次真实运行，不累计各轮，不把失败或expected failure当通过'},regressions,command_indexes:[commandIndex,A+'/f01/commands.jsonl',A+'/f02/commands.jsonl',A+'/f03/commands.jsonl',A+'/f05-upgrade/commands.jsonl'],forbidden_actions_performed:[],limitations:['真实GUI/长期资源/四平台/真实供应商/安装和升级仍见BLOCKED_ITEMS','首次GUI缓存隔离缺口与现存Data目录未删除，见GUI_REPORT','最终候选包以最后PACKAGE_REPORT与source-snapshot为准，中间快照不外推','本轮未提交：HEAD坐标不能单独代表修复字节，以清单为准'],not_reproduced_findings:[]});
console.log(JSON.stringify({regressions:regressions.length,scenarios:scenarios.length,tasks:phases.flatMap(p=>p.tasks).length,failed:failed.length}));
