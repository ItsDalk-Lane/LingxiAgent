#!/usr/bin/env python3
# 只读列出实际换行转换，不保存文件正文、不写 Git 对象。
import pathlib,json,hashlib,zlib,subprocess,os,math,datetime
root=pathlib.Path('/Users/study_superior/Desktop/Code/LingxiAgent');out=pathlib.Path('/private/tmp/rr3-delivery-space-20261007')
s=json.loads((out/'estimate-summary.json').read_text());paths=s['normalized_paths'];rows=[]
for path in paths:
    raw=(root/path).read_bytes();filtered=raw.replace(b'\r\n',b'\n');row={'path':path}
    for prefix,data in [('raw',raw),('filtered',filtered)]:
        blob=('blob '+str(len(data))+'\0').encode()+data
        row.update({prefix+'_bytes':len(data),prefix+'_content_sha256':hashlib.sha256(data).hexdigest(),prefix+'_blob_sha1':hashlib.sha1(blob).hexdigest(),prefix+'_blob_sha256':hashlib.sha256(blob).hexdigest(),prefix+'_zlib_level1':len(zlib.compress(blob,1))})
    rows.append(row)
env=dict(os.environ,GIT_OPTIONAL_LOCKS='0',GIT_NO_LAZY_FETCH='1')
r=subprocess.run(['git','-C',str(root),'hash-object','--stdin-paths'],input=('\n'.join(paths)+'\n').encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,check=True)
assert r.stdout.decode().splitlines()==[x['filtered_blob_sha1']for x in rows]
raw_unique={x['raw_blob_sha1']:x for x in rows};ids=sorted(raw_unique)
r=subprocess.run(['git','-C',str(root),'cat-file','--batch-check=%(objectname) %(objecttype) %(objectsize)'],input=('\n'.join(ids)+'\n').encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,check=True)
missing=[]
for line,oid in zip(r.stdout.decode().splitlines(),ids):
    bits=line.split();assert bits[0]==oid
    if bits[1]=='missing':missing.append(raw_unique[oid])
    else:assert bits[1]=='blob' and int(bits[2])==raw_unique[oid]['raw_bytes']
result={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'purpose':'原证实际交付须保留原字节的待处理接口；只读记录，不批准或执行stage方案','actual_git_object_format':'sha1','path_count':len(rows),'raw_unique_blobs':len(raw_unique),'raw_missing_in_repository':len(missing),'raw_missing_level1_4k_upper_addition_to_filtered_estimate':sum(math.ceil(x['raw_zlib_level1']/4096)*4096 for x in missing),'raw_missing_level1_bytes':sum(x['raw_zlib_level1']for x in missing),'raw_bytes':sum(x['raw_bytes']for x in rows),'filtered_bytes':sum(x['filtered_bytes']for x in rows),'raw_filter_relation':'filtered = raw.replace(CRLF, LF); filtered SHA1逐项与只读git hash-object --stdin-paths相等；未输出正文','rows':rows}
(out/'normalization-boundary.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items()if k!='rows'},ensure_ascii=False,indent=2))
