import json,subprocess,pathlib,sys
p=pathlib.Path(__file__).resolve().parent
for tag,cmd in json.loads((p/"commands.json").read_text()):
 r=subprocess.run([sys.executable,str(p/"run.py"),tag]+cmd)
 print(tag,r.returncode,flush=True)
