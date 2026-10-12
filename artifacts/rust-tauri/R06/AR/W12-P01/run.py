import os,sys,subprocess,json,time,pathlib,tempfile,hashlib
out=pathlib.Path(__file__).parent.resolve(); name=sys.argv[1]; cmd=sys.argv[2:]; start=time.time(); env=os.environ.copy(); env.update(LINGXI_HOME=tempfile.mkdtemp(prefix='w12-p01-'+name+'-'),TEST_MEM_LIMIT_MB='8192',TEST_TIME_LIMIT_SEC='600',TEST_GUARD_VERBOSE='1');env.pop('LINGXI_TEST_GUARD',None)
assert not (out/(name+'.log')).exists()
cwd=env.get('W12_CWD','/Users/study_superior/Desktop/Code/LingxiAgent')
with (out/(name+'.log')).open('w') as f:
 f.write(json.dumps({'command':cmd,'cwd':cwd,'home':env['LINGXI_HOME'],'start':start})+'\n');f.flush()
 try:r=subprocess.run(cmd,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT,timeout=600);code=r.returncode
 except subprocess.TimeoutExpired:code=124
res={'command':cmd,'cwd':cwd,'home':env['LINGXI_HOME'],'start':start,'end':time.time(),'exit_code':code,'log_sha256':hashlib.sha256((out/(name+'.log')).read_bytes()).hexdigest()};(out/(name+'.json')).write_text(json.dumps(res,indent=2));print(json.dumps(res));sys.exit(code)
