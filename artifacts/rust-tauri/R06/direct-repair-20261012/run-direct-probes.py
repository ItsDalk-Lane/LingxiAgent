from pathlib import Path
import concurrent.futures, json, os, subprocess
root=Path(__file__).resolve().parent
cache=Path('/Users/study_superior/.codex/r06-w05-r01-runtime/target/debug/deps')
checks=[
('F01','r06_ar_f01_history','f01_c01_real_authenticated_history_and_isolation'),
('F02','r06_ar_f02_file_restore','ar_f02_native_filetools_write_then_restore_and_session_witness_binding'),
('F03','r06_ar_f03_operations','positive_and_fork_event_atomicity'),
('F04','r06_ar_f04_lifecycle','f04_c01_formal_fork_retains_attachment_in_siblings'),
('F05','r06_ar_f05_cache','c01_actual_calls_keep_system_frozen_and_tools_live_after_description_update'),
('F06','r06_ar_f06_config','c01_flow_disabled_real_request'),
('F07','r06_ar_f07_catalog','first_layer_c03_five_protocols_keep_actual_full_description_schema_name_generation'),
('F09','r06_ar_w09_embedding_batches','c01_exact_batch_boundaries'),
('F10','r06_ar_w10_embedding_identity','c02_same_dimension_reload_never_poisons_first_variant'),
]
def run(item):
 issue,suite,name=item
 bins=[p for p in cache.glob(suite+'-*') if p.is_file() and os.access(p,os.X_OK)]
 binary=max(bins,key=lambda p:p.stat().st_mtime)
 env=os.environ.copy()
 home=root/'synthetic-home'/issue;home.mkdir(parents=True,exist_ok=True)
 evidence=root/'direct-probes'/issue;evidence.mkdir(parents=True,exist_ok=True)
 env.update(LINGXI_HOME=str(home),TEST_TIME_LIMIT_SEC='90',W01_EVIDENCE=str(evidence),LINGXI_W03_EVIDENCE=str(evidence),LINGXI_F05_EVIDENCE=str(evidence),W09_EVIDENCE=str(evidence),W10_EVIDENCE=str(evidence))
 proc=subprocess.run([str(binary),name,'--exact','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
 (evidence/'check.log').write_text(proc.stdout)
 result={'issue':issue,'suite':suite,'test':name,'exitCode':proc.returncode,'log':str(evidence/'check.log')}
 print(json.dumps(result),flush=True)
 if proc.returncode: print('\n'.join(proc.stdout.splitlines()[-24:]),flush=True)
 return result
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as executor:
 results=list(executor.map(run,checks))
(root/'direct-probe-results.json').write_text(json.dumps(results,indent=2)+'\n')
