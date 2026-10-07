# 只读取已识别的本轮运行器的日志级别；其他环境值不落盘、不输出。
import ctypes,ctypes.util,json,datetime,errno,subprocess
from pathlib import Path
base=Path(__file__).resolve().parents[1]
rows=json.loads((base/'input/caller-logging-environment.json').read_text())
lib=ctypes.CDLL(ctypes.util.find_library('c'),use_errno=True)
lib.sysctl.argtypes=[ctypes.POINTER(ctypes.c_int),ctypes.c_uint,ctypes.c_void_p,ctypes.POINTER(ctypes.c_size_t),ctypes.c_void_p,ctypes.c_size_t]
out=[]
for row in rows:
 pid=int(row['pid']);mib=(ctypes.c_int*3)(1,49,pid);size=ctypes.c_size_t(1024*1024);buf=ctypes.create_string_buffer(size.value)
 rc=lib.sysctl(mib,3,buf,ctypes.byref(size),None,0)
 result={'pid':pid,'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sysctlExit':rc,'errno':ctypes.get_errno() if rc else 0,'observable':rc==0,'selector':'CTL_KERN/KERN_PROCARGS2/PID','boundary':'其他环境值只在内存用于定位分隔符，不保存'}
 if rc==0:
  raw=buf.raw[:size.value];argc=int.from_bytes(raw[:4],'little');pos=raw.index(b'\0',4)+1
  while pos<len(raw) and raw[pos]==0:pos+=1
  for _ in range(argc):pos=raw.index(b'\0',pos)+1
  envs=raw[pos:].split(b'\0');level=[x[len(b'RUST_LOG='):].decode() for x in envs if x.startswith(b'RUST_LOG=')]
  result.update({'RUST_LOG':level,'argc':argc})
 out.append(result)
(base/'input/caller-env-sysctl.json').write_text(json.dumps(out,ensure_ascii=False,indent=2));print(json.dumps(out,ensure_ascii=False,indent=2))
