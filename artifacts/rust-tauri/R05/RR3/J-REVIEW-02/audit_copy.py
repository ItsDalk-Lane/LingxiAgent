import os,json,subprocess,hashlib,stat,datetime
from pathlib import Path
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent');EV=ROOT/'artifacts/rust-tauri/R05/RR3/J-REVIEW-02';COPY=Path((EV/'copy-path.txt').read_text().strip())
ENV=dict(os.environ,GIT_OPTIONAL_LOCKS='0')
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(EV/n).write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def git(root,*args):return subprocess.check_output(['git','-C',str(root),*args],env=ENV)
start=datetime.datetime.now(datetime.timezone.utc).isoformat();head=git(ROOT,'rev-parse','HEAD').decode().strip();rows=[]
tree=git(ROOT,'ls-tree','-rlz',head);index=git(COPY,'ls-files','-sz');(EV/'head-tree.nul').write_bytes(tree);(EV/'copy-index.nul').write_bytes(index)
expected={}
for row in tree.split(b'\0'):
 if row:
  fields,path=row.split(b'\t',1);mode,kind,oid,size=fields.split();expected[os.fsdecode(path)]=(mode.decode(),oid.decode(),int(size))
indexrows={}
for row in index.split(b'\0'):
 if row:
  fields,path=row.split(b'\t',1);mode,oid,stage=fields.split();assert stage==b'0';indexrows[os.fsdecode(path)]=(mode.decode(),oid.decode())
assert set(expected)==set(indexrows)
materialized=json.loads((EV/'default-prefix/git-preparation/tracked-files.json').read_text());assert len(materialized)==len(expected)
for row in materialized:assert (row['mode'],row['oid'],row['size'])==expected[row['path']]
counts={'tracked':len(expected),'trackedArtifacts':0,'sourceCandidateContent':0,'exactHeadContent':0,'independentInodes':0};differences=[]
for name,(mode,oid,size) in expected.items():
 src=ROOT/name;dst=COPY/name;di=dst.lstat();overlay=name.startswith(('rust/','scripts/','docs/rust-tauri/')) or name in ['package.json','package-lock.json','.npmrc']
 if name.startswith('artifacts/'):counts['trackedArtifacts']+=1
 if src.exists() and not src.is_symlink():
  si=src.stat();assert (si.st_dev,si.st_ino)!=(di.st_dev,di.st_ino),name;counts['independentInodes']+=1
 if overlay:
  assert sha(src)==sha(dst),name;assert stat.S_IMODE(src.stat().st_mode)==stat.S_IMODE(di.st_mode),name;counts['sourceCandidateContent']+=1
 else:
  h=hashlib.sha1();h.update(f'blob {di.st_size}\0'.encode())
  with dst.open('rb') as f:
   while block:=f.read(1024*1024):h.update(block)
  assert h.hexdigest()==oid,name;assert stat.S_IMODE(di.st_mode)==(0o755 if mode=='100755' else 0o644),name;counts['exactHeadContent']+=1
 assert indexrows[name]==(mode,oid),name
for name in ['.git/HEAD','.git/index']:
 a=(ROOT/name).stat();b=(COPY/name).stat();assert (a.st_dev,a.st_ino)!=(b.st_dev,b.st_ino);counts['independentInodes']+=1
assert git(COPY,'rev-parse','HEAD').decode().strip()==head
assert git(COPY,'diff','--cached','--exit-code')==b''
prior=ROOT/'artifacts/rust-tauri/R05/RR3/J-REVIEW-01';old=json.loads((prior/'node-preparation/result.json').read_text());new=json.loads((EV/'default-prefix/node-preparation/result.json').read_text())
node={'dependencyManifestEqual':old['dependencyManifestSha256']==new['dependencyManifestSha256'],'toolsEqual':old['tools']==new['tools'],'inputsEqual':old['inputs']==new['inputs'],'metadataEqual':old['metadata']==new['metadata'],'manifestSha256':new['dependencyManifestSha256'],'tools':new['tools']}
assert all(node[k] for k in ['dependencyManifestEqual','toolsEqual','inputsEqual','metadataEqual'])
h02=json.loads((prior/'H02-source-binary-reuse.json').read_text());inputs={p:{'expected':v['expected'],'main':sha(ROOT/p),'copy':sha(COPY/p)} for p,v in h02['inputs'].items()};assert all(v['expected']==v['main']==v['copy'] for v in inputs.values());assert sha(Path(h02['binary']))==h02['binarySha256']
save('H02-source-binary-reuse.json',{'inputs':inputs,'binary':h02['binary'],'binarySha256':h02['binarySha256'],'count':len(inputs),'scope':'仅按375输入精确相等复用历史服务实物；本审无新构建'})
save('node-prior-evidence-reuse.json',node)
save('independent-full-copy-audit.json',{'startedUtc':start,'finishedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'copy':str(COPY),'head':head,'counts':counts,'indexMatchesAllHeadEntries':True,'materializationManifestMatchesRealTree':True,'copyHeadIndexIndependent':True,'fullGitCachedDiffEmpty':True,'boundary':'物化时HEAD清单逐项与真实Git核对；覆盖后依原candidate overlay比较，不能把当前dirty字节冒称HEAD'})
print(counts)
