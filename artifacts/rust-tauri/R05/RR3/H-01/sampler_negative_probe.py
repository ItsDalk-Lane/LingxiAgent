"""仅隔离的测试装备副本使用假采样输出，主工作区及系统工具不变。"""
import datetime
import hashlib
import json
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import time

root = pathlib.Path.cwd()
evidence = root / "artifacts/rust-tauri/R05/RR3/H-01" / (sys.argv[1] if len(sys.argv)>1 else "sampler-negative-isolated")
evidence.mkdir()
source = root / "rust/target/debug/deps/r05_t08_resources-f66968a3ba499b0b"
equipment = evidence / "r05_t08_resources"
shutil.copy2(source, equipment)
digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
assert digest(source) == digest(equipment)
rows = []
for label, mode, expected in [("normal", "normal", 0), ("fake-fd-zero", "fd", 101), ("fake-tcp-zero", "tcp", 101), ("restored", "normal", 0)]:
    run = evidence / label
    run.mkdir()
    environment = os.environ.copy()
    environment.pop("RUST_LOG", None)
    environment["TMPDIR"] = str(root / "artifacts/rust-tauri/R05/RR3/H-01/resource-tmp")
    if mode != "normal":
        bin_dir = run / "bin"
        bin_dir.mkdir()
        shim = bin_dir / "lsof"
        calls = run / "calls.jsonl"
        shim.write_text("#!/usr/bin/env python3\nimport os,sys,json,pathlib\n"
            "args=sys.argv[1:]\npid=args[args.index('-p')+1]\n"
            f"with pathlib.Path({str(calls)!r}).open('a') as f: f.write(json.dumps({{'pid':int(pid),'args':args}})+'\\n')\n"
            "fields=args[args.index('-F')+1]\n"
            f"if {mode!r} == 'fd' and fields == 'fn':\n print('p'+pid)\n"
            f"elif {mode!r} == 'tcp' and fields == 'fpPTn':\n print('p'+pid+'\\nf0\\nf1\\nf2')\n"
            "else:\n os.execv('/usr/sbin/lsof',['/usr/sbin/lsof']+args)\n")
        shim.chmod(0o700)
        environment["PATH"] = str(bin_dir) + ":" + environment["PATH"]
    command = [str(equipment), "f27_sampler_controls_detect_growth_release_and_failure", "--exact", "--nocapture"]
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (run / "stdout.log").open("wb") as output:
        process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT, cwd=evidence, env=environment, start_new_session=True)
        code = process.wait(timeout=45)
    # 只处理刚刚创建的独立进程组；失败检查可能留下已知 sleep 控制进程。
    def members():
        text = subprocess.check_output(["ps", "-axo", "pid=,pgid=,comm="], text=True)
        return [line for line in text.splitlines() if len(line.split()) >= 3 and line.split()[1] == str(process.pid)]
    before_cleanup = members()
    if before_cleanup:
        os.killpg(process.pid, signal.SIGKILL)
    deadline = time.monotonic() + 5
    after_cleanup = members()
    while after_cleanup and time.monotonic() < deadline:
        time.sleep(0.05)
        after_cleanup = members()
    output_text = (run / "stdout.log").read_text()
    named = ("invalid live-process sample" in output_text) if mode == "fd" else ("必须看见已知保留连接的两端" in output_text) if mode == "tcp" else "test result: ok. 1 passed" in output_text
    record = {"command": command, "cwd": str(evidence), "startedAt": started, "endedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(), "exitCode": code, "expectedExitCode": expected, "targetNamed": named, "mode": mode, "equipmentSource": str(source), "sourceHash": digest(source), "copyHash": digest(equipment), "stdoutHash": digest(run / "stdout.log"), "cleanup": {"processGroup": process.pid, "membersBefore": before_cleanup, "membersAfter": after_cleanup}, "status": "PASS" if code == expected and named and not after_cleanup else "FAIL"}
    (run / "command.json").write_text(json.dumps(record, ensure_ascii=False, indent=2)+"\n")
    rows.append(record)
result = {"boundary": "compiled candidate resource test copied byte-for-byte into isolated evidence; only copied process PATH receives output shim; no production source/system mutation", "rows": rows, "status": "PASS" if all(row["status"] == "PASS" for row in rows) else "FAIL"}
(evidence / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2)+"\n")
print(json.dumps({"status": result["status"], "exitCodes": [row["exitCode"] for row in rows], "named": [row["targetNamed"] for row in rows]}, ensure_ascii=False))
raise SystemExit(0 if result["status"] == "PASS" else 1)
