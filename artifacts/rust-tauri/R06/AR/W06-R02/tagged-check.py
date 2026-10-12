import json,pathlib,sys
p=pathlib.Path(__file__).resolve().parent
rows=json.loads((p/'tagged-value-results.json').read_text())['records'];checks=[]
for r in rows:
 for part,expected,actual in [('read_status',400,r['actual_flags'][0]),('run_terminal','failed',r['terminal'][0]),('zero_provider_wire',0,len(r['provider_request'])),('patch_status',400,r['patch'][0]),('unchanged_after_rejected_patch',r['yaml'],r['config_after_patch'])]:
  checks.append(dict(name=r['name'],part=part,expected=expected,actual=actual,status='PASS' if expected==actual else 'FAIL'))
result={'checks':checks,'passed':sum(x['status']=='PASS' for x in checks),'failed':sum(x['status']=='FAIL' for x in checks)}
(p/'tagged-check-results.json').write_text(json.dumps(result,ensure_ascii=False,indent=2));print(json.dumps(result,ensure_ascii=False));sys.exit(1 if result['failed'] else 0)
