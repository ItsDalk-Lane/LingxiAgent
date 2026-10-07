import os,json,hashlib,pathlib,datetime,shutil
EV=pathlib.Path(__file__).resolve().parent
ROOT=EV.parents[4]
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def write(name,obj): (EV/name).write_text(json.dumps(obj,ensure_ascii=False,indent=2)+'\n')
# 仅清本轮第一次、未能形成有效反例的可重建夹具，证据日志保留。
failed=EV/'tmp/r05-node-selfcheck-v0rtfuag'
receipt={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'failedFixture':str(failed),'legacyTemporaryPath':str(EV/'tmp/r02-a16.cy5Jqx'),'legacyTemporaryPathExists':(EV/'tmp/r02-a16.cy5Jqx').exists(),'reason':'initial selfcheck fixture was nested under main repository; old CLI found ancestor ws; this attempt is INVALID_REVIEW_SETUP, raw logs retained','preserved':[str(EV/'copy'),'/private/tmp/r05-node-selfcheck-bz5g0665'],'beforeFree':shutil.disk_usage(EV).free,'failureLogSha256':sha(EV/'selfcheck.stderr.log')}
if failed.exists():
 receipt['removedFileCount']=sum(1 for p in failed.rglob('*') if p.is_file())
 receipt['removedLogicalBytes']=sum(p.lstat().st_size for p in failed.rglob('*') if p.is_file() and not p.is_symlink())
 shutil.rmtree(failed)
receipt['afterFree']=shutil.disk_usage(EV).free
write('own-invalid-fixture-cleanup.json',receipt)
sources_a=json.loads((EV/'main-sources-before.json').read_text());sources_b=json.loads((EV/'main-sources-after.json').read_text())
commands=[json.loads(line) for line in (EV/'commands.jsonl').read_text().splitlines()]
write('REVIEW_RESULT.json',{'reviewer':'rr3_j_review_01','reviewRound':'J-REVIEW-01','status':'BLOCKED_ENVIRONMENT','localNodeAndMinimumBusiness':'PASS','defaultPreparation':'BLOCKED_ENOSPC','legacyDirected':'BLOCKED_ENOSPC_BASE_CHECKOUT','R06_READY':False,'productMustFix':[],'blockingNextSteps':['new implementation and new independent review for full default isolated checkout under actual disk limits','preserve exact historical BASE content/purity and all original legacy assertions while resolving its checkout space requirement','fresh G02 default N01–N16 and complete affected R02 after preparation is independently accepted'],'sourceFileCount':len(sources_a),'sourceFilesUnchanged':sources_a==sources_b,'sourceHead':'b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b','binarySha256':'7fa13a3bc7ad8d8fde1d55d33242f0eca8b17913172daf8fe513d899c224cd7e','commandReceipts':len(commands),'fullMainGitUnchanged':False,'gitDifferenceReceipt':'main-git-differences.json','finalizedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'stopWritingAfterDelivery':True})
print(json.dumps(receipt,ensure_ascii=False))
