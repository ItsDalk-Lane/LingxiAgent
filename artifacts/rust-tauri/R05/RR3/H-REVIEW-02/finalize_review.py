from capture import *
import shutil
commands=[]
invalid={'primary-default-build','redaction-final','acceptance-independent-analysis'}
reds={'f47-old-warn','f47-guard-red','f47-guard-red-driver','f47-root-red','f47-root-red-driver','f48-boundary-red','f48-boundary-red-driver','f48-boundary-unit-red','f46-old-six-command'}
for p in sorted((EV/'commands').glob('*/command.json')):
 d=json.loads(p.read_text());label=p.parent.name
 classification='INVALID_CACHE_PREPARATION_RETAINED' if label in invalid else 'TARGET_RED' if label in reds else 'EXECUTED'
 assert d['inputEqual'] and d['actualSourceEqual'],label
 commands.append({'label':label,'classification':classification,'record':str(p.relative_to(EV)),'recordSha256':sha(p),'exit':d['exit'],'counts':d['counts'],'leafCounts':d['leafCounts'],'binary':d['binary']})
write(EV/'COMMAND_INDEX.json',commands)
analysis=json.loads((EV/'RESOURCE_ANALYSIS-resources-final.json').read_text());assert analysis['status']=='PASS'
acceptance=json.loads((EV/'ACCEPTANCE_RECOMPUTATION.json').read_text());sampler=json.loads((EV/'sampler-negative-final/result.json').read_text());assert sampler['status']=='PASS'
required=['primary-default-build-2','redaction-final-2','logging-final','ordinary-cancel-recovery','late-result-next-run','resources-final','resources-independent-analysis','sampler-negatives','a01-final-both-levels','a13-final','live-correlation-final','f46-final-six-command','f46-explicit-error','acceptance-independent-analysis-2']
index={c['label']:c for c in commands};assert all(index[x]['exit']==0 for x in required)
for label in reds:assert index[label]['exit'] in (1,101),(label,index[label]['exit'])
rebuild=json.loads((EV/'commands/primary-default-build-2/proven-actual-rebuild.json').read_text());assert len(rebuild)==2 and all(not x['fresh'] and x['target']['src_path'].startswith(str(ROOT/'rust')) for x in rebuild)
source=snapshot();original=json.loads((EV/'commands/baseline-identity/input-before.json').read_text());assert original==source
binary=ROOT/'rust/target/debug/lingxi-service';binary_sha=sha(binary)
for label in ['resources-final','a01-final-both-levels','a13-final','live-correlation-final','f46-final-six-command','f46-explicit-error']:
 assert index[label]['binary']['sha256']==binary_sha,label
runtime=json.loads((EV/'commands/resources-final/runtime-binding.json').read_text());assert all(r['binarySha256']==binary_sha for r in runtime['rows'])
equipment=json.loads((EV/'resource-equipment-binding.json').read_text());during=json.loads((EV/'resource-equipment-during-run.json').read_text());assert any(x['sha256']==equipment['equipmentSha256'] and x['path']==equipment['equipment'] for x in during['rows'])
assert all(x['sourceHash']==equipment['equipmentSha256'] for x in sampler['rows'])
artifacts=EV/'verified-binaries';artifacts.mkdir(exist_ok=False);shutil.copy2(binary,artifacts/'lingxi-service');assert sha(artifacts/'lingxi-service')==binary_sha
write(EV/'FINAL_SOURCE_BINDING.json',{'UTC':utc(),'reviewer':'rr3_h_review_02','status':'PASS','scope':['F47','F48','affected F27/I10 resources','F46'],'mustFix':[],'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'sourceScope':'375 current Rust/contracts/toolchain plus three H-related scripts; explicitly not whole repository/active J negative-gate/docs freeze','inputHashes':source,'inputDigest':hashlib.sha256(json.dumps(source,sort_keys=True).encode()).hexdigest(),'sourceEqualDispatchToFinal':original==source,'binary':{'actualPath':str(binary),'sha256':binary_sha,'retainedCopy':str(artifacts/'lingxi-service'),'actualPrimaryRebuild':'commands/primary-default-build-2/proven-actual-rebuild.json'},'equipment':equipment,'limitations':['R05_ONLY; not G default16 or final stage PASS','macOS arm64 debug resource correctness, not release performance improvement','no LIVE, Linux/Windows, full workspace/§5.3, system permission or Git writes'],'invalidPreparationsPreserved':sorted(invalid),'cacheRecoveryReceipt':'cache-rebuild-receipt.json'})
a13=acceptance['A13'];resourcecmd=json.loads((EV/'commands/resources-final/command.json').read_text());live=json.loads((EV/'live-correlation-final/result.json').read_text());assert live['actual']==5 and live['failed']==0
report=f'''# RR3 H-REVIEW-02 独立验收

结论：**PASS，无 mustFix**。仅签 F47、F48 及受影响的 F27/I10 资源、F46；不代替 G 默认16、全 R05 或新 FINAL，R06_READY 不由本包改变。

本轮由全新独立 `rr3_h_review_02` 亲跑；未参与实施/前审、未派代理、未修改被审源码或现行文档、未操作 Git/系统许可。仅新本目录保存证据。旧 H-REVIEW-01 中断无最终结论，旧 G01 实际失败亦不被本 PASS 冲销。

## 当前对象与缓存恢复

候选 HEAD 为 `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，包含已有未提交修复。正式工具链 rustup Rust/Cargo 1.98.1、--locked/offline；实际 service SHA256 `{binary_sha}`。主 Cargo JSON 证明库与主程序均 `fresh=false`、编译来源主目录；之后完整资源、A01、A13、五类拒绝和六次日志检查的实际对象逐项同 SHA。留存副本见 `verified-binaries/lingxi-service`。

开工实物为旧 H01 隔离对象 ad4aa037…，不能使用作者 3c2c… 或旧 C/F46 hash。核旧隔离 Rust 四文件等当前，仅旧 warn 脚本不同（`interrupted-predecessor-audit.json`）。本轮首次 primary-default-build 虽 exit0/fresh=true，物理对象仍是本轮隔离编译物；随后 redaction-final 实际22/3复用此前 mutant unit，均标 **INVALID_CACHE_PREPARATION_RETAINED**，不计产品失败/通过。记录原样保留。按各自编译 UTC 精确核定本轮自产三个 Cargo 指纹，仅保存并删除48字节指纹后，用正常参数实际重编 primary-default-build-2、redaction-final-2，未触主源码字节/mtime，未清空历史缓存/删除旧二进制。见 `cache-rebuild-receipt.json`。

最终375实际相关输入从 dispatch baseline 到收尾逐项相等，hash/锁/schema/脚本全见 `FINAL_SOURCE_BINDING.json`。只声明实际执行输入静默，不把根台账、J脚本或整个仓库说成冻结。

## F47：原断言保留，真实正负控

逐字对比确认原 A01 全脚本仅新增 INFO 前置；epoch、迁移、原叶与所有旧断言未改。最终外层 warn/info 各 **17/0**；cli/env/config-file 三来源均真实退出2、epoch2拒绝、INFO选根/来源正确、原印章/数据及备用根保持、无DB/token/READY，正常启动/健康版本和清理亦通过。独立复算 `ACCEPTANCE_RECOMPUTATION.json`。

- `f47-old-warn`：原旧检查器实际 exit1、11项中1红，精确 `a01-newer-data-epoch-no-root-switch`；服务保护与实物保持成立，只缺 INFO。
- `f47-restored-green`：恢复当前检查器17/0（该步仅脚本恢复，正常二进制未改变）。
- `f47-guard-red`：只隔离改真实guard退出2→0，exit1、精确 refused 红；实际guard仍抵达且无READY。恢复后 `f47-restored-green-2` 实际重编、17/0。
- `f47-root-red`：只隔离把实际选根切fallback，stdout真READY指向fallback，日志真DB迁移及token初始化，原 refused 目标红exit1、有界回收。后续精确还原main并实编，最终主对象 warn/info原17项全绿；不是仅变更日志字符串。

用户路径继续遮盖；正式脚本使用其原短段临时目录，个人长路径保护由真实 redactor 测试验证，未为了grep泄密。

## F48：宿主编号与秘密保护

源码核实 `DiagnosticRequestId` 仅内部诊断格式包装，Debug引号/转义分割字段；调用来自最外层宿主生成并写入扩展的 RequestId，仅AUTH/TRANSPORT/WS/request handled/error-buffer诊断使用。随机生成保持 req+32hex、36字符，裸赋值47字符；未采用用户头/载荷编号，未增加req前缀/长token白名单。所有原redaction函数、40阈值、字符类（含/与=）、前缀/secret-key清单及原22测试逐字保持，见 `static-contract-audit.json`。

最终 redaction **25/0/0/336 filtered**、logging原 **9/0/0/352 filtered**。原完整A13真实 exit0：6类秘密 **{a13['actualReads']}/{a13['targetFiles']}** 实际读取、0泄漏/不可读，correlation/session/run、3个source-bound retained日志及完整inventory均通过；独立逐文件SHA/大小复算，不只读SCAN。P3真实状态计数 `{a13['p3ActualStatusCounts']}`，不把所有请求说成200。

额外真实5类拒绝：坏/缺Bearer、WS坏query、合法认证坏升级、坏Origin，分别401/401/401/400/403；5个不同随机36字符响应ID逐字匹配对应marker和文件request handled，伪X-Request-ID未采用，伪编号认证秘密不泄漏，实际TERM exit0/reap/home删除。见 `live-correlation-final/`。

仅撤隔离新引号/转义：`f48-boundary-red` 原A13的6秘密扫描仍246/41绿，随后真实correlation精确红exit1；`f48-boundary-unit-red` 三个永久形状/恶意输入/路径测试0/3红exit101。精确源还原+Cargo库/main实际fresh=false重编后 `f48-restored-green-2` 完整A13绿；当前主对象再次完整A13及25测试绿。未知编号不补造、恶意req秘密/拼接/URL/worker/大小写/Bearer/base64与注入转义/个人路径保护均由原22+新3覆盖。

## F46与资源

logging源码 SHA `495608195d8c7702707b710781a4fb1be836ad6cf5904fef8558d2c1337ca301` 保持，open→prune/轮转/明确错误/原上限3不改。当前完整service隔离副本只改旧顺序，真实六轮 `[1,2,3,4,4,4]`，exit1；精确字节恢复重编后 `[1,2,3,3,3,3]`，exit0，每次ready/TERM/reap和根删除均核验。最终主对象六轮再绿，目录型旧log引发清理失败时实际stderr-only显式错误验证通过。未给旧二进制套新hash。

原资源套件本轮 `{resourcecmd['counts'][0]}`；exit0、无忽略/零匹配。原160轮=60预算408+60错误+15正常+10长响应+15worker；54二进制点、61owner点（15真峰值、45稳态）、15存活worker。`RESOURCE_ANALYSIS-resources-final.json` 独立重算{len(analysis['checks'])}项均通过：每PID RSS/FD/TCP与树求和、绝对/增长原阈值、任务/permits/等待队列、临时文件、所有采样日志≤3/最后重启3/实际轮转和最终回收。

本轮service RSS {analysis['ranges']['rssKiB']} KiB、FD {analysis['ranges']['fds']}；完整树RSS {analysis['treeRanges']['rssKiB']} KiB、FD {analysis['treeRanges']['fds']}。原RSS400MiB/增长150MiB、FD400/增长64/日志3未放宽。共{len(analysis['knownPids'])}个实际参与PID回读已退出，最后服务exit0、reap、stub停止和home/workspace/config删除均成立。

60个408段是预算future drop后60 running经重启消解且零重执行，未冒充普通取消同实例恢复。另亲跑 `subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap`（1/0、7 filtered）和 `late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing`（1/0、4 filtered），证明原具名同实例恢复和迟到隔离，分别归属。

本次真实新资源装备 SHA `{equipment['equipmentSha256']}`；运行中抓取实际service/config/fixture/worker摘要，编译JSON/运行中/末尾装备摘要一致，不套作者50e8。装备字节副本normal0→假FD0导致101具名invalid sample→假TCP0导致101具名已知连接断言→恢复0，每轮真实1测试/1 filtered，失败控制进程组清理为空。已知FD3→6、TCP2→0、3临时文件持有→释放，以及死PID/空输出/错身份UNKNOWN拒绝均亲跑。

## 证据与停止边界

所有有效测试及目标红实际argv/cwd/UTC/exit/计数/ignored-filtered、源前后、binary/log摘要见 `COMMAND_INDEX.json` 和 `commands/`，配置/夹具/装备见运行绑定，原序列完整保存。历史完整读取摘要见 `history-full-read-audit.json`（1651文件），历史只作线索。初次历史扫描因旧序列无owner键产生的准备KeyError已记录，不冒称检查结果。

仅macOS arm64离线合成材料/真实回环/调试资源正确性；未宣称release性能收益、Linux/Windows、LIVE、完整workspace/原§5.3、默认N01–N16、G历史缺口或D入站条件已通过。没有生产修复或新的mustFix；本包可独立签PASS，完整R05仍由总控安排后续新G/E/FINAL。交付后停止写。
'''
(EV/'REVIEW.md').write_text(report)
manifest=[]
for p in sorted(EV.rglob('*')):
 if not p.is_file() or 'dispatch' in p.relative_to(EV).parts or '__pycache__' in p.parts or p.name=='manifest.json':continue
 manifest.append({'path':str(p.relative_to(EV)),'bytes':p.stat().st_size,'sha256':sha(p)})
write(EV/'manifest.json',{'UTC':utc(),'scope':'all own evidence, immutable isolated source, retained binary; excludes dispatch publication and python cache; manifest excludes itself','files':manifest})
print({'review':'PASS','mustFix':[],'commands':len(commands),'evidenceFiles':len(manifest),'binarySHA256':binary_sha,'resourcePids':len(analysis['knownPids'])})
