from review_support import *
import re
b=ROOT/'artifacts/rust-tauri/R05/RR3/I-01'
m=json.loads((b/'manifest.json').read_text());diff=[]
for rel,item in m['files'].items():
 p=b/rel
 if not p.is_file() or sha(p)!=item['sha256'] or p.stat().st_size!=item['bytes']:diff.append(rel)
# 全量校验历史 manifest；原log全部读取，关键语义另逐项复算。
logs=[p for p in b.rglob('*') if p.is_file() and (p.suffix=='.log' or p.name in ['commands.json','result.json','command.json']) and 'own-target' not in str(p)]
readlog={str(p.relative_to(b)):{'sha256':sha(p),'bytes':p.stat().st_size} for p in logs}
old=json.loads((b/'old-red-receipt.json').read_text());k='rust/crates/lingxi-kernel/src/lib.rs'
assert old['exitCode']==1 and old['pristineSha256']==sha(b/'old-red-pristine'/k) and old['afterOriginalResetSha256']==sha(b/'old-red-copy'/k)
checks=json.loads((b/'selfcheck-final/result.json').read_text());assert checks['checks']==56 and checks['failed']==0
sync=[]
for name in ['sync-production-normal','sync-production-midbyte','sync-production-restored']:
 r=json.loads((b/'selfcheck-final'/name/'verify-stage-result.json').read_text());assert all(x['exitCode']==0 and x['status']=='PASS' for x in r['commands'])
 assert r['runnerSourceBinding']['status']=='PASS'
 cmd=next(x for x in checks['results'] if x['check']==name+'-wait')
 observed=datetime.datetime.fromisoformat(cmd['endUTC']).timestamp()*1000
 assert r['candidateSourceBinding']['before']['atUnixMs']<observed
 expected=(name!='sync-production-midbyte');assert r['candidateSourceBinding']['stable']==expected and r['overall']==('PASS' if expected else 'FAIL')
 if not expected:
  app=next(x for x in checks['results'] if x['check']==name+'-append');assert cmd['endUTC']<=app['startUTC']
 sync.append({'name':name,'overall':r['overall'],'stable':expected,'checkpoints':len(r['candidateSourceBinding']['checkpointAfterEveryCommand']),'resultSha256':sha(b/'selfcheck-final'/name/'verify-stage-result.json')})
(g:=ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-01')
drift=json.loads((g/'source-drift-default-failure.json').read_text());assert drift['defaultExit']==2 and drift['actualCaseRows']==15 and not drift['N16reuseExecuted']
console=(g/'default-console.log').read_text();assert 'line 598' in console and 'syntax error' in console
assert len((g/'default16-01/case-results.tsv').read_text().splitlines())==15
copy=Path(json.loads((g/'copy-path.json').read_text())['path'])
oldsha=sha(b/'original-negative-gate.sh');copyscriptsha=sha(copy/'scripts/rust-tauri/r05_t08_negative_gate.sh');assert oldsha==copyscriptsha=='4853e129e2d0cf7dbd6c93c8c158186fad23e6f231a03ff64f140cca61e0042f'
refs={str(x.relative_to(ROOT)):sha(x) for x in [g/'source-drift-default-failure.json',g/'default-console.log',g/'default-command.json',g/'n06-binding-audit.json',g/'default16-01/case-results.tsv']}
save('history-audit.json',{'UTC':utc(),'IManifestSha256':sha(b/'manifest.json'),'manifestFileCount':m['fileCount'],'checked':len(m['files']),'differences':diff,'rawLogsRead':readlog,'authorFinal':{'checks':checks['checks'],'sync':sync},'G':{'defaultExit':2,'actualRows':15,'N16reuseExecuted':False,'oldCOPYScriptHash':copyscriptsha,'PRIMARYChanged':True,'rootCoordinationFailure':True,'references':refs},'boundary':'核历史来源不把作者自检算独立亲跑；COPY不变不代表PRIMARY实际入口不变'})
assert all(x.startswith("dispatch/") for x in diff),diff
print('I manifest',len(m['files']),'logs',len(logs),'differences',len(diff))
