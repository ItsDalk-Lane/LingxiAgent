from capture import *
obj=load(ROOT/R/'D-REVIEW-01/prepared-object.json');actual=sha(obj['absolutePath']);record={'utc':now(),'absolutePath':obj['absolutePath'],'actualSha256':actual,'reviewSha256':obj['sha256'],'sameBytes':actual==obj['sha256'],'cdhashFromBoundIndependentReview':obj['cdhash'],'boundary':'本E只读字节回读；同SHA绑定D原完整签名回执，不伪称本E重新构建/运行r00或申请权限。FINAL重链接仍须按实际新对象。'}
dump(E/'d-object-readback.json',record);assert record['sameBytes'];print(json.dumps(record,ensure_ascii=False))
