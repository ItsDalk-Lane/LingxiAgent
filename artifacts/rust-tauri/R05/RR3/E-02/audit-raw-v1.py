from capture import *
import re,collections
checks=[]
def check(ok,label):
 checks.append({'check':label,'ok':bool(ok)})
 if not ok:raise AssertionError(label)
a=ROOT/R/'A-REVIEW-02';cf=ROOT/R/'C-F46-REVIEW-01';d=ROOT/R/'D-REVIEW-01'
ain=load(a/'input-manifest.json');adel=load(a/'delivery-inputs.json');check(len(ain['files'])==len(adel['files'])==19,'A19 bound input count')
for x in ain['files']:check(sha(ROOT/x['path'])==x['sha256'],'A current input '+x['path'])
counts=load(a/'verified-counts.json');check(counts['validators-old-red']['passed']==8 and counts['validators-old-red']['faultPassed']==0,'A original target red retained');check(counts['validators-restored-green']['passed']==24,'A corrected24')
acmd=load(a/'commands.json');check(len(acmd)==639,'A639 actual records not tests')
for x in acmd:
 if 'log' in x and 'logSha256' in x:check(sha(a/x['log'])==x['logSha256'],'A command actual log '+x['name'])
raw=load(cf/'resources-01/f27-resource-series.json');receipt=load(cf/'resources-01/command.json');analysis=load(cf/'RESOURCE_ANALYSIS.json');check(receipt['exitCode']==0 and '2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 337.41s' in receipt['counts'][0],'C actual raw test summary');check(receipt['outputHash']==sha(cf/'resources-01/stdout.log'),'C command actual stdout SHA')
check(len(raw['cycleResults'])==160 and len(raw['series'])==54 and len(raw['ownerResourceSeries'])==61,'C all160/54/61 raw records')
check(collections.Counter(x['phase'] for x in raw['cycleResults'])=={'cancel':60,'error':60,'ok':15,'long':10,'worker':15},'original load distribution')
check(raw['thresholds']==load(ROOT/R/'F46-01/resources-01/f27-resource-series.json')['thresholds'],'C thresholds unchanged original')
for x in raw['series']+raw['ownerResourceSeries']:
 tree=x['serviceTree']
 for field in ['rssKiB','fds']:check(tree[field]==sum(p[field] for p in tree['processes']),'each PID '+field+' sum')
 check(tree['establishedTcpCount']==sum(len(p['establishedTcp']) for p in tree['processes']),'each PID TCP sum')
 logs=[z for z in x['files']['home'] if re.fullmatch(r'lingxi-service/logs/service-\d+\.log',z['path'])];check(len(logs)<=3,'every phase original log threshold3')
 threshold=raw['thresholds'];check(tree['rssKiB']<=threshold['rssBoundKiB'] and tree['fds']<=threshold['fdBound'],'every phase resource absolute bound')
steady=[x for x in raw['ownerResourceSeries'] if x['phase']=='released-steady'];check(len(steady)==45 and all(all(v==0 or v==[] for v in x['owners'].values()) for x in steady),'owner all45 active/permit/waiter/ids released')
live=[x for x in raw['series'] if x['phase']=='worker-live'];released=[x for x in raw['series'] if x['phase']=='worker-released'];check(len(live)==len(released)==15 and all(len(x['serviceTree']['processes'])==2 and x['serviceTree']['establishedTcpCount']>=2 for x in live),'all15 workers peak observed');check(all(len(x['serviceTree']['processes'])==1 and x['serviceTree']['establishedTcpCount']==0 for x in released),'all15 workers release observed')
check(raw['load']['cancelSettled']==0 and raw['load']['cancelDanglingActive']==60,'408 budget restart separate ordinary cancel')
neg=load(cf/'sampler-negative-isolated/result.json');check([x['exitCode'] for x in neg['rows']]==[0,101,101,0] and all(x['targetNamed'] for x in neg['rows']),'C normal→FD/TCP target red→restored')
for folder,filtered in [('ordinary-subagent',7),('ordinary-late',4)]:
 x=load(cf/folder/'command.json');check(x['exitCode']==0 and f'1 passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out' in x['counts'][0],'new ordinary cancel actual receipt '+folder)
for field in ['rssKiB','fds']:check([min(x[field] for x in raw['series']),max(x[field] for x in raw['series'])]==analysis['ranges'][field],'C original ranges recalculated '+field)
binding=load(cf/'FINAL_SOURCE_BINDING.json');runtime=binding['executionInputComparison']['actualHashes'];check(len(runtime)==321,'C321 execution inputs')
for path,v in runtime.items():check(sha(ROOT/path)==v,'C execution source '+path)
for x in ['inputs-before-build.json','inputs-before-run.json','inputs-after-run.json']:
 # 只消费真实记录文件名。
 p=d/x
 if p.exists():
  data=load(p)
  for path,v in data['manifest'].items():
   if v['kind']=='file':check(sha(ROOT/path)==v['sha256'],'D source '+path)
dr=load(d/'r00-formal-01.json');check(dr['actualExitCode']==101 and not dr['supervisorTimedOut'],'D real gate101 no supervisor timeout')
log=(d/'r00-formal-01.log').read_text();check('0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out' in log,'D raw0/1/0/0 counts')
obj=load(d/'prepared-object.json');check(obj['sha256']=='9f7489029c91d1c232e3c204236854bd53c69cee2fef33d6fd778a27f44696d3' and obj['cdhash']=='d9676388f524872f1269c13f61f413458c0e9e52' and not obj['systemChangesExecuted'],'D current exact identity no system changes')
# E-01完整冻结清单亲读，旧FAIL、报告与脚本不覆盖。
mp=ROOT/R/'E-01/manifest.json';entries=load(mp)['files'];verified=[]
for x in entries:
 p=mp.parent/x['path'];check(sha(p)==x['sha256'] and p.stat().st_size==x['bytes'],'E01 immutable evidence '+x['path']);verified.append(x)
dump(E/'E01-manifest-consumption.json',{'manifest':str(mp.relative_to(ROOT)),'sha256':sha(mp),'count':len(verified),'files':verified})
# 保存本轮实际读的完整源文件摘要与重要区段，非只按字符串出现次数推断接口。
h=load(D/'R05_HANDOFF.json');srcs=sorted({s.split('::',1)[0] for s in _strings(h['consumer_contract'])}) if False else []
srcs=['rust/crates/lingxi-service/src/workerrpc.rs','rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs','rust/crates/lingxi-kernel/src/model_exchange.rs','rust/crates/lingxi-kernel/src/lib.rs','rust/crates/lingxi-kernel/src/ports.rs','rust/crates/lingxi-kernel/src/usage.rs','rust/crates/lingxi-adapters/src/models/credentials.rs','rust/crates/lingxi-adapters/src/models/auxiliary.rs','rust/crates/lingxi-adapters/src/models/operations/embedding.rs','rust/crates/lingxi-service/src/operations.rs','rust/crates/lingxi-service/src/lib.rs','rust/crates/lingxi-service/src/runs.rs','rust/crates/lingxi-service/src/workermodel.rs','rust/crates/lingxi-adapters/src/storage/migrations.rs','rust/crates/lingxi-adapters/src/storage/run_store.rs','rust/crates/lingxi-protocol/src/handshake.rs','rust/crates/lingxi-protocol/src/wire.rs','rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs']
source_records={p:{'sha256':sha(ROOT/p),'bytes':(ROOT/p).stat().st_size} for p in srcs}
segments=[]
for p,ranges in [('rust/crates/lingxi-service/src/workerrpc.rs',[(11,24),(529,541),(1285,1302),(1320,1361),(1388,1418),(1515,1541)]),('rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs',[(149,158)])]:
 lines=(ROOT/p).read_text().splitlines();segments+=[{'path':p,'startLine':a,'endLine':b,'content':'\n'.join(lines[a-1:b])} for a,b in ranges]
dump(E/'source-audit.json',{'utc':now(),'files':source_records,'worker_segments':segments,'boundary':'源码字段/声明/真实分流/fixture与原始运行消费自检；未重新Cargo或启动worker。'})
dump(E/'raw-audit.json',{'utc':now(),'checks':checks,'status':'SELF_CHECKED','assertions':len(checks),'boundary':'亲读独立报告对应完整manifest全部原始文件、逐记录解析/摘要核对，不冒称本E重新执行A/C/D或独立PASS。'})
print(json.dumps({'assertions':len(checks),'status':'SELF_CHECKED','E01_files':len(verified)},ensure_ascii=False))
