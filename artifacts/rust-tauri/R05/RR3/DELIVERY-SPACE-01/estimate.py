#!/usr/bin/env python3
# 只读估算：不写 Git、不执行历史程序、不创建压缩副本。
import os, sys, json, hashlib, zlib, subprocess, pathlib, stat, struct, collections, datetime, math
ROOT=pathlib.Path('/Users/study_superior/Desktop/Code/LingxiAgent')
OUT=pathlib.Path('/private/tmp/rr3-delivery-space-20261007')
PREP=ROOT/'artifacts/rust-tauri/R05/RR3/DELIVERY-PREP-02'
ENV=dict(os.environ, GIT_OPTIONAL_LOCKS='0', GIT_NO_LAZY_FETCH='1', PYTHONDONTWRITEBYTECODE='1')
def git(*args,data=None):
    r=subprocess.run(['git','-C',str(ROOT),*args],input=data,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=ENV)
    if r.returncode: raise RuntimeError((args,r.returncode,r.stderr.decode(errors='replace')))
    return r.stdout

def fs():
    v=os.statvfs(ROOT)
    return {'bsize':v.f_bsize,'frsize':v.f_frsize,'available':v.f_bavail*v.f_frsize}

def identity(p):
    s=p.stat(); return {'size':s.st_size,'mtime_ns':s.st_mtime_ns,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}

def compressed(p,normalize=False):
    size=p.stat().st_size
    if normalize:
        # 仅在只读 Git 哈希证明 LF 转换时使用；不自行推断 text=auto。
        size=0; tail=b''
        with p.open('rb') as f:
            for chunk in iter(lambda:f.read(1024*1024),b''):
                chunk=tail+chunk; tail=b'\r' if chunk.endswith(b'\r') else b''
                if tail: chunk=chunk[:-1]
                size+=len(chunk.replace(b'\r\n',b'\n'))
        size+=len(tail)
    header=('blob '+str(size)+'\0').encode(); h1=hashlib.sha1(header); h2=hashlib.sha256(header); raw=hashlib.sha256()
    zs={n:zlib.compressobj(n) for n in (1,6)}; lens={n:len(c.compress(header)) for n,c in zs.items()}; crlf=False; tail=b''
    with p.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):
            raw.update(chunk); crlf=crlf or b'\r\n' in tail+chunk
            if normalize:
                chunk=tail+chunk; tail=b'\r' if chunk.endswith(b'\r') else b''
                if tail: chunk=chunk[:-1]
                chunk=chunk.replace(b'\r\n',b'\n')
            else: tail=chunk[-1:]
            h1.update(chunk);h2.update(chunk)
            for n,c in zs.items(): lens[n]+=len(c.compress(chunk))
    if normalize and tail:
        h1.update(tail);h2.update(tail)
        for n,c in zs.items():lens[n]+=len(c.compress(tail))
    for n,c in zs.items():lens[n]+=len(c.flush())
    return {'oid':h1.hexdigest(),'git_sha256':h2.hexdigest(),'content_sha256':raw.hexdigest(),'bytes':size,'z1':lens[1],'z6':lens[6],'crlf':crlf}

def aggregate(objs):
    block=4096
    return {'objects':len(objs),'logical':sum(o['bytes'] for o in objs),'compressed_level1':sum(o['z1'] for o in objs),'compressed_level6':sum(o['z6'] for o in objs),'level1_4k_blocks':sum(math.ceil(o['z1']/block)*block for o in objs),'level6_4k_blocks':sum(math.ceil(o['z6']/block)*block for o in objs),'max_blob':max((o['bytes'] for o in objs),default=0),'max_z1':max((o['z1'] for o in objs),default=0)}

start=datetime.datetime.now(datetime.timezone.utc).isoformat(); before=fs(); index_before=identity(ROOT/'.git/index')
rows=json.loads((PREP/'merged-paths.json').read_text()); include=(PREP/'include-paths.txt').read_text().splitlines(); bypath={r['path']:r for r in rows}; skipped=[]; measured=[]
for path in include:
    r=bypath[path];p=ROOT/path
    if r['category']=='INCLUDE_CURRENT_DOC':
        skipped.append({'path':path,'reason':'current_document_owner_active_or_coordinator','old_bytes':r['bytes']});continue
    try:s=p.lstat()
    except FileNotFoundError:skipped.append({'path':path,'reason':'missing','old_bytes':r['bytes']});continue
    if not stat.S_ISREG(s.st_mode):skipped.append({'path':path,'reason':'not_regular','old_bytes':r['bytes']});continue
    if s.st_size!=r['bytes'] or s.st_mtime_ns!=r['mtimeNs']:
        skipped.append({'path':path,'reason':'old_metadata_changed_not_source_drift_judgment','old_bytes':r['bytes'],'current_bytes':s.st_size});continue
    o=compressed(p);after=p.stat()
    if (s.st_ino,s.st_size,s.st_mtime_ns)!=(after.st_ino,after.st_size,after.st_mtime_ns):raise RuntimeError('read changed: '+path)
    if 'sha256'in r and o['content_sha256']!=r['sha256']:raise RuntimeError('old SHA mismatch: '+path)
    o.update(path=path,mode='100755' if s.st_mode&0o111 else '100644',old_sha_checked='sha256'in r,category=r['category'],size_on_disk=s.st_blocks*512)
    measured.append(o)
    if len(measured)%2000==0:print('measured',len(measured),flush=True)
# 验证实际属性与 Git 转换结果。没有 -w；先排除会运行外部 clean 过滤器的属性。
paths=[o['path'] for o in measured];attr=git('check-attr','-z','--stdin','filter','text','eol','working-tree-encoding',data=b'\0'.join(os.fsencode(p) for p in paths)+b'\0').split(b'\0');attrs=collections.Counter()
for i in range(0,len(attr)-1,3):
    path,key,val=attr[i:i+3];attrs[(key.decode(),val.decode())]+=1
    if key==b'filter' and val not in (b'unspecified',b'unset'):raise RuntimeError('external filter requires separate analysis')
    if key==b'working-tree-encoding' and val!=b'unspecified':raise RuntimeError('encoding requires separate analysis')
hashes=git('hash-object','--stdin-paths',data=('\n'.join(paths)+'\n').encode()).decode().splitlines();assert len(hashes)==len(measured)
normalized=[]
for o,actual in zip(measured,hashes):
    if o['oid']!=actual:
        n=compressed(ROOT/o['path'],True)
        if n['oid']!=actual:raise RuntimeError('unmodelled conversion: '+o['path'])
        normalized.append(o['path']);o.update(n)
unique={o['oid']:o for o in measured};names=sorted(unique)
check=git('cat-file','--batch-check=%(objectname) %(objecttype) %(objectsize) %(objectsize:disk)',data=('\n'.join(names)+'\n').encode()).decode().splitlines();existing=[];missing=[]
for oid,line in zip(names,check):
    bits=line.split();assert bits[0]==oid
    if bits[1]=='missing':missing.append(unique[oid])
    else:
        assert bits[1]=='blob' and int(bits[2])==unique[oid]['bytes'];existing.append(unique[oid])
# 只使用当前 index 元数据建容量模型，不写 tree 或新 index，不生成可暂存清单。
staged=git('ls-files','--stage','-z').split(b'\0');tracked={};index_entries=0
for rec in staged:
    if not rec:continue
    meta,path=rec.split(b'\t',1);mode,oid,stage=meta.split();index_entries+=1
    if stage!=b'0':raise RuntimeError('unmerged index')
    tracked[os.fsdecode(path)]=(mode.decode(),oid.decode())
model=dict(tracked)
for o in measured:model[o['path']]=(o['mode'],o['oid'])
new_entries=len(set(model)-set(tracked));index_version=struct.unpack('>I',(ROOT/'.git/index').read_bytes()[4:8])[0]
v2_size=12+20+sum(((62+len(os.fsencode(p))+1+7)//8)*8 for p in model)
# 每一级目录的 tree 内容上限；所有 tree 按新对象计，避免依赖尚未存在的最终 tree。
trees=collections.defaultdict(dict)
for path,(mode,oid) in model.items():
    parts=path.split('/');parent='/'.join(parts[:-1]);trees[parent][parts[-1]]=(mode,oid)
    for i in range(len(parts)-1):trees['/'.join(parts[:i])][parts[i]]=('40000','0'*40)
tree_z=tree_alloc=tree_raw=tree_bound=tree_bound_alloc=0
for entries in trees.values():
    body=b''.join(mode.encode()+b' '+os.fsencode(name)+b'\0'+bytes.fromhex(oid) for name,(mode,oid) in sorted(entries.items()))
    content=('tree '+str(len(body))+'\0').encode()+body;size=len(zlib.compress(content,1));tree_raw+=len(content);tree_z+=size;tree_alloc+=math.ceil(size/4096)*4096
    n=len(content);bound=n+(n>>12)+(n>>14)+(n>>25)+13;tree_bound+=bound;tree_bound_alloc+=math.ceil(bound/4096)*4096
# 新材料只统计类型/体积，不接收为交付清单，不跟随链接/嵌套仓库/依赖/缓存。
new_names=['DELIVERY-PREP-01','DELIVERY-PREP-02','DOC-INPUT-BOUNDARY-01','J-02','J-REVIEW-01','J-REVIEW-02','G-REVIEW-02','STORAGE-03']
new_summary={}
for name in new_names:
    base=ROOT/'artifacts/rust-tauri/R05/RR3'/name;stats=collections.defaultdict(lambda:collections.Counter());pruned=[]
    for dirpath,dirs,files in os.walk(base,followlinks=False):
        d=pathlib.Path(dirpath);relative=d.relative_to(base);bucket=relative.parts[0] if relative.parts else '(root files)'
        if relative.parts and ('.git'in dirs or '.git'in files or d.name in ('node_modules','target','__pycache__')):
            pruned.append(str(relative));dirs[:]=[];continue
        for fn in files:
            p=d/fn;s=p.lstat();st=stats[bucket]
            if stat.S_ISREG(s.st_mode):st['regular_files']+=1;st['bytes']+=s.st_size;st['blocks']+=s.st_blocks*512;st['round4k_upper']+=max(4096,math.ceil(s.st_size/4096)*4096);st['zlib_compressBound_upper']+=s.st_size+(s.st_size>>12)+(s.st_size>>14)+(s.st_size>>25)+13+64
            elif stat.S_ISLNK(s.st_mode):st['symlinks']+=1;st['symlink_bytes']+=s.st_size
            else:st['special']+=1
        for dn in dirs:
            p=d/dn
            if p.is_symlink():stats[bucket]['symlink_dirs']+=1
    new_summary[name]={'buckets':{k:dict(v)for k,v in sorted(stats.items())},'pruned_no_size_claim':pruned}
# 只存摘要；滚动摘要绑定所测路径、内容和 Git 形式，避免再生成数十 MB 库存。
bind=hashlib.sha256();git256bind=hashlib.sha256()
for o in sorted(measured,key=lambda x:x['path']):
    bind.update((o['path']+'\0'+o['content_sha256']+'\0'+o['oid']+'\0'+str(o['z1'])+'\n').encode());git256bind.update((o['path']+'\0'+o['git_sha256']+'\n').encode())
index_after=identity(ROOT/'.git/index');assert index_before==index_after
summary={'started_utc':start,'ended_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'repository':str(ROOT),'git_version':git('--version').decode().strip(),'head':git('rev-parse','HEAD').decode().strip(),'branch':git('symbolic-ref','--short','HEAD').decode().strip(),'object_format':git('rev-parse','--show-object-format').decode().strip(),'zlib_runtime':zlib.ZLIB_RUNTIME_VERSION,'before_fs':before,'after_fs':fs(),'inputs':{p.name:identity(p) for p in (PREP/'merged-paths.json',PREP/'include-paths.txt')},'old_include_count':len(include),'old_include_bytes':sum(bypath[p]['bytes'] for p in include),'measured_paths':aggregate(measured),'unique_blobs':aggregate(list(unique.values())),'existing_blobs':aggregate(existing),'missing_blobs':aggregate(missing),'duplicate_path_count':len(measured)-len(unique),'old_content_sha_checked':sum(o['old_sha_checked']for o in measured),'old_metadata_only_count':sum(not o['old_sha_checked']for o in measured),'raw_sha256_and_git_sha1_digest':bind.hexdigest(),'git_sha256_digest':git256bind.hexdigest(),'normalized_paths':normalized,'attributes':[{'name':k[0],'value':k[1],'count':v}for k,v in sorted(attrs.items())],'skipped':skipped,'index':{'before':index_before,'after':index_after,'version':index_version,'existing_entries':index_entries,'model_entries':len(model),'added_entries':new_entries,'v2_entry_bytes_model':v2_size},'tree_upper_model':{'all_directories':len(trees),'uncompressed':tree_raw,'synthetic_zero_subtree_oid_zlib_level1_not_upper':tree_z,'synthetic_round4k_not_upper':tree_alloc,'compressBound_upper':tree_bound,'compressBound_round4k_upper':tree_bound_alloc},'missing_oid_directories':len(set(o['oid'][:2]for o in missing)),'missing_oid_new_directories':len(set(o['oid'][:2]for o in missing) - set(p.name for p in (ROOT/'.git/objects').iterdir() if p.is_dir())),'new_unclassified_directory_stats':new_summary,'e03_and_future_outputs':'UNKNOWN; active current documents not read/frozen; E03/Ereview03/final inventory not sized','warning':'PREP02旧列表及本容量模型均不是最终授权stage列表；既有元数据相等不等于旧字节逐项有历史SHA证明。'}
(OUT/'estimate-summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:summary[k]for k in ('before_fs','after_fs','old_include_count','measured_paths','unique_blobs','existing_blobs','missing_blobs','duplicate_path_count','old_content_sha_checked','old_metadata_only_count','index','tree_upper_model','missing_oid_directories','missing_oid_new_directories')},indent=2));print('skipped',len(skipped),'normalized',len(normalized))
