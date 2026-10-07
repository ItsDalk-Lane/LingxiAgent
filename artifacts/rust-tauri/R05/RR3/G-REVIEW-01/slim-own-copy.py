import pathlib,subprocess,json,hashlib,datetime,shutil
R=pathlib.Path.cwd(); E=R/'artifacts/rust-tauri/R05/RR3/G-REVIEW-01'; C=pathlib.Path(json.loads((E/'copy-path.json').read_text())['path'])
assert C==pathlib.Path('/Users/study_superior/r05t08-work/negcopy.hTXzzN')
def q(root,*a): return subprocess.check_output(['git','-C',str(root),*a])
def sha(b): return hashlib.sha256(b).hexdigest()
def ident(root):
 return {'head':q(root,'rev-parse','HEAD').decode().strip(),'tree':q(root,'rev-parse','HEAD^{tree}').decode().strip(),'refs':sha(q(root,'show-ref')),'status':sha(q(root,'status','--porcelain=v1','-z','--untracked-files=all')),'index':sha((root/'.git/index').read_bytes())}
start=datetime.datetime.now(datetime.timezone.utc).isoformat(); before=ident(C); mainBefore=ident(R)
obj=C/'.git/objects'; inv=[]
for p in obj.rglob('*'):
 if p.is_file():inv.append({'path':str(p.relative_to(obj)),'bytes':p.stat().st_size,'sha256':sha(p.read_bytes())})
source=pathlib.Path(q(R,'rev-parse','--git-path','objects').decode().strip()).resolve(); assert source==R/'.git/objects'
backup=C/'.git/objects-own-fresh-clone-backup'; obj.rename(backup); (obj/'info').mkdir(parents=True); (obj/'info/alternates').write_text(str(source)+'\n')
after=ident(C); mainAfter=ident(R)
if before!=after or mainBefore!=mainAfter:
 shutil.rmtree(obj);backup.rename(obj);raise RuntimeError('identity mismatch: own original objectstore restored')
shutil.rmtree(backup)
doc={'startUTC':start,'endUTC':datetime.datetime.now(datetime.timezone.utc).isoformat(),'copy':str(C),'sourceReadOnlyObjectStore':str(source),'before':before,'after':after,'mainBefore':mainBefore,'mainAfter':mainAfter,'copyIdentityEqual':before==after,'mainIdentityEqual':mainBefore==mainAfter,'removedOnlyTaskCreatedDuplicateObjects':inv,'bytesFreed':sum(x['bytes'] for x in inv),'boundary':'only this run fresh clone duplicate objects; source, refs, index and evidence retained; no main Git write or other copy deletion'}
(E/'own-storage-receipt.json').write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'bytesFreed':doc['bytesFreed'],'identityEqual':before==after,'mainIdentityEqual':mainBefore==mainAfter}))
