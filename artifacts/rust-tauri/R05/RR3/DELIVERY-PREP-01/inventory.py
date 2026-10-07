#!/usr/bin/env python3
"""只读清点仓库候选；仅向本报告目录写入，不遍历忽略的构建缓存。"""
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess

ROOT = Path.cwd()
RR3 = 'artifacts/rust-tauri/R05/RR3/'
OUT = ROOT / RR3 / 'DELIVERY-PREP-01'
COMMANDS = []
def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(data):
    return hashlib.sha256(data).hexdigest()
def dump(name, value):
    (OUT/name).write_text(json.dumps(value, ensure_ascii=False, indent=2)+'\n')
def command(argv, name):
    start = utc()
    p = subprocess.run(argv, cwd=ROOT, capture_output=True, env={**os.environ, 'GIT_OPTIONAL_LOCKS':'0'})
    (OUT/(name+'.stdout')).write_bytes(p.stdout)
    (OUT/(name+'.stderr')).write_bytes(p.stderr)
    COMMANDS.append(dict(argv=argv,cwd=str(ROOT),startUTC=start,endUTC=utc(),exitCode=p.returncode,
                         stdout=name+'.stdout',stdoutSHA256=sha(p.stdout),stderr=name+'.stderr',stderrSHA256=sha(p.stderr)))
    if p.returncode: raise RuntimeError(argv)
    return p.stdout
def split(data):
    return [x.decode() for x in data.split(b'\0') if x]
started = utc()
head=command(['git','rev-parse','HEAD'],'head').decode().strip()
branch=command(['git','branch','--show-current'],'branch').decode().strip()
status=command(['git','status','--porcelain=v1','-z','--untracked-files=all'],'status')
untracked=split(command(['git','ls-files','--others','--exclude-standard','-z'],'untracked'))
modified=split(command(['git','diff','--name-only','-z'],'modified'))
staged=split(command(['git','diff','--cached','--name-only','-z'],'staged'))
ignored=split(command(['git','ls-files','--others','--ignored','--exclude-standard','--directory','-z','--',RR3],'ignored-directories'))
command(['git','diff','--stat'],'diff-stat')
paths=sorted(set(untracked+modified+staged)-{p for p in untracked if p.startswith(str(OUT.relative_to(ROOT))+'/')})
cacheparts={'target','own-target','node_modules','.git','__pycache__','incremental','.fingerprint'}
copyroots={'isolated','runner-copy','old-red-copy','old-red-pristine'}
runtime_names={'local-token.json','device-credentials.json','devices.json','management.json','instance.lock'}
binary_magic={b'\xcf\xfa\xed\xfe',b'\xce\xfa\xed\xfe',b'\xfe\xed\xfa\xcf',b'\xfe\xed\xfa\xce',b'\xca\xfe\xba\xbe',b'\x7fELF'}
rows={}
for name in paths:
    p=ROOT/name
    try: s=p.lstat()
    except FileNotFoundError:
        rows[name]=dict(path=name,category='REFRESH_DISAPPEARED',bytes=None,type='missing');continue
    parts=p.relative_to(ROOT).parts
    group=parts[4] if name.startswith(RR3) else ('production' if name.startswith(('rust/','scripts/')) else 'current-docs')
    category='INCLUDE_EVIDENCE'; reason='正式证据、输入摘要、日志、测量或复现材料；按路径逐项交付，仍需最终内容检查'
    typ='regular'; header=b''
    if stat.S_ISLNK(s.st_mode):
        typ='symlink';category='LOCAL_SYMLINK';reason='本机绝对工具链接不能作为远端可用工具'
    elif stat.S_ISDIR(s.st_mode):
        typ='directory';category='LOCAL_NESTED_REPOSITORY';reason='Git只列目录的嵌套试验仓库/副本，不递归接收也不变成gitlink；内部未展开清点'
    elif not stat.S_ISREG(s.st_mode):
        typ='special';category='REVIEW_SPECIAL';reason='非普通文件，禁止自动暂存'
    elif name.startswith(('rust/','scripts/')):
        category='INCLUDE_PRODUCTION';reason='生产修复/永久回归候选，当前仍需最终冻结复核'
    elif name.startswith('docs/'):
        category='INCLUDE_CURRENT_DOC';reason='当前交付文档/工作说明，后续E须统一真实最终状态'
    elif any(x in cacheparts for x in parts) or p.suffix in {'.o','.rlib','.rmeta'}:
        category='LOCAL_BUILD_CACHE';reason='构建中间对象/依赖缓存，原件本地保留，不混入提交'
        if not p.suffix and 'incremental' not in parts:
            with p.open('rb') as f: header=f.read(4)
            if header in binary_magic or header[:2]==b'MZ':
                typ='executable-binary';category='LOCAL_BINARY';reason='缓存目录内实际可执行装备；保留其身份但不把缓存目录带入提交'
    elif any(x == 'home' or x.startswith('home-') for x in parts) or p.name in runtime_names or p.suffix in {'.db','.sqlite','.sqlite3','.pem','.key'} or re.search(r'p\d+-.*credential.*\.body$',p.name):
        category='LOCAL_RUNTIME_STATE';reason='测试运行根/一次性凭证或数据库；未读原文，不等于真实用户秘密，须保留来源与脱敏摘要'
    else:
        with p.open('rb') as f: header=f.read(4)
        if header in binary_magic or header[:2]==b'MZ':
            typ='executable-binary';category='LOCAL_BINARY';reason='实际运行装备仅本机保留，交付真实SHA及构建/运行来源；重建不等于历史字节原件'
        elif any(x in copyroots for x in parts):
            category='LOCAL_ISOLATED_SOURCE';reason='隔离源码/故障注入工作副本不可整树提交；保留driver、变异说明与输入摘要作为复现依据'
        elif p.suffix in {'.rs','.toml','.lock'} or (p.suffix in {'.sh','.py','.js','.ts','.json'} and any(x in {'repo','copy','pristine','mutated','snapshot','source-before'} for x in parts)):
            category='REVIEW_SOURCE_SNAPSHOT';reason='源码/配置快照须确认非故障注入工作副本后逐文件保留，不按整个目录接收'
    row=dict(path=name,gitState='modified' if name in modified else ('staged' if name in staged else 'untracked'),owner=group,
             category=category,type=typ,bytes=s.st_size,mtimeNs=s.st_mtime_ns,reason=reason,
             over100MiB=s.st_size>100*1024*1024,referenceSources=[])
    if typ=='symlink': row['linkTarget']=os.readlink(p)
    rows[name]=row

# 只读正式报告和摘要/清单，避开运行凭证、工作副本与构建缓存；绝不读仓库外路径。
metadata=[]; refs=[]; localfields=[]; readhash=[]; unresolved=[]; declared_roots={}
def consider(source, token, locator, claimed=None):
    if not isinstance(token,str) or '\n' in token or len(token)>1200 or '://' in token: return
    token=token.split('#')[0].strip('`')
    if not token or not ('/' in token or re.search(r'\.(?:json|jsonl|log|md|rs|py|sh|tsv|csv|rlib|o|txt|gz)$',token)): return
    sp=Path(source); own=declared_roots.get(source,ROOT/Path(*sp.parts[:5]) if source.startswith(RR3) else ROOT/sp.parent)
    if source.endswith('/G-REVIEW-01/own-storage-receipt.json'):
        if re.fullmatch('[0-9a-f]{2}/[0-9a-f]{38}',token):
            refs.append(dict(source=source,locator=locator,target=token,status='HISTORICAL_DELETED_CLONE_OBJECT_RECEIPT',claimedSHA256=claimed));return
    if token.startswith(str(ROOT)+'/'): guesses=[Path(token)]
    elif token.startswith('/'):
        # 仅保留引用标识，不访问真实用户home/外置缓存。
        if '/r05' in token or '/artifacts/' in token:
            refs.append(dict(source=source,locator=locator,target=token,status='EXTERNAL_NOT_ACCESSED',claimedSHA256=claimed))
        return
    elif token.startswith(('artifacts/','docs/','rust/','scripts/')): guesses=[ROOT/token,own/token,ROOT/sp.parent/token]
    else: guesses=[own/token,ROOT/sp.parent/token,ROOT/token]
    target=None
    for q in guesses:
        q=Path(os.path.normpath(q))
        if not q.is_relative_to(ROOT): continue
        if q.is_symlink() or q.exists(): target=q;break
    if target is None:
        # 只把结构化清单内的候选原件记为缺失；一般散文不强推成路径。
        if claimed or token.startswith(RR3):
            q=guesses[0]
            refs.append(dict(source=source,locator=locator,target=str(q.relative_to(ROOT)) if q.is_relative_to(ROOT) else str(q),status='UNRESOLVED_REFERENCE_NOT_PROVEN_MISSING',claimedSHA256=claimed))
        return
    name=str(target.relative_to(ROOT))
    if not name.startswith(RR3): return
    status='PRESENT_DIRECTORY' if target.is_dir() and not target.is_symlink() else 'PRESENT_LOCAL'
    row=rows.get(name)
    if row and source not in row['referenceSources']: row['referenceSources'].append(source)
    refs.append(dict(source=source,locator=locator,target=name,status=status,category=row.get('category') if row else 'IGNORED_OR_TRACKED',claimedSHA256=claimed))
def walk(source,obj,loc=''):
    if isinstance(obj,dict):
        claim=obj.get('sha256') or obj.get('SHA256')
        claim=claim if isinstance(claim,str) and re.fullmatch(r'[0-9a-fA-F]{64}',claim) else None
        for k,v in obj.items():
            if k.lower() in {'localonly','local_only','local-only'}: localfields.append(dict(source=source,locator=loc+'/'+k,value=v))
            ownclaim=v.get('sha256') or v.get('SHA256') if isinstance(v,dict) else None
            consider(source,k,loc+'/'+k,ownclaim if isinstance(ownclaim,str) and re.fullmatch('[0-9a-fA-F]{64}',ownclaim) else None)
            if isinstance(v,str): consider(source,v,loc+'/'+k,claim if k.lower() in {'path','file','artifact','relativepath'} else None)
            else: walk(source,v,loc+'/'+k)
    elif isinstance(obj,list):
        for i,v in enumerate(obj):
            if isinstance(v,str):consider(source,v,loc+'/'+str(i))
            else:walk(source,v,loc+'/'+str(i))
for name,row in rows.items():
    p=ROOT/name
    if row['category'].startswith('LOCAL_') or row['category'] in {'REVIEW_SOURCE_SNAPSHOT','REFRESH_DISAPPEARED'}:continue
    base=p.name.lower()
    ismeta=p.suffix=='.md' or (p.suffix=='.json' and (any(w in base for w in ('manifest','digest','hash','binding','reference','receipt')) or (name.startswith('docs/') and name in modified)))
    if not ismeta:continue
    data=p.read_bytes();readhash.append(dict(path=name,bytes=len(data),sha256=sha(data)));metadata.append(name)
    text=data.decode('utf-8',errors='replace')
    if p.suffix=='.json':
        try:
            obj=json.loads(text)
            if isinstance(obj,dict) and isinstance(obj.get('manifest'),str) and obj['manifest'].startswith(RR3):
                declared_roots[name]=ROOT/Path(obj['manifest']).parent
            if name.endswith('/G-INTERRUPTION-01/r02-and-binding-index.json'):
                declared_roots[name]=ROOT/RR3/'G-REVIEW-01'
            walk(name,obj)
        except json.JSONDecodeError as e:unresolved.append(dict(path=name,reason=str(e)))
    else:
        for lineno,line in enumerate(text.splitlines(),1):
            for token in re.findall(r'`([^`]+)`|\]\(([^)]+)\)',line):consider(name,next(t for t in token if t),f'line:{lineno}')

# 大原件及被正式引用的隔离源码只哈希一次；数千缓存对象完全不重新逐字节哈希。
localrecords=[]
for name,row in rows.items():
    if row['category'] in {'LOCAL_BINARY','LOCAL_ISOLATED_SOURCE','LOCAL_SYMLINK'} or row['over100MiB']:
        if row['category']=='LOCAL_ISOLATED_SOURCE' and not row['referenceSources']:continue
        p=ROOT/name
        data=os.readlink(p).encode() if p.is_symlink() else p.read_bytes()
        row['sha256']=sha(data)
        localrecords.append(dict(path=name,type=row['type'],bytes=row['bytes'],sha256=row['sha256'],
                                 hashMeaning='link-text' if p.is_symlink() else 'file-content',localOnly=True,
                                 remoteOriginalAvailable=False,referenceSources=row['referenceSources'],
                                 reproductionStatus='SEE_OWNER_DRIVERS_AND_COMMANDS_NOT_REEXECUTED'))
    elif row['category'] in {'INCLUDE_PRODUCTION','INCLUDE_CURRENT_DOC'}:
        row['sha256']=sha((ROOT/name).read_bytes())

counts=collections.Counter(r['category'] for r in rows.values())
sizes=collections.Counter()
for r in rows.values():sizes[r['category']]+=r.get('bytes') or 0
dump('paths.json',list(rows.values()))
dump('references.json',refs)
dump('metadata-read-hashes.json',readhash)
dump('local-originals.json',localrecords)
dump('existing-localOnly-fields.json',localfields)
dump('metadata-parse-errors.json',unresolved)
dump('summary.json',dict(startUTC=started,endUTC=utc(),head=head,branch=branch,isFinalFreeze=False,
    totalPaths=len(rows),totalBytes=sum(sizes.values()),categories={k:dict(paths=counts[k],bytes=sizes[k]) for k in sorted(counts)},
    metadataReadCount=len(metadata),referenceCount=len(refs),referenceStatusCounts=dict(collections.Counter(x['status'] for x in refs)),
    existingExplicitLocalOnlyFields=len(localfields),stagedCount=len(staged),ignoredDirectoryOrFileCount=len(ignored),
    requiredFinalRefresh='J及后续G/E/FINAL完成停写后，以相同规则刷新；当前清单不能用于宣称最终冻结或远端原件可达'))
for kind in ('INCLUDE','LOCAL','REVIEW','REFRESH'):
    (OUT/(kind.lower()+'-paths.txt')).write_text(''.join(n+'\n' for n,r in rows.items() if r['category'].startswith(kind)))
command(['git','check-ignore','-v','--stdin'],'ignore-rule-evidence') if False else None
dump('commands.json',COMMANDS)
print(json.dumps(dict(paths=len(rows),categories=dict(counts),references=len(refs),localOriginals=len(localrecords),metadataParseErrors=len(unresolved)),ensure_ascii=False))
