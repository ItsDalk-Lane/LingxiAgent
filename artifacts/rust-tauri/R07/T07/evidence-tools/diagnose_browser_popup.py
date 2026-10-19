#!/usr/bin/env python3
"""同一候选真实测试二进制取证，保留首个失败及二进制身份；不是最终门禁。"""
import pathlib,hashlib,json,subprocess,sys,os
binary=pathlib.Path('/private/tmp/lingxi-r07-build/debug/deps/r07_browser_chromium-c876379d58688d69')
sha=lambda:hashlib.sha256(binary.read_bytes()).hexdigest()
before=sha()
print(json.dumps({'binary':str(binary),'binarySha256Before':before}),flush=True)
command=[str(binary),'trusted_click_discovers_a_real_popup_and_dialog_events_do_not_deadlock','--exact','--nocapture']
env=os.environ.copy();env['RUST_BACKTRACE']='1'
exit_code=0
for iteration in range(1,11):
 print(json.dumps({'diagnosticIteration':iteration,'command':command,'RUST_BACKTRACE':'1'}),flush=True)
 r=subprocess.run(command,env=env);print(json.dumps({'diagnosticIteration':iteration,'exitCode':r.returncode}),flush=True)
 if r.returncode:exit_code=r.returncode;break
after=sha();print(json.dumps({'binarySha256After':after,'binaryUnchanged':before==after,'diagnosticOnly':True}),flush=True)
sys.exit(exit_code if before==after else 99)
