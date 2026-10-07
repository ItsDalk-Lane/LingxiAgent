from pathlib import Path
import json,hashlib,subprocess,datetime,sys
ROOT=Path.cwd();E=ROOT/'artifacts/rust-tauri/R05/RR3/E-01';B=json.loads((E/'before.json').read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def timestamp():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def fingerprints():
 d={p:sha(ROOT/p) for p in B['owned']};d['artifacts/rust-tauri/R05/RR3/E-01/verify.py']=sha(E/'verify.py');d['source_scope_manifest']=sha(E/'inputhash-before.json');return d
commands=[['/Users/study_superior/.cargo/bin/rustc','--version'],['/Users/study_superior/.cargo/bin/cargo','--version'],['node','--version'],['npm','--version'],['python3','--version'],['uname','-sm'],['git','rev-parse','HEAD'],['git','rev-parse','origin/codex/rust-tauri-migration']]
commands += [[sys.executable,str(E/'verify.py'),m] for m in ['json','links','consistency','history','source','evidence']]
commands += [['git','diff','--check','--',*B['owned']]]
rows=[];run_dir=E/'checks-03';run_dir.mkdir(exist_ok=False)
for index,argv in enumerate(commands,1):
 start=timestamp();before=fingerprints();r=subprocess.run(argv,cwd=ROOT,text=True,capture_output=True);end=timestamp();after=fingerprints();name=f'{index:02}'
 (run_dir/(name+'-stdout.log')).write_text(r.stdout);(run_dir/(name+'-stderr.log')).write_text(r.stderr)
 row={'argv':argv,'cwd':str(ROOT),'startedAt':start,'endedAt':end,'exitCode':r.returncode,'inputHashesBefore':before,'inputHashesAfter':after,'inputsEqual':before==after,'stdout':str((run_dir/(name+'-stdout.log')).relative_to(ROOT)),'stderr':str((run_dir/(name+'-stderr.log')).relative_to(ROOT)),'stdoutSha256':sha(run_dir/(name+'-stdout.log')),'stderrSha256':sha(run_dir/(name+'-stderr.log'))};rows.append(row)
 (run_dir/(name+'-command.json')).write_text(json.dumps(row,ensure_ascii=False,indent=2)+'\n');print(name,r.returncode,r.stdout.strip()[:500],r.stderr.strip()[:500],flush=True)
(E/'commands-03.json').write_text(json.dumps({'kind':'E_SELF_CHECK_NOT_INDEPENDENT','recordedAt':timestamp(),'commands':rows,'allExitZero':all(r['exitCode']==0 for r in rows)},ensure_ascii=False,indent=2)+'\n')
sys.exit(0 if all(r['exitCode']==0 for r in rows) else 1)
