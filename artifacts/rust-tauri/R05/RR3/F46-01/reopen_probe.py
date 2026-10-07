import datetime, hashlib, json, pathlib, selectors, signal, subprocess, tempfile, time, shutil, sys
root=pathlib.Path.cwd()
ev=root/"artifacts/rust-tauri/R05/RR3/F46-01"/sys.argv[1]
ev.mkdir()
binary=root/"rust/target/debug/lingxi-service"
home=pathlib.Path(tempfile.mkdtemp(prefix="lingxi-r05-f46-reopen-"))
rows=[]
for restart in range(6):
    command=[str(binary),"--home",str(home),"--bind","127.0.0.1:0","--log-max-files","3","--log-max-bytes","65536"]
    started=datetime.datetime.now(datetime.timezone.utc).isoformat()
    stderr=(ev/("service-%d-stderr.log"%restart)).open("wb")
    process=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=stderr)
    selector=selectors.DefaultSelector();selector.register(process.stdout,selectors.EVENT_READ)
    out=[];ready=False;deadline=time.monotonic()+30
    try:
        while time.monotonic()<deadline:
            if not selector.select(max(0,deadline-time.monotonic())):break
            line=process.stdout.readline()
            if not line:break
            out.append(line.decode(errors="replace"))
            if line.startswith(b"LINGXI_SERVICE_READY "):ready=True;break
        if not ready:raise RuntimeError("BLOCKED: actual binary readiness not observed")
        inventory=[{"name":p.name,"bytes":p.stat().st_size} for p in sorted((home/"lingxi-service/logs").glob("service-*.log"))]
        row={"restartIndex":restart,"command":command,"startedAt":started,"pid":process.pid,"ready":ready,"files":inventory,"count":len(inventory),"threshold":3,"assertionPass":len(inventory)<=3}
    finally:
        selector.close()
        if process.poll() is None:process.send_signal(signal.SIGTERM)
        try:code=process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill();code=process.wait();raise RuntimeError("BLOCKED: graceful stop exceeded15s")
        stderr.close()
        (ev/("service-%d-stdout.log"%restart)).write_text(''.join(out))
    row["exitCode"]=code;row["endedAt"]=datetime.datetime.now(datetime.timezone.utc).isoformat();rows.append(row)
result={"caseId":"R05-T08-C12","finding":"F46","binaryPath":str(binary),"binarySha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"input":"six real opens/readiness/SIGTERM cycles on same isolated home; no injected logging state/provider", "rows":rows,"bound":3,"status":"FAIL" if any(not x["assertionPass"] for x in rows) else "PASS"}
shutil.rmtree(home)
result["cleanup"]={"homeRemoved":not home.exists(),"allProcessesReaped":True}
(ev/"result.json").write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
print(json.dumps({"finding":"F46","counts":[x["count"] for x in rows],"threshold":3,"status":result["status"]},ensure_ascii=False))
sys.exit(1 if result["status"]=="FAIL" else 0)
