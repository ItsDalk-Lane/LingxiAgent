from capture import *
import re
# 只补准确的最终自检计数并绑定报告摘要，不再改状态或生产输入。
p=E/'REPORT.md';s=p.read_text().replace('3254断言','首次3254、最终3260断言');p.write_text(s)
hp=D/'R05_HANDOFF.json';text=hp.read_text();h=json.loads(text);hashes=h['artifact_hashes'];hashes[R+'E-02/REPORT.md']=sha(p)
pattern=re.compile(r'^  "artifact_hashes": ',re.M);match=pattern.search(text);start=match.end();_,length=json.JSONDecoder().raw_decode(text[start:]);rendered=json.dumps(hashes,ensure_ascii=False,indent=2).replace('\n','\n  ');hp.write_text(text[:start]+rendered+text[start+length:])
changed=load(E/'changed-files.json');changed['files']['docs/rust-tauri/R05/R05_HANDOFF.json']['afterSha256']=sha(hp);dump(E/'changed-files.json',changed)
code=run('31-delivery-validation',['python3','artifacts/rust-tauri/R05/RR3/E-02/verify-delivery.py']);assert code==0
# 子检查命令和日志已结束才生成清单，持续由外部写入的dispatch不纳入。
files={}
for q in sorted(E.rglob('*')):
 if not q.is_file() or q.name=='manifest.json' or 'dispatch' in q.relative_to(E).parts:continue
 files[str(q.relative_to(E))]={'sha256':sha(q),'bytes':q.stat().st_size}
source_before=load(E/'inputhash-before.json');source_after=load(E/'inputhash-after.json')
manifest={'utc':now(),'executor':'rr3_e_impl_02','status':'SELF_CHECKED_PENDING_NEW_INDEPENDENT_REVIEW','mustFixAddressedSelfcheck':['MF-E01','MF-E02'],'independentReview':'PENDING（另一全新E-REVIEW-02，本人不签独立PASS）','candidateHead':load(E/'before.json')['head'],'sourceDigest':source_before['digest'],'sourceCount':423,'scope':source_before['scope'],'sourceEqual':source_before['files']==source_after['files'],'changedOwnedCount':13,'ownedCount':14,'ownedFilesAfter':{p:sha(ROOT/p) for p in OWNED},'commandCount':len(load(E/'commands.json')),'excluded':['manifest.json itself','dispatch/**（外部总控唯一所有权，持续追加，本E不写）'],'files':files,'boundary':'G仍运行并写evidence；只限定生产输入相等，不称总树静默或FINAL freeze；所有原FAIL保留，R06_READY=false。'}
dump(E/'manifest.json',manifest)
for rel,x in files.items():assert sha(E/rel)==x['sha256'],rel
for rel,x in manifest['ownedFilesAfter'].items():assert sha(ROOT/rel)==x,rel
assert not any((ROOT/R/'G-REVIEW-01'/n).exists() for n in ['REVIEW.md','REPORT.md','RESULT.json','MANIFEST.json','manifest.json']),'G交付前已完成，需要先亲读完整结果后新补录'
print(json.dumps({'status':'SELF_CHECKED','manifestFiles':len(files),'commands':manifest['commandCount'],'changedOwned':13,'unchangedOwned':1,'sourceCount':423,'sourceEqual':manifest['sourceEqual'],'G':'RUNNING','FINAL':'NOT RUN','independentE':'PENDING_NEW_REVIEWER','writingStopped':True},ensure_ascii=False))
