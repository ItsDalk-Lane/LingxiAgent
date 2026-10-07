#!/usr/bin/python3
"""只读校验本轮准备对象；不登记、修改或允许任何系统规则。"""
import json, sys
from pathlib import Path
sys.dont_write_bytecode = True
from review_tools import identity, inputs, utc
out = Path(__file__).resolve().parent
expected = json.loads((out/'prepared-object.json').read_text())
try:
    actual = identity(expected['absolutePath'])
    current = inputs()
    matches = (not actual['isSymlink'] and actual['resolvedPath'] == expected['absolutePath']
        and actual['sha256'] == expected['sha256'] and actual['cdhash'] == expected['cdhash']
        and actual['verification']['exitCode'] == 0
        and current['digest'] == expected['sourceInputDigest'])
    result = dict(utc=utc(), status='MATCH' if matches else 'STALE', expected=expected,
        actualIdentity=actual, currentInputDigest=current['digest'], systemChangesExecuted=False)
except Exception as error:
    matches=False
    result=dict(utc=utc(), status='BLOCKED', error=str(error), systemChangesExecuted=False)
print(json.dumps(result, ensure_ascii=False, indent=2))
sys.exit(0 if matches else 1)
