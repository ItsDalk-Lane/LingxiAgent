import datetime, hashlib, json, os, pathlib, re, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parents[5]
EV = pathlib.Path(__file__).resolve().parent
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def snapshot():
    files = [ROOT/'rust-toolchain.toml', ROOT/'rust/Cargo.lock', ROOT/'rust/Cargo.toml']
    files += [p for base in ['rust/crates', 'scripts/rust-tauri', 'contracts'] for p in (ROOT/base).rglob('*') if p.is_file() and 'target' not in p.parts and '__pycache__' not in p.parts]
    return {str(p.relative_to(ROOT)): sha(p) for p in sorted(set(files))}
def write(p, value): p.write_text(json.dumps(value, ensure_ascii=False, indent=2))
def run(label, argv, extra=None, cwd=ROOT):
    out = EV/'commands'/label
    out.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    env.update({'PATH':'/Users/study_superior/.cargo/bin:'+env.get('PATH',''), 'CARGO_TARGET_DIR':str(ROOT/'rust/target'), 'CARGO_NET_OFFLINE':'true', 'TMPDIR':'/tmp'})
    for key in ['all_proxy','ALL_PROXY','http_proxy','HTTP_PROXY','https_proxy','HTTPS_PROXY']: env.pop(key,None)
    env.update(extra or {})
    before = snapshot(); write(out/'input-before.json',before)
    start=utc()
    with (out/'stdout.log').open('wb') as stdout, (out/'stderr.log').open('wb') as stderr:
        proc=subprocess.run(argv,cwd=cwd,env=env,stdout=stdout,stderr=stderr)
    after=snapshot();write(out/'input-after.json',after)
    combined=(out/'stdout.log').read_text(errors='replace')+(out/'stderr.log').read_text(errors='replace')
    counts=re.findall(r'test result: .*',combined)
    leaves=[]
    for p in out.rglob('*leaf-cases.json'):
        cases=json.loads(p.read_text())['cases'];leaves.append({'path':str(p),'actual':len(cases),'failed':sum(not c['ok'] for c in cases)})
    binary=ROOT/'rust/target/debug/lingxi-service'
    record={'argv':argv,'cwd':str(cwd),'UTC_start':start,'UTC_end':utc(),'exit':proc.returncode,'counts':counts,'leafCounts':leaves,'inputEqual':before==after,'inputBeforeDigest':hashlib.sha256(json.dumps(before,sort_keys=True).encode()).hexdigest(),'inputAfterDigest':hashlib.sha256(json.dumps(after,sort_keys=True).encode()).hexdigest(),'environment':{k:env.get(k) for k in ['PATH','CARGO_TARGET_DIR','CARGO_NET_OFFLINE','TMPDIR','RUST_LOG']},'stdoutSHA256':sha(out/'stdout.log'),'stderrSHA256':sha(out/'stderr.log'),'binary':{'path':str(binary),'sha256':sha(binary)} if binary.is_file() else None,'boundary':'正式Cargo和真实服务入口；仅本包隔离变异明确单列；无供应商外发','head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()}
    write(out/'command.json',record)
    print(json.dumps({'label':label,'exit':proc.returncode,'counts':counts,'leafCounts':leaves,'inputEqual':before==after},ensure_ascii=False),flush=True)
    return proc.returncode
if __name__=='__main__':
    label=sys.argv[1];argv=sys.argv[2:]
    extra={}
    while argv and '=' in argv[0] and not argv[0].startswith('/'):
        key,value=argv.pop(0).split('=',1);extra[key]=value
    sys.exit(run(label,argv,extra))
