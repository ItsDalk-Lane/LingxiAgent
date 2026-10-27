#!/usr/bin/env python3
"""R09-T07 确定性夹具生成器（A13 长会话/工具卡；无 GUI）。

策略（不新造第二套数据格式）：
1. **复用 R08 既有生成器** `scripts/rust-tauri/r08_t04_fixture_gen.mjs`
   （mulberry32 同种子逐字节可复现；v3Base 锚点 `80a8d2c2…`），连跑两次 → diff -r 必须为空；
2. 在其输出**同一 legacy 会话格式**（`agents/a1/sessions/<id>.jsonl`，entryType
   user/assistant/runBoundary…）上扩展：
   - 1000 条消息的长会话（500 user + 500 assistant，固定时间戳/内容）；
   - 工具卡会话 3 个（成功 / 失败 / 无 final），工具条目按 legacy 候选 schema 写入并
     **显式标记 schemaStatus=candidate-unverified**（当前 Rust legacy 导入未知 entryType
     会进 quarantine，不能假装能渲染）；
3. 记录 seed/两次 rootHash/锚点/文件清单/每会话摘要，供 A13 证据引用。

边界（如实）：
- Tauri rust-mode 的 `app-init` 目前停在 `status.rustCoreUnavailable`，**不加载会话列表**
  （native-caps §9 归类为 server/React 非宿主面）；因此这些夹具先作为“真实存储写入”
  素材与后续服务侧读取/E2E 的输入，不在 UI 上伪造长会话/工具卡渲染。
- 将 legacy 夹具导入 Rust home 需要 R08-T04i cutover 导入 CLI；本脚本只 stage +
  记录 import 命令模板，调用与否由 cleared 的 E2E 批次决定。

用法：
  python3 -B scripts/rust-tauri/r09_t07_fixture.py --home <isolated-LINGXI_HOME> --evidence <dir>
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
GENERATOR = ROOT / "scripts/rust-tauri/r08_t04_fixture_gen.mjs"
SEED = 20261023
V3_ANCHOR = "80a8d2c2640e2b2974b299a10f36679bc19daf0911e51f82f0c43399cb32feaa"
BASE_MS = 1_760_000_000_000


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_digest(root: Path) -> dict:
    entries = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        for name in sorted(filenames):
            path = Path(dirpath) / name
            entries.append((str(path.relative_to(root)), sha256_file(path)))
    digest = hashlib.sha256()
    for name, file_hash in entries:
        digest.update(name.encode())
        digest.update(b"\0")
        digest.update(file_hash.encode())
        digest.update(b"\0")
    return {"digest": digest.hexdigest(), "fileCount": len(entries)}


def run_generator(out: Path) -> dict:
    if out.exists():
        shutil.rmtree(out)
    started = time.time()
    process = subprocess.run(
        ["node", str(GENERATOR), "--seed", str(SEED), "--out", str(out)],
        cwd=str(ROOT), capture_output=True, text=True, timeout=300,
    )
    if process.returncode != 0:
        raise SystemExit(f"r08_t04_fixture_gen failed ({process.returncode}): {process.stderr[-2000:]}")
    last_line = [line for line in process.stdout.strip().splitlines() if line.strip().startswith("{")][-1]
    receipt = json.loads(last_line)
    receipt["durationSecs"] = round(time.time() - started, 3)
    return receipt


def deterministic_jsonl(lines: list[dict]) -> str:
    return "\n".join(json.dumps(line, ensure_ascii=False, separators=(",", ":")) for line in lines) + "\n"


class Content:
    """确定性词流：不用 Math.random/Date.now；同一 seed 逐字节一致。"""

    def __init__(self, seed: int) -> None:
        self._state = seed & 0xFFFFFFFF

    def next(self) -> float:
        self._state = (self._state + 0x6D2B79F5) & 0xFFFFFFFF
        t = self._state
        t = ((t ^ (t >> 15)) * (t | 1)) & 0xFFFFFFFF
        t ^= (t + ((t ^ (t >> 7)) * (t | 61)) & 0xFFFFFFFF) & 0xFFFFFFFF
        return ((t ^ (t >> 14)) & 0xFFFFFFFF) / 4294967296

    def words(self, count: int) -> str:
        vocab = ["灵犀", "长会话", "滚动", "工具卡", "焦点", "输入法", "滚动列表", "引用", "草稿", "窗口"]
        return " ".join(vocab[int(self.next() * len(vocab))] for _ in range(count))


def long_session(lines: list[dict], content: Content) -> list[dict]:
    session_id = "sess-t07-long-1000"
    ts = BASE_MS
    lines.append({"entryType": "sessionMeta", "id": "t07-meta-00000000000000000000000000000001",
                  "sessionId": session_id, "agentId": "a1", "createdAt": ts,
                  "title": "R09-T07 长会话（1000 条，确定性夹具）"})
    run_id = "run-t07-long-1"
    lines.append({"entryType": "runBoundary", "id": "t07-rb-00000000000000000000000000000001",
                  "runId": run_id, "sessionId": session_id, "ts": ts, "kind": "start"})
    for index in range(1000):
        ts += 1
        role = "user" if index % 2 == 0 else "assistant"
        lines.append({
            "entryType": role,
            "id": f"t07-msg-{index:08d}",
            "sessionId": session_id,
            "runId": run_id,
            "ts": ts,
            "content": content.words(6 if role == "user" else 18),
        })
        if index > 0 and index % 20 == 0:
            lines.append({"entryType": "runBoundary", "id": f"t07-rb-{index:08d}",
                          "runId": run_id, "sessionId": session_id, "ts": ts, "kind": "end"})
            run_id = f"run-t07-long-{index // 20 + 1}"
            lines.append({"entryType": "runBoundary", "id": f"t07-rb-{index:08d}-start",
                          "runId": run_id, "sessionId": session_id, "ts": ts, "kind": "start"})
    lines.append({"entryType": "runBoundary", "id": "t07-rb-end-0000000000000000000000000001",
                  "runId": run_id, "sessionId": session_id, "ts": ts, "kind": "end"})
    return lines


def tool_card_session(session_id: str, title: str, status: str, content: Content) -> list[dict]:
    ts = BASE_MS + 10_000
    run_id = f"run-{session_id}"
    lines = [
        {"entryType": "sessionMeta", "id": f"{session_id}-meta", "sessionId": session_id,
         "agentId": "a1", "createdAt": ts, "title": title},
        {"entryType": "runBoundary", "id": f"{session_id}-rb-start", "runId": run_id,
         "sessionId": session_id, "ts": ts, "kind": "start"},
        {"entryType": "user", "id": f"{session_id}-u1", "sessionId": session_id, "runId": run_id,
         "ts": ts + 1, "content": content.words(5)},
        # 工具卡候选 schema（legacy reader 可能 quarantine；schemaStatus 明示未验证）
        {"entryType": "tool", "id": f"{session_id}-tool1", "sessionId": session_id, "runId": run_id,
         "ts": ts + 2, "toolName": "read_file", "toolUseId": f"toolu-{session_id}",
         "status": status, "input": {"path": "/tmp/t07-fixture.txt"},
         "result": {"ok": status == "succeeded", "bytes": 42}},
    ]
    if status != "no-final":
        lines.append({"entryType": "assistant", "id": f"{session_id}-a1", "sessionId": session_id,
                      "runId": run_id, "ts": ts + 3, "content": content.words(12)})
    lines.append({"entryType": "runBoundary", "id": f"{session_id}-rb-end", "runId": run_id,
                  "sessionId": session_id, "ts": ts + 4, "kind": "end"})
    return lines


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--home", required=True, help="隔离 LINGXI_HOME")
    parser.add_argument("--evidence", required=True, help="证据目录（写 manifest）")
    parser.add_argument("--runtime", default=None, help="生成器工作目录（默认 home/.t07-fixtures）")
    args = parser.parse_args()
    home = Path(args.home).resolve()
    evidence = Path(args.evidence).resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    home.mkdir(parents=True, exist_ok=True)
    runtime = Path(args.runtime) if args.runtime else home / ".t07-fixtures"
    runtime.mkdir(parents=True, exist_ok=True)

    first_dir = runtime / "gen-a"
    second_dir = runtime / "gen-b"
    first = run_generator(first_dir)
    second = run_generator(second_dir)
    diff = subprocess.run(["diff", "-r", str(first_dir), str(second_dir)], capture_output=True, text=True)
    deterministic = diff.returncode == 0 and first["rootHash"] == second["rootHash"]
    anchor_ok = first["rootHash"] == V3_ANCHOR
    staged = home / "t07-fixtures" / "v3"
    if staged.exists():
        shutil.rmtree(staged)
    shutil.copytree(first_dir, staged)

    content = Content(0x07_2026_1023)
    sessions_dir = staged / "agents" / "a1" / "sessions"
    sessions_dir.mkdir(parents=True, exist_ok=True)
    generated = {}
    long_lines = long_session([], content)
    long_path = sessions_dir / "sess-t07-long-1000.jsonl"
    long_path.write_text(deterministic_jsonl(long_lines), encoding="utf-8")
    generated["sess-t07-long-1000"] = {
        "file": str(long_path.relative_to(staged)), "entryCount": len(long_lines),
        "messageCount": 1000, "sha256": sha256_file(long_path),
    }
    for status, suffix in (("succeeded", "tools-ok"), ("failed", "tools-fail"), ("no-final", "tools-nofinal")):
        session_id = f"sess-t07-{suffix}"
        lines = tool_card_session(session_id, f"R09-T07 工具卡（{status}）", status, content)
        path = sessions_dir / f"{session_id}.jsonl"
        path.write_text(deterministic_jsonl(lines), encoding="utf-8")
        generated[session_id] = {
            "file": str(path.relative_to(staged)), "entryCount": len(lines),
            "toolCardStatus": status, "sha256": sha256_file(path),
            "schemaStatus": "candidate-unverified",
            "note": "legacy entryType=tool 为候选 schema；Rust legacy 导入未知 entryType 会 quarantine，UI 不得伪造渲染",
        }
    staged_tree = tree_digest(staged)
    manifest = {
        "schema": "lingxi.r09.t07-fixtures.v1",
        "generatedAtUnixMs": int(time.time() * 1000),
        "source": {
            "generator": "scripts/rust-tauri/r08_t04_fixture_gen.mjs",
            "seed": SEED,
            "kind": "既有 R08 生成器（同种子逐字节可复现）",
            "anchorExpected": V3_ANCHOR,
        },
        "determinism": {
            "firstRootHash": first["rootHash"], "secondRootHash": second["rootHash"],
            "diffExit": diff.returncode, "deterministic": deterministic,
            "anchorMatchesV3Base": anchor_ok,
        },
        "staged": {"path": str(staged), "treeDigest": staged_tree["digest"], "fileCount": staged_tree["fileCount"]},
        "generatedSessions": generated,
        "longSession": {"sessionId": "sess-t07-long-1000", "messages": 1000},
        "toolCards": ["succeeded", "failed", "no-final"],
        "importBoundary": {
            "status": "STAGED_NOT_IMPORTED",
            "reason": "legacy 夹具进入 Rust home 需 R08-T04i cutover 导入 CLI；且 rust-mode app-init 停在 "
                      "status.rustCoreUnavailable，不加载会话列表（native-caps §9 非宿主面）。A13 长会话/"
                      "工具卡 UI 断言依赖服务侧读取接线后的 cleared E2E 批次，不在 UI 上伪造。",
            "importCommandTemplate": "R08-T04i cutover import（见 docs/rust-tauri/R08/PROGRESS-t04i.md）",
        },
    }
    (evidence / "fixtures-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(json.dumps({
        "deterministic": deterministic,
        "anchorMatchesV3Base": anchor_ok,
        "stagedTreeDigest": staged_tree["digest"],
        "longSessionMessages": 1000,
        "toolCardSessions": 3,
        "evidence": str(evidence / "fixtures-manifest.json"),
    }, ensure_ascii=False))
    return 0 if deterministic and anchor_ok else 1


if __name__ == "__main__":
    sys.exit(main())
