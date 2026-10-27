#!/usr/bin/env python3
"""核对实际人工操作过的同一原生产物；不声称再次运行系统选择器。"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[5]
proof_path = root / sys.argv[1]
sha = lambda data: hashlib.sha256(data).hexdigest()
if sha(proof_path.read_bytes()) != sys.argv[2]:
    raise SystemExit('原生直接检查的回执已变化')
proof = json.loads(proof_path.read_text())
folder = proof_path.parent
original = proof['originalBoundaryReceipt']
if sha((folder / original['path']).read_bytes()) != original['sha256']:
    raise SystemExit('原生操作后的原始边界回执已变化')
launch = json.loads((folder / 'launch.json').read_text())
test_path = (folder / proof['actualTestReceipt']).resolve()
if sha(test_path.read_bytes()) != proof['actualTestReceiptSha256']:
    raise SystemExit('实际测试命令回执已变化')
test = json.loads(test_path.read_text())
if test['exitCode'] != 0 or '--test' not in test['command'] or 'directory_picker_host' not in test['command']:
    raise SystemExit('缺少真实系统选择器检查的成功退出结果')
if sha((root / test['log']).read_bytes()) != test['logSha256']:
    raise SystemExit('实际测试原始日志已变化')
for entry in proof['evidence']:
    if sha((folder / entry['path']).read_bytes()) != entry['sha256']:
        raise SystemExit('实际原生操作材料已变化：' + entry['path'])
for name, expected in launch['sourceHashes'].items():
    if sha((root / name).read_bytes()) != expected:
        raise SystemExit('原生入口代码已变化，必须重做真实选择与取消：' + name)
closure = proof['nativeSourceClosureAfterActualRun']['files']
current_paths = set(subprocess.check_output(['rg','--files','desktop/src-tauri','desktop/src/tauri-resources'],cwd=root,text=True).splitlines()) | {'desktop/vite.tauri-resources.config.ts','package.json','package-lock.json','rust-toolchain.toml'}
if set(closure) != current_paths:
    raise SystemExit('原生代码/权限/构建定义闭包已增删，必须重做真实检查')
for name, expected in closure.items():
    if sha((root / name).read_bytes()) != expected:
        raise SystemExit('运行后记录的原生源码闭包已变化：' + name)
binary = root / 'desktop/src-tauri/target/debug/lingxi-resource-desktop'
if sha(binary.read_bytes()) != proof['binary']['sha256']:
    raise SystemExit('当前原生构建与实际操作的二进制不同，必须重做真实检查')
native = json.loads((folder / 'native-receipt.json').read_text())
if native != proof['nativeResults'] or native['cancelled']['result'] is not None:
    raise SystemExit('原生取消没有真实空回包证据')
if native['selected']['result'] != native['selected']['input'] or native['cancelled']['input'] != native['selected']['input']:
    raise SystemExit('所选路径/取消保留路径的原始结果不一致')
if native['readonly']['ok'] or not proof['cleanup']['driverPortClosed'] or proof['cleanup']['ownedAppProcesses']:
    raise SystemExit('只读权限或资源退出事实不符')
print(json.dumps({'status':'EXACT_NATIVE_BINARY_AND_SOURCE_PROOF_BOUND','directTestReceipt':str(test_path.relative_to(root)),'binarySha256':proof['binary']['sha256'],'rawCancelResult':None,'checks':native['checks'],'scope':'重复核对实际操作过的同一二进制与原生代码；不是本命令重跑系统选择器，也不证明其他操作系统'},ensure_ascii=False))
