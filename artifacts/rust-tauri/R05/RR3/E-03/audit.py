import collections, datetime, difflib, gzip, hashlib, json, os, pathlib, re, stat, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parents[5]
EV = pathlib.Path(__file__).resolve().parent
D = ROOT / 'docs/rust-tauri/R05'
NAMES = ['R05_REPORT.md','R05_INDEPENDENT_REVIEW.md','R05_BLOCKERS.md','R05_HANDOFF.json','PROGRESS_LEDGER.json','R05_ACCEPTANCE_LEDGER.json','R05_TEST_MAP.json','R05_NEGATIVE_GATE_REPORT.md','R05_PERFORMANCE_RESULTS.json','R05_LIVE_VERIFICATION.json','WORKER_MODEL_BOUNDARY.md','MODEL_USAGE_SEMANTICS.md','R05_INTERFACE_EVOLUTION.md']
DOCS = ['docs/rust-tauri/R05/' + n for n in NAMES] + ['docs/rust-tauri/ORCHESTRATOR_PROGRESS.json']
G = ROOT/'artifacts/rust-tauri/R05/RR3/G-REVIEW-02'
H = ROOT/'artifacts/rust-tauri/R05/RR3/H-REVIEW-02'

def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(b): return hashlib.sha256(b).hexdigest()
def load(p): return json.loads(pathlib.Path(p).read_text())
def atom(p, b):
    p = pathlib.Path(p); p.parent.mkdir(parents=True, exist_ok=True)
    tmp = p.with_name(p.name+'.e03-tmp')
    with tmp.open('wb') as f: f.write(b); f.flush(); os.fsync(f.fileno())
    os.replace(tmp,p)
def save(n,d): atom(EV/n,(json.dumps(d,ensure_ascii=False,indent=2)+'\n').encode())
def entry(p):
    p=pathlib.Path(p); s=p.lstat(); link=os.readlink(p) if p.is_symlink() else None
    b=link.encode() if link is not None else p.read_bytes()
    return dict(path=p.relative_to(ROOT).as_posix(),sha256=sha(b),bytes=len(b),mode=stat.S_IMODE(s.st_mode),link=link)
def digest(rows): return sha(''.join(f"{r['sha256']}  {r['mode']}  {r['path']}\n" for r in rows).encode())
def protected_paths():
    # 产品与门禁输入采用安全超集；只排除逐项列明的输出文档和协调元数据。
    paths={r['path'] for r in load(G/'metadata/source-before.json')['files']}
    paths |= set(load(H/'FINAL_SOURCE_BINDING.json')['inputHashes'])
    for base in ['rust','scripts','contracts','shared','tests','docs/rust-tauri','.sync-audit','cli','core','server','desktop','channels','bridges']:
        for here,dirs,files in os.walk(ROOT/base,followlinks=False):
            dirs[:]=[x for x in dirs if x not in ['target','.git','node_modules','__pycache__']]
            for name in files:
                p=pathlib.Path(here)/name
                if p.suffix not in ['.pyc'] and not p.name.endswith('.DS_Store'): paths.add(p.relative_to(ROOT).as_posix())
    for name in ['package.json','package-lock.json','.npmrc','rust-toolchain.toml','tsconfig.json','tsconfig.node.json','tsconfig.test.json','vitest.config.ts']:
        if (ROOT/name).exists(): paths.add(name)
    excluded={p for p in paths if p in DOCS or p.startswith('docs/rust-tauri/R05/repair-current/RR3_')}
    return sorted(paths-excluded), sorted(excluded)
def snapshot():
    paths,ex=protected_paths(); rows=[entry(ROOT/p) for p in paths]
    return dict(at=now(),files=rows,count=len(rows),digest=digest(rows),algorithm='sha256(sorted UTF-8: sha256 + two spaces + decimal mode + two spaces + repo path + newline)',excluded_report_metadata=ex,scope='G02 source inventory + H02 actual inputs + freshly enumerated rust/scripts/contracts/shared/tests/docs-rust-tauri/.sync-audit/cli/core/server/desktop/channels/bridges + root locks/config. target/.git/node_modules/__pycache__ excluded; E14 and explicitly named RR3 coordination metadata separated. Includes actual docs authority and every newly added helper. Not whole candidate or a stage PASS.')
def record(cmd):
    start=now(); r=subprocess.run(cmd,cwd=ROOT,capture_output=True)
    i=sum(1 for _ in (EV/'commands.jsonl').open()) if (EV/'commands.jsonl').exists() else 0
    label=f'cmd-{i+1:02d}'; atom(EV/(label+'.stdout'),r.stdout); atom(EV/(label+'.stderr'),r.stderr)
    row=dict(argv=cmd,cwd=str(ROOT),started_at=start,ended_at=now(),exit_code=r.returncode,stdout=label+'.stdout',stderr=label+'.stderr',stdout_sha256=sha(r.stdout),stderr_sha256=sha(r.stderr))
    with (EV/'commands.jsonl').open('a') as f:f.write(json.dumps(row,ensure_ascii=False)+'\n')
    return r
def capture():
    assert not (EV/'before-documents.json.gz').exists()
    docs={p:(ROOT/p).read_text() for p in DOCS}
    atom(EV/'before-documents.json.gz',gzip.compress(json.dumps(docs,ensure_ascii=False).encode(),mtime=0))
    save('documents-before.json',[entry(ROOT/p) for p in DOCS])
    save('semantic-inputs-before.json',snapshot())
    save('g02-current-before-comparison.json',compare_g())
    save('h02-current-before-comparison.json',compare_h())
    for cmd in [['git','rev-parse','HEAD'],['git','rev-parse','--abbrev-ref','HEAD'],['git','status','--short'],['git','rev-parse','origin/codex/rust-tauri-migration'],['df','-k','.'],['/Users/study_superior/.cargo/bin/rustc','--version'],['/Users/study_superior/.cargo/bin/cargo','--version'],['node','--version'],['npm','--version'],['python3','--version'],['uname','-sm']]:record(cmd)
    print('captured',len(docs),'documents; semantic inputs',load(EV/'semantic-inputs-before.json')['count'])
def compare_g():
    baseline=load(G/'metadata/source-before.json'); changes=[]
    for old in baseline['files']:
        p=ROOT/old['path']; new=entry(p) if p.exists() or p.is_symlink() else None
        if new is None or any(new[k]!=old[k] for k in ['sha256','mode','link','bytes']):changes.append(dict(path=old['path'],before=old,after=new))
    return dict(at=now(),baseline_ref=str((G/'metadata/source-before.json').relative_to(ROOT)),baseline_count=len(baseline['files']),baseline_digest=baseline.get('digest'),changed=changes,whole_candidate_equal=False,qualification='This retained G inventory is not full candidateSourceBinding. Changed metadata listed honestly; additional E03/storage/coordination evidence also changes whole candidate membership. Old G facts retain original tested HEAD+dirty, never rebound to a later commit.')
def compare_h():
    old=load(H/'FINAL_SOURCE_BINDING.json')['inputHashes']; changes=[p for p,v in old.items() if sha((ROOT/p).read_bytes())!=v]
    return dict(at=now(),reference='artifacts/rust-tauri/R05/RR3/H-REVIEW-02/FINAL_SOURCE_BINDING.json',count=len(old),changes=changes,all_equal=not changes)
def strict_pairs(pairs):
    d={}
    for k,v in pairs:
        if k in d: raise ValueError('duplicate key '+k)
        d[k]=v
    return d

if __name__=='__main__':
    if sys.argv[1]=='capture':capture()
    elif sys.argv[1]=='record':
        r=record(sys.argv[2:]); print(r.stdout.decode(errors='replace'));print(r.stderr.decode(errors='replace'),file=sys.stderr);sys.exit(r.returncode)
