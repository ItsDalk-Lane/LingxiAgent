import pathlib,json,hashlib,datetime,struct
E=pathlib.Path(__file__).resolve().parent;rows=[]
for p in sorted(E.rglob('verify-stage-result.json')):
 d=json.loads(p.read_text());b=d.get('candidateSourceBinding',{}); sides={}
 for side in ['before','after']:
  ref=b.get(side,{})
  if not ref.get('manifestPath'):continue
  mp=pathlib.Path(ref['manifestPath']);m=json.loads(mp.read_text());h=hashlib.sha256(b'lingxi-candidate-source-v1\0')
  for x in m['entries']:
   for v in [x['pathBytesHex'],x['kind'],x.get('sha256') or '',str(x['mode']) if x.get('mode') is not None else '']:
    v=v.encode();h.update(struct.pack('>Q',len(v)));h.update(v)
  kernel=[x for x in m['entries'] if x['display']=='rust/crates/lingxi-kernel/src/lib.rs'];sides[side]={'path':str(mp),'manifestActualSHA256':hashlib.sha256(mp.read_bytes()).hexdigest(),'manifestReferenceSHA256':ref['manifestSha256'],'fileCount':len(m['entries']),'referenceFileCount':ref['fileCount'],'digestRecomputed':h.hexdigest(),'digestDeclared':m['digestSha256'],'digestReference':ref['digestSha256'],'uniquePaths':len(set(x['pathBytesHex'] for x in m['entries']))==len(m['entries']),'kernel':kernel}
  sides[side]['allEqual']=sides[side]['manifestActualSHA256']==ref['manifestSha256'] and h.hexdigest()==m['digestSha256']==ref['digestSha256'] and len(m['entries'])==ref['fileCount']==m['fileCount'] and sides[side]['uniquePaths']
 before=pathlib.Path(b.get('before',{}).get('manifestPath','/nonexistent'));after=pathlib.Path(b.get('after',{}).get('manifestPath','/nonexistent'));changes=[]
 if before.is_file() and after.is_file():
  aa={x['pathBytesHex']:x for x in json.loads(before.read_text())['entries']};bb={x['pathBytesHex']:x for x in json.loads(after.read_text())['entries']};changes=[{'path':bytes.fromhex(k).decode(errors='replace'),'before':aa.get(k),'after':bb.get(k)} for k in sorted(aa.keys()|bb.keys()) if aa.get(k)!=bb.get(k)]
 rows.append({'result':str(p.relative_to(E)),'resultSHA256':hashlib.sha256(p.read_bytes()).hexdigest(),'stage':d['stage'],'overall':d['overall'],'stable':b.get('stable'),'sides':sides,'changes':changes,'checkpoints':b.get('checkpointAfterEveryCommand',[]),'runnerSourceBinding':d.get('runnerSourceBinding',{}),'commands':d['commands']})
pristine=E/'default16-01/pristine/rust/crates/lingxi-kernel/src/lib.rs';raw=pristine.read_bytes();k0=hashlib.sha256(raw).hexdigest();one=raw+b'\n// N06 mid-gate mutation: a candidate byte change during the gate run\n';two=one+b'\n// N16 post-acceptance mutation: the candidate moved after run A\n';out={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'results':rows,'allManifestDigestsRecomputed':all(s['allEqual'] for r in rows for s in r['sides'].values()),'kernelCommentIdentity':{'pristine':k0,'N06carry':hashlib.sha256(one).hexdigest(),'N16after':hashlib.sha256(two).hexdigest(),'boundary':'生产append注释只改变身份；N07至N16-A并非原候选字节，所有功能目标由实际断言与恢复对照分别证明'}}
(E/'binding-manifest-audit.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'allManifestDigestsRecomputed':out['allManifestDigestsRecomputed'],'results':[{k:v for k,v in r.items() if k not in ['sides','runnerSourceBinding','commands','checkpoints','changes']} for r in rows]},ensure_ascii=False))
