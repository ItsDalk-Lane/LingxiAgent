from pathlib import Path
import re,json,hashlib,sys
p=Path(sys.argv[1]);doc=p.read_text();src=Path('rust/crates/lingxi-service/src/workerrpc.rs');s=src.read_text();fixture=Path('rust/crates/lingxi-service/src/bin/r04_t07_fixture.rs').read_text()
kind=re.search(r'行协议:\s*kind=([\w.]+)',doc)[1];actual=re.search(r'match parsed.get\("kind"\)[\s\S]*?Some\("([^"]+)"\) =>',s)[1];control='"kind": "callback", "cb_id": "cb-1", "op": "model.complete"' in fixture
result={'boundary':'源码与现行文档的独立静态一致性；非本E生产进程调用','document':str(p),'document_kind':kind,'production_first_branch_kind':actual,'production_rejects_other_kind':'unexpected line kind {other:?} (only callback/result are' in s,'fixture_has_callback_with_op':control,'source_sha256':hashlib.sha256(src.read_bytes()).hexdigest(),'status':'PASS' if kind==actual and control and 'op=model.complete' in doc else 'FAIL'}
print(json.dumps(result,ensure_ascii=False,indent=2));sys.exit(result['status']!='PASS')
