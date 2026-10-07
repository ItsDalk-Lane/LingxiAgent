import subprocess,json,hashlib,sys,datetime
from pathlib import Path
r=Path(__file__).resolve().parent
label=sys.argv[1]; argv=sys.argv[2:]; now=lambda:datetime.datetime.now(datetime.timezone.utc).isoformat()
start=now(); p=subprocess.run(argv,capture_output=True); end=now()
logs={}
for name,data in [('stdout',p.stdout),('stderr',p.stderr)]:
 f=r/(label+'-'+name+'.log');f.write_bytes(data);logs[name]={'path':str(f),'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)}
d={'argv':argv,'cwd':str(Path.cwd()),'startUTC':start,'endUTC':end,'exitCode':p.returncode,'logs':logs}
(r/(label+'-command.json')).write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'label':label,'exit':p.returncode,'stdout':p.stdout.decode(errors='replace')[-3000:],'stderr':p.stderr.decode(errors='replace')[-1000:]},ensure_ascii=False))
sys.exit(p.returncode)
