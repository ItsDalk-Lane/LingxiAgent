import sys,runpy,json,shutil,hashlib,os,datetime
from pathlib import Path
sys.argv=['review_driver','extra'];d=runpy.run_path(str(Path(__file__).with_name('review_driver.py')))
ROOT,EV,run,sha,save,ENV=[d[k] for k in ['ROOT','EV','run','sha','save','ENV']]
COPY=Path((EV/'copy-path.txt').read_text().strip())
old=ROOT/'artifacts/rust-tauri/R05/RR3/A-REVIEW-02/discover-old.py';target=EV/'discover-old.py';shutil.copyfile(old,target)
assert run('fd-old-red',['python3','-B',ROOT/'scripts/rust-tauri/r02_run_output_regression.py','--evidence',EV/'fd-old-red','--discovery',target])==1
case=EV/'fd-old-red/old-untracked-parent-child'
assert (case/'before.tsv').read_bytes()==(case/'after.tsv').read_bytes()
assert 'DIR artifacts/rust-tauri/R05/run001' in (case/'sinks.txt').read_text()
assert '旧证据变更' in (EV/'fd-old-red.stderr').read_text()
assert run('fd-restored',['python3','-B',ROOT/'scripts/rust-tauri/r02_run_output_regression.py','--evidence',EV/'fd-restored'])==0
save('fd-old-restored.json',{'historicalDiscovery':str(old),'oldSha256':sha(old),'isolatedSha256':sha(target),'oldCounterexampleReached':True,'oldBeforeAfterEqual':True,'restoredExit':0,'boundary':'真实OS父child fd和原binder；只替换独立指定旧发现器，不手造FILE'})
receipt=EV/'default-prefix/node-preparation/result.json'
def verify(name,code):
 assert run(name,['python3','-B',ROOT/'scripts/rust-tauri/r05_t08_prepare_node.py','--verify',receipt,'--evidence',EV/name])==code
p=COPY/'node_modules/ws/index.js';source=ROOT/'node_modules/ws/index.js';original=p.read_bytes();before=sha(source)
p.write_bytes(original+'\n// 独立副本变更检查\n'.encode())
try:
 assert sha(source)==before
 verify('node-content-red',1)
 assert 'copy dependencies changed' in json.loads((EV/'node-content-red/result.json').read_text())['error']
finally:p.write_bytes(original)
verify('node-content-restored',0)
alternate=COPY/'.git/objects/info/alternates';saved=alternate.read_bytes();alternate.write_text(str(EV/'objects-do-not-exist')+'\n')
try:
 verify('node-shared-object-red',1)
 assert 'command failed' in json.loads((EV/'node-shared-object-red/result.json').read_text())['error']
finally:alternate.write_bytes(saved)
verify('node-shared-object-restored',0)
save('copy-write-isolation.json',{'source':str(source),'sourceBefore':before,'sourceAfter':sha(source),'copyRestored':sha(p),'sourceUnchanged':before==sha(source)==sha(p),'sharedObjectsRestoredSha256':sha(alternate),'boundary':'本轮完整copy实写；未修改主依赖或主Git对象'})
assert run('client',['python3','-B',COPY/'scripts/rust-tauri/r02_client_leaf_matrix.py',EV/'CLIENT'],COPY)==0
assert run('cli-args',[COPY/'node_modules/.bin/vitest','run','tests/cli-args.test.ts'],COPY)==0
verify('node-final-verify',0)
assert run('shell-syntax',['bash','-n',ROOT/'scripts/rust-tauri/r05_t08_negative_gate.sh',ROOT/'scripts/rust-tauri/r02_t08_legacy_entry_regression.sh'])==0
save('main-sources-final.json',d['sources']());save('main-git-final.json',d['inventory'](ROOT/'.git'))
for n in ['main-sources','main-git']:
 a=json.loads((EV/(n+'-before.json')).read_text());b=json.loads((EV/(n+'-final.json')).read_text());save(n+'-differences.json',{'countBefore':len(a),'countAfter':len(b),'different':{k:{'before':a.get(k),'after':b.get(k)} for k in a.keys()|b.keys() if a.get(k)!=b.get(k)}})
print('All additional controls and final verification completed',flush=True)
