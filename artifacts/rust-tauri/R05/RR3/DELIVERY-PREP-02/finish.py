#!/usr/bin/env python3
"""核对本目录交付资料；不运行产品或历史证据驱动。"""
from pathlib import Path
import collections
import datetime
import hashlib
import json
import re

OUT=Path(__file__).resolve().parent
ROOT=OUT.parents[4]
RR='artifacts/rust-tauri/R05/RR3/'
START=datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(data): return hashlib.sha256(data).hexdigest()
def pairs(items):
    out={}
    for k,v in items:
        if k in out: raise ValueError('重复JSON键: '+k)
        out[k]=v
    return out
def load(p): return json.loads(p.read_bytes(),object_pairs_hook=pairs)
def dump(name,x): (OUT/name).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def names(name): return set((OUT/name).read_text().splitlines())
assert ROOT.name=='LingxiAgent'
old=load(ROOT/RR/'DELIVERY-PREP-01/paths-resolved.json')
decisions=load(OUT/'decisions.json'); corrections=load(OUT/'supplemental-corrections.json')
merged=load(OUT/'merged-paths.json'); summary=load(OUT/'summary.json')
rules={r['id']:r for r in load(OUT/'classification-rules.json')}
local={r['path']:r for r in load(OUT/'local-originals.json')}
identities={r['path']:r for r in load(OUT/'read-input-identities.json')}
checks=[]
def check(label,result,detail=None):
    checks.append({'check':label,'ok':bool(result),'detail':detail})

check('768条唯一且完全覆盖旧待定',len(decisions)==len({r['path'] for r in decisions})==768 and {r['path'] for r in decisions}=={r['path'] for r in old if r['category']=='REVIEW_SOURCE_SNAPSHOT'})
check('逐项明确决定且没有UNKNOWN',all(r['decision'] in ['INCLUDE_EVIDENCE_SNAPSHOT','LOCAL_ONLY'] and r['ruleId'] in rules and r['reason'] and r['sourceReferences'] for r in decisions))
check('计数等于报告',collections.Counter(r['decision'] for r in decisions)=={'INCLUDE_EVIDENCE_SNAPSHOT':257,'LOCAL_ONLY':511} and len(corrections)==558)
check('规则映射及基线比较成立',all(r['decision']==rules[r['ruleId']]['decision'] and all(x['equalsCurrentSnapshot'] for x in r['baselineClaims']+r['historicalClaims']) for r in decisions+corrections))
check('旧全集不增漏且唯一',len(merged)==len({r['path'] for r in merged})==len(old)==19734 and {r['path'] for r in merged}=={r['path'] for r in old})
include=names('include-paths.txt'); localnames=names('local-paths.txt'); unknown=names('unknown-paths.txt')
check('精确列表互斥且覆盖',not include&localnames and not include&unknown and not localnames&unknown and include|localnames|unknown=={r['path'] for r in merged})
check('列表计数',len(include)==13828 and len(localnames)==5906 and not unknown)
check('总字节与初步清点相等',sum(r['bytes'] for r in merged)==sum(r['bytes'] for r in old)==1790455388)
check('报告字节和清单相等',sum(r['bytes'] for r in merged if r['path'] in include)==572265729 and sum(r['bytes'] for r in merged if r['path'] in localnames)==1218189659)
check('新排除原件身份与索引相同',all(r['path'] in local and all(local[r['path']][k]==r[k] for k in ['sha256','bytes','localOnly']) and local[r['path']]['remoteOriginalAvailable'] is False for r in decisions+corrections if r['localOnly']))
check('合成清单不纳三种工作副本或临时票据',all(not any(p.startswith(RR+x) for x in ['A-REVIEW-01/snapshot/','A-REVIEW-02/snapshot/','I-REVIEW-01/old-independent-copy/']) and not p.endswith('/p1-ticket.body') for p in include))
check('已有报告原日志测量未被误作补正对象',all(r['ruleId'] in ['A_REVIEW1_WORKCOPY','A_REVIEW2_WORKCOPY','I_OLD_WORKCOPY','SYNTHETIC_RUNTIME_TICKET'] for r in corrections))
refchecks=[]
for r in decisions+corrections:
    for p in r['sourceReferences']+r['referenceSources']+[x['source'] for x in r['baselineClaims']+r['historicalClaims']]:
        q=ROOT/p
        refchecks.append({'source':r['path'],'reference':p,'present':q.exists(),'withinRepository':q.is_relative_to(ROOT)})
check('本轮每条来源和历史引用实物存在',all(x['present'] and x['withinRepository'] for x in refchecks),len(refchecks))
statchecks=[]
for r in decisions+corrections:
    s=(ROOT/r['path']).stat(); oldstat=identities[r['path']]
    statchecks.append({'path':r['path'],'bytesSame':s.st_size==r['bytes'],'mtimeSameSinceHash':s.st_mtime_ns==oldstat['mtimeNs']})
check('分类后到封口原件stat未变',all(x['bytesSame'] and x['mtimeSameSinceHash'] for x in statchecks),len(statchecks))
for p in sorted(OUT.glob('*.json')):
    if p.name not in ['QA.json','MANIFEST.json','finish-receipt.json']:
        load(p)
check('JSON可解析且无重复键',True)
markdown_refs=[]
for p in OUT.glob('*.md'):
    for target in re.findall(r'\]\(([^)]+)\)',p.read_text()):
        if '://' in target: continue
        q=p.parent/target.split('#')[0]
        # 本次封口将生成这些文件。
        planned=target in ['QA.json','MANIFEST.json','finish-receipt.json']
        markdown_refs.append({'source':p.name,'target':target,'presentOrPlanned':q.exists() or planned})
check('本目录Markdown链接有效',all(x['presentOrPlanned'] for x in markdown_refs),len(markdown_refs))
check('未更改Git暂存清单',summary['stagedUnchanged'] and summary['stagedBeforeCount']==0)
check('清楚声明非最终冻结',summary['isFinalFreeze'] is False and '后续' in summary['requiredRefresh'])
dump('source-reference-check.json',refchecks)
dump('reviewed-original-stat-check.json',statchecks)
dump('markdown-link-check.json',markdown_refs)
qa={'startUTC':START,'endUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'仅交付资料的JSON/集合/引用/自身身份一致性，不是产品测试或最终交付验收',
    'checks':checks,'failed':sum(not x['ok'] for x in checks),'status':'PASS' if all(x['ok'] for x in checks) else 'FAIL'}
dump('QA.json',qa)
assert qa['failed']==0, [x for x in checks if not x['ok']]
files=[]
for p in sorted(OUT.iterdir()):
    if p.is_file() and p.name not in ['MANIFEST.json','finish-receipt.json']:
        data=p.read_bytes();files.append({'path':p.name,'bytes':len(data),'sha256':sha(data)})
dump('MANIFEST.json',{'schemaVersion':1,'scope':'本目录自身资料；排除本MANIFEST和记录其hash的finish-receipt以避免循环摘要',
                      'isFinalDeliveryFreeze':False,'files':files,'count':len(files)})
manifest_bytes=(OUT/'MANIFEST.json').read_bytes(); manifest=load(OUT/'MANIFEST.json')
verified=[]
for x in manifest['files']:
    b=(OUT/x['path']).read_bytes()
    assert len(b)==x['bytes'] and sha(b)==x['sha256']
    verified.append(x['path'])
dump('finish-receipt.json',{'startUTC':START,'endUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),
                           'argv':['python3',str(OUT.relative_to(ROOT)/'finish.py')],
                           'manifestPath':'MANIFEST.json','manifestSHA256':sha(manifest_bytes),'verifiedFiles':len(verified),
                           'QA':'PASS','exitCode':0,'exitCodeMeaning':'脚本末尾即将正常返回；实际命令退出码另由工具回执确认',
                           'selfHashExcluded':True,'writesAfterReceipt':'NONE_BY_THIS_AGENT',
                           'originalsChanged':False,'gitWritePerformed':False,'productTestsOrBuildsExecuted':False})
print(json.dumps({'status':'PASS','checks':len(checks),'manifestFiles':len(verified),'manifestSHA256':sha(manifest_bytes)},ensure_ascii=False))
