import sys,subprocess,json,datetime,hashlib
from pathlib import Path
P=Path(__file__).resolve().parent
args=sys.argv[1:]; label=args.pop(0)
start=datetime.datetime.now(datetime.timezone.utc).isoformat()
r=subprocess.run(args,capture_output=True)
out=P/(label+'-stdout.log');err=P/(label+'-stderr.log')
out.write_bytes(r.stdout);err.write_bytes(r.stderr)
rec={'reviewer':'rr3_e_review_01','argv':args,'cwd':str(Path.cwd()),'utc_start':start,'utc_end':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exit_code':r.returncode,'stdout':str(out.relative_to(Path.cwd())),'stderr':str(err.relative_to(Path.cwd())),'stdout_sha256':hashlib.sha256(r.stdout).hexdigest(),'stderr_sha256':hashlib.sha256(r.stderr).hexdigest()}
(P/(label+'-command.json')).write_text(json.dumps(rec,ensure_ascii=False,indent=2)+'\n')
print(json.dumps(rec,ensure_ascii=False));print(r.stdout.decode(errors='replace'));print(r.stderr.decode(errors='replace'));sys.exit(r.returncode)
