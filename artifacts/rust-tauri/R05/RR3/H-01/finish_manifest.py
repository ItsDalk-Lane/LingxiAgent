from capture import *

started=utc()
out=EV/'commands/final-manifest'
out.mkdir(exist_ok=False)
before=snapshot()
for p in sorted((EV/'commands').glob('*/command.json')):
    d=json.loads(p.read_text())
    assert sha(p.parent/'stdout.log')==d['stdoutSHA256'],p
    assert sha(p.parent/'stderr.log')==d['stderrSHA256'],p
assert sha(ROOT/'rust/target/debug/lingxi-service')==json.loads((EV/'FINAL_SOURCE_BINDING.json').read_text())['binarySha256']
after=snapshot();assert before==after
record={'argv':['python3',str(EV/'finish_manifest.py')],'cwd':str(ROOT),'UTC_start':started,'UTC_end':utc(),'exit':0,'counts':[],'leafCounts':[],'inputEqual':True,'inputBeforeDigest':hashlib.sha256(json.dumps(before,sort_keys=True).encode()).hexdigest(),'inputAfterDigest':hashlib.sha256(json.dumps(after,sort_keys=True).encode()).hexdigest(),'boundary':'只校验并归档H证据，不是独立验收或全阶段封印','environment':{'RUST_LOG':os.environ.get('RUST_LOG')},'binary':json.loads((EV/'FINAL_SOURCE_BINDING.json').read_text())['binarySha256']}
(out/'stdout.log').write_text('全部已完成命令输出摘要相等；最终源/程序未漂移。\n')
(out/'stderr.log').write_text('')
record['stdoutSHA256']=sha(out/'stdout.log');record['stderrSHA256']=sha(out/'stderr.log');write(out/'command.json',record)
index=json.loads((EV/'COMMAND_INDEX.json').read_text());existing={r['label'] for r in index}
for p in sorted((EV/'commands').glob('*/command.json')):
    if p.parent.name in existing:continue
    d=json.loads(p.read_text());index.append({'label':p.parent.name,'classification':'EVIDENCE_VERIFICATION','record':str(p.relative_to(EV)),'recordSha256':sha(p),'exit':d['exit'],'RustActualCounts':[],'leafCases':[],'nonTestCounts':'不适用；证据核验不算Rust测试'})
write(EV/'COMMAND_INDEX.json',index)
write(EV/'manifest.json',{str(p.relative_to(EV)):{'sha256':sha(p),'bytes':p.stat().st_size} for p in sorted(EV.rglob('*')) if p.is_file() and p.name!='manifest.json' and '__pycache__' not in p.parts})
manifest=json.loads((EV/'manifest.json').read_text())
assert all(sha(EV/name)==v['sha256'] for name,v in manifest.items())
print({'status':'SELF_CHECKED','files':len(manifest),'commands':len(index),'independentReview':'PENDING','evidenceVerified':True})
