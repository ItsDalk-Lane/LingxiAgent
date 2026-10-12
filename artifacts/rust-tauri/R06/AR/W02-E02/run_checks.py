import subprocess, pathlib, json, hashlib, datetime, os, tempfile, re
root=pathlib.Path(__file__).resolve().parents[5]
folder=pathlib.Path(__file__).resolve().parent
cargo='/Users/study_superior/.cargo/bin/cargo'
commands=[
 ('final-selfcheck',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-j8','-p','lingxi-service','--test','r06_ar_f02_file_restore','--','--nocapture'],{}),
 ('noncanonical-selfcheck',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-j8','-p','lingxi-service','--test','r06_ar_f02_file_restore','--','--nocapture'],{'LINGXI_W02_KEEP_HOME_ALIAS':'1'}),
 ('t03-t05-regression',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-j8','--no-fail-fast','-p','lingxi-service','--test','r06_t03_session_tree','--test','r06_t05_session_files','--test','r06_t05_resources','--test','r06_t05_resourceio','--test','r06_t05_filehistory','--test','r06_t05_fsread','--test','r06_t05_preview','--test','r06_t05_bridgemedia','--test','r04_t04_file_tools','--','--nocapture'],{}),
 ('adapter-store-regression',[cargo,'test','--manifest-path','rust/Cargo.toml','--locked','-j8','--no-fail-fast','-p','lingxi-adapters','--test','session_tree_r06t03','--test','session_file_store','--test','migration_idempotency','--','--nocapture'],{}),
 ('fmt-owned',['/Users/study_superior/.cargo/bin/rustfmt','--check','--edition','2021','--config','skip_children=true']+json.loads((folder/'owned-files.json').read_text()),{}),
 ('clippy-owned',[cargo,'clippy','--manifest-path','rust/Cargo.toml','--locked','-j8','-p','lingxi-service','--all-targets','--','-D','warnings'],{}),
]
results=[]
for name,cmd,extra in commands:
 env=os.environ.copy();home=tempfile.mkdtemp(prefix='lingxi-w02-e02-'+name+'-',dir='/tmp');env['LINGXI_HOME']=home;env.update(extra)
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();log=folder/(name+'.log')
 with log.open('w') as f:
  f.write('COMMAND '+json.dumps(cmd)+'\nISOLATED_LINGXI_HOME '+home+'\nSTART '+start+'\n');f.flush()
  rc=subprocess.run(cmd,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 end=datetime.datetime.now(datetime.timezone.utc).isoformat();text=log.read_text()
 summaries=re.findall(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored',text)
 results.append(dict(name=name,command=cmd,start_time=start,end_time=end,exit_code=rc,lingxi_home=home,test_count=sum(int(s[1])+int(s[2]) for s in summaries),passed=sum(int(s[1]) for s in summaries),failed=sum(int(s[2]) for s in summaries),ignored=sum(int(s[3]) for s in summaries),evidence_hash=hashlib.sha256(log.read_bytes()).hexdigest(),log=log.name))
 (folder/'command-results.json').write_text(json.dumps(results,ensure_ascii=False,indent=2)+'\n');print(name,rc,results[-1]['test_count'],flush=True)
