from pathlib import Path
import json,hashlib,datetime,subprocess,sys
P=Path(__file__).resolve().parent;R=Path.cwd();rows=[]
def check(n,v,detail=None):rows.append({'name':n,'pass':bool(v),'detail':detail})
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def load(p):return json.loads(Path(p).read_text())
# 原始统计逐项重新求值，未运行原作者检查器。
h=load('docs/rust-tauri/R05/R05_HANDOFF.json');pf=load('docs/rust-tauri/R05/R05_PERFORMANCE_RESULTS.json')['rr3_resource_selfcheck'];raw=load(pf['raw_series'])
for src,k,name in [('rssKiB','service_rss_kib','service RSS'),('fds','service_fds','service FD')]:
 vals=[x[src] for x in raw['series']];check(name,pf[k]==[min(vals),max(vals)],[min(vals),max(vals)])
for src,k in [('rssKiB','tree_rss_kib'),('fds','tree_fds')]:
 vals=[x['serviceTree'][src] for x in raw['series']];check(k,pf[k]==[min(vals),max(vals)],[min(vals),max(vals)])
for group,tree in [('series','serviceTree'),('ownerResourceSeries','processTree')]:
 check('所有'+group+'进程数值实际求和',all(sum(q['fds'] for q in x[tree]['processes'])==x[tree]['fds'] and sum(q['rssKiB'] for q in x[tree]['processes'])==x[tree]['rssKiB'] for x in raw[group]))
 check('所有'+group+'日志原上限',all(sum(f['path'].startswith('lingxi-service/logs/service-') for f in x['files']['home'])<=3 for x in raw[group]))
steady=[x for x in raw['ownerResourceSeries'] if x['phase']=='released-steady'];check('45点owner全部回零',len(steady)==45 and all(all(v in [0,[]] for v in x['owners'].values()) for x in steady))
check('15存活worker峰值',len([x for x in raw['series'] if x['phase']=='worker-live'])==15 and all(len(x['serviceTree']['processes'])==2 and x['serviceTree']['establishedTcpCount']>=2 for x in raw['series'] if x['phase']=='worker-live'))
check('15worker释放',len([x for x in raw['series'] if x['phase']=='worker-released'])==15 and all(len(x['serviceTree']['processes'])==1 and x['serviceTree']['establishedTcpCount']==0 for x in raw['series'] if x['phase']=='worker-released'))
b=Path('artifacts/rust-tauri/R05/RR2/B-R2/R2/s5-full-5/ev/legacy-entry');cs=(b/'e5-candidate-summary-counts.txt').read_text();bs=(b/'e5-base-summary-counts.txt').read_text();check('原始npm失败统计',all(x in cs for x in ['files_failed=3','tests_failed=6']),cs);check('base原始统计', 'files_failed=0' in bs and 'tests_failed=0' in bs,bs)
s=(b/'summary.txt').read_text();check('raw candidate1 base0明确红','candidate raw exit 1, base raw exit 0' in s,s.splitlines()[-1])
r02=Path(h['rr3_current']['historical_r02']['summary_ref']).read_text();check('directed真实E5 scope skip','SKIP E5' in r02 and 'DIRECTED (E0–E4.5) ALL GREEN' in r02)
for x in load('docs/rust-tauri/R05/R05_TEST_MAP.json')['rr3_I10_mapping']['ordinary_cancel_same_instance']:
 j=load(x['receipt']);summary=j['summaries'][0];check('普通取消统计核'+x['test'],all(t in summary for t in ['1 passed','0 failed','0 ignored',str(x['filtered'])+' filtered']),summary)
# 本审独立选择的实现区段和完整文件摘要，不仅消费作者摘录。
spans=[('rust/crates/lingxi-protocol/src/handshake.rs',15,40),('rust/crates/lingxi-protocol/src/lib.rs',370,396),('rust/crates/lingxi-kernel/src/model_exchange.rs',240,355),('rust/crates/lingxi-kernel/src/model_exchange.rs',754,902),('rust/crates/lingxi-kernel/src/usage.rs',380,455),('rust/crates/lingxi-kernel/src/lib.rs',30,48),('rust/crates/lingxi-adapters/src/models/credentials.rs',192,219),('rust/crates/lingxi-adapters/src/models/auxiliary.rs',166,230),('rust/crates/lingxi-service/src/operations.rs',210,250),('rust/crates/lingxi-service/src/operations.rs',973,1072),('rust/crates/lingxi-service/src/lib.rs',1260,1306),('rust/crates/lingxi-service/src/runs.rs',1110,1255),('rust/crates/lingxi-service/src/runs.rs',1480,1502),('rust/crates/lingxi-service/src/runs.rs',3540,3630),('rust/crates/lingxi-service/src/streaming_norm.rs',840,950),('rust/crates/lingxi-adapters/src/storage/run_store.rs',2696,2810),('rust/crates/lingxi-adapters/src/storage/migrations.rs',320,419),('rust/crates/lingxi-adapters/src/models/usage.rs',342,422),('rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs',962,1020)]
a=[]
for path,start,end in spans:
 lines=Path(path).read_text().splitlines();a.append({'path':path,'start':start,'end':end,'sha256':sha(path),'excerpt':'\n'.join(lines[start-1:end])})
(P/'source-audit.json').write_text(json.dumps({'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'boundary':'声明及生产接线源码取证；本E没有运行cargo/样例/LIVE','sources':a},ensure_ascii=False,indent=2)+'\n')
# 对阅读范围留完整字节快照，避免并行台账之后的变化污染本次判断。
refs=['docs/rust-tauri/R05/repair-current/'+x for x in ['RR3_E_BRIEF.md','RR3_REVIEW_BRIEF.md','RR3_BRIEF.md','RR1_MASTER_PROMPT_2026-10-04.md','RR2_MASTER_PROMPT_2026-10-06.md','RR3_ISSUE_MATRIX.json','RR3_PROGRESS.md','RR3_HANDOFF.md']]
refs+=list(load('artifacts/rust-tauri/R05/RR3/E-01/before.json')['owned'])
refs+=['artifacts/rust-tauri/R05/RR3/'+x+'/REVIEW.md' for x in ['A-REVIEW-02','C-F46-REVIEW-01']]
reading=[]
for path in refs:
 q=P/'read-snapshot'/path;q.parent.mkdir(parents=True,exist_ok=True);q.write_bytes(Path(path).read_bytes());reading.append({'path':path,'sha256':sha(path),'snapshot':str(q.relative_to(R)),'method':'brief/MASTER/Markdown全文；大型JSON完整解析，现行块及新增字段/全量历史差异详查，不重新签收旧全部用例'})
(P/'reading.json').write_text(json.dumps(reading,ensure_ascii=False,indent=2)+'\n')
(P/'supplemental-results.json').write_text(json.dumps(rows,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'checks':len(rows),'failures':[x for x in rows if not x['pass']],'sources':len(a),'reading':len(reading)},ensure_ascii=False,indent=2));sys.exit(any(not x['pass'] for x in rows))
