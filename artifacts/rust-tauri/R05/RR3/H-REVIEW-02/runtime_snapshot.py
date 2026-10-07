from capture import *
rows=[]
text=subprocess.check_output(['ps','-axo','pid=,ppid=,command='],text=True)
for line in text.splitlines():
 if '/rust/target/debug/lingxi-service --home '+str(EV/'resource-tmp') not in line:continue
 fields=line.strip().split(None,2)
 if not fields[0].isdigit():continue
 command=fields[2].split(); d={'pid':int(fields[0]),'ppid':int(fields[1]),'command':command,'binarySha256':sha(pathlib.Path(command[0]))}
 if '--config' in command:
  p=pathlib.Path(command[command.index('--config')+1]); b=p.read_bytes(); cfg=json.loads(b)
  d['configPath']=str(p);d['configSha256']=hashlib.sha256(b).hexdigest()
  for provider in cfg['providers'].values():
   if 'auth' in provider:provider['auth']={'kind':provider['auth']['kind'],'material':'[合成夹具已脱敏]'}
  d['configRedacted']=cfg
  worker=pathlib.Path(cfg['workers']['argv'][0]);d['workerPath']=str(worker);d['workerSha256']=sha(worker)
  ws=pathlib.Path(cfg['workspace']);d['inputFixtureHashes']={p.name:sha(p) for p in ws.iterdir() if p.is_file()}
 rows.append(d)
label=sys.argv[1] if len(sys.argv)>1 else 'resources-green'
(EV/'commands'/label/'runtime-binding.json').write_text(json.dumps({'utc':utc(),'rows':rows,'equipment': [{'path':str(q),'sha256':sha(q)} for q in (ROOT/'rust/target/debug/deps').glob('r05_t08_resources-*') if q.is_file() and q.stat().st_mode & 0o111], 'inputs':snapshot()},ensure_ascii=False,indent=2))
print({'runtimeProcessesBound':len(rows)})
