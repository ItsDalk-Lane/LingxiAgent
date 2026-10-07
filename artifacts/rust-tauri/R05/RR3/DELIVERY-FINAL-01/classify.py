#!/usr/bin/env python3
"""仅在本证据目录写交付分类；不执行历史驱动，不写 Git，不跟随外链。"""
import collections, datetime, hashlib, json, os, re, stat, subprocess, sys, zlib
from pathlib import Path
ROOT = Path(__file__).resolve().parents[5]
RR3 = 'artifacts/rust-tauri/R05/RR3/'
OUT = Path(__file__).resolve().parent
OWN = str(OUT.relative_to(ROOT)) + '/'
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(b): return hashlib.sha256(b).hexdigest()
def strict_pairs(pairs):
    d={}
    for k,v in pairs:
        if k in d: raise ValueError('重复 JSON 键: '+k)
        d[k]=v
    return d
def load(p): return json.loads((ROOT/p).read_bytes(), object_pairs_hook=strict_pairs)
def save(n,v): (OUT/n).write_text(json.dumps(v,ensure_ascii=False,separators=(',',':'))+'\n')
COMMANDS=[]
def command(argv, data=None):
    t=utc(); p=subprocess.run(argv,input=data,cwd=ROOT,capture_output=True,env={**os.environ,'GIT_OPTIONAL_LOCKS':'0'})
    r={'argv':argv,'startUTC':t,'endUTC':utc(),'exit':p.returncode,'stdoutBytes':len(p.stdout),'stdoutSHA256':sha(p.stdout),'stderrBytes':len(p.stderr),'stderrSHA256':sha(p.stderr)}
    if len(p.stdout)<3000:r['stdout']=p.stdout.decode(errors='replace')
    if p.stderr:r['stderr']=p.stderr.decode(errors='replace')
    if data is not None:r['stdinBytes']=len(data);r['stdinSHA256']=sha(data)
    COMMANDS.append(r)
    with (OUT/'commands.jsonl').open('a') as f:f.write(json.dumps(r,ensure_ascii=False,separators=(',',':'))+'\n')
    if p.returncode:raise RuntimeError(argv)
    return p.stdout
def enumerate_paths():
    kinds={}
    for state,argv in [('untracked',['git','ls-files','--others','--exclude-standard','-z']),('modified',['git','diff','--name-only','-z']),('staged',['git','diff','--cached','--name-only','-z'])]:
        for b in command(argv).split(b'\0'):
            if b:kinds.setdefault(os.fsdecode(b),[]).append(state)
    return kinds
def info(name, compress=False):
    p=ROOT/name;s=p.lstat(); mode=stat.S_IMODE(s.st_mode)
    r={'path':name,'bytes':s.st_size,'mode':mode,'mtimeNs':s.st_mtime_ns}
    if stat.S_ISDIR(s.st_mode):r.update(type='directory',sha256=None);return r
    if stat.S_ISLNK(s.st_mode):
        b=os.fsencode(os.readlink(p));r.update(type='symlink',sha256=sha(b),bytes=len(b),gitMode='120000');return r
    if not stat.S_ISREG(s.st_mode):r.update(type='special',sha256=None);return r
    r['type']='regular';r['gitMode']='100755' if mode&0o111 else '100644'
    h=hashlib.sha256();g=hashlib.sha1();g.update(b'blob '+str(s.st_size).encode()+b'\0')
    c1=zlib.compressobj(1);c6=zlib.compressobj(6);n1=n6=0
    if compress:
        header=b'blob '+str(s.st_size).encode()+b'\0';n1+=len(c1.compress(header));n6+=len(c6.compress(header))
    with p.open('rb') as f:
        while b:=f.read(1024*1024):
            h.update(b);g.update(b)
            if compress:n1+=len(c1.compress(b));n6+=len(c6.compress(b))
    t=p.lstat()
    if (s.st_size,s.st_mtime_ns,s.st_mode)!=(t.st_size,t.st_mtime_ns,t.st_mode):raise RuntimeError('读取期间变化: '+name)
    r['sha256']=h.hexdigest();r['rawBlobSHA1']=g.hexdigest()
    if compress:r.update(zlib1=n1+len(c1.flush()),zlib6=n6+len(c6.flush()))
    return r
def semantic():
    source=RR3+'E-03/semantic-inputs-after.json';d=load(source); bad=[];dig=hashlib.sha256();count=0;skip=[]
    for r in d['files']:
        p=r['path']
        if '.DS_Store' in Path(p).parts or Path(p).name=='AGENTS.md':skip.append(p);continue
        x=info(p);count+=1;dig.update(json.dumps([p,x['sha256'],x['bytes'],x['mode']],separators=(',',':')).encode()+b'\0')
        if x['sha256']!=r['sha256'] or x['bytes']!=r['bytes'] or x['mode']!=r['mode']:bad.append({'path':p,'expected':r,'actual':x})
    return {'utc':utc(),'source':source,'sourceSHA256':sha((ROOT/source).read_bytes()),'count':count,'digest':dig.hexdigest(),'differences':bad,'excludedLocalUserMetadata':skip}
def classify(name, old):
    p=ROOT/name;parts=p.parts
    if p.is_dir() and not p.is_symlink():return 'local','nested_repository'
    if p.is_symlink():return 'local','local_link'
    if name.startswith(('rust/','scripts/rust-tauri/')):return 'include','authorized_production'
    if name.startswith('docs/rust-tauri/R05/') or name=='docs/rust-tauri/ORCHESTRATOR_PROGRESS.json':return 'include','authorized_document'
    if not name.startswith(RR3):return 'unknown','outside_authorized_scope'
    # 原有分类仅在用途与身份检查后继承；最终所有普通文件均重新读取摘要。
    if name in old:return ('include' if old[name]['category'].startswith('INCLUDE') else 'local'),'prep02_inherited_role'
    if '/CLI_RUST_MIN/home/' in name:return 'local','synthetic_runtime_home'
    if '/AUTH/' in name and p.name in {'creds-before-authz.json','creds-after-authz.json','devices-before-authz.json','devices-after-authz.json'}:return 'local','synthetic_auth_state_copy'
    if '/pristine/' in name or '/snapshot-cmp-failure/' in name:return 'include','independent_restore_baseline'
    if p.name.endswith('.before'):return 'include','j02_before_source'
    if any(v in parts for v in {'target','own-target','node_modules','.git','__pycache__','incremental','.fingerprint'}):return 'local','build_cache'
    if p.suffix in {'.o','.rlib','.rmeta','.db','.sqlite','.sqlite3','.db-wal','.db-shm'}:return 'local','cache_or_database'
    if p.name in {'local-token.json','device-credentials.json','devices.json','management.json','instance.lock'} or re.fullmatch(r'p\d+-.*(?:credential|ticket).*\.body',p.name):return 'local','runtime_credential'
    if any(v in parts for v in {'isolated','runner-copy','old-red-copy','old-independent-copy'}):return 'local','fault_workcopy'
    if p.suffix in {'.rs','.toml','.lock'}:return 'unknown','new_source_role_unproven'
    return 'include','new_evidence_record'
def main():
    start=utc();kinds=enumerate_paths();oldlist=load(RR3+'DELIVERY-PREP-02/merged-paths.json');old={r['path']:r for r in oldlist}
    head=command(['git','rev-parse','HEAD']).decode().strip();branch=command(['git','branch','--show-current']).decode().strip()
    assert head=='b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b'; assert branch=='codex/rust-tauri-migration'
    assert not any('staged' in s for s in kinds.values())
    initial_index=info('.git/index');sb=semantic();save('semantic-before.json',sb)
    rows=[];changed=[];known={r['path']:r for r in load(RR3+'DELIVERY-PREP-02/local-originals.json')};historical_mismatch=[]
    for name in sorted(kinds):
        if name.startswith(OWN) or name.startswith(RR3+'E-REVIEW-03/'):continue
        cat,rule=classify(name,old);r=info(name,cat=='include');r.update(category=cat,rule=rule,gitStates=kinds[name]);rows.append(r)
        if name in old:
            prev=old[name]
            if r['bytes']!=prev['bytes'] or r['mtimeNs']!=prev['mtimeNs']:changed.append({'path':name,'previousBytes':prev['bytes'],'previousMtimeNs':prev['mtimeNs'],'nowSHA256':r['sha256'],'category':cat})
            expected=prev.get('sha256') or known.get(name,{}).get('sha256')
            if expected and expected!=r['sha256']:historical_mismatch.append({'path':name,'expected':expected,'actual':r['sha256']})
    with (OUT/'inventory.jsonl').open('w') as f:
        for r in rows:f.write(json.dumps(r,ensure_ascii=False,separators=(',',':'))+'\n')
    save('old-snapshot-delta.json',{'changedStats':changed,'historicalSHA256Mismatches':historical_mismatch,'missingOldPaths':sorted(set(old)-set(kinds)),'oldSource':RR3+'DELIVERY-PREP-02/merged-paths.json','oldSourceSHA256':sha((ROOT/(RR3+'DELIVERY-PREP-02/merged-paths.json')).read_bytes())})
    save('initial-state.json',{'startUTC':start,'endUTC':utc(),'head':head,'branch':branch,'index':initial_index,'enumerated':len(kinds),'classified':len(rows),'counts':dict(collections.Counter(r['category'] for r in rows)),'newPaths':sum(r['path'] not in old for r in rows),'isFinal':False,'awaiting':'root confirms E-REVIEW-03 stopped; then incremental enumeration and final stabilization','availableBytes':os.statvfs(ROOT).f_bavail*os.statvfs(ROOT).f_frsize})
    print(json.dumps({'paths':len(rows),'categories':dict(collections.Counter(r['category'] for r in rows)),'changedOld':len(changed),'historicalMismatch':len(historical_mismatch),'semanticDifferences':len(sb['differences'])}))
if __name__=='__main__':main()
