from review_support import *
# 交付只验证现有证据，不重跑或扩大业务检查。
protected=json.loads((EV/'source-before.json').read_text())['files']
assert all(sha(ROOT/p)==h for p,h in protected.items())
history=json.loads((EV/'history-audit.json').read_text())
assert all(sha(ROOT/p)==h for p,h in history['G']['references'].items())
result=json.loads((EV/'permanent-final/result.json').read_text());assert result['checks']==56 and result['failed']==0
assert len(result['results'])==56 and all(r['exitCode']==r['expectedExitCode'] for r in result['results'])
for r in result['results']:
 if 'stdoutSha256' in r:
  assert sha(EV/'permanent-final'/(r['check']+'.stdout.log'))==r['stdoutSha256']
  assert sha(EV/'permanent-final'/(r['check']+'.stderr.log'))==r['stderrSha256']
 if 'logSha256' in r: assert sha(EV/'permanent-final'/(r['check']+'.run.log'))==r['logSha256']
for r in json.loads((EV/'sync-independent-audit.json').read_text())['rows']:
 assert sha(Path(r['argv'][-1])/'verify-stage-result.json')==r['resultSha256']
 log=Path(r['argv'][-1]).parent/(r['name']+'.live.log');assert sha(log)==r['logSha256']
 assert sha(r['argv'][0])==r['binarySha256']
assert 'R06_READY=false' in (EV/'REVIEW.md').read_text()
save('delivery-verification.json',{'UTC':utc(),'status':'PASS','protectedSourceCount':len(protected),'mainHEADIndexBytesEqual':True,'historicalGReferencesEqual':True,'permanentChecksVerified':56,'productionRunsExtraVerified':3,'scope':'证据交付检查，不是完整业务或全树freeze'})
print('证据交付检查 PASS')
