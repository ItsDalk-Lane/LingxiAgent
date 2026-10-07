import subprocess,pathlib,time,datetime,json,hashlib,re
E=pathlib.Path(__file__).resolve().parent
seen=set(); events=[]
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
while not (E/'default-command.json').exists():time.sleep(.25)
while not (E/'extra-complete.json').exists():
 r=subprocess.run(['ps','-axo','pid,ppid,command'],text=True,capture_output=True)
 for line in r.stdout.splitlines():
  z=line.strip().split(None,2)
  if len(z)!=3: continue
  pid,ppid,argv=z
  if not ('lingxi-r05-neg-target' in argv or '/negcopy.hTXzzN/' in argv): continue
  if (pid,argv) in seen: continue
  seen.add((pid,argv)); exe=argv.split()[0]; rec={'utc':utc(),'pid':pid,'ppid':ppid,'observedCommand':argv,'executable':exe}
  p=pathlib.Path(exe)
  if p.is_file():
   try: rec.update(bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest())
   except OSError as ex: rec['readError']=str(ex)
  summary=E/'default16-01/summary.txt'
  rec['phase']='supplement-live-observation'; rec['completedCommands']=[x['name'] for x in json.loads((E/'supplement-commands.json').read_text())] if (E/'supplement-commands.json').exists() else []
  events.append(rec)
  (E/'supplement-live-binary-observations.json').write_text(json.dumps(events,ensure_ascii=False,indent=2)+'\n')
 time.sleep(.25)
print(len(events))
