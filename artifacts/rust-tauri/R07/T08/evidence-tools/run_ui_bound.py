#!/usr/bin/env python3
"""将页面回归绑定到同一候选正式构建后的可执行文件。"""
import hashlib,json,pathlib,subprocess,sys,datetime
root=pathlib.Path(__file__).resolve().parents[5]
p=pathlib.Path(__file__).resolve().parents[1]
name=sys.argv[1]
gate=sys.argv[2] if len(sys.argv)>2 else 'ui'
if gate not in {'ui','card-navigation-desktop','card-navigation-mobile','metadata-redaction-binary'}: raise SystemExit('不支持的构建后检查')
binary=pathlib.Path('/private/tmp/lingxi-r07-build/debug/lingxi-service')
sha=lambda data:hashlib.sha256(data).hexdigest()
report=p/f'{name}-{gate}-binary-binding.json'
if report.exists(): raise SystemExit('已有绑定记录，拒绝覆盖')
build_path=p/f'{name}-build.json'
build=json.loads(build_path.read_text())
manifest=json.loads((p/f'SOURCE_MANIFEST_{name}.json').read_text())
if build['exitCode']!=0 or not build['sourceUnchanged'] or build['sourceFingerprint']!=manifest['workingTreeCodeFingerprint']: raise SystemExit('正式构建尚未通过')
before=sha(binary.read_bytes())
if before != build['binary']['sha256']: raise SystemExit('正式构建后服务二进制已变化，拒绝借用构建结果')
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
command=json.loads((p/'GATE_PLAN.json').read_text())['gates'][gate]
result=subprocess.run([sys.executable,str(p/'evidence-tools/run_candidate.py'),'run',name,gate,'--',*command],cwd=root)
after=sha(binary.read_bytes())
ui_path=p/f'{name}-{gate}.json'
record={'candidate':name,'gate':gate,'wrapperSha256':sha(pathlib.Path(__file__).read_bytes()),'binary':str(binary),'binarySha256Before':before,'binarySha256After':after,'binaryUnchanged':before==after,'buildReceipt':build_path.name,'buildReceiptSha256':sha(build_path.read_bytes()),'uiReceipt':ui_path.name,'uiReceiptSha256':sha(ui_path.read_bytes()) if ui_path.exists() else None,'sourceFingerprint':manifest['workingTreeCodeFingerprint'],'startedAt':started,'endedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'wrapperExitCode':result.returncode,'scope':'fresh same-candidate build used by actual service UI test; build success does not imply other platforms or manual browser takeover'}
report.write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(record,ensure_ascii=False))
raise SystemExit(result.returncode if before==after else 99)
