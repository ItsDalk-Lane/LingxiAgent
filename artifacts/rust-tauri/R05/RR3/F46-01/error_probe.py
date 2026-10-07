"""用自己的临时目录验证日志失败明确告知；服务原有显式降级仍保留。"""
import datetime, hashlib, json, pathlib, selectors, shutil, signal, subprocess, tempfile, time

root = pathlib.Path.cwd()
ev = root / "artifacts/rust-tauri/R05/RR3/F46-01/error-explicit-binary"
ev.mkdir()
home = pathlib.Path(tempfile.mkdtemp(prefix="lingxi-f46-log-error-"))
logs = home / "lingxi-service/logs"
(logs / "service-000001.log").mkdir(parents=True)
for seq in [2, 3]:
    (logs / f"service-{seq:06}.log").write_text("older\n")
binary = root / "rust/target/debug/lingxi-service"
command = [str(binary), "--home", str(home), "--bind", "127.0.0.1:0", "--log-max-files", "3", "--log-max-bytes", "65536"]
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
ready = False
with (ev / "stderr.log").open("wb") as stderr:
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=stderr)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    output = []
    try:
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and selector.select(max(0, deadline-time.monotonic())):
            line = process.stdout.readline()
            if not line:
                break
            output.append(line.decode(errors="replace"))
            if line.startswith(b"LINGXI_SERVICE_READY "):
                ready = True
                break
    finally:
        selector.close()
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
        code = process.wait(timeout=15)
(ev / "stdout.log").write_text("".join(output))
errors = (ev / "stderr.log").read_text()
explicit = "LINGXI_SERVICE_LOG_WRITE_FAILED" in errors and "continuing stderr-only" in errors
inventory = [{"name":p.name,"isDirectory":p.is_dir()} for p in sorted(logs.iterdir())]
shutil.rmtree(home)
result = {"command":command,"pid":process.pid,"startedAt":started,"endedAt":datetime.datetime.now(datetime.timezone.utc).isoformat(),"serviceExitCode":code,"ready":ready,"explicitErrorMarker":explicit,"binarySha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"inventoryOnError":inventory,"boundary":"only isolated temporary filesystem entry denies prune; error is expected and explicitly reported; no count PASS asserted for failed logging attachment","cleanup":{"homeRemoved":not home.exists(),"serviceReaped":process.poll() is not None},"status":"PASS" if ready and explicit and code == 0 else "FAIL"}
(ev / "result.json").write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
print(json.dumps({"status":result["status"],"explicitErrorMarker":explicit,"ready":ready,"serviceExitCode":code}))
raise SystemExit(0 if result["status"] == "PASS" else 1)
