#!/usr/bin/python3
"""只校验已准备对象；不会执行添加、删除或允许入站操作。"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

BINARY = Path('/Users/study_superior/Desktop/Code/LingxiAgent/rust/target/debug/deps/r00_management_leaves-294787929158efb0')
EXPECTED_SHA = 'c5975a452505bd4f90fcf87509902f812148f809b21674302e6dbd34886daa6a'
EXPECTED_CDHASH = '6eadd46c232408547f08e305c792f1c4c2614a94'
if not BINARY.is_file():
    print(json.dumps(dict(status='STALE', reason='prepared binary missing')))
    sys.exit(1)
sha = hashlib.sha256(BINARY.read_bytes()).hexdigest()
signature = subprocess.run(['/usr/bin/codesign', '-dvvv', str(BINARY)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
text = signature.stdout.decode(errors='replace')
cdhash = next((line[len('CDHash='):] for line in text.splitlines() if line.startswith('CDHash=')), None)
verification = subprocess.run(['/usr/bin/codesign', '--verify', '--verbose=4', str(BINARY)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
ok = sha == EXPECTED_SHA and cdhash == EXPECTED_CDHASH and signature.returncode == 0 and verification.returncode == 0
print(json.dumps(dict(status='MATCH' if ok else 'STALE', binary=str(BINARY), sha256=sha, cdhash=cdhash,
                     signatureExitCode=signature.returncode, verifyExitCode=verification.returncode,
                     systemChangesExecuted=False)))
sys.exit(0 if ok else 1)
