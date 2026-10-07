from pathlib import Path
import json,hashlib,re,sys,collections,datetime
ROOT=Path.cwd();R=ROOT/'artifacts/rust-tauri/R05/RR3';OUT=Path(__file__).resolve().parent;D=ROOT/'docs/rust-tauri/R05';checks=[]
def ck(n,v,detail=None):checks.append({'name':n,'ok':bool(v),'detail':detail})
def j(p):return json.loads(Path(p).read_text())
def sh(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
h=j(D/'R05_HANDOFF.json');raw=j(R/'C-F46-REVIEW-01/resources-01/f27-resource-series.json');p=j(D/'R05_PERFORMANCE_RESULTS.json')['rr3_resource_independent_review'];s=raw['series'];o=raw['ownerResourceSeries'];cycles=raw['cycleResults'];t=raw['thresholds'];count=lambda a:dict(collections.Counter(x['phase'] for x in a));bounds=lambda a,k:[min(x[k] for x in a),max(x[k] for x in a)]
ck('160/54/61 exact',len(cycles)==p['cycles']==160 and len(s)==p['binary_points']==54 and len(o)==p['owner_points']==61)
ck('load exactly preserved',count(cycles)=={'cancel':60,'error':60,'ok':15,'long':10,'worker':15})
ck('phase count raw vs doc',count(s)==p['binary_phases'] and count(o)==p['owner_phases'])
ck('preregistered thresholds actual',t==p['original_thresholds']=={'fdBound':400,'fdGrowthBound':64,'logFilesBound':3,'registeredBeforeRun':True,'rssBoundKiB':409600,'rssGrowthBoundKiB':153600})
ck('raw resource hash actual',sh(R/'C-F46-REVIEW-01/resources-01/f27-resource-series.json')==p['raw_sha256'])
ck('service extrema doc',p['ranges']=={k:bounds(s,k) for k in ['rssKiB','fds']})
ck('binary tree extrema doc',p['tree_ranges']=={k:bounds([x['serviceTree'] for x in s],k) for k in ['rssKiB','fds']})
ck('owner tree extrema doc',p['owner_tree_ranges']=={k:bounds([x['processTree'] for x in o],k) for k in ['rssKiB','fds','establishedTcpCount']})
for label,items,tree in [('binary',s,'serviceTree'),('owner',o,'processTree')]:
 base=items[0][tree]
 for i,x in enumerate(items):
  v=x[tree];ps=v['processes'];ck(label+' sums '+str(i),v['rssKiB']==sum(x['rssKiB'] for x in ps) and v['fds']==sum(x['fds'] for x in ps) and v['establishedTcpCount']==sum(len(x['establishedTcp']) for x in ps))
  ck(label+' absolute/growth '+str(i),v['rssKiB']<=t['rssBoundKiB'] and v['fds']<=t['fdBound'] and v['rssKiB']-base['rssKiB']<=t['rssGrowthBoundKiB'] and v['fds']-base['fds']<=t['fdGrowthBound'])
  logs=[z for z in x['files']['home'] if z['path'].startswith('lingxi-service/logs/service-') and z['path'].endswith('.log')];ck(label+' logs bound '+str(i),len(logs)<=3)
for phase,expectedProc,cmp in [('worker-live',2,lambda n:n>=2),('worker-released',1,lambda n:n==0)]:
 matches=[x for x in s if x['phase']==phase];ck('15 real '+phase,len(matches)==15 and all(len(x['serviceTree']['processes'])==expectedProc and cmp(x['serviceTree']['establishedTcpCount']) for x in matches))
steady=[x for x in o if x['phase']=='released-steady'];ck('owner45 all drained',len(steady)==45 and all(x['owners']=={'activeSessionRuns':0,'backgroundIds':[],'liveRunIds':[],'modelPermits':0,'modelWaiters':0,'toolPermits':0,'toolWaiters':0} for x in steady))
ck('no steady tmp',all(not f['path'].endswith('.tmp') for x in steady for values in x['files'].values() for f in values))
ck('budget408 not ordinary recovery',raw['load']['cancelDanglingActive']==60 and raw['load']['cancelSettled']==0 and all(x['httpStatus']==408 and x['status']=='running' and x['providerConnectionReclaimed'] for x in cycles if x['phase']=='cancel'))
ck('actual cleanup matches doc',p['cleanup']==raw['cleanup'] and all(raw['cleanup'][x] for x in ['configRemoved','equipmentStubStopped','homeRemoved','serviceStopped','workspaceRemoved']) and raw['cleanup']['lastServiceExit']==0)
for folder,filtered in [('ordinary-subagent',7),('ordinary-late',4)]:
 cmd=j(R/'C-F46-REVIEW-01'/folder/'command.json');log=(R/'C-F46-REVIEW-01'/folder/'stdout.log').read_text();ck('ordinary recovery actual '+folder,cmd['exitCode']==0 and '--exact' in cmd['command'] and f'1 passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered' in log)
neg=j(R/'C-F46-REVIEW-01/sampler-negative-isolated/result.json');ck('FD/TCP raw controls',[(x['mode'],x['exitCode']) for x in neg['rows']]==[('normal',0),('fd',101),('tcp',101),('restored',0)])
for tag in ['isolated-normal','isolated-old','isolated-restored']:
 x=j(R/'C-F46-REVIEW-01'/tag/'result.json');ck('six log runs '+tag,x.get('counts')==([1,2,3,4,4,4] if tag=='isolated-old' else [1,2,3,3,3,3]),list(x))
# 当前真正执行输入逐项比对，广义文档快照变化保留。
f=j(R/'C-F46-REVIEW-01/FINAL_SOURCE_BINDING.json');fs=f['executionInputComparison']['actualHashes'];ck('C321 every runtime input same',len(fs)==321 and all(sh(ROOT/p)==v for p,v in fs.items()))
a=j(R/'A-REVIEW-02/input-manifest.json')['files'];ck('A19 every input same',len(a)==19 and all(sh(ROOT/x['path'])==x['sha256'] for x in a))
b=j(R/'B-REVIEW-01/source-before-production.json')['files'];names=['scripts/rust-tauri/r05_t08_negative_gate.sh','scripts/rust-tauri/r05_t08_mutate_pin.py','scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py','docs/rust-tauri/R05/r05_stage_pins.tsv'];ck('B relevant four inputs same',all(sh(ROOT/n)==next(x['sha256'] for x in b if x['path']==n) for n in names))
# 真实历史逐层结果，不让runner成功覆盖来源不稳定。
for layer in h['rr3_current']['historical_layers']:
 x=j(ROOT/layer['result_ref']);cp=x['candidateSourceBinding']['checkpointAfterEveryCommand'];ck('raw historical layer '+layer['stage'],x['overall']==layer['overall']=='FAIL' and x['candidateSourceBinding']['stable']==layer['candidate_stable'] and len(cp)==layer['checkpoints'] and sum(v['stable'] for v in cp)==layer['stable_checkpoints'] and len(x['commands'])==layer['commands_total'] and sum(c['status']=='PASS' for c in x['commands'])==layer['commands_passed'] and x['runnerSourceBinding']['status']==layer['runner_status'])
scope=j(D/'R05_SCOPE_MATRIX.json')['counts'];ck('130 correct scope ownership',scope['supplemental_total']==130 and scope['share']==119 and scope['full']==6 and scope['deferred']==5 and scope['dual_stage_r05_r07']==124)
# 旧原始npm红与严格允许范围，由原始文件直接读取。
base=ROOT/'artifacts/rust-tauri/R05/RR2/B-R2/R2/s5-full-5';paths=list(base.rglob('*counts*'))+[base/'run-root/stdout.log'];saved=[]
for path in paths:
 if path.is_file():data=path.read_text();saved.append({'path':str(path.relative_to(ROOT)),'sha256':sh(path),'text':data})
ck('full original raw npm evidence read',len(saved)>1)
for item in saved:
 if 'candidate-counts' in item['path']:print(item['path'],item['text'][:300])
summary=Path(ROOT/h['rr3_current']['historical_r02']['summary_ref']).read_text();ck('directed E5 actual legal scope','ALL GREEN' in summary and 'E5' in summary and 'SKIP' in summary)
# 实际源码语义再取证，避免仅以文档字符串代替检查。
source={}
for n in ['anthropic_messages','google_generative_ai','openai_responses']:
 path=ROOT/f'rust/crates/lingxi-adapters/src/models/{n}.rs';text=path.read_text();source[str(path.relative_to(ROOT))]={'sha256':sh(path),'replay':text[text.index('fn enforce_turn_origin'):text.index('fn enforce_turn_origin')+2100]};ck(n+' enforces opaque source',all(z in source[str(path.relative_to(ROOT))]['replay'] for z in ['origin.authorizes(&route.provider, &route.model)','None => Err','no recorded serving origin']))
vers=j(ROOT/'shared/contract-versions.json');ck('real data epoch and legacy versions',vers['DATA_EPOCH']==h['data_epoch']==1 and vers['PRELOAD_API_VERSION']==h['protocol_version']['legacy_preload_api']==1 and vers['SERVER_PROTOCOL_VERSION']==h['protocol_version']['legacy_server_protocol']==1)
wire=(ROOT/'rust/crates/lingxi-protocol/src/wire.rs').read_text();ck('event schema actual1','pub const EVENT_SCHEMA_VERSION: u32 = 1;' in wire and h['protocol_version']['event_schema_version']==1)
(OUT/'source-semantic-sections.json').write_text(json.dumps(source,indent=2)+'\n');(OUT/'raw-npm-source-read.json').write_text(json.dumps(saved,ensure_ascii=False,indent=2)+'\n')
(OUT/'supplementary-results.json').write_text(json.dumps({'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'checks':checks,'passed':sum(x['ok'] for x in checks),'failed':sum(not x['ok'] for x in checks)},ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'passed':sum(x['ok'] for x in checks),'failed':[x for x in checks if not x['ok']]},ensure_ascii=False));sys.exit(1 if any(not x['ok'] for x in checks) else 0)
