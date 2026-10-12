"""汇总本轮逐断言审阅；只从完整原断言建立正式证明，局部证明保留缺口。"""
import json,hashlib,re
from pathlib import Path
root=Path.cwd();out=root/'artifacts/rust-tauri/R06/original-proofs-20261012'
original={r['id']:r for r in json.loads((root/'artifacts/rust-tauri/R06/residual-repair-20261012/remaining-original-obligations.json').read_text())['items']}
files=['sessions','chat-subagent','agents-persona','resources','knowledge','memory-skills','skills-bundles']
items=[];seen=set();counts={}
def as_list(value):
 if not value:return []
 return value if isinstance(value,list) else [value]
for name in files:
 doc=json.loads((out/'mappings'/f'{name}.json').read_text());counts[name]={'requirements':len(doc['items']),'ready':0,'gap':0}
 for row in doc['items']:
  rid=row['id'];assert rid in original and rid not in seen,rid;seen.add(rid)
  orig=original[rid];assert row['r00Assertions']==orig['r00Assertions'],rid;assert row['r00Due']==orig['r00Due'],rid;assert row['r00Then']==orig['r00Then'],rid
  groups=[];gaps=as_list(row.get('gaps'))+as_list(row.get('outcomeGaps'));indices=[]
  for group in row['groups']:
   i=group.get('index',group.get('assertionIndex'));assert type(i)is int and i not in indices and 0<=i<len(orig['r00Assertions']),(rid,i);indices.append(i)
   assert group.get('assertion',orig['r00Assertions'][i])==orig['r00Assertions'][i],rid
   coverage='FULL' if group['status']=='READY' else 'PARTIAL'
   missing=as_list(group.get('missingClauses',group.get('unmetClauses',group.get('gap'))))
   if coverage=='PARTIAL' and not missing:missing=as_list(group.get('reason'))
   if coverage=='PARTIAL':
    assert missing,(rid,i,'缺具体未满足子句');gaps.extend(missing)
   raw_tests=group.get('tests',group.get('checks',[]));tests=[];anchors=[];meanings=[]
   for t in raw_tests:
    minimal={k:t[k] for k in ('package','suite','test')};path=Path('rust/crates')/t['package']/'tests'/(t['suite']+'.rs')
    if t['suite']=='lib':
     path=Path(t['proofLocations'][0]['path']);minimal.update(kind='rust_library_test',sourcePath=str(path))
    text=(root/path).read_text();lines=text.splitlines();h=hashlib.sha256((root/path).read_bytes()).hexdigest()
    supplied=t.get('sourceSHA256',t.get('sourceSha256',doc.get('proofSourceSha256',{}).get(str(path))))
    if supplied:assert supplied==h,(rid,str(path),'审阅后源码变化，须更新映射')
    start=next(j for j,l in enumerate(lines) if re.search(r'\bfn '+re.escape(t['test'].rsplit('::',1)[-1])+r'\(',l));end=next((j for j in range(start+1,len(lines)) if re.match(r'(?:async )?fn \w+\(',lines[j])),len(lines))
    raw_locs=t.get('assertionLocations',t.get('proofLocations',[]));assert raw_locs,(rid,t['test'],'无具体断言定位')
    locs=[]
    for loc in raw_locs:
     line=loc.get('line',loc.get('startLine'));quote=loc.get('excerpt',loc.get('quote',loc.get('assertion')))
     if not quote or not line:continue
     actual='\n'.join(lines[line-1:line-1+len(quote.splitlines())])
     if actual.strip()!=quote.strip():
      matches=['\n'.join(lines[line-1:line-1+k]) for k in range(1,25) if ' '.join('\n'.join(lines[line-1:line-1+k]).split())==' '.join(quote.split())]
      assert matches,(rid,t['test'],line,'实际断言定位不一致');actual=matches[0]
     quote=actual
     locs.append({'line':line,'quote':quote})
    assert locs,(rid,t['test'],'无真实断言')
    if minimal not in tests:tests.append(minimal)
    anchors.append({'path':str(path),'test':t['test'],'sourceSHA256':h,'testStartLine':start+1,'testEndLine':end,'assertions':locs})
    meanings.append(t.get('meaning',t.get('reason','真实具名断言')))
   if tests:
    meaning=group.get('meaning',group.get('reason')) or '；'.join(dict.fromkeys(meanings))
    groups.append({'index':i,'tests':tests,'meaning':meaning,'proofLocations':anchors,'coverage':coverage,'missingClauses':missing})
   elif coverage=='FULL':raise AssertionError((rid,i,'完整断言没有真实检查'))
  assert set(indices)==set(range(len(orig['r00Assertions']))),(rid,'逐原断言登记不完整')
  status=row['status'];assert status in ['READY','GAP'],(rid,status)
  if status=='READY':assert len(groups)==len(orig['r00Assertions']) and all(g['coverage']=='FULL' for g in groups) and not gaps,rid
  else:assert gaps,rid
  counts[name]['ready' if status=='READY' else 'gap']+=1
  items.append({'id':rid,'assertions':orig['r00Assertions'],'then':orig['r00Then'],'status':status,'groups':groups,'gaps':[json.loads(v) for v in dict.fromkeys(json.dumps(g,ensure_ascii=False,sort_keys=True) for g in gaps)],'reviewSource':str((out/'mappings'/f'{name}.json').relative_to(root))})
assert seen==set(original),(len(seen),len(original),set(original)-seen)
result={'schema':'lingxi.r06.original-proof-map.v1','scope':'仅补原279条已有功能证明；未实现或不满足完整原断言的条目保留未通过；不新增产品实现','items':sorted(items,key=lambda r:r['id'])}
(root/'scripts/rust-tauri/r06_original_leaf_bindings.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'groups':counts,'total':len(items),'ready':sum(r['status']=='READY' for r in items),'gap':sum(r['status']=='GAP' for r in items),'uniqueChecks':len({tuple(t[k] for k in ('package','suite','test')) for r in items for g in r['groups'] for t in g['tests']})},ensure_ascii=False))
