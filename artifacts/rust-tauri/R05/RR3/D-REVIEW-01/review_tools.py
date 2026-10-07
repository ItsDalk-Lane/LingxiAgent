#!/usr/bin/python3
"""只读对象核对和有界运行；证据仅写入本审查目录。"""
import datetime, hashlib, json, os, re, signal, subprocess, sys, time
from pathlib import Path

OUT = Path(__file__).resolve().parent
REPO = OUT.parents[4]
ALF = '/usr/libexec/ApplicationFirewall/socketfilterfw'
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(name, data):
    (OUT / (name + '.json')).write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
def capture(argv):
    start = utc()
    p = subprocess.run(argv, cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
    return dict(argv=argv, startUTC=start, endUTC=utc(), exitCode=p.returncode, output=p.stdout.decode(errors='replace'))
def identity(path):
    p = Path(path)
    sig = capture(['/usr/bin/codesign', '-d', '--verbose=4', str(p)])
    cd = re.search(r'^CDHash=(\S+)', sig['output'], re.M)
    return dict(path=str(p.absolute()), resolvedPath=str(p.resolve()), isSymlink=p.is_symlink(), sha256=sha(p),
                stat=dict(inode=p.stat().st_ino, size=p.stat().st_size, mtimeNS=p.stat().st_mtime_ns),
                cdhash=cd.group(1) if cd else None, signature=sig,
                verification=capture(['/usr/bin/codesign', '--verify', '--verbose=4', str(p)]))
def inputs():
    command = ['git', '-c', 'core.fsmonitor=false', 'ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', 'rust', 'scripts/rust-tauri', 'rust-toolchain.toml', '.cargo']
    p = subprocess.run(command, cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode: raise RuntimeError(p.stderr.decode())
    files = {}
    for item in sorted(set(p.stdout.split(b'\0'))):
        if not item: continue
        name = os.fsdecode(item); path = REPO / name
        if path.is_symlink(): files[name] = dict(kind='symlink', target=os.readlink(path))
        elif path.is_file(): files[name] = dict(kind='file', sha256=sha(path), size=path.stat().st_size)
        else: files[name] = dict(kind='missing')
    raw = json.dumps(files, sort_keys=True, separators=(',', ':')).encode()
    return dict(utc=utc(), manifest=files, digest=hashlib.sha256(raw).hexdigest(), enumeration=dict(argv=command, exitCode=p.returncode))
def snapshot(name, binary):
    data = dict(utc=utc(), identity=identity(binary), inputs=inputs(),
        git=[capture(['git', '--no-optional-locks', 'rev-parse', 'HEAD']), capture(['git', '--no-optional-locks', 'branch', '--show-current']), capture(['git', '--no-optional-locks', 'status', '--short']), capture(['git', '--no-optional-locks', 'diff', '--stat', 'HEAD'])],
        alf=[capture([ALF, flag]) for flag in ['--getglobalstate', '--getblockall', '--getallowsigned', '--listapps']] + [capture([ALF, '--getappblocked', binary])],
        network=[capture(['/sbin/ifconfig']), capture(['/usr/sbin/netstat', '-rn', '-f', 'inet']), capture(['/sbin/route', '-n', 'get', '192.168.3.5'])])
    save(name, data)
    print(json.dumps({k:data[k] for k in ['utc', 'identity']}, ensure_ascii=False))
def run(spec):
    start=utc(); t0=time.monotonic(); killed=[]; timed=False; observed={}
    log=OUT/(spec['name']+'.log'); listeners=OUT/(spec['name']+'-listeners.jsonl')
    with log.open('wb') as output:
        proc=subprocess.Popen(spec['argv'], cwd=REPO, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        next_sample=0
        while proc.poll() is None:
            if spec.get('monitor') and time.monotonic() >= next_sample:
                ps=capture(['/bin/ps', '-axo', 'pid,ppid,lstart,comm'])
                rows=[x for x in ps['output'].splitlines() if 'r00_management' in x]
                sample=dict(utc=utc(), ps=dict(argv=ps['argv'], exitCode=ps['exitCode'], rows=rows), processes=[])
                for row in rows:
                    parts=row.split(); pid=int(parts[0]); ppid=int(parts[1]); executable=parts[-1]
                    if ppid != proc.pid: continue
                    if pid not in observed:
                        observed[pid]=dict(pid=pid, parentPID=ppid, firstUTC=utc(), identity=identity(executable))
                    sample['processes'].append(dict(pid=pid, parentPID=ppid,
                        listeners=capture(['/usr/sbin/lsof','-nP','-a','-p',str(pid),'-iTCP','-sTCP:LISTEN']),
                        machineListeners=capture(['/usr/sbin/lsof','-nP','-a','-p',str(pid),'-iTCP','-sTCP:LISTEN','-Fpcn'])))
                with listeners.open('a') as f: f.write(json.dumps(sample, ensure_ascii=False)+'\n')
                next_sample=time.monotonic()+1
            if time.monotonic()-t0 > spec['timeout']:
                timed=True; killed.append(dict(signal='SIGTERM', utc=utc())); os.killpg(proc.pid,signal.SIGTERM)
                try: proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    killed.append(dict(signal='SIGKILL', utc=utc())); os.killpg(proc.pid,signal.SIGKILL); proc.wait()
                break
            time.sleep(.1)
    code=proc.wait(); content=log.read_text(errors='replace')
    counts=[dict(zip(['passed','failed','ignored','measured','filtered'],map(int,m))) for m in re.findall(r'(\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',content)]
    result=dict(spec,cwd=str(REPO),startUTC=start,endUTC=utc(),elapsedSeconds=time.monotonic()-t0,parentPID=proc.pid,
        actualExitCode=code,supervisorTimedOut=timed,supervisorExitCode=124 if timed else code,signalsSent=killed,
        observedBinaryProcesses=list(observed.values()),counts=counts or None,log=str(log),logSha256=sha(log))
    if listeners.exists(): result['listenerLogSha256']=sha(listeners)
    save(spec['name'],result); print(json.dumps(result,ensure_ascii=False)); return result['supervisorExitCode']
if __name__=='__main__':
    if sys.argv[1]=='run': sys.exit(run(json.loads(sys.argv[2])))
    elif sys.argv[1]=='snapshot': snapshot(sys.argv[2],sys.argv[3])
    elif sys.argv[1]=='inputs': save(sys.argv[2],inputs())
