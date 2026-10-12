"""只更新旧子任务检查的位置与指纹；原断言文字、条目状态和缺口不变。"""
import hashlib,json,re
from pathlib import Path

root=Path.cwd()
base=root/'artifacts/rust-tauri/R06/original-proofs-20261012'
suites={'subagent_closeout','subagent_lifecycle','subagent_permission_inheritance'}
updated=0;identity_updates=0
for path in (base/'mappings').glob('*.json'):
 doc=json.loads(path.read_text());changed=False
 for relative in doc.get('proofSourceSha256',{}):
  if Path(relative).stem in suites and Path(relative).parent==Path('rust/crates/lingxi-service/tests'):
   doc['proofSourceSha256'][relative]=hashlib.sha256((root/relative).read_bytes()).hexdigest();changed=True
 for row in doc.get('items',[]):
  for group in row['groups']:
   for test in group.get('tests',group.get('checks',[])):
    if test['suite'] not in suites:continue
    source=root/'rust/crates'/test['package']/'tests'/(test['suite']+'.rs')
    raw=source.read_text();lines=raw.splitlines();sha=hashlib.sha256(source.read_bytes()).hexdigest()
    name=test['test'].rsplit('::',1)[-1]
    starts=[i for i,line in enumerate(lines) if re.search(r'\bfn '+re.escape(name)+r'\(',line)]
    assert len(starts)==1,(source,name)
    start=starts[0];end=next((i for i in range(start+1,len(lines)) if re.match(r'(?:async )?fn \w+\(',lines[i])),len(lines))
    body='\n'.join(lines[start:end])
    for location in test.get('assertionLocations',test.get('proofLocations',[])):
     quote=location.get('excerpt',location.get('quote',location.get('assertion')))
     if not quote:
      if location.get('kind')=='named_test_body':location.update(startLine=start+1,endLine=end)
      continue
     if test['suite']=='subagent_permission_inheritance' and name=='operate_parent_child_write_is_executed_once' and 'vec![format!("{child_run}|write")]' in quote:
      # 正式登记改变工具身份的表示；恰好一次、实际写工具和其余断言逐字保留。
      quote=quote.replace('vec![format!("{child_run}|write")]','vec![format!("{child_run}|tool:first-party:write")]')
      field='excerpt' if 'excerpt' in location else 'quote' if 'quote' in location else 'assertion'
      location[field]=quote;identity_updates+=1
     pattern=r'\s+'.join(re.escape(word) for word in quote.split())
     matches=list(re.finditer(pattern,body));assert len(matches)==1,(source,name,quote,'原断言必须原样且唯一保留')
     match=matches[0];line=start+1+body[:match.start()].count('\n')
     if 'line' in location:location['line']=line
     else:location['startLine']=line
     if 'endLine' in location:location['endLine']=line+match.group().count('\n')
     updated+=1
    for field in ['sourceSHA256','sourceSha256']:
     if field in test:test[field]=sha
    changed=True
 if changed:path.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'updatedOriginalAssertionLocations':updated,'canonicalToolIdentityOnlyUpdates':identity_updates,'originalRequirementTextAndStatusesChanged':False},ensure_ascii=False))
