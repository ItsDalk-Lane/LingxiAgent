import argparse, datetime, hashlib, json, os, pathlib, signal, subprocess, time

ROOT = pathlib.Path(__file__).resolve().parents[5]
EVIDENCE = ROOT / 'artifacts/rust-tauri/R06/AR/W05-E01'

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def inputs():
    names = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
    names += [str(p.relative_to(ROOT)) for p in (ROOT/'rust').rglob('*.rs') if 'target' not in str(p)]
    names += [str(p.relative_to(ROOT)) for p in EVIDENCE.glob('*.py')]
    names += [str(p.relative_to(ROOT)) for p in (EVIDENCE/'local-only/mutant-source/rust').rglob('*') if p.is_file()]
    return {n: digest(ROOT/n) for n in sorted(set(names)) if n and (ROOT/n).is_file()}

def executable_inputs():
    return {str(p.relative_to(ROOT)):digest(p) for target in ['target-w05-e01','target-w05-e01-mutant'] for p in (ROOT/'rust'/target/'debug').glob('deps/*') if p.is_file() and '.' not in p.name and os.access(p,os.X_OK)}

def group_members(group):
    rows=subprocess.check_output(['ps','-axo','pid=,pgid=,rss=']).decode().splitlines()
    return [(int(pid),int(rss)) for row in rows for pid,gid,rss in [row.split()] if int(gid)==group]

def run(name, argv, budget):
    home = EVIDENCE/'local-only'/('home-'+name)
    home.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT/'rust/target-w05-e01'), LINGXI_HOME=str(home),
               TEST_TIME_LIMIT_SEC='600', TEST_MEM_LIMIT_MB='8192', TEST_GUARD_VERBOSE='1', LINGXI_TEST_GUARD='1')
    env['PATH']='/Users/study_superior/.cargo/bin:'+env['PATH']
    env['CARGO_BUILD_JOBS']='8'
    env['LINGXI_F05_LOCAL_ROOT']=str(EVIDENCE/'local-only')
    env['LINGXI_F05_EVIDENCE']=str(EVIDENCE/name)
    before = inputs()
    binary = ROOT/'rust/target-w05-e01/debug/lingxi-service'
    meta = dict(argv=argv, cwd=str(ROOT), home=str(home), homeSource='outer_env; tests may create their own temp home',
                start=datetime.datetime.now(datetime.timezone.utc).isoformat(), budgetSeconds=budget,
                guard=dict(timeSeconds=600, memoryMiB=8192, enabled=True), inputBefore=before,
                binaryBefore=digest(binary) if binary.exists() else None, executableBefore=executable_inputs(), rssPeak=None)
    meta['toolchain']={name:subprocess.check_output(args,env=env,cwd=ROOT,text=True).strip() for name,args in {'cargo':['/Users/study_superior/.cargo/bin/cargo','--version'],'node':['node','--version']}.items()}
    started = time.monotonic()
    with (EVIDENCE/'commands'/(name+'.log')).open('wb') as log:
        proc = subprocess.Popen(argv, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        meta['pid']=proc.pid
        peak=0; samples=0; stopped=None; observed={}; seen_pids={proc.pid}
        while proc.poll() is None:
            rows=subprocess.check_output(['ps','-axo','pid=,ppid=,pgid=,rss=,comm=']).decode().splitlines()
            rows=[(int(pid),int(ppid),int(gid),int(rss),comm) for row in rows for pid,ppid,gid,rss,comm in [row.split(None,4)]]
            live={pid for pid,_,gid,_,_ in rows if gid==proc.pid}
            for _ in range(len(rows)):
                added={pid for pid,ppid,_,_,_ in rows if ppid in live}
                if added<=live: break
                live |= added
            seen_pids |= live
            members=[(pid,rss) for pid,_,_,rss,_ in rows if pid in live]
            peak=max(peak,sum(rss for _,rss in members)); samples+=1
            for pid,ppid,gid,rss,comm in rows:
                if pid not in live or (pid,comm) in observed: continue
                path=pathlib.Path(comm)
                observed[(pid,comm)]={'pid':pid,'parentPid':ppid,'processGroup':gid,'executable':comm,'observedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    'sha256AtObservedStart':digest(path) if path.is_file() else None,'startMeaning':'host observed running process, not retroactive pre-exec hash'}
            if time.monotonic()-started > budget: stopped='deadline'; break
            if sum(rss for _,rss in members)>8192*1024: stopped='memory'; break
            time.sleep(.2)
        meta['timedOut']=stopped=='deadline'; meta['memoryGuardTriggered']=stopped=='memory'
        if stopped:
            os.killpg(proc.pid,signal.SIGTERM)
            try: result=proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid,signal.SIGKILL); result=proc.wait(timeout=10)
        else: result=proc.wait()
        # 只清理本次明确拥有的进程组；原退出码不会被清理覆盖。
        remainder=group_members(proc.pid); meta['remainingBeforeCleanup']=remainder
        if remainder:
            os.killpg(proc.pid,signal.SIGTERM); time.sleep(.3)
            if group_members(proc.pid): os.killpg(proc.pid,signal.SIGKILL); time.sleep(.2)
        meta['remainingAfterCleanup']=group_members(proc.pid)
        meta['observedProcessExecutables']=list(observed.values())
        for item in meta['observedProcessExecutables']:
            path=pathlib.Path(item['executable']); item['sha256AtWindowEnd']=digest(path) if path.is_file() else None
        live_rows=subprocess.check_output(['ps','-axo','pid=,ppid=,pgid=,comm=']).decode().splitlines()
        meta['observedDescendantsRemainingAtWindowEnd']=[row for row in live_rows if int(row.split(None,3)[0]) in seen_pids and int(row.split(None,3)[0])!=proc.pid]
        meta['rssPeak']=peak*1024; meta['rssSampleCount']=samples; meta['rssSource']='host ps supervised process-group and observed descendant RSS sum; target 200ms sampling plus ps/hash overhead'
    meta.update(exitCode=result, durationSeconds=time.monotonic()-started,
                end=datetime.datetime.now(datetime.timezone.utc).isoformat(), inputAfter=inputs(),
                binaryAfter=digest(binary) if binary.exists() else None, executableAfter=executable_inputs(), cleanup='owned process group TERM; root waited/reaped')
    meta['inputUnchanged']=meta['inputBefore']==meta['inputAfter']
    (EVIDENCE/'commands'/(name+'.json')).write_text(json.dumps(meta, ensure_ascii=False, indent=2))
    print(json.dumps({k:meta[k] for k in ['exitCode','durationSeconds','inputUnchanged','timedOut']}), flush=True)
    return result

if __name__ == '__main__':
    p=argparse.ArgumentParser(); p.add_argument('name'); p.add_argument('budget',type=int); p.add_argument('argv',nargs=argparse.REMAINDER)
    a=p.parse_args(); raise SystemExit(run(a.name,a.argv,a.budget))
