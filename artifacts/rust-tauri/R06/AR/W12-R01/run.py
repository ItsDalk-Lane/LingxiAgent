import os,sys,subprocess,json,time,pathlib,tempfile,hashlib
out=pathlib.Path(__file__).parent
name=sys.argv[1]; cmd=sys.argv[2:]; start=time.time(); env=os.environ.copy(); env.update(LINGXI_HOME=tempfile.mkdtemp(prefix='w12-r01-'+name+'-'),CARGO_TARGET_DIR='/Users/study_superior/.codex/worktrees/r06-w12-review/LingxiAgent/rust/target',TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',TEST_GUARD_VERBOSE='1'); env.pop('LINGXI_TEST_GUARD',None)
with (out/(name+'.log')).open('w') as f:
 f.write(json.dumps({'command':cmd,'home':env['LINGXI_HOME'],'start':start})+'\n');f.flush();r=subprocess.run(cmd,cwd='/Users/study_superior/.codex/worktrees/r06-w12-review/LingxiAgent',env=env,stdout=f,stderr=subprocess.STDOUT)
res={'command':cmd,'home':env['LINGXI_HOME'],'start':start,'end':time.time(),'exit_code':r.returncode,'log_sha256':hashlib.sha256((out/(name+'.log')).read_bytes()).hexdigest()};(out/(name+'.json')).write_text(json.dumps(res,indent=2));print(json.dumps(res));sys.exit(r.returncode)
