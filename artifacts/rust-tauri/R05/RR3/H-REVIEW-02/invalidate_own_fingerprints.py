from capture import *
import shutil
items=[('lingxi-service-d1b960a1b85442c2/lib-lingxi_service','f46-restored-green-build'),('lingxi-service-f70d3da7f61fe4be/bin-lingxi-service','f46-restored-green-build'),('lingxi-service-0532c49ca08cc763/test-lib-lingxi_service','f48-boundary-unit-red')]
rows=[];out=EV/'cache-invalidated';out.mkdir()
for rel,label in items:
 p=ROOT/'rust/target/debug/.fingerprint'/rel;d=json.loads((EV/'commands'/label/'command.json').read_text());start=datetime.datetime.fromisoformat(d['UTC_start']).timestamp();end=datetime.datetime.fromisoformat(d['UTC_end']).timestamp();st=p.stat()
 assert start<=st.st_mtime<=end,(p,st.st_mtime,start,end)
 assert st.st_size==16
 dest=out/(p.parent.name+'-'+p.name);shutil.copy2(p,dest)
 rows.append({'path':str(p),'bytes':st.st_size,'sha256':sha(p),'mtimeUTC':datetime.datetime.fromtimestamp(st.st_mtime,datetime.timezone.utc).isoformat(),'owningCommand':label,'sourceCwd':d['cwd'],'savedBeforeDeletion':str(dest)})
 p.unlink()
write(EV/'cache-rebuild-receipt.json',{'UTC':utc(),'action':'仅使本轮自产三个16字节Cargo指纹失效；旧二进制/原证全部保留；不改主源码字节或mtime','rows':rows})
print({'invalidatedOwnCacheBytes':sum(r['bytes'] for r in rows)})
