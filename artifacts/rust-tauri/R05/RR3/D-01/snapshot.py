#!/usr/bin/python3
"""记录实际候选、输入摘要和本次 Cargo 返回的测试产物身份。"""
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys

OUT = Path(__file__).resolve().parent
def capture(argv):
    result = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    return dict(argv=argv, exitCode=result.returncode, output=result.stdout.decode(errors='replace'))

messages = []
for line in (OUT / 'prebuild-r00.log').read_text().splitlines():
    try:
        message = json.loads(line)
    except ValueError:
        continue
    if message.get('reason') == 'compiler-artifact' and message.get('target', {}).get('name') == 'r00_management_leaves':
        messages.append(message)
assert len(messages) == 1 and messages[0]['executable'], messages
binary = Path(messages[0]['executable']).resolve()
paths = subprocess.check_output(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', 'rust', 'scripts/rust-tauri']).split(b'\0')
manifest = {}
for raw in sorted(set(paths)):
    if not raw:
        continue
    path = Path(raw.decode())
    if path.is_file():
        manifest[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
manifest['rust-toolchain.toml'] = hashlib.sha256(Path('rust-toolchain.toml').read_bytes()).hexdigest()
encoded = json.dumps(manifest, sort_keys=True, separators=(',', ':')).encode()
alf = '/usr/libexec/ApplicationFirewall/socketfilterfw'
result = dict(time=datetime.datetime.now().astimezone().isoformat(),
              head=capture(['git', 'rev-parse', 'HEAD']), branch=capture(['git', 'branch', '--show-current']),
              workingTree=capture(['git', 'status', '--short']),
              inputManifest=manifest, inputManifestSha256=hashlib.sha256(encoded).hexdigest(),
              executable=str(binary), executableSha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
              executableStat=capture(['/usr/bin/stat', '-f', '%N inode=%i size=%z mtime=%Sm', str(binary)]),
              codesign=capture(['/usr/bin/codesign', '-dvvv', str(binary)]),
              codesignVerification=capture(['/usr/bin/codesign', '--verify', '--verbose=4', str(binary)]),
              appFilter=capture([alf, '--getappblocked', str(binary)]),
              globalState=capture([alf, '--getglobalstate']), blockAll=capture([alf, '--getblockall']),
              signedState=capture([alf, '--getallowsigned']), applications=capture([alf, '--listapps']),
              addresses=[capture(['/usr/sbin/ipconfig', 'getifaddr', interface]) for interface in ['en0', 'en1']],
              routes=capture(['/usr/sbin/netstat', '-rn', '-f', 'inet']),
              os=capture(['/usr/bin/sw_vers']), architecture=capture(['/usr/bin/uname', '-m']),
              toolchain=[capture([str(Path('/Users/study_superior/.cargo/bin') / tool), '+1.98.1', '--version']) for tool in ['cargo', 'rustc']],
              cargoArtifact=messages[0])
(OUT / (sys.argv[1] + '.json')).write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({key:result[key] for key in ['time', 'executable', 'executableSha256', 'inputManifestSha256', 'appFilter', 'codesign']}, ensure_ascii=False))
