import pathlib,subprocess,sys
p=pathlib.Path(__file__).resolve().parent
for f in ['probe-v2','effects-probe-v2','additional-family-v2','tagged-value-v2']:
 r=subprocess.run([sys.executable,str(p/'run.py'),f,sys.executable,str(p/(f+'.py'))]);print(f,r.returncode,flush=True)
