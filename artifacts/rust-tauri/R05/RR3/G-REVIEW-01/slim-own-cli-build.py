import pathlib,json,hashlib,subprocess,datetime,shutil,os
E=pathlib.Path(__file__).resolve().parent;C=pathlib.Path(json.loads((E/'copy-path.json').read_text())['path'])
receipt=pathlib.Path(__import__('sys').argv[1]);doc=json.loads(receipt.read_text());binary=pathlib.Path(doc['binaryPath']);target=binary.parent.parent
assert target.name.startswith('r02-cli-rust-target-') and target.is_absolute() and binary.is_file()
assert str(receipt).startswith(str(E/'default16-01')) or str(receipt).startswith(str(E/'supplements'))
assert hashlib.sha256(binary.read_bytes()).hexdigest()==doc['binarySha256']
start=datetime.datetime.fromisoformat(json.loads((E/'reviewer-identity.json').read_text()).get('createdUTC','2026-10-07T01:24:42+00:00')).timestamp()
assert target.stat().st_birthtime>=start
ps=subprocess.check_output(['ps','-axo','pid,ppid,command'],text=True);active=[l for l in ps.splitlines() if str(target) in l and 'slim-own-cli-build.py' not in l];assert not active,active
exec((E/'run-default.py').read_text().split("save('source-before.json'")[0]); before=snap(C); removed=[];free=shutil.disk_usage(E).free
for name in ['deps','incremental','build','.fingerprint']:
 p=target/'debug'/name
 if not p.exists():continue
 files=[]
 for f in p.rglob('*'):
  if f.is_file():files.append({'path':str(f.relative_to(target)),'bytes':f.stat().st_size,'sha256':hashlib.sha256(f.read_bytes()).hexdigest()})
 removed.append({'directory':str(p),'files':files,'bytes':sum(f['bytes'] for f in files)})
 shutil.rmtree(p)
after=snap(C);now=datetime.datetime.now(datetime.timezone.utc).isoformat();out={'utc':now,'sourceReceipt':str(receipt),'sourceReceiptSHA256':hashlib.sha256(receipt.read_bytes()).hexdigest(),'taskCreatedTarget':str(target),'creationUTC':datetime.datetime.fromtimestamp(target.stat().st_birthtime,datetime.timezone.utc).isoformat(),'retainedBinary':str(binary),'binarySHA256Before':doc['binarySha256'],'binarySHA256After':hashlib.sha256(binary.read_bytes()).hexdigest(),'sourceCopyEqualBeforeAfter':before==after,'removedOwnBuildCache':removed,'freeBefore':free,'freeAfter':shutil.disk_usage(E).free,'boundary':'只清本次生产脚本新建且已停止的CLI独立编译缓存；原路径真实service二进制/证据/源码/Git/共享NEG_TARGET及其他历史副本全部保留'}
outpath=E/('own-cli-cache-'+target.name+'.json');outpath.write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'receipt':str(outpath),'sourceEqual':before==after,'binaryEqual':out['binarySHA256Before']==out['binarySHA256After'],'freed':out['freeAfter']-free}))
