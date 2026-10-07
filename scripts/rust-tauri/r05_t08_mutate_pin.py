#!/usr/bin/env python3
"""N03 隔离副本注入：从权威钉表读取唯一目标，恰好降低一次计数。"""

import hashlib
import json
import pathlib
import re
import sys


TARGET = "svc:r05_t01_model_plane"


def mutate(path: pathlib.Path, receipt: pathlib.Path) -> dict:
    before = path.read_bytes()
    text = before.decode("utf-8")
    rows = text.splitlines(keepends=True)
    matches = [i for i, row in enumerate(rows)
               if row.split()[:2] == ["pin", TARGET]]
    if len(matches) != 1:
        raise ValueError(f"N03 {TARGET}: expected exactly one anchor, matched {len(matches)}")
    index = matches[0]
    parts = rows[index].split()
    if len(parts) != 4 or not parts[2].isascii() or not parts[2].isdecimal():
        raise ValueError(f"N03 {TARGET}: malformed authoritative pin row")
    old = int(parts[2])
    if old <= 1:
        raise ValueError(f"N03 {TARGET}: cannot lower positive coverage count {old}")
    new = old - 1
    # 只替换计数字段，保留其他行及本行的空白、目标和筛选词。
    fields = list(re.finditer(r"\S+", rows[index]))
    count = fields[2]
    old_row = rows[index]
    rows[index] = old_row[:count.start()] + str(new) + old_row[count.end():]
    after = "".join(rows).encode("utf-8")
    if sum(a != b for a, b in zip(text.splitlines(keepends=True), rows)) != 1:
        raise ValueError(f"N03 {TARGET}: mutation did not change exactly one row")
    record = {"caseId": "R05-GATE-N03", "findingId": "F45", "target": TARGET,
              "table": str(path.resolve()), "line": index + 1,
              "matches": len(matches), "mutations": 1, "old": old, "new": new,
              "oldRow": old_row.rstrip("\r\n"), "newRow": rows[index].rstrip("\r\n"),
              "beforeSha256": hashlib.sha256(before).hexdigest(),
              "afterSha256": hashlib.sha256(after).hexdigest()}
    path.write_bytes(after)
    if path.read_bytes() != after:
        raise ValueError(f"N03 {TARGET}: mutation readback differs")
    receipt.write_text(json.dumps(record, indent=2, ensure_ascii=False) + "\n")
    return record


if __name__ == "__main__":
    try:
        if len(sys.argv) != 3:
            raise ValueError("usage: r05_t08_mutate_pin.py ISOLATED_PIN_TABLE RECEIPT_JSON")
        print(json.dumps(mutate(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])),
                         ensure_ascii=False))
    except (OSError, UnicodeError, ValueError) as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
