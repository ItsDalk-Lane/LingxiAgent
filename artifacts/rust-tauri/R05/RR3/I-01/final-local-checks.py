from pathlib import Path
import subprocess,json,datetime,hashlib,re
r=Path.cwd();e=r/'artifacts/rust-tauri/R05/RR3/I-01';rows=[]
def utc():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def run(name,argv):
 start=utc(); p=subprocess.run(argv,cwd=r,capture_output=True,text=True);(e/(name+'.stdout.log')).write_text(p.stdout);(e/(name+'.stderr.log')).write_text(p.stderr)
 row={'name':name,'argv':argv,'cwd':str(r),'startUTC':start,'endUTC':utc(),'exitCode':p.returncode,'stdoutSha256':hashlib.sha256(p.stdout.encode()).hexdigest(),'stderrSha256':hashlib.sha256(p.stderr.encode()).hexdigest()};rows.append(row);assert p.returncode==0,(name,p.stderr);return p
run('bash-syntax-final',['bash','-n','scripts/rust-tauri/r05_t08_negative_gate.sh'])
a=run('b15-final',['python3','scripts/rust-tauri/r05_t08_negative_gate_selfcheck.py']);assert json.loads(a.stdout)['passed']==15
origin=r/'artifacts/rust-tauri/R05/RR3/B-REVIEW-01/adversarial-checks.py'; body=origin.read_text().replace("EV = ROOT / 'artifacts/rust-tauri/R05/RR3/B-REVIEW-01/adversarial-with-times'", "EV = ROOT / 'artifacts/rust-tauri/R05/RR3/I-01/b41-final'");q=e/'b41-final-replay.py';q.write_text(body)
a=run('b41-final',['python3',str(q)]);assert json.loads(a.stdout)['checks']==41
(e/'final-local-commands.json').write_text(json.dumps(rows,indent=2)+'\n')
print(json.dumps({'bashSyntax':0,'B15':15,'B41':41,'status':'SELF_CHECKED'}))
