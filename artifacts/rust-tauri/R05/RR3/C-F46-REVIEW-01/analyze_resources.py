from review_capture import *
from collections import Counter
p=EV/'resources-01/f27-resource-series.json'
if not p.exists():
 p=next((EV/'tmp').glob('lingxi-r05f27-f27-series-*/f27-resource-series.json'))
 shutil.copy2(p,EV/'resources-01/f27-resource-series.json');p=EV/'resources-01/f27-resource-series.json'
d=json.loads(p.read_text());series=d['series'];owners=d['ownerResourceSeries'];checks={}
checks['counts']=len(series)==54 and len(owners)==61 and len(d['cycleResults'])==160
checks['loadDistribution']=dict(Counter(r['phase'] for r in d['cycleResults']))=={'cancel':60,'error':60,'ok':15,'long':10,'worker':15}
checks['originalThresholds']=d['thresholds']=={'rssBoundKiB':409600,'rssGrowthBoundKiB':153600,'fdBound':400,'fdGrowthBound':64,'logFilesBound':3,'registeredBeforeRun':True}
checks['allBinaryAbsoluteBounds']=all(0<r['rssKiB']<409600 and 3<=r['fds']<400 and 0<r['serviceTree']['rssKiB']<409600 and 3<=r['serviceTree']['fds']<400 for r in series)
checks['growth']=series[-2]['rssKiB']-series[0]['rssKiB']<153600 and series[-2]['fds']-series[0]['fds']<=64
checks['allTreeGrowthFromBaseline']=all(r['serviceTree']['rssKiB']-series[0]['serviceTree']['rssKiB']<153600 and r['serviceTree']['fds']-series[0]['serviceTree']['fds']<=64 for r in series)
checks['ownerAbsoluteAndGrowthBounds']=all(0<r['processTree']['rssKiB']<409600 and 3<=r['processTree']['fds']<400 and r['processTree']['rssKiB']-owners[0]['processTree']['rssKiB']<153600 and r['processTree']['fds']-owners[0]['processTree']['fds']<=64 for r in owners)
checks['everyOwnerTreeSum']=all(sum(x['rssKiB'] for x in r['processTree']['processes'])==r['processTree']['rssKiB'] and sum(x['fds'] for x in r['processTree']['processes'])==r['processTree']['fds'] and sum(len(x['establishedTcp']) for x in r['processTree']['processes'])==r['processTree']['establishedTcpCount'] for r in owners)
checks['everyTreeSum']=all(sum(x['rssKiB'] for x in r['serviceTree']['processes'])==r['serviceTree']['rssKiB'] and sum(x['fds'] for x in r['serviceTree']['processes'])==r['serviceTree']['fds'] and sum(len(x['establishedTcp']) for x in r['serviceTree']['processes'])==r['serviceTree']['establishedTcpCount'] for r in series)
live=[r for r in series if r['phase']=='worker-live']; released=[r for r in series if r['phase']=='worker-released'];peaks=[r for r in owners if r['phase']=='worker-and-queued-live'];steady=[r for r in owners if r['phase']=='released-steady']
checks['binary15LiveAndReleased']=len(live)==len(released)==15 and all(len(r['serviceTree']['processes'])==2 and r['serviceTree']['establishedTcpCount']>=2 for r in live) and all(len(r['serviceTree']['processes'])==1 and r['serviceTree']['establishedTcpCount']==0 for r in released)
checks['owner15RealPeaks']=len(peaks)==15 and all(r['owners']['activeSessionRuns']==2 and len(r['owners']['liveRunIds'])==2 and r['owners']['modelPermits']==r['owners']['modelWaiters']==r['owners']['toolPermits']==1 and len(r['processTree']['processes'])==2 for r in peaks)
checks['owner45Steady']=len(steady)==45 and all(all(r['owners'][k]==0 for k in ('activeSessionRuns','modelPermits','modelWaiters','toolPermits','toolWaiters')) and r['owners']['liveRunIds']==r['owners']['backgroundIds']==[] and len(r['processTree']['processes'])==1 and r['processTree']['establishedTcpCount']==0 for r in steady)
def logs(r):return [x for x in r['files']['home'] if x['path'].startswith('lingxi-service/logs/service-') and x['path'].endswith('.log')]
checks['allLogsBound']=all(len(logs(r))<=3 for r in series+owners)
checks['finalRestartLogs']=len(logs(series[-1]))==3
checks['realWritesAndRotations']=max(int(x['path'].split('service-')[-1].split('.')[0]) for r in series for x in logs(r))>=5 and sum(x['bytes'] for x in logs(series[-1]))>65536
checks['noTemporaryFilesSteady']=all(not x['path'].endswith('.tmp') for r in released+steady for bucket in r['files'].values() for x in bucket)
checks['cancelConnections']=d['environment']['stubDroppedConnections']==60 and all(r['providerConnectionReclaimed'] and r['httpStatus']==408 for r in d['cycleResults'] if r['phase']=='cancel')
checks['budgetVsOrdinaryHonest']=d['load']['cancelDanglingActive']==60 and d['load']['cancelSettled']==0
cleanup=d['cleanup'];checks['actualFinalReapAndRootsRemoved']=all(cleanup[k] for k in ('serviceStopped','homeRemoved','workspaceRemoved','configRemoved','equipmentStubStopped')) and cleanup['lastServiceExit']==0
pids=sorted({x['pid'] for r in series for x in r['serviceTree']['processes']}|{x['pid'] for r in owners for x in r['processTree']['processes']})
now=subprocess.run(['ps','-axo','pid=,ppid=,comm='],capture_output=True,text=True);alive={int(l.split()[0]) for l in now.stdout.splitlines() if l.split()}; checks['participatingPidsGone']=now.returncode==0 and not set(pids)&alive
result={'utc':utc(),'rawSha256':sha(p),'checks':checks,'status':'PASS' if all(checks.values()) else 'FAIL','seriesPhaseCounts':dict(Counter(r['phase'] for r in series)),'ownerPhaseCounts':dict(Counter(r['phase'] for r in owners)),'cyclePhaseCounts':dict(Counter(r['phase'] for r in d['cycleResults'])),'ranges':{k:[min(r[k] for r in series),max(r[k] for r in series)] for k in ('rssKiB','fds')},'treeRanges':{k:[min(r['serviceTree'][k] for r in series),max(r['serviceTree'][k] for r in series)] for k in ('rssKiB','fds')},'ownerTreeRanges':{k:[min(r['processTree'][k] for r in owners),max(r['processTree'][k] for r in owners)] for k in ('rssKiB','fds','establishedTcpCount')},'logCounts':[len(logs(r)) for r in series], 'ownerLogCounts':[len(logs(r)) for r in owners],'postRestartLogs':logs(series[-1]),'knownPids':pids,'pidReadbackCommand':['ps','-axo','pid=,ppid=,comm='],'pidReadbackExitCode':now.returncode,'remainingPids':sorted(set(pids)&alive),'cleanup':cleanup,'boundary':'independent full raw analysis; binary tree and in-process owners kept separate; no second resource management authority'}
(EV/'pid-readback.log').write_text(now.stdout);(EV/'RESOURCE_ANALYSIS.json').write_text(json.dumps(result,ensure_ascii=False,indent=2));print(json.dumps({k:v for k,v in result.items() if k in ('status','checks','ranges','treeRanges')},ensure_ascii=False))
