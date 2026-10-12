import hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parent
records=[]
def check(f,r,expected):
    actual={'flags_status':r['actual_flags'][0],'terminal':r['terminal'][0],
            'wire_count':len(r['provider_request']),'config_unchanged':r['config_unchanged']}
    actual.update({k:r.get(k) for k in expected if k not in actual})
    assert all(actual[k]==v for k,v in expected.items()),(f,r['name'],expected,actual)
    assert r['guard_armed'] and r['fixture_removed'] and r['child_exit_code']==0
    assert r['input_sha256']==hashlib.sha256(r['yaml'].encode()).hexdigest()
    records.append({'source':f,'case':r['name'],'expected':expected,'actual':actual,'status':'PASS'})
for file in ['probe-results.json','family-probe-results.json','effects-probe-results.json']:
    for r in json.loads((ROOT/'final-family-R01'/file).read_text())['records']:
        if file=='probe-results.json':
            valid=r['expected_memory'] is not None
            check(file,r,{'flags_status':200 if valid else 400,'terminal':'completed' if valid else 'failed','wire_count':1 if valid else 0,'config_unchanged':True})
            if valid:
                assert r['actual_flags'][1]['memoryEnabled']==r['expected_memory']
                assert ('REVIEWER-ONLY-NONCE' in r['provider_request'][0]['messages'][0]['content'])==r['expected_memory']
        elif file=='family-probe-results.json':
            check(file,r,{'flags_status':400,'terminal':'failed','wire_count':0,'config_unchanged':True})
        else:
            valid=r['name'].endswith('block-control')
            check(file,r,{'flags_status':200 if valid or r['name']=='orphan-complex-peer' else 400,'terminal':'completed' if valid else 'failed','wire_count':1 if valid else 0,'config_unchanged':True})
            assert r['cleanup_preview'][0]==(200 if valid else 400)
            if r['name']=='project-block-control':assert not r['project_in_wire']
out={'status':'PASS','case_count':len(records),'records':records,
     'contract':'复杂敏感键明确拒绝；旧脚本的默认值匹配标记只保留原始历史，不作为当前正确性的判据。真实配置、HTTP、wire、SQLite终态均逐项对账。',
     'mock_boundary':'仅外部模型loopback，业务入口不mock'}
(ROOT/'final-r01-contract-results.json').write_text(json.dumps(out,ensure_ascii=False,indent=2))
print(json.dumps({'status':'PASS','cases':len(records)}))
