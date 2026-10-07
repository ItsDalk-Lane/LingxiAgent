from pathlib import Path
import sys,json,hashlib,datetime,subprocess
root=Path('/Users/study_superior/Desktop/Code/LingxiAgent')
out=Path('/private/tmp/rr3-doc-input-boundary-1pgmn8m7')
args=sys.argv[1:]
start=datetime.datetime.now(datetime.timezone.utc).isoformat()
if args[0]=='read':
    cmd=['cat',*args[1:]]
else: cmd=args
p=subprocess.run(cmd,cwd=root,capture_output=True,text=True)
end=datetime.datetime.now(datetime.timezone.utc).isoformat()
files=[]
for a in args[1:]:
    f=Path(a) if a.startswith('/') else root/a
    if f.is_file():
        b=f.read_bytes(); files.append({'path':str(f),'sha256':hashlib.sha256(b).hexdigest(),'bytes':len(b)})
log={'startUtc':start,'endUtc':end,'argv':cmd,'exitCode':p.returncode,'files':files,'stdout':p.stdout,'stderr':p.stderr}
with (out/'read-commands.jsonl').open('a') as w:w.write(json.dumps(log,ensure_ascii=False)+'\n')
for f in files:
    with (out/'read-inputs.jsonl').open('a') as w:w.write(json.dumps(f,ensure_ascii=False)+'\n')
print(p.stdout,end=''); print(p.stderr,end='',file=sys.stderr)
sys.exit(p.returncode)
