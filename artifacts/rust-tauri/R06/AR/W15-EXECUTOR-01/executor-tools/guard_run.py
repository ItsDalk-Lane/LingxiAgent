import os, sys, subprocess, time, json, pathlib, hashlib, signal, datetime

base = pathlib.Path(__file__).parent
root = base / 'own.git'
tag = sys.argv[1]
argv = sys.argv[2:]
out = base / 'evidence' / tag
out.mkdir(parents=True, exist_ok=False)
env = dict(os.environ)
env.update(LINGXI_HOME=str(base/'home'), TMPDIR=str(base/'tmp'), LINGXI_PRODUCT_DIR=str(root/'lib'), CARGO_TARGET_DIR=str(base/'target'), CARGO_INCREMENTAL='0', LINGXI_TEST_GUARD='1', TEST_TIME_LIMIT_SEC='600', TEST_MEM_LIMIT_MB='8192', TEST_GUARD_VERBOSE='1', R06_T08_EVAL_OUT=str(out/'evaluation.jsonl'), R06_T08_RAW_OUT=str(out/'raw'))
start = time.monotonic()
record = dict(argv=argv, cwd=str(root), head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(), started=datetime.datetime.now(datetime.timezone.utc).isoformat(), effective_env={k:env.get(k) for k in ['HOME','LINGXI_HOME','TMPDIR','LINGXI_PRODUCT_DIR','CARGO_INCREMENTAL','CARGO_TARGET_DIR','LINGXI_TEST_GUARD','TEST_TIME_LIMIT_SEC','TEST_MEM_LIMIT_MB']}, lock_sha256=hashlib.sha256((root/'rust/Cargo.lock').read_bytes()).hexdigest(), controller_seconds=2700, process_seconds=600, process_rss_mib=8192, supervision='external descendant PID sampling plus internal guard')
def snapshot(label):
    paths=[]
    for area in ['rust','core','lib','shared','server','scripts/rust-tauri']:
        paths.extend(p for p in (root/area).rglob('*') if p.is_file() and 'target' not in p.parts and p.suffix in ('.rs','.toml','.lock','.ts','.json','.cjs','.mjs','.py','.sh','.md'))
    manifest=[dict(path=str(p.relative_to(root)),sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sorted(set(paths))]
    (out/f'source-{label}.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2))
    bins=[]
    for p in (base/'target/debug').glob('*'):
        if p.is_file() and os.access(p,os.X_OK) and p.stat().st_size>0:bins.append(dict(path=str(p),sha256=hashlib.sha256(p.read_bytes()).hexdigest()))
    for p in (base/'target/debug/deps').glob('*'):
        if p.is_file() and os.access(p,os.X_OK) and p.stat().st_size>0 and p.suffix not in ('.d','.rlib','.rmeta','.o'):bins.append(dict(path=str(p),sha256=hashlib.sha256(p.read_bytes()).hexdigest()))
    (out/f'binaries-{label}.json').write_text(json.dumps(bins,indent=2))
snapshot('before')
seen = {}
breaches=[]
def table():
    text=subprocess.check_output(['ps','-axo','pid=,ppid=,rss=,comm=,args='],text=True)
    rows={}
    for line in text.splitlines():
        bits=line.strip().split(None,4)
        if len(bits)>=4:
            try: rows[int(bits[0])] = (int(bits[1]),int(bits[2])*1024,bits[3],bits[4] if len(bits)>4 else '')
            except ValueError: pass
    return rows
def terminate(pids):
    for pid in reversed(list(pids)):
        try:os.kill(pid,signal.SIGTERM)
        except ProcessLookupError:pass
    time.sleep(0.2)
    for pid in reversed(list(pids)):
        try:os.kill(pid,signal.SIGKILL)
        except ProcessLookupError:pass
with (out/'stdout.log').open('w') as stdout, (out/'stderr.log').open('w') as stderr:
    table()
    proc=subprocess.Popen(argv,cwd=root,env=env,stdout=stdout,stderr=stderr)
    record['controller_pid']=proc.pid
    (out/'start.json').write_text(json.dumps(record,ensure_ascii=False,indent=2))
    while proc.poll() is None:
        rows=table(); owned={proc.pid}; changed=True
        while changed:
            prev=len(owned);owned.update(pid for pid,r in rows.items() if r[0] in owned);changed=len(owned)>prev
        now=time.monotonic()
        for pid in owned:
            if pid not in rows:continue
            parent,rss,comm,args=rows[pid]
            controller=pathlib.Path(comm).name in ('cargo','rustc','rustdoc','cc','clang','ld','ar','ranlib','sccache') or 'build-script-' in comm
            rec=seen.setdefault(pid,dict(pid=pid,ppid=parent,argv=args,comm=comm,first_seen_elapsed=now-start,last_seen_elapsed=None,peak_rss_bytes=None,limit_seconds=2700 if controller else 600,controller=controller))
            rec['last_seen_elapsed']=now-start;rec['peak_rss_bytes']=max(rec['peak_rss_bytes'] or 0,rss)
            if (rss>8192*1024*1024 or now-start-rec['first_seen_elapsed']>rec['limit_seconds']):breaches.append(dict(pid=pid,rss=rss,elapsed=now-start-rec['first_seen_elapsed']))
        if now-start>2700:breaches.append(dict(controller_budget_exceeded=True))
        if breaches:terminate(owned);break
        time.sleep(0.1)
    code=proc.wait()
record.update(ended=datetime.datetime.now(datetime.timezone.utc).isoformat(), elapsed_seconds=time.monotonic()-start, exit_code=code, guard_breaches=breaches, processes=list(seen.values()))
snapshot('after')
(out/'run.json').write_text(json.dumps(record,ensure_ascii=False,indent=2))
print(json.dumps({'evidence':str(out),'exit_code':code,'elapsed_seconds':record['elapsed_seconds'],'guard_breaches':breaches}))
sys.exit(code if code>=0 else 128-code)
