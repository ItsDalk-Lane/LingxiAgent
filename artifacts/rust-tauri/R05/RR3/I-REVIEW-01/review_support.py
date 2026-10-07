from pathlib import Path
import datetime,hashlib,json,os,subprocess
ROOT=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
EV=ROOT/'artifacts/rust-tauri/R05/RR3/I-REVIEW-01'
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(name,obj): (EV/name).write_text(json.dumps(obj,ensure_ascii=False,indent=2)+'\n')
def run(name,argv,cwd=ROOT,env=None,expected=None):
    start=utc()
    with (EV/(name+'.stdout.log')).open('w') as out,(EV/(name+'.stderr.log')).open('w') as err:
        p=subprocess.run([str(a) for a in argv],cwd=cwd,env=env,stdout=out,stderr=err)
    receipt={'name':name,'argv':[str(a) for a in argv],'cwd':str(cwd),'startUTC':start,'endUTC':utc(),'exitCode':p.returncode,'expectedExitCode':expected,'stdoutSha256':sha(EV/(name+'.stdout.log')),'stderrSha256':sha(EV/(name+'.stderr.log'))}
    if env is not None: receipt['environmentOverrides']={k:env[k] for k in ['CARGO_NET_OFFLINE','CARGO_TARGET_DIR'] if k in env}
    with (EV/'commands.jsonl').open('a') as out: out.write(json.dumps(receipt,ensure_ascii=False)+'\n')
    print(name,p.returncode,flush=True)
    if expected is not None: assert p.returncode==expected,receipt
    return receipt
if __name__=='__main__':
    paths=['scripts/rust-tauri/r05_t08_negative_gate.sh','scripts/rust-tauri/r05_t08_restore_selfcheck.py','scripts/rust-tauri/r05_t08_mutate_pin.py','scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py','scripts/rust-tauri/run_output_sinks.py','rust-toolchain.toml','rust/Cargo.lock','docs/rust-tauri/R05/r05_stage_pins.tsv','.git/HEAD','.git/index']
    paths += [str(p.relative_to(ROOT)) for p in (ROOT/'rust/crates/xtask/src').rglob('*') if p.is_file()]
    save('source-before.json',{'UTC':utc(),'files':{p:sha(ROOT/p) for p in paths},'boundary':'I/B/xtask/locks及主HEAD/index局部范围；H并行输入只由实际新copy界定'})
    for name,argv in [('branch',['git','branch','--show-current']),('head',['git','rev-parse','HEAD']),('status',['git','status','--short']),('rustc',['/Users/study_superior/.cargo/bin/rustc','--version']),('cargo',['/Users/study_superior/.cargo/bin/cargo','--version']),('node',['node','--version']),('npm',['npm','--version']),('platform',['uname','-a']),('syntax',['bash','-n','scripts/rust-tauri/r05_t08_negative_gate.sh'])]: run(name,argv,expected=0)
    run('permanent-final',['python3','scripts/rust-tauri/r05_t08_restore_selfcheck.py','--evidence',str(EV/'permanent-final'),'--production-sync'],expected=0)
