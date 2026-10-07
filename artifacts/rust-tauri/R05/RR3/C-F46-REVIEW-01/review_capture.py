import pathlib,hashlib,json,subprocess,datetime,sys,os,re,time,shutil
ROOT=pathlib.Path('/Users/study_superior/Desktop/Code/LingxiAgent'); EV=ROOT/'artifacts/rust-tauri/R05/RR3/C-F46-REVIEW-01'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def binding():
 paths=[ROOT/'rust-toolchain.toml',ROOT/'docs/rust-tauri/PERFORMANCE_THRESHOLDS.json',ROOT/'docs/rust-tauri/R05/R05_PERFORMANCE_RESULTS.json']
 for directory, dirs, files in os.walk(ROOT/'rust'):
  dirs[:]=[d for d in dirs if d not in ('target','.git','xtask')]
  paths += [pathlib.Path(directory)/name for name in files]
 paths += [p for p in (ROOT/'contracts').rglob('*') if p.is_file()]
 rows={str(p.relative_to(ROOT)):sha(p) for p in sorted(set(paths))}
 return {'utc':utc(),'files':rows,'digest':hashlib.sha256(json.dumps(rows,sort_keys=True).encode()).hexdigest(),'scope':'runtime Rust all crates except xtask, contracts, locks, toolchain, registered performance config; other owners excluded'}
def run(label,command):
 out=EV/label; out.mkdir(); before=binding(); (out/'input-before.json').write_text(json.dumps(before,indent=2))
 env=os.environ.copy();env['TMPDIR']=str(EV/'tmp');env.pop('RUST_LOG',None)
 (EV/'tmp').mkdir(exist_ok=True); start=utc()
 with (out/'stdout.log').open('wb') as f: proc=subprocess.Popen(command,cwd=ROOT,env=env,stdout=f,stderr=subprocess.STDOUT); code=proc.wait()
 raw=(out/'stdout.log').read_text(errors='replace'); after=binding(); (out/'input-after.json').write_text(json.dumps(after,indent=2))
 for name in re.findall(r'^F27 raw resource series: (.+)$',raw,re.M):
  p=pathlib.Path(name)
  if p.is_file():shutil.copy2(p,out/'f27-resource-series.json')
 binaries={str(p.relative_to(ROOT)):sha(p) for p in (ROOT/'rust/target/debug').glob('*') if p.is_file() and os.access(p,os.X_OK)}
 record={'command':command,'cwd':str(ROOT),'startedAt':start,'endedAt':utc(),'exitCode':code,'counts':re.findall(r'test result:.*',raw),'inputBeforeDigest':before['digest'],'inputAfterDigest':after['digest'],'inputEqual':before['files']==after['files'],'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'binaryHashes':binaries,'outputHash':sha(out/'stdout.log'),'environment':{'TMPDIR':env['TMPDIR'],'RUST_LOG':'unset'}}
 (out/'command.json').write_text(json.dumps(record,ensure_ascii=False,indent=2)); print(json.dumps(record,ensure_ascii=False)); return code
if __name__=='__main__': sys.exit(run(sys.argv[1],sys.argv[2:]))
