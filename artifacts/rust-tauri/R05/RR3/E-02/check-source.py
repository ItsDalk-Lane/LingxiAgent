from pathlib import Path
import json,re,sys,hashlib,collections,os,urllib.parse
ROOT=Path.cwd();E=ROOT/'artifacts/rust-tauri/R05/RR3/E-02';D=ROOT/'docs/rust-tauri/R05'
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def read(p):return (ROOT/p).read_text()
def obj(p):return json.loads(read(p))
before=json.loads((E/'before.json').read_text()); owned=list(before['owned']);checks=[]
def check(ok,label):
 checks.append(label)
 if not ok:raise AssertionError(label)
def fields(p,typ):
 s=read(p);block=s.split('pub struct '+typ+' {',1)[1].split('\n}',1)[0]
 return re.findall(r'^\s*pub\s+(\w+)\s*:',block,re.M)
def walk_strings(d):
 if isinstance(d,dict):
  for v in d.values():yield from walk_strings(v)
 elif isinstance(d,list):
  for v in d:yield from walk_strings(v)
 elif isinstance(d,str):yield d

h=obj('docs/rust-tauri/R05/R05_HANDOFF.json');c=h['consumer_contract'];spec=[('gateway','request_fields','request_source','ModelRouteRequest'),('context','fields','source','RunContext'),('model_turn','fields','source','ModelTurnInput'),('auxiliary','request_fields','source','AuxiliaryRequest'),('embedding','request_fields','request_source','EmbeddingRequest'),('embedding','context_fields','source','OperationCallContext'),('usage_query','fields','source','ModelUsageQuery')]
# 路由请求与端口在同一源文件。
for key,field,src,typ in spec:
 p=c[key].get(src,c[key]['source']);check(fields(p,typ)==c[key][field],typ+' exact current fields')
check('WIRE_PROTOCOL_MIN_SUPPORTED: u32 = 1' in read('rust/crates/lingxi-protocol/src/handshake.rs') and 'WIRE_PROTOCOL_MAX_SUPPORTED: u32 = 1' in read('rust/crates/lingxi-protocol/src/handshake.rs'),'wire versions 1/1')
check('EVENT_SCHEMA_VERSION: u32 = 1' in read('rust/crates/lingxi-protocol/src/wire.rs'),'event schema 1')
check(obj('shared/contract-versions.json')['DATA_EPOCH']==h['data_epoch']==1,'data epoch 1')
migrations=read(c['versions_source']);check(max(map(int,re.findall(r'version:\s*(\d+),',migrations)))==h['storage_schema_version']==7,'schema max migration 7')
check('model_call_usage_rr1_f38_attempts_nullable' in migrations,'migration v7 name')
for p,v in h['dependency_locks'].items():check(sha(ROOT/p)==v['sha256'],'actual dependency lock '+p)
check('channel = "1.98.1"' in read('rust-toolchain.toml'),'toolchain pin actual root')
check('LedgerWorkerCallbackTrace::new(' in read('rust/crates/lingxi-service/src/lib.rs'),'Ledger production composition wired')
runs=read(c['model_turn']['runtime_source']);check('deadline_unix_ms: call_deadline' in runs and 'remaining_budget_ms(call_deadline)' in runs and 'system_prompt: None' in runs,'runtime deadline/system actual')
check('.next_turn(&call_ctx, &call_id, &turn_input, &sink)' in runs,'actual driver call signature')
ports=read(c['model_turn']['port_source']);check('deltas: &\'a dyn TurnDeltaSink' in ports and 'input: &\'a ModelTurnInput' in ports,'next_turn streaming sink argument')
op=read(c['embedding']['source']);check(bool(re.search(r'pub async fn embed\(\s*&self,\s*request: dialects::embedding::EmbeddingRequest,\s*deadline_unix_ms: Option<u64>,\s*context: Option<&OperationCallContext>',op)),'embedding actual signature')
check(bool(re.search(r'pub async fn rerank\(\s*&self,\s*request: RerankRequest,\s*deadline_unix_ms: Option<u64>,\s*context: Option<&OperationCallContext>',op)),'rerank actual signature')
sample=read('rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs').split('async fn rr1_f21_operation_context_carries_session_run_and_cause()',1)[1].split('\n// ──',1)[0]
for token in ['"sess_rr1_ctx"','"run-rr1-ctx"','"run-rr1-ctx#a1"','"run-rr1-ctx-tc0007"','"context-bound input"','dimensions: Some(2)','context_window: None','input_type: Default::default()','Some(&context)','[0.25,0.75]','"prompt_tokens":5,"total_tokens":9','CallOutcome::Succeeded']:
 check(token in sample,'real normal sample token '+token)
for test in ['rr1_f21_operation_queue_timeout_never_invents_an_http_attempt','rr1_f22_operation_invalid_usage_must_not_persist_secret_payload']:check(test in read('rust/crates/lingxi-service/tests/r05_t07_rr1_usage_ledger.rs'),'error/invalid sample source '+test)
for test,p in [('parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap','rust/crates/lingxi-service/tests/subagent_closeout.rs'),('r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing','rust/crates/lingxi-service/tests/late_result_fence.rs')]:check(test in read(p),'I10 real test source '+test)
# 不把这些检查当编译或重新运行样例。

print(json.dumps({"mode":"source","assertions":len(checks),"status":"SELF_CHECKED","provenance":"沿用E-01字段自检，仅自检不作独立签收"},ensure_ascii=False))
