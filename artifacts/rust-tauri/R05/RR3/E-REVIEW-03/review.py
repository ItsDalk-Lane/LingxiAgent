# 本脚本只读现行材料；所有输出和错误状态样本仅在本审查目录。
import json, pathlib, hashlib, os, stat, datetime, subprocess, sys, gzip, re, collections, copy
R=pathlib.Path.cwd(); E=R/'artifacts/rust-tauri/R05/RR3/E-REVIEW-03'; A=R/'artifacts/rust-tauri/R05/RR3/E-03'; D=R/'docs/rust-tauri/R05'; H=R/'artifacts/rust-tauri/R05/RR3/H-REVIEW-02'; G=R/'artifacts/rust-tauri/R05/RR3/G-REVIEW-02'
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(b): return hashlib.sha256(b).hexdigest()
def write(n,v): (E/n).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def pairs(v):
 out={}
 for k,x in v:
  if k in out: raise ValueError('duplicate key: '+k)
  out[k]=x
 return out
def load(p): return json.loads(pathlib.Path(p).read_text(),object_pairs_hook=pairs)
def item(p):
 p=pathlib.Path(p); link=os.readlink(p) if p.is_symlink() else None; b=link.encode() if link else p.read_bytes()
 return dict(path=str(p.relative_to(R)),bytes=len(b),sha256=sha(b),mode=stat.S_IMODE(p.lstat().st_mode),link=link)
def run(argv,label):
 t=utc(); p=subprocess.run(argv,cwd=R,capture_output=True); end=utc()
 (E/(label+'.stdout')).write_bytes(p.stdout); (E/(label+'.stderr')).write_bytes(p.stderr)
 row=dict(argv=argv,cwd=str(R),start=t,end=end,exit=p.returncode,stdout=label+'.stdout',stderr=label+'.stderr',stdoutSha256=sha(p.stdout),stderrSha256=sha(p.stderr))
 with (E/'commands.jsonl').open('a') as f:f.write(json.dumps(row,ensure_ascii=False)+'\n')
 return p
DOCS=[x['path'] for x in load(A/'documents-after.json')]
def inventory():
 paths={x['path'] for x in load(G/'metadata/source-before.json')['files']} | set(load(H/'FINAL_SOURCE_BINDING.json')['inputHashes'])
 for base in ['rust','scripts','contracts','shared','tests','docs/rust-tauri','.sync-audit','cli','core','server','desktop','channels','bridges']:
  for dp,dirs,files in os.walk(R/base,followlinks=False):
   dirs[:]=[d for d in dirs if d not in {'target','.git','node_modules','__pycache__'}]
   for f in files:
    if f.endswith('.pyc') or f.endswith('.DS_Store'):continue
    paths.add(str((pathlib.Path(dp)/f).relative_to(R)))
 for n in ['package.json','package-lock.json','.npmrc','rust-toolchain.toml','tsconfig.json','tsconfig.node.json','tsconfig.test.json','vitest.config.ts']:
  if (R/n).exists():paths.add(n)
 paths={p for p in paths if p not in DOCS and not p.startswith('docs/rust-tauri/R05/repair-current/RR3_')}
 rows=[item(R/p) for p in sorted(paths)]
 digest=sha(''.join(f"{x['sha256']}  {x['mode']}  {x['path']}\n" for x in rows).encode())
 return dict(at=utc(),files=rows,count=len(rows),digest=digest)
C=[]
def ck(name,ok,detail=None): C.append(dict(name=name,pass_=bool(ok),detail=detail))
def baseline():
 write('inputs-before.json',inventory()); write('documents-before.json',[item(R/p) for p in DOCS]); write('git-before.json',[item(R/'.git'/n) for n in ['HEAD','index']])
 for i,argv in enumerate([['git','rev-parse','HEAD'],['git','branch','--show-current'],['git','diff','--check','--']+DOCS,['git','diff','--cached','--name-only'],['df','-k','.'],['python3','--version'],['/Users/study_superior/.cargo/bin/rustc','--version'],['/Users/study_superior/.cargo/bin/cargo','--version'],['node','--version'],['npm','--version'],['uname','-sm']]):run(argv,f'baseline-{i:02}')
 print('baseline captured',load(E/'inputs-before.json')['count'])
def check_current(h):
 bad=[];c=h['rr3_current'];p=c['packages'];g=p['G']
 def req(v,n):
  if not v:bad.append(n)
 req(c['stage_readiness']=='NOT_ACCEPTED' and c['R06_READY'] is False and h['accepted_tasks']==[], 'stage_not_released')
 req(g['status']=='BLOCKED_BY_STORAGE' and g['default_shell_exit']=='UNKNOWN' and g['recorder_tool_exit']==1 and g['observer_tool_exit']==143,'G02_blocked_exit_separation')
 req(g['N01']['restored_execution']=='NOT RUN' and g['N02']['target_reached'] is False and g['N02']['producer_runs_completed']==0,'no_fabricated_target_or_restore')
 req(g['unexecuted_cases']==[f'N{i:02}' for i in range(3,17)] and g['full_R02_runs']==0 and g['full_E5']=='NOT RUN','G02_unexecuted_scope')
 req(p['FINAL']['result_ref'] is None and p['FINAL']['tested_sha'] is None and p['FINAL']['status']=='NOT RUN','FINAL_not_run')
 req(p['D']['current_final_binary_identity'] is None and '历史' in p['D']['binary_identity']['identity_scope'],'D_historical_identity')
 req(p['C_F46']['evidence_ref'].endswith('H-REVIEW-02/REVIEW.md'),'current_resource_uses_H02')
 for k in ['A','B','C_F46','H','I','J']:
  req(p[k]['status']=='CLOSED' and p[k]['independent_review']=='PASS',k+'_closed')
  req(sha((R/p[k]['evidence_ref']).read_bytes())==p[k]['review_sha256'],k+'_actual_review_digest')
 req(all(c['I01_I11'][f'I{i:02}']['status']=='PENDING_CURRENT_COMBINATION' for i in range(1,10)),'I01_to_I09_pending')
 req(c['I01_I11']['I10']['status']=='PASS_LIMITED_REUSE' and c['I01_I11']['I11']['status']=='INCOMPLETE','I10_I11_boundary')
 req(c['live_verification']=='BLOCKED_NOT_AUTHORIZED','LIVE_not_claimed')
 req(h['allowed_next_scope']['stage']=='R05_ONLY','no_R06_execution')
 req(h['git_delivery_receipt_contract']['committed_sha'] is None and h['git_delivery_receipt_contract']['pushed_sha'] is None,'git_not_invented')
 return bad

def audit():
 docs={p:(R/p).read_text() for p in DOCS}; h=load(D/'R05_HANDOFF.json'); c=h['rr3_current'];b=json.loads(gzip.decompress((A/'before-documents.json.gz').read_bytes()))
 for p,t in docs.items():
  if p.endswith('.json'):
   j=json.loads(t,object_pairs_hook=pairs); x=j['stages']['R05']['rr3_current'] if p.endswith('ORCHESTRATOR_PROGRESS.json') else j['rr3_current'];ck('same current '+p,x==c)
 ck('current matches independent facts',check_current(h)==[],check_current(h))
 for row in load(A/'documents-after.json'):ck('E03 delivered document '+row['path'],item(R/row['path'])==row)
 m=load(A/'MANIFEST.json')
 for row in m['files']:
  p=A/row['path'];ck('author manifest '+row['path'],p.exists() and len(p.read_bytes())==row['bytes'] and sha(p.read_bytes())==row['sha256'])
 for p,old in b.items():
  if not p.endswith('.json'):continue
  old=json.loads(old);new=json.loads(docs[p])
  # 所有未在本轮明确更新的顶层历史记录完整保留。
  allow={'generated_by','generated_at','historical_record_notice','rr3_current','rr3_current_history','working_tree_digest','working_tree_digest_scope','source_sha_semantics','unresolved_items','allowed_next_scope','artifact_hashes','consumer_contract','rr3_I10_mapping','rr3_I10_mapping_history','rr3_resource_independent_review','rr3_resource_independent_review_history','rr3_platform_verification','review_status'}
  if p.endswith('ORCHESTRATOR_PROGRESS.json'):
   ck('ORCH non R05 stages unchanged',{k:v for k,v in old['stages'].items() if k!='R05'}=={k:v for k,v in new['stages'].items() if k!='R05'})
   ck('ORCH tasks unchanged',old['tasks']==new['tasks']);continue
  for k,v in old.items():
   if k not in allow:ck('historical top field '+p+'/'+k,new.get(k)==v)
  ck('previous current retained '+p,any(x.get('value',x.get('snapshot',x.get('current',x)))==old['rr3_current'] for x in new.get('rr3_current_history',[])))
 for n,key in [('PROGRESS_LEDGER.json','tasks'),('R05_ACCEPTANCE_LEDGER.json','acceptances'),('R05_TEST_MAP.json','entries')]:ck('large historical field '+n,json.loads(b['docs/rust-tauri/R05/'+n])[key]==load(D/n)[key])
 for n in ['WORKER_MODEL_BOUNDARY.md','R05_INTERFACE_EVOLUTION.md']:ck('unchanged interface '+n,b['docs/rust-tauri/R05/'+n]==(D/n).read_text())
 for p,v in h['dependency_locks'].items():ck('lock '+p,sha((R/p).read_bytes())==v['sha256'])
 for p,v in h['artifact_hashes'].items():
  expected=v['sha256'] if isinstance(v,dict) else v
  ck('handoff evidence '+p,(R/p).is_file() and sha((R/p).read_bytes())==expected)
 for p in DOCS:
  if not p.endswith('.md'):continue
  t=docs[p];links=[]
  for label,target in re.findall(r'\[([^\]]+)\]\(([^)]+)\)',t):
   if re.match(r'^[a-zA-Z]+:',target) or target.startswith('#'):continue
   name,_,anchor=target.partition('#'); dest=(R/p).parent/name
   if not dest.exists():links.append(target)
   elif anchor and dest.suffix=='.md':
    text=dest.read_text();anchors=re.findall(r'<a\s+id="([^"]+)"',text)
    headings=[re.sub(r'[^\w\- ]','',s.lower()).replace(' ','-') for s in re.findall(r'^#+\s+(.+)$',text,re.M)]
    if anchor not in anchors+headings:links.append(target)
  ck('markdown references '+p,not links,links)
 before=load(A/'semantic-inputs-before.json');after=load(A/'semantic-inputs-after.json');now=inventory();write('inputs-audit.json',now)
 ck('author semantic before after equal',before['files']==after['files']);ck('independent enumeration equal author',now['files']==after['files']);ck('semantic digest',now['digest']==h['working_tree_digest']);ck('all H375 source equal',all(sha((R/p).read_bytes())==v for p,v in load(H/'FINAL_SOURCE_BINDING.json')['inputHashes'].items()))
 # 原G摘要保持，回填的字节变化真实登记。
 oldg=load(G/'metadata/source-before.json');dif=[x['path'] for x in oldg['files'] if item(R/x['path'])!=x];write('g-current-differences.json',dif)
 declared=[x['path'] for x in load(A/'g02-current-after-comparison.json')['changed']];ck('G preserved changes explicit',dif==declared)
 write('audit-results.json',dict(at=utc(),checks=C,failures=[x for x in C if not x['pass_']],assertions=len(C)))
 print(json.dumps(dict(assertions=len(C),failures=[x['name'] for x in C if not x['pass_']]),ensure_ascii=False));return 1 if any(not x['pass_'] for x in C) else 0
if __name__=='__main__':
 if sys.argv[1]=='baseline':baseline()
 elif sys.argv[1]=='audit':sys.exit(audit())
 elif sys.argv[1]=='run':
  p=run(sys.argv[3:],sys.argv[2]);print(p.stdout.decode(errors='replace'));print(p.stderr.decode(errors='replace'),file=sys.stderr);sys.exit(p.returncode)
